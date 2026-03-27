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
    let mut song_rx = song_tx.subscribe();
    let mut terminal_lyric_rx = lyric_tx.subscribe();

    let app_state = AppState {
        lyric_tx: lyric_tx.clone(),
    };

    tokio::spawn(async move {
        while let Ok(song_id) = song_rx.recv().await {
            let url = format!("https://music.163.com/api/song/detail/?id={}&ids=[{}]", song_id, song_id);
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(pic) = json["songs"][0]["album"]["picUrl"].as_str() {
                        let _ = graphics::fetch_and_print_cover(pic).await;
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        while let Ok(lyric) = terminal_lyric_rx.recv().await {
            graphics::render_text_to_console(&lyric);
        }
    });

    let (serial_tx, mut serial_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    std::thread::spawn(move || {
        let port_name = "COM5";
        let baud_rate = 115200; 

        println!("正在尝试连接 ({})...", port_name);
        let mut port = match serialport::new(port_name, baud_rate)
            .timeout(std::time::Duration::from_millis(5000))
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

        while let Some(packet) = serial_rx.blocking_recv() {
            println!("准备向单片机发送包裹，大小: {} bytes", packet.len());
            if let Err(e) = port.write_all(&packet) {
                eprintln!("串口发送失败: {}", e);
            }
        }
    });

    let serial_tx_for_lyric = serial_tx.clone();
    let mut serial_lyric_rx = lyric_tx.subscribe();
    
    tokio::spawn(async move {
        while let Ok(lyric) = serial_lyric_rx.recv().await {
            let lines: Vec<String> = if let Ok(parsed) = serde_json::from_str(lyric.trim()) {
                parsed
            } else {
                vec![String::new(), String::new(), lyric.trim().to_string(), String::new(), String::new()]
            };

            if lines.iter().all(|l| l.trim().is_empty()) { continue; }

            if let Some(layers) = graphics::generate_text_layers(&lines) {
                // Clear the whole right section first
                let clear_packet = protocol::pack_clear_rect(400, 0, 400, 480);
                let _ = serial_tx_for_lyric.send(clear_packet);
                
                // Keep some delay interval? In blocking queue it's pushed serially anyway.
                for layer in layers {
                    let packet = protocol::pack_text_layer(&layer);
                    let _ = serial_tx_for_lyric.send(packet);
                }
            }
        }
    });    let serial_tx_for_cover = serial_tx.clone();
    let mut song_rx_for_cover = song_tx.subscribe();
    
    tokio::spawn(async move {
        while let Ok(song_id) = song_rx_for_cover.recv().await {
            let url = format!("https://music.163.com/api/song/detail/?id={}&ids=[{}]", song_id, song_id);
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(pic) = json["songs"][0]["album"]["picUrl"].as_str() {
                        if let Ok(matrix) = graphics::fetch_cover_matrix(pic).await {
                            graphics::print_cover_to_console(&matrix);
                            let packet = protocol::pack_cover_matrix(&matrix);
                            let _ = serial_tx_for_cover.send(packet);
                        }
                    }
                }
            }
        }
    });

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
        tokio::spawn(async move {
            let _ = provider_api::listen_smtc_and_sync(tx_clone, song_tx_clone).await;
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

                            if let Ok(session) = manager.GetCurrentSession() {
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
        let text_to_send = if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&lyric) {
            if parsed.len() == 5 {
                parsed[2].clone()
            } else {
                lyric.clone()
            }
        } else {
            lyric.clone()
        };
        
        if socket.send(Message::Text(text_to_send.into())).await.is_err() {
            break;
        }
    }
}