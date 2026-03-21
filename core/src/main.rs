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

#[derive(Clone)]
struct AppState {
    lyric_tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("核心歌词引擎启动...");
    
    // 初始化通信层 (与外部语言通信接口层)
    bridge::init();

    let (lyric_tx, _) = broadcast::channel::<String>(100);
    let app_state = AppState {
        lyric_tx: lyric_tx.clone(),
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(app_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 18333));
    println!("API 服务尝试启动在 {}", addr);
    
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(_) => {
            println!("端口已被占用，说明后台已存在守护进程，自身静默退出。");
            std::process::exit(0);
        }
    };

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    #[cfg(feature = "memory-access")]
    {
        println!("内存访问已开启。");
        provider_memory::fetch_memory_lyric();
    }

    #[cfg(not(feature = "memory-access"))]
    {
        println!("内存访问未开启，默认使用 API 模式。");
        if let Err(e) = provider_api::listen_smtc_and_sync(lyric_tx).await {
            println!("API 监听运行中发生错误: {}", e);
        }
    }

    tokio::signal::ctrl_c().await.unwrap();
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.lyric_tx.subscribe();
    println!("新 IDE 插件已连接到 WebSocket!");

    while let Ok(lyric) = rx.recv().await {
        if socket.send(Message::Text(lyric.into())).await.is_err() {
            println!("IDE 插件已断开连接");
            break; 
        }
    }
}