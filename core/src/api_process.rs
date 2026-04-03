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
        
        let mut api_path = std::path::PathBuf::from("./netease-api/api.js");
        if !api_path.exists() {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    let dev_path = exe_dir.join("../../../core/netease-api/api.js");
                    if dev_path.exists() {
                        api_path = dev_path;
                    } else {
                        let deploy_path = exe_dir.join("../netease-api/api.js");
                        if deploy_path.exists() {
                            api_path = deploy_path;
                        }
                    }
                }
            }
        }

        let mut cmd = Command::new("node");
        if let Some(parent) = api_path.parent() {
            cmd.current_dir(parent);
            cmd.arg(api_path.file_name().unwrap());
        } else {
            cmd.arg(&api_path);
        }

        let child = cmd.spawn()?;

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