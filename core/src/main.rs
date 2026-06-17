/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-21
 */
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    routing::get,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::sync::broadcast;
use tokio::time::sleep;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

#[derive(Debug)]
enum HwEvent {
    Command(u8),
    Seek(u16),
    LyricClick(u16),
}

enum SerialPacket {
    Data(Vec<u8>),
    Tagged { generation: u64, packet: Vec<u8> },
}

mod api_process;
mod graphics;
mod protocol;

#[cfg(test)]
const META_BASE_X: i16 = 360;
const META_CLEAR_X: u16 = 320;
const META_CLEAR_Y: u16 = 0;
const META_CLEAR_W: u16 = 480;
const META_CLEAR_H: u16 = 115;
const LYRIC_BACKTRACK_SUPPRESS_MS: u64 = 3200;

/// 维护全局异步共享的应用程序状态
#[derive(Clone)]
struct AppState {
    lyric_tx: broadcast::Sender<String>,
}

#[derive(Clone, Copy)]
struct CoverBlockInfo {
    width: u16,
    height: u16,
    chunk_y: u16,
    chunk_h: u16,
}

fn parse_cover_block_info(packet: &[u8]) -> Option<CoverBlockInfo> {
    if packet.len() < 18 || packet.get(2).copied()? != protocol::PacketType::CoverRgb565Block as u8
    {
        return None;
    }

    let payload_len = u32::from_le_bytes([packet[3], packet[4], packet[5], packet[6]]) as usize;
    if payload_len < 11 || packet.len() < 7 + payload_len + 1 {
        return None;
    }

    Some(CoverBlockInfo {
        width: u16::from_le_bytes([packet[7], packet[8]]),
        height: u16::from_le_bytes([packet[9], packet[10]]),
        chunk_y: u16::from_le_bytes([packet[14], packet[15]]),
        chunk_h: u16::from_le_bytes([packet[16], packet[17]]),
    })
}

fn encode_meta_pixel(alpha: u8, is_active: bool) -> u8 {
    if alpha == 0 {
        return 0;
    }

    let level = ((alpha as u16 * 127 + 127) / 255).max(1).min(127) as u8;
    if is_active { 0x80 | level } else { level }
}

fn put_meta_pixel(canvas: &mut [u8], idx: usize, pixel: u8) {
    if pixel == 0 {
        return;
    }

    let current = canvas[idx];
    if current == 0 || (pixel & 0x7F) >= (current & 0x7F) {
        canvas[idx] = pixel;
    }
}

fn compose_meta_bitmap(layers: &[graphics::TextLayer]) -> graphics::LyricBitmap {
    let width = META_CLEAR_W as usize;
    let height = META_CLEAR_H as usize;
    let mut pixels = vec![0u8; width * height];
    for layer in layers {
        if layer.width == 0 || layer.height == 0 {
            continue;
        }

        let draw_w = layer.width as usize;
        let draw_x0 = layer.x - META_CLEAR_X as i16;
        let is_active = layer.is_active == 1;

        for y in 0..layer.height as usize {
            let dst_y = layer.y + y as i16;
            if dst_y < META_CLEAR_Y as i16 || dst_y >= (META_CLEAR_Y + META_CLEAR_H) as i16 {
                continue;
            }

            for dx in 0..draw_w {
                let dst_x = draw_x0 + dx as i16;
                if dst_x < 0 || dst_x >= META_CLEAR_W as i16 {
                    continue;
                }

                let src_x = ((dx as u32 * layer.width as u32) / draw_w as u32)
                    .min(layer.width as u32 - 1) as usize;
                let src_idx = y * layer.width as usize + src_x;
                let alpha = layer.pixel_data[src_idx];
                if alpha <= 10 {
                    continue;
                }

                let dst_idx = dst_y as usize * width + dst_x as usize;
                put_meta_pixel(&mut pixels, dst_idx, encode_meta_pixel(alpha, is_active));
            }
        }
    }

    graphics::LyricBitmap {
        x: META_CLEAR_X,
        y: META_CLEAR_Y,
        width: META_CLEAR_W,
        height: META_CLEAR_H,
        pixels,
    }
}

async fn send_meta_layers_direct(
    serial_tx: &tokio::sync::mpsc::UnboundedSender<SerialPacket>,
    generation: u64,
    next_layers: &[graphics::TextLayer],
) {
    let bitmap = compose_meta_bitmap(next_layers);
    let _ = serial_tx.send(SerialPacket::Tagged {
        generation,
        packet: protocol::pack_meta_bitmap_cropped(&bitmap),
    });
}

async fn send_cover_blocks(
    serial_tx: &tokio::sync::mpsc::UnboundedSender<SerialPacket>,
    cover_blocks: Vec<Vec<u8>>,
) {
    for block in cover_blocks {
        let _ = serial_tx.send(SerialPacket::Data(block));
    }
}

fn parse_lyric_message(lyric: &str) -> (Vec<String>, Vec<f64>) {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(lyric.trim()) {
        if let Some(arr) = parsed.as_array() {
            let lines = arr
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_string())
                .collect();
            return (lines, vec![0.0; 11]);
        }

        if let (Some(l_arr), Some(t_arr)) = (parsed["lines"].as_array(), parsed["times"].as_array())
        {
            let lines = l_arr
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_string())
                .collect();
            let times = t_arr.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect();
            return (lines, times);
        }
    }

    let mut lines = Vec::with_capacity(11);
    for _ in 0..5 {
        lines.push(String::new());
    }
    lines.push(lyric.trim().to_string());
    while lines.len() < 11 {
        lines.push(String::new());
    }
    (lines, vec![0.0; 11])
}

fn pack_lyric_message(lyric: &str, refresh_only: bool) -> Option<(Vec<u8>, Vec<f64>)> {
    let (lines, times) = parse_lyric_message(lyric);
    if lines.iter().all(|line| line.trim().is_empty()) {
        return None;
    }

    let bitmap = graphics::generate_lyric_bitmap(&lines);
    let packet = if refresh_only {
        protocol::pack_lyric_bitmap_refresh_cropped(&bitmap)
    } else {
        protocol::pack_lyric_bitmap_cropped(&bitmap)
    };

    Some((packet, times))
}

/// 后台守护进程核心入口，初始化 WebSocket 服务与 SMTC 监听，建立多线程异步通信循环
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let api_process = api_process::NeteaseApiProcess::start()
        .expect("拉起 Node 服务失败，请检查是否已安装 Node.js");
    sleep(Duration::from_secs(2)).await;

    let (lyric_tx, _) = broadcast::channel::<String>(100);
    let (song_tx, _) = broadcast::channel::<String>(100);
    let (progress_tx, _) = broadcast::channel::<(u16, u64, u64)>(100);
    let (play_state_tx, _) = broadcast::channel::<bool>(100);

    let current_lyric_times = Arc::new(RwLock::new(vec![0.0_f64; 11]));
    let current_lyric_times_for_event = current_lyric_times.clone();
    let mut terminal_lyric_rx = lyric_tx.subscribe();

    let app_state = AppState {
        lyric_tx: lyric_tx.clone(),
    };

    tokio::spawn(async move {
        while let Ok(lyric) = terminal_lyric_rx.recv().await {
            graphics::render_text_to_console(&lyric);
        }
    });

    let (serial_tx, mut serial_rx) = tokio::sync::mpsc::unbounded_channel::<SerialPacket>();
    let (serial_high_tx, mut serial_high_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (hw_event_tx, mut hw_event_rx) = tokio::sync::mpsc::unbounded_channel::<HwEvent>();
    let serial_generation = Arc::new(AtomicU64::new(0));
    let serial_generation_for_thread = serial_generation.clone();

    std::thread::spawn(move || {
        let port_name = "COM5";
        let baud_rate = 2_000_000;

        println!("正在尝试连接 ({})...", port_name);
        let mut port = match serialport::new(port_name, baud_rate)
            .timeout(std::time::Duration::from_millis(2000))
            .open()
        {
            Ok(mut p) => {
                println!("硬件连接成功");
                let _ = p.write_data_terminal_ready(true);
                let _ = p.write_request_to_send(true);
                p
            }
            Err(e) => {
                eprintln!("串口打开失败: {}", e);
                return;
            }
        };

        let mut read_buf = [0u8; 1024];
        let mut rx_state = 0;
        let mut seek_permille = 0u16;
        let mut click_y = 0u16;
        let mut cover_tx_start: Option<Instant> = None;
        let mut cover_tx_bytes = 0usize;
        let mut cover_tx_blocks = 0usize;

        loop {
            let mut work_done = false;

            let packet = if let Ok(packet) = serial_high_rx.try_recv() {
                Some(packet)
            } else {
                let mut selected = None;
                loop {
                    match serial_rx.try_recv() {
                        Ok(SerialPacket::Data(packet)) => {
                            selected = Some(packet);
                            break;
                        }
                        Ok(SerialPacket::Tagged { generation, packet }) => {
                            if generation == serial_generation_for_thread.load(Ordering::SeqCst) {
                                selected = Some(packet);
                                break;
                            }
                            work_done = true;
                        }
                        Err(_) => break,
                    }
                }
                selected
            };

            if let Some(packet) = packet {
                println!(
                    "[DEBUG] 发送数据包到下位机，字节大小: {} bytes",
                    packet.len()
                );
                let cover_info = parse_cover_block_info(&packet);
                if let Some(info) = cover_info {
                    if info.chunk_y == 0 || cover_tx_start.is_none() {
                        cover_tx_start = Some(Instant::now());
                        cover_tx_bytes = 0;
                        cover_tx_blocks = 0;
                        println!(
                            "[DEBUG][cover-tx] start {}x{} first_block_rows={}",
                            info.width, info.height, info.chunk_h
                        );
                    }
                }
                let packet_send_start = Instant::now();
                let mut offset = 0;
                while offset < packet.len() {
                    let end = std::cmp::min(offset + 1024, packet.len());
                    if let Err(e) = port.write_all(&packet[offset..end]) {
                        eprintln!("串口发送失败: {}", e);
                        break;
                    }
                    offset = end;

                    if let Ok(bytes_to_read) = port.bytes_to_read() {
                        if bytes_to_read > 0 {
                            let to_read = std::cmp::min(bytes_to_read as usize, read_buf.len());
                            if let Ok(n) = std::io::Read::read(&mut *port, &mut read_buf[..to_read])
                            {
                                for &b in &read_buf[..n] {
                                    if rx_state == 1 {
                                        seek_permille = b as u16;
                                        rx_state = 2;
                                    } else if rx_state == 2 {
                                        seek_permille |= (b as u16) << 8;
                                        rx_state = 0;
                                        println!("接收到跳转请求: {}/1000", seek_permille);
                                        let _ = hw_event_tx.send(HwEvent::Seek(seek_permille));
                                    } else if rx_state == 3 {
                                        click_y = b as u16;
                                        rx_state = 4;
                                    } else if rx_state == 4 {
                                        click_y |= (b as u16) << 8;
                                        rx_state = 0;
                                        println!("接收到歌词点击请求，Y坐标: {}", click_y);
                                        let _ = hw_event_tx.send(HwEvent::LyricClick(click_y));
                                    } else {
                                        if b == b'P' {
                                            println!("接收到播放/暂停请求");
                                            let _ = hw_event_tx.send(HwEvent::Command(b'P'));
                                        } else if b == b'L' {
                                            println!("接收到上一首请求");
                                            let _ = hw_event_tx.send(HwEvent::Command(b'L'));
                                        } else if b == b'N' {
                                            println!("接收到下一首请求");
                                            let _ = hw_event_tx.send(HwEvent::Command(b'N'));
                                        } else if b == b'E' {
                                            println!("接收到了不在范围内的点击");
                                        } else if b == b'S' {
                                            rx_state = 1;
                                        } else if b == b'C' {
                                            rx_state = 3;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let packet_send_elapsed = packet_send_start.elapsed();
                if let Some(info) = cover_info {
                    cover_tx_bytes += packet.len();
                    cover_tx_blocks += 1;
                    if (info.chunk_y as u32 + info.chunk_h as u32) >= info.height as u32 {
                        if let Some(start) = cover_tx_start.take() {
                            let elapsed = start.elapsed();
                            let kbps = if elapsed.as_secs_f64() > 0.0 {
                                cover_tx_bytes as f64 / 1024.0 / elapsed.as_secs_f64()
                            } else {
                                0.0
                            };
                            println!(
                                "[DEBUG][cover-tx] complete {}x{} blocks={} bytes={} elapsed={}ms last_block={}ms throughput={:.1}KiB/s",
                                info.width,
                                info.height,
                                cover_tx_blocks,
                                cover_tx_bytes,
                                elapsed.as_millis(),
                                packet_send_elapsed.as_millis(),
                                kbps
                            );
                        }
                    }
                }
                work_done = true;
            }

            if let Ok(bytes_to_read) = port.bytes_to_read() {
                if bytes_to_read > 0 {
                    work_done = true;
                    let to_read = std::cmp::min(bytes_to_read as usize, read_buf.len());
                    if let Ok(n) = std::io::Read::read(&mut *port, &mut read_buf[..to_read]) {
                        for &b in &read_buf[..n] {
                            if rx_state == 1 {
                                seek_permille = b as u16;
                                rx_state = 2;
                            } else if rx_state == 2 {
                                seek_permille |= (b as u16) << 8;
                                rx_state = 0;
                                println!("接收到跳转请求: {}/1000", seek_permille);
                                let _ = hw_event_tx.send(HwEvent::Seek(seek_permille));
                            } else if rx_state == 3 {
                                click_y = b as u16;
                                rx_state = 4;
                            } else if rx_state == 4 {
                                click_y |= (b as u16) << 8;
                                rx_state = 0;
                                println!("接收到歌词点击请求，Y坐标: {}", click_y);
                                let _ = hw_event_tx.send(HwEvent::LyricClick(click_y));
                            } else {
                                if b == b'P' {
                                    println!("接收到播放/暂停请求");
                                    let _ = hw_event_tx.send(HwEvent::Command(b'P'));
                                } else if b == b'L' {
                                    println!("接收到上一首请求");
                                    let _ = hw_event_tx.send(HwEvent::Command(b'L'));
                                } else if b == b'N' {
                                    println!("接收到下一首请求");
                                    let _ = hw_event_tx.send(HwEvent::Command(b'N'));
                                } else if b == b'E' {
                                    println!("接收到了不在范围内的点击");
                                } else if b == b'S' {
                                    rx_state = 1;
                                } else if b == b'C' {
                                    rx_state = 3;
                                }
                            }
                        }
                    }
                }
            }

            if !work_done {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    });

    tokio::spawn(async move {
        if let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            if let Ok(manager) = op.await {
                let mut last_hw_event_time =
                    tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
                while let Some(event) = hw_event_rx.recv().await {
                    let now = tokio::time::Instant::now();
                    if now.duration_since(last_hw_event_time).as_millis() < 300 {
                        continue;
                    }
                    last_hw_event_time = now;

                    let mut session_to_use = None;
                    if let Ok(sessions) = manager.GetSessions() {
                        for session in sessions {
                            if let Ok(app_id) = session.SourceAppUserModelId() {
                                let id_str = app_id.to_string().to_lowercase();
                                if id_str.contains("netease") || id_str.contains("cloudmusic") {
                                    session_to_use = Some(session);
                                    break;
                                }
                            }
                        }
                    }

                    if let Some(session) =
                        session_to_use.or_else(|| manager.GetCurrentSession().ok())
                    {
                        match event {
                            HwEvent::Command(b'P') => {
                                let _ = session.TryTogglePlayPauseAsync();
                            }
                            HwEvent::Command(b'L') => {
                                let _ = session.TrySkipPreviousAsync();
                            }
                            HwEvent::Command(b'N') => {
                                let _ = session.TrySkipNextAsync();
                            }
                            HwEvent::Seek(permille) => {
                                if let Ok(timeline) = session.GetTimelineProperties() {
                                    if let Ok(end) = timeline.EndTime() {
                                        let total = end.Duration as f64;
                                        let target = (total * (permille as f64 / 1000.0)) as i64;
                                        let _ = session.TryChangePlaybackPositionAsync(target);
                                    }
                                }
                            }
                            HwEvent::LyricClick(y) => {
                                let index = if y >= graphics::LYRIC_BITMAP_Y
                                    && y < graphics::LYRIC_BITMAP_Y + graphics::LYRIC_BITMAP_HEIGHT
                                {
                                    Some(
                                        ((y - graphics::LYRIC_BITMAP_Y) as usize
                                            * graphics::LYRIC_BITMAP_LINES
                                            / graphics::LYRIC_BITMAP_HEIGHT as usize)
                                            .min(graphics::LYRIC_BITMAP_LINES - 1),
                                    )
                                } else {
                                    None
                                };

                                if let Some(idx) = index {
                                    let times = current_lyric_times_for_event.read().await;
                                    if idx < times.len() {
                                        let target_sec = times[idx];
                                        if target_sec > 0.0 {
                                            let target_100ns =
                                                (target_sec * 10_000_000.0) as i64 + 200_000;
                                            let _ = session
                                                .TryChangePlaybackPositionAsync(target_100ns);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    });

    let serial_tx_for_lyric = serial_tx.clone();
    let mut serial_lyric_rx = lyric_tx.subscribe();
    let current_lyric_times_for_rx = current_lyric_times.clone();
    let serial_generation_for_lyric = serial_generation.clone();
    let lyric_block_until = Arc::new(RwLock::new(
        tokio::time::Instant::now() - Duration::from_secs(1),
    ));
    let lyric_block_until_for_rx = lyric_block_until.clone();

    tokio::spawn(async move {
        let mut last_generation = serial_generation_for_lyric.load(Ordering::SeqCst);
        let mut last_center_time: Option<f64> = None;

        while let Ok(lyric) = serial_lyric_rx.recv().await {
            let now = tokio::time::Instant::now();
            let Some((packet, times)) = pack_lyric_message(&lyric, false) else {
                continue;
            };

            let generation = serial_generation_for_lyric.load(Ordering::SeqCst);
            if generation != last_generation {
                last_generation = generation;
                last_center_time = None;
                *lyric_block_until_for_rx.write().await = now - Duration::from_millis(1);
            }

            if now < *lyric_block_until_for_rx.read().await {
                continue;
            }

            let center_time = times.get(5).copied();
            if let (Some(last), Some(center)) = (last_center_time, center_time) {
                if center <= 3.0 && center + 5.0 < last {
                    *lyric_block_until_for_rx.write().await =
                        now + Duration::from_millis(LYRIC_BACKTRACK_SUPPRESS_MS);
                    continue;
                }
            }

            {
                let mut guard = current_lyric_times_for_rx.write().await;
                if times.len() == 11 {
                    for i in 0..11 {
                        guard[i] = times[i];
                    }
                }
            }
            last_center_time = center_time;

            let _ = serial_tx_for_lyric.send(SerialPacket::Tagged { generation, packet });
        }
    });

    let serial_tx_for_progress = serial_high_tx.clone();
    let mut serial_progress_rx = progress_tx.subscribe();

    tokio::spawn(async move {
        let mut last_send = tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
        while let Ok((progress, current_sec, total_sec)) = serial_progress_rx.recv().await {
            let now = tokio::time::Instant::now();
            if now.duration_since(last_send).as_millis() >= 1000 {
                let packet = protocol::pack_progress(progress);
                let _ = serial_tx_for_progress.send(packet);

                let current_str = format!("{:02}:{:02}", current_sec / 60, current_sec % 60);
                let total_str = format!("{:02}:{:02}", total_sec / 60, total_sec % 60);

                let clear_left = protocol::pack_clear_rect(20, 445, 70, 20);
                let _ = serial_tx_for_progress.send(clear_left);
                let clear_right = protocol::pack_clear_rect(715, 445, 70, 20);
                let _ = serial_tx_for_progress.send(clear_right);

                if let Some(mut layer_left) = graphics::generate_time_layer(&current_str, 40, 447) {
                    layer_left.x = 85 - layer_left.width as i16;
                    let p = protocol::pack_text_layer(&layer_left);
                    let _ = serial_tx_for_progress.send(p);
                }

                if let Some(layer_right) = graphics::generate_time_layer(&total_str, 715, 447) {
                    let p = protocol::pack_text_layer(&layer_right);
                    let _ = serial_tx_for_progress.send(p);
                }

                last_send = now;
            }
        }
    });

    let serial_tx_for_play_state = serial_high_tx.clone();
    let mut serial_play_state_rx = play_state_tx.subscribe();

    tokio::spawn(async move {
        while let Ok(is_playing) = serial_play_state_rx.recv().await {
            let clear_packet = protocol::pack_clear_rect(60, 380, 220, 40);
            let _ = serial_tx_for_play_state.send(clear_packet);

            let layers = graphics::generate_media_controls_layers(is_playing);
            for layer in layers {
                let packet = protocol::pack_text_layer(&layer);
                let _ = serial_tx_for_play_state.send(packet);
            }
        }
    });

    let serial_tx_for_cover = serial_tx.clone();
    let mut song_rx_for_cover = song_tx.subscribe();
    let serial_generation_for_cover = serial_generation.clone();
    let resend_play_state_tx = play_state_tx.clone();

    let last_play_state_store = Arc::new(RwLock::new(None::<bool>));
    let last_play_state_store_for_update = last_play_state_store.clone();
    let mut play_state_rx_for_store = play_state_tx.subscribe();

    tokio::spawn(async move {
        while let Ok(is_playing) = play_state_rx_for_store.recv().await {
            *last_play_state_store_for_update.write().await = Some(is_playing);
        }
    });

    let last_lyric_store = Arc::new(RwLock::new(String::new()));
    let last_lyric_store_for_update = last_lyric_store.clone();
    let lyric_block_until_for_store = lyric_block_until.clone();
    let mut lyric_rx_for_store = lyric_tx.subscribe();
    tokio::spawn(async move {
        while let Ok(lyric) = lyric_rx_for_store.recv().await {
            if tokio::time::Instant::now() < *lyric_block_until_for_store.read().await {
                continue;
            }
            *last_lyric_store_for_update.write().await = lyric;
        }
    });

    let last_meta_layers = Arc::new(RwLock::new(None::<Vec<graphics::TextLayer>>));

    {
        tokio::spawn(async move {
            let mut current_task: Option<tokio::task::JoinHandle<()>> = None;
            while let Ok(song_id) = song_rx_for_cover.recv().await {
                let song_generation =
                    serial_generation_for_cover.fetch_add(1, Ordering::SeqCst) + 1;
                *last_lyric_store.write().await = String::new();
                if let Some((packet, _)) = pack_lyric_message("加载中", true) {
                    let _ = serial_tx_for_cover.send(SerialPacket::Tagged {
                        generation: song_generation,
                        packet,
                    });
                }

                if let Some(task) = current_task.take() {
                    task.abort();
                }

                let serial_tx_clone = serial_tx_for_cover.clone();
                let serial_generation_for_task = serial_generation_for_cover.clone();
                let resend_play_state_tx_clone = resend_play_state_tx.clone();
                let last_play_state_store_clone = last_play_state_store.clone();
                let last_meta_layers_clone = last_meta_layers.clone();

                current_task = Some(tokio::spawn(async move {
                    let url = format!(
                        "https://music.163.com/api/song/detail/?id={}&ids=[{}]",
                        song_id, song_id
                    );
                    if let Ok(resp) = reqwest::get(&url).await {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            let mut pic_url = json["songs"][0]["album"]["picUrl"].as_str();
                            if pic_url.is_none() {
                                pic_url = json["songs"][0]["al"]["picUrl"].as_str();
                            }

                            if let Some(pic) = pic_url {
                                let pic_string = pic.to_string();
                                let title = json["songs"][0]["name"]
                                    .as_str()
                                    .unwrap_or("未知歌曲")
                                    .to_string();

                                let mut artist = json["songs"][0]["artists"][0]["name"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                if artist.is_empty() {
                                    artist = json["songs"][0]["ar"][0]["name"]
                                        .as_str()
                                        .unwrap_or("-")
                                        .to_string();
                                }

                                let mut album = json["songs"][0]["album"]["name"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                if album.is_empty() {
                                    album = json["songs"][0]["al"]["name"]
                                        .as_str()
                                        .unwrap_or("-")
                                        .to_string();
                                }

                                let artist_album = format!("{} - {}", artist, album);

                                let cover_task = tokio::spawn({
                                    let pic_string = pic_string.clone();
                                    async move {
                                        let started = Instant::now();
                                        let result =
                                            graphics::fetch_cover_matrix(&pic_string).await;
                                        (result, started.elapsed())
                                    }
                                });

                                let meta_layers = tokio::task::spawn_blocking({
                                    let title = title.clone();
                                    let artist_album = artist_album.clone();
                                    move || graphics::generate_meta_layers(&title, &artist_album)
                                })
                                .await
                                .ok()
                                .flatten();

                                if serial_generation_for_task.load(Ordering::SeqCst)
                                    != song_generation
                                {
                                    return;
                                }
                                if let Some(meta_layers) = meta_layers {
                                    send_meta_layers_direct(
                                        &serial_tx_clone,
                                        song_generation,
                                        &meta_layers,
                                    )
                                    .await;
                                    if serial_generation_for_task.load(Ordering::SeqCst)
                                        != song_generation
                                    {
                                        return;
                                    }
                                    *last_meta_layers_clone.write().await = Some(meta_layers);
                                }

                                let cover_blocks = match cover_task.await {
                                    Ok((Ok(matrix), process_elapsed)) => {
                                        graphics::print_cover_to_console(&matrix);
                                        let pack_started = Instant::now();
                                        let blocks =
                                            protocol::pack_cover_matrix_rgb565_blocks(&matrix);
                                        let pack_elapsed = pack_started.elapsed();
                                        let bytes: usize = blocks.iter().map(Vec::len).sum();
                                        println!(
                                            "[DEBUG][cover] processed={}ms packed={}ms size={}x{} blocks={} bytes={} approx_wire={}ms",
                                            process_elapsed.as_millis(),
                                            pack_elapsed.as_millis(),
                                            matrix.width,
                                            matrix.height,
                                            blocks.len(),
                                            bytes,
                                            (bytes as f64 * 10.0 / 2_000_000.0 * 1000.0).round()
                                                as u64
                                        );
                                        blocks
                                    }
                                    Ok((Err(err), process_elapsed)) => {
                                        eprintln!(
                                            "[DEBUG][cover] failed after {}ms: {}",
                                            process_elapsed.as_millis(),
                                            err
                                        );
                                        Vec::new()
                                    }
                                    Err(err) => {
                                        eprintln!("[DEBUG][cover] task failed: {}", err);
                                        Vec::new()
                                    }
                                };

                                if serial_generation_for_task.load(Ordering::SeqCst)
                                    != song_generation
                                {
                                    return;
                                }
                                send_cover_blocks(&serial_tx_clone, cover_blocks).await;

                                if serial_generation_for_task.load(Ordering::SeqCst)
                                    != song_generation
                                {
                                    return;
                                }
                                let last_play_state =
                                    last_play_state_store_clone.read().await.clone();
                                if let Some(is_playing) = last_play_state {
                                    let _ = resend_play_state_tx_clone.send(is_playing);
                                }
                            }
                        }
                    }
                }));
            }
        });
    }

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(app_state.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], 18333));

    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(_) => {
            std::process::exit(0);
        }
    };

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    #[cfg(feature = "memory-access")]
    {
        provider_memory::fetch_memory_lyric();
    }

    #[cfg(not(feature = "memory-access"))]
    {
        let tx_clone = app_state.lyric_tx.clone();
        let song_tx_clone = song_tx.clone();
        let progress_tx_clone = progress_tx.clone();
        let play_state_tx_clone = play_state_tx.clone();
        tokio::spawn(async move {
            let _ = provider_api::listen_smtc_and_sync(
                tx_clone,
                song_tx_clone,
                progress_tx_clone,
                play_state_tx_clone,
            )
            .await;
        });
    }

    tokio::spawn(async move {
        if let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            if let Ok(manager) = op.await {
                let stdin = io::stdin();
                let mut reader = BufReader::new(stdin);
                let mut line = String::new();

                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let cmd = line.trim().to_lowercase();
                            if cmd.is_empty() {
                                continue;
                            }

                            let mut session_to_use = None;
                            if let Ok(sessions) = manager.GetSessions() {
                                for session in sessions {
                                    if let Ok(app_id) = session.SourceAppUserModelId() {
                                        let id_str = app_id.to_string().to_lowercase();
                                        if id_str.contains("netease")
                                            || id_str.contains("cloudmusic")
                                        {
                                            session_to_use = Some(session);
                                            break;
                                        }
                                    }
                                }
                            }

                            if let Some(session) =
                                session_to_use.or_else(|| manager.GetCurrentSession().ok())
                            {
                                match cmd.as_str() {
                                    "play" => {
                                        let _ = session.TryPlayAsync();
                                    }
                                    "pause" => {
                                        let _ = session.TryPauseAsync();
                                    }
                                    "stop" => {
                                        let _ = session.TryStopAsync();
                                    }
                                    "next" => {
                                        let _ = session.TrySkipNextAsync();
                                    }
                                    "previous" | "prev" => {
                                        let _ = session.TrySkipPreviousAsync();
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    tokio::signal::ctrl_c().await.unwrap();

    drop(api_process);

    std::process::exit(0);
}

/// 接受并拦截 HTTP 路由请求，将其升级为 WebSocket 连接，传递应用共享状态
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// 维持具体的 WebSocket 持续会话，接收内部通道的歌词变更并执行推流广播
async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.lyric_tx.subscribe();

    while let Ok(lyric) = rx.recv().await {
        let text_to_send = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&lyric) {
            if let Some(arr) = parsed.as_array() {
                if arr.len() == graphics::LYRIC_BITMAP_LINES
                    || arr.len() == graphics::LYRIC_BITMAP_LINES + 2
                {
                    arr[arr.len() / 2].as_str().unwrap_or("").to_string()
                } else {
                    lyric.clone()
                }
            } else if let Some(l_arr) = parsed["lines"].as_array() {
                if l_arr.len() == graphics::LYRIC_BITMAP_LINES
                    || l_arr.len() == graphics::LYRIC_BITMAP_LINES + 2
                {
                    l_arr[l_arr.len() / 2].as_str().unwrap_or("").to_string()
                } else {
                    lyric.clone()
                }
            } else {
                lyric.clone()
            }
        } else {
            lyric.clone()
        };

        let text_to_send = text_to_send.replace('\n', " - ");

        if socket
            .send(Message::Text(text_to_send.into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_clear_rect_covers_left_up_animation_bounds() {
        assert!(META_CLEAR_X <= META_BASE_X as u16);
        assert_eq!(META_CLEAR_X + META_CLEAR_W, 800);
        assert_eq!(META_CLEAR_Y, 0);
        assert_eq!(META_CLEAR_H, 115);
    }

    fn bitmap_packet_bounds(packet: &[u8]) -> (u16, u16, u16, u16) {
        assert_eq!(packet[0], 0xAA);
        assert_eq!(packet[1], 0x55);
        assert_eq!(packet[2], protocol::PacketType::MetaBitmap as u8);
        (
            u16::from_le_bytes([packet[7], packet[8]]),
            u16::from_le_bytes([packet[9], packet[10]]),
            u16::from_le_bytes([packet[11], packet[12]]),
            u16::from_le_bytes([packet[13], packet[14]]),
        )
    }

    #[test]
    fn lyric_backtrack_suppression_covers_delayed_song_switch() {
        assert!(LYRIC_BACKTRACK_SUPPRESS_MS >= 3000);
    }

    #[tokio::test]
    async fn meta_packet_can_be_queued_before_cover_blocks() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SerialPacket>();
        let cover_a = vec![0x07, 0x01];
        let cover_b = vec![0x07, 0x02];
        let meta_layer = graphics::TextLayer {
            x: META_BASE_X,
            y: 10,
            width: 1,
            height: 1,
            is_active: 0,
            pixel_data: vec![255],
            line_index: 0,
        };

        send_meta_layers_direct(&tx, 7, &[meta_layer]).await;
        send_cover_blocks(&tx, vec![cover_a.clone(), cover_b.clone()]).await;

        match rx.recv().await {
            Some(SerialPacket::Tagged { generation, packet }) => {
                assert_eq!(generation, 7);
                assert_eq!(packet[2], protocol::PacketType::MetaBitmap as u8);
                let (x, y, w, h) = bitmap_packet_bounds(&packet);
                assert!(x >= META_CLEAR_X);
                assert!(y >= META_CLEAR_Y);
                assert!(w <= META_CLEAR_W);
                assert!(h <= META_CLEAR_H);
                assert!((x as u32 + w as u32) <= (META_CLEAR_X + META_CLEAR_W) as u32);
                assert!((y as u32 + h as u32) <= (META_CLEAR_Y + META_CLEAR_H) as u32);
            }
            _ => panic!("expected tagged metadata packet"),
        }
        match rx.recv().await {
            Some(SerialPacket::Data(packet)) => assert_eq!(packet, cover_a),
            _ => panic!("expected cover data packet"),
        }
        match rx.recv().await {
            Some(SerialPacket::Data(packet)) => assert_eq!(packet, cover_b),
            _ => panic!("expected cover data packet"),
        }
    }

    #[test]
    fn lyric_refresh_message_uses_refresh_packet_type() {
        let normal = pack_lyric_message("hello", false).unwrap().0;
        let refresh = pack_lyric_message("hello", true).unwrap().0;

        assert_eq!(normal[2], protocol::PacketType::LyricBitmap as u8);
        assert_eq!(refresh[2], protocol::PacketType::LyricBitmapRefresh as u8);
    }
}
