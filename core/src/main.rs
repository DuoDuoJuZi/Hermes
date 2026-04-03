/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-21
 */

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
use std::time::Duration;
use tokio::time::sleep;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
enum HwEvent {
    Command(u8),
    Seek(u16),
    LyricClick(u16),
}

mod graphics;
mod api_process;
mod protocol;

/// 维护全局异步共享的应用程序状态
#[derive(Clone)]
struct AppState {
    lyric_tx: broadcast::Sender<String>,
}

/// 后台守护进程核心入口，初始化 WebSocket 服务与 SMTC 监听，建立多线程异步通信循环
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let api_process = api_process::NeteaseApiProcess::start().expect("拉起 Node 服务失败，请检查是否已安装 Node.js");
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

    let (serial_tx, mut serial_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (hw_event_tx, mut hw_event_rx) = tokio::sync::mpsc::unbounded_channel::<HwEvent>();

    std::thread::spawn(move || {
        let port_name = "COM5";
        let baud_rate = 115200;

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
            },
            Err(e) => {
                eprintln!("串口打开失败: {}", e);
                return;
            }
        };

        let mut read_buf = [0u8; 1024];
        let mut rx_state = 0;
        let mut seek_permille = 0u16;
        let mut click_y = 0u16;

        loop {
            let mut work_done = false;

            match serial_rx.try_recv() {
                Ok(packet) => {
                    println!("[DEBUG] 发送数据包到下位机，字节大小: {} bytes", packet.len());
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
                    }
                    work_done = true;
                }
                Err(_) => {}
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
                let mut last_hw_event_time = tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
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

                    if let Some(session) = session_to_use.or_else(|| manager.GetCurrentSession().ok()) {
                        match event {
                            HwEvent::Command(b'P') => { let _ = session.TryTogglePlayPauseAsync(); },
                            HwEvent::Command(b'L') => { let _ = session.TrySkipPreviousAsync(); },
                            HwEvent::Command(b'N') => { let _ = session.TrySkipNextAsync(); },
                            HwEvent::Seek(permille) => {
                                if let Ok(timeline) = session.GetTimelineProperties() {
                                    if let Ok(end) = timeline.EndTime() {
                                        let total = end.Duration as f64;
                                        let target = (total * (permille as f64 / 1000.0)) as i64;
                                        let _ = session.TryChangePlaybackPositionAsync(target);
                                    }
                                }
                            },
                            HwEvent::LyricClick(y) => {
                                let mut index = None;
                                if y >= 75 && y < 108 { index = Some(0); }
                                else if y >= 108 && y < 142 { index = Some(1); }
                                else if y >= 142 && y < 176 { index = Some(2); }
                                else if y >= 176 && y < 210 { index = Some(3); }
                                else if y >= 210 && y < 244 { index = Some(4); }
                                else if y >= 244 && y < 296 { index = Some(5); }
                                else if y >= 296 && y < 329 { index = Some(6); }
                                else if y >= 329 && y < 363 { index = Some(7); }
                                else if y >= 363 && y < 397 { index = Some(8); }
                                else if y >= 397 && y < 431 { index = Some(9); }
                                else if y >= 431 && y <= 480 { index = Some(10); }

                                if let Some(idx) = index {
                                    let times = current_lyric_times_for_event.read().await;
                                    if idx < times.len() {
                                        let target_sec = times[idx];
                                        if target_sec > 0.0 {
                                            let target_100ns = (target_sec * 10_000_000.0) as i64 + 200_000;
                                            let _ = session.TryChangePlaybackPositionAsync(target_100ns);
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

    tokio::spawn(async move {
        while let Ok(lyric) = serial_lyric_rx.recv().await {
            let (lines, times): (Vec<String>, Vec<f64>) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(lyric.trim()) {
                if let Some(arr) = parsed.as_array() {
                    let l = arr.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
                    (l, vec![0.0; 11])
                } else if let (Some(l_arr), Some(t_arr)) = (parsed["lines"].as_array(), parsed["times"].as_array()) {
                    let l = l_arr.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
                    let t = t_arr.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect();
                    (l, t)
                } else {
                    let mut f = Vec::with_capacity(11);
                    for _ in 0..5 { f.push(String::new()); }
                    f.push(lyric.trim().to_string());
                    while f.len() < 11 { f.push(String::new()); }
                    (f, vec![0.0; 11])
                }
            } else {
                let mut f = Vec::with_capacity(11);
                for _ in 0..5 { f.push(String::new()); }
                f.push(lyric.trim().to_string());
                while f.len() < 11 { f.push(String::new()); }
                (f, vec![0.0; 11])
            };

            if lines.iter().all(|l| l.trim().is_empty()) { continue; }

            {
                let mut guard = current_lyric_times_for_rx.write().await;
                if times.len() == 11 {
                    for i in 0..11 { guard[i] = times[i]; }
                }
            }

            if let Some(layers) = graphics::generate_text_layers(&lines) {
                let clear_packet = protocol::pack_clear_rect(360, 115, 440, 305);
                let _ = serial_tx_for_lyric.send(clear_packet);

                for layer in layers {
                    let packet = protocol::pack_text_layer(&layer);
                    let _ = serial_tx_for_lyric.send(packet);
                }
            }
        }
    });

    let serial_tx_for_progress = serial_tx.clone();
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

    let serial_tx_for_play_state = serial_tx.clone();
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
    let resend_lyric_tx = lyric_tx.clone();
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
    let mut lyric_rx_for_store = lyric_tx.subscribe();    tokio::spawn(async move {
        while let Ok(lyric) = lyric_rx_for_store.recv().await {
            *last_lyric_store_for_update.write().await = lyric;
        }
    });

    {
        tokio::spawn(async move {
            let mut current_task: Option<tokio::task::JoinHandle<()>> = None;
            while let Ok(song_id) = song_rx_for_cover.recv().await {
                if let Some(task) = current_task.take() {
                    task.abort();
                }
                
                let serial_tx_clone = serial_tx_for_cover.clone();
                let resend_lyric_tx_clone = resend_lyric_tx.clone();
                let resend_play_state_tx_clone = resend_play_state_tx.clone();
                let last_lyric_store_clone = last_lyric_store.clone();
                let last_play_state_store_clone = last_play_state_store.clone();

                current_task = Some(tokio::spawn(async move {
                    let url = format!("https://music.163.com/api/song/detail/?id={}&ids=[{}]", song_id, song_id);
                    if let Ok(resp) = reqwest::get(&url).await {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            let mut pic_url = json["songs"][0]["album"]["picUrl"].as_str();
                            if pic_url.is_none() { pic_url = json["songs"][0]["al"]["picUrl"].as_str(); }

                            if let Some(pic) = pic_url {
                                let pic_string = pic.to_string();
                                let title = json["songs"][0]["name"].as_str().unwrap_or("未知歌曲").to_string();
                                
                                let mut artist = json["songs"][0]["artists"][0]["name"].as_str().unwrap_or("").to_string();
                                if artist.is_empty() { artist = json["songs"][0]["ar"][0]["name"].as_str().unwrap_or("-").to_string(); }
                                
                                let mut album = json["songs"][0]["album"]["name"].as_str().unwrap_or("").to_string();
                                if album.is_empty() { album = json["songs"][0]["al"]["name"].as_str().unwrap_or("-").to_string(); }
                                
                                let artist_album = format!("{} - {}", artist, album);

                                let (cover_res, meta_layers_res) = tokio::join!(
                                    tokio::spawn({
                                        let pic_string = pic_string.clone();
                                        async move {
                                            graphics::fetch_cover_matrix(&pic_string).await
                                        }
                                    }),
                                    tokio::task::spawn_blocking({
                                        let title = title.clone();
                                        let artist_album = artist_album.clone();
                                        move || {
                                            graphics::generate_meta_layers(&title, &artist_album)
                                        }
                                    })
                                );

                                if let Ok(Ok(matrix)) = cover_res {
                                    graphics::print_cover_to_console(&matrix);
                                    let packet = protocol::pack_cover_matrix(&matrix);
                                    let _ = serial_tx_clone.send(packet);
                                }
                                
                                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

                                if let Ok(Some(meta_layers)) = meta_layers_res {
                                    let clear_meta = protocol::pack_clear_rect(360, 0, 440, 115);
                                    let _ = serial_tx_clone.send(clear_meta);
                                    for layer in meta_layers {
                                        let packet = protocol::pack_text_layer(&layer);
                                        let _ = serial_tx_clone.send(packet);
                                    }
                                }

                                let last_lyric = last_lyric_store_clone.read().await.clone();
                                if !last_lyric.is_empty() {
                                    let _ = resend_lyric_tx_clone.send(last_lyric);
                                }
                                
                                let last_play_state = last_play_state_store_clone.read().await.clone();
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
            let _ = provider_api::listen_smtc_and_sync(tx_clone, song_tx_clone, progress_tx_clone, play_state_tx_clone).await;
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
                            if cmd.is_empty() { continue; }

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

                            if let Some(session) = session_to_use.or_else(|| manager.GetCurrentSession().ok()) {
                                match cmd.as_str() {
                                    "play" => { let _ = session.TryPlayAsync(); },
                                    "pause" => { let _ = session.TryPauseAsync(); },
                                    "stop" => { let _ = session.TryStopAsync(); },
                                    "next" => { let _ = session.TrySkipNextAsync(); },
                                    "previous" | "prev" => { let _ = session.TrySkipPreviousAsync(); },
                                    _ => {},
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
                if arr.len() == 11 {
                    arr[5].as_str().unwrap_or("").to_string()
                } else {
                    lyric.clone()
                }
            } else if let Some(l_arr) = parsed["lines"].as_array() {
                if l_arr.len() == 11 {
                    l_arr[5].as_str().unwrap_or("").to_string()
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

        if socket.send(Message::Text(text_to_send.into())).await.is_err() {
            break;
        }
    }
}