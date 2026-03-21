/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-21
 */

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("核心歌词引擎启动...");
    
    // 初始化通信层 (与外部语言通信接口层，默认通过API访问)
    bridge::init();

    #[cfg(feature = "memory-access")]
    {
        println!("内存访问已开启。");
        provider_memory::fetch_memory_lyric();
    }

    #[cfg(not(feature = "memory-access"))]
    {
        println!("内存访问未开启，默认使用 API 模式。");
        // 这里可以直接调用 provider_api 里面提供的核心逻辑进行监听
        if let Err(e) = provider_api::listen_smtc_and_sync().await {
            println!("API 监听运行中发生错误: {}", e);
        }
    }

    Ok(())
}