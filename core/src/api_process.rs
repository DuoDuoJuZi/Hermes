/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-25
 */
use std::process::{Child, Command};

/// 负责管理后台网易云 Node.js API 服务的生命周期
pub struct NeteaseApiProcess {
    child: Child,
}

impl NeteaseApiProcess {
    /// 启动后台服务并返回被管理的进程实例
    pub fn start() -> Result<Self, Box<dyn std::error::Error>> {
        println!("正在后台拉起 Node.js API 服务...");
        let child = Command::new("node")
            .arg("./netease-api/api.js")
            .spawn()?;

        Ok(Self { child })
    }
}

impl Drop for NeteaseApiProcess {
    fn drop(&mut self) {
        println!("主程序准备退出，正在清理 Node.js 进程...");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}