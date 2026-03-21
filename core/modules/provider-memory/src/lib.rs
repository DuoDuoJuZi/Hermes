// @Author: DuoDuoJuZi
// @Date: 2026-03-21
use std::io::{self, Write};
use std::mem::size_of;
use std::thread;
use std::time::{Duration, Instant};
use winapi::shared::minwindef::{LPCVOID, LPVOID};
use winapi::um::memoryapi::{ReadProcessMemory, VirtualQueryEx};
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::winnt::{HANDLE, MEM_COMMIT, PAGE_READWRITE, PAGE_EXECUTE_READWRITE, PAGE_READONLY, PROCESS_ALL_ACCESS, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ, MEMORY_BASIC_INFORMATION};
use sysinfo::{PidExt, ProcessExt, System, SystemExt};
use rayon::prelude::*; 

fn get_process_ids() -> Vec<u32> {
    let mut sys = System::new_all();
    sys.refresh_processes();
    let mut pids = Vec::new();
    for (pid, process) in sys.processes() {
        if process.name().eq_ignore_ascii_case("cloudmusic.exe") {
            pids.push(pid.as_u32());
        }
    }
    pids
}

fn open_process(pid: u32) -> Result<usize, String> {
    unsafe {
        let handle = OpenProcess(PROCESS_ALL_ACCESS | PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            Err(format!("打开进程失败，PID: {}", pid))
        } else {
            Ok(handle as usize)
        }
    }
}

fn read_string_from_memory(process_handle: usize, address: usize, max_len: usize) -> Option<(String, Vec<u16>)> {
    let mut raw_bytes = vec![0u8; max_len];
    let mut bytes_read = 0;
    let success = unsafe {
        ReadProcessMemory(
            process_handle as HANDLE, 
            address as LPCVOID,
            raw_bytes.as_mut_ptr() as LPVOID,
            max_len,
            &mut bytes_read,
        )
    };
    if success == 0 || bytes_read == 0 {
        return None;
    }
    
    let mut u16_chars = Vec::new();
    for chunk in raw_bytes[..bytes_read].chunks_exact(2) {
        let val = u16::from_le_bytes([chunk[0], chunk[1]]);
        if val == 0 { break; }
        u16_chars.push(val);
    }
    
    let decoded = String::from_utf16_lossy(&u16_chars).replace('\u{3000}', " ").trim().to_string();
    Some((decoded, u16_chars))
}

fn scan_memory_for_string(process_handle: usize, target: &str) -> Vec<usize> {
    let mut results = Vec::new();
    let mut current_address = 0usize;
    let mut mem_info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    
    let mut targets_bytes = Vec::new();
    let target_u16: Vec<u16> = target.encode_utf16().collect();
    targets_bytes.push(target_u16.iter().flat_map(|&c| c.to_le_bytes().to_vec()).collect::<Vec<u8>>());
    
    if target.contains(' ') {
        let var_u16: Vec<u16> = target.replace(' ', "\u{3000}").encode_utf16().collect();
        targets_bytes.push(var_u16.iter().flat_map(|&c| c.to_le_bytes().to_vec()).collect::<Vec<u8>>());
    }

    unsafe {
        while VirtualQueryEx(
            process_handle as HANDLE,
            current_address as LPCVOID,
            &mut mem_info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        ) == size_of::<MEMORY_BASIC_INFORMATION>()
        {
            let is_commit = mem_info.State == MEM_COMMIT;
            let is_readable = (mem_info.Protect & (PAGE_READWRITE | PAGE_EXECUTE_READWRITE | PAGE_READONLY)) != 0;
            if is_commit && is_readable {
                let region_size = mem_info.RegionSize;
                let base_address = mem_info.BaseAddress as usize;
                let mut buffer = vec![0u8; region_size];
                let mut bytes_read = 0;
                if ReadProcessMemory(
                    process_handle as HANDLE,
                    base_address as LPCVOID,
                    buffer.as_mut_ptr() as LPVOID,
                    region_size,
                    &mut bytes_read,
                ) != 0 && bytes_read > 0
                {
                    buffer.truncate(bytes_read);
                    for t_bytes in &targets_bytes {
                        let t_len = t_bytes.len();
                        if t_len > 0 && buffer.len() >= t_len {
                            for i in 0..=buffer.len() - t_len {
                                if &buffer[i..i + t_len] == t_bytes.as_slice() {
                                    results.push(base_address + i);
                                }
                            }
                        }
                    }
                }
            }
            current_address = (mem_info.BaseAddress as usize) + mem_info.RegionSize;
        }
    }
    results
}

/**
 * 启动基于 Windows 内存扫描的歌词引擎
 */
pub fn fetch_memory_lyric() {
    let mut lyrics = Vec::new();
    println!("请输入接下来的 5 句歌词，每输入一句按下回车确认（快歌建议缩短截取关键字）");
    let stdin = io::stdin();
    for i in 1..=5 {
        print!("第 {} 句: ", i);
        io::stdout().flush().unwrap();
        let mut input = String::new();
        stdin.read_line(&mut input).unwrap();
        lyrics.push(input.trim().to_string());
    }

    println!("\n准备就绪，请在网易云中播放歌曲，程序开始锁定并扫描内存信息...");

    let pids = get_process_ids();
    if pids.is_empty() {
        println!("[系统错误] 未找到任何 cloudmusic.exe 进程");
        return;
    }

    let mut process_handles: Vec<usize> = Vec::new();
    for pid in pids {
        if let Ok(h) = open_process(pid) {
            process_handles.push(h);
        }
    }

    if process_handles.is_empty() {
        println!("[系统错误] 无法打开任何找到的网易云进程的句柄");
        return;
    }

    let mut candidate_addresses: Vec<(usize, usize)> = Vec::new();
    let mut current_lyric_index = 0;
    let mut is_first_scan = true;
    let mut wait_start_time = Instant::now();

    loop {
        if current_lyric_index >= lyrics.len() {
            println!("\n=============================================");
            println!("所有歌词匹配完毕，筛选出的完全对应内存地址如下:");
            for (h, addr) in &candidate_addresses {
                println!("- 进程句柄: 0x{:X}, 地址: 0x{:X}", *h, addr);
            }
            println!("=============================================");
            println!("开始极速多线程实时监听歌词变化 (按 Ctrl+C 退出)...");
            
            let mut last_lyric = String::new();
            loop {
                // 最终监听阶段：并发读取所有存活的内存地址
                let current_readings: Vec<_> = candidate_addresses
                    .par_iter()
                    .filter_map(|&(h, addr)| read_string_from_memory(h, addr, 512))
                    .collect();

                for (read_text, _) in current_readings {
                    if !read_text.is_empty() && read_text != last_lyric {
                        println!(">> [极速捕获] 当前播放歌词: {}", read_text);
                        last_lyric = read_text;
                        break; 
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        let target_lyric = &lyrics[current_lyric_index];
        
        if is_first_scan {
            print!("\x1b[K\r正在网易云进程内存中扫描第 1 句歌词: {} ...", target_lyric);
            io::stdout().flush().unwrap();
            
            let found_candidates: Vec<_> = process_handles.par_iter().flat_map(|&h| {
                let found_addrs = scan_memory_for_string(h, target_lyric);
                let mut local_candidates = Vec::new();
                for addr in found_addrs {
                    if let Some((read_text, u16_data)) = read_string_from_memory(h, addr, 512) {
                        if read_text == *target_lyric {
                            local_candidates.push((h, addr, u16_data, read_text));
                        }
                    }
                }
                local_candidates
            }).collect();

            for (h, addr, u16_data, read_text) in found_candidates {
                let hex_str = u16_data.iter().map(|c| format!("{:04X}", c)).collect::<Vec<String>>().join(" ");
                println!("\n[初始捕获] 句柄: 0x{:X}, 内存基址: 0x{:X}", h, addr);
                println!("       UTF-16 : {}", hex_str);
                println!("       反解文本: {}", read_text);
                candidate_addresses.push((h, addr));
            }
            
            if !candidate_addresses.is_empty() {
                println!("\n成功匹配第 1 句歌词，找到 {} 个候选内存块，进入等待追踪模式", candidate_addresses.len());
                current_lyric_index += 1;
                is_first_scan = false;
                wait_start_time = Instant::now();
            }
        } else {
            let prompt_text = format!("极速监听第 {} 句歌词: {} (当前并发监控堆块数量: {})", current_lyric_index + 1, target_lyric, candidate_addresses.len());
            print!("\x1b[K\r{}", prompt_text);
            io::stdout().flush().unwrap();

            let newly_matched: Vec<_> = candidate_addresses
                .par_iter() 
                .filter_map(|&(h, addr)| {
                    if let Some((read_text, u16_data)) = read_string_from_memory(h, addr, 512) {
                        if read_text == *target_lyric {
                            return Some((h, addr, u16_data, read_text));
                        }
                    }
                    None
                })
                .collect();
            
            if !newly_matched.is_empty() {
                print!("\x1b[K\r"); 
                candidate_addresses = newly_matched.into_iter().map(|(h, addr, u16_data, read_text)| {
                    let hex_str = u16_data.iter().map(|c| format!("{:04X}", c)).collect::<Vec<String>>().join(" ");
                    println!("[多线程捕获] 句柄: 0x{:X}, 内存基址: 0x{:X}", h, addr);
                    println!("       UTF-16 : {}", hex_str);
                    println!("       反解文本: {}", read_text);
                    (h, addr)
                }).collect();

                current_lyric_index += 1;
                println!("\n成功匹对第 {} 句歌词，当前并发候选区域数量缩小为: {}", current_lyric_index, candidate_addresses.len());
                wait_start_time = Instant::now();

            } else if wait_start_time.elapsed().as_secs() >= 60 { // 修改为统一的 60 秒
                println!("\n[超时] 等待第 {} 句歌词超时 60 秒，退回全局扫描重新定位", current_lyric_index + 1);
                candidate_addresses.clear();
                current_lyric_index = 0;
                is_first_scan = true;
                continue;
            }
            
            if !is_first_scan && candidate_addresses.is_empty() {
                println!("\n所有的候选内存块全部失效，追踪失败，退回全局扫描重新定位");
                current_lyric_index = 0;
                is_first_scan = true;
                continue;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}