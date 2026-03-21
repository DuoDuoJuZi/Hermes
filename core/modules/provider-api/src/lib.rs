/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-21
 */
use reqwest;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSession,
    CurrentSessionChangedEventArgs,
    MediaPropertiesChangedEventArgs,
    GlobalSystemMediaTransportControlsSessionMediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus
};

#[derive(Deserialize, Debug)]
struct LrcResponse {
    lrc: Option<LrcData>,
}

#[derive(Deserialize, Debug)]
struct LrcData {
    lyric: String,
}

#[derive(Debug, Clone)]
struct LyricLine {
    time: f64,
    text: String,
}

/**
 * 访问网易云音乐公共 API 获取对应的 LRC 文件并解析为时间轴格式数组返回
 */
async fn fetch_and_parse_lrc(client: &reqwest::Client, song_id: &str) -> std::result::Result<Vec<LyricLine>, Box<dyn std::error::Error>> {
    let url = format!("https://music.163.com/api/song/lyric?id={}&lv=1&kv=1&tv=-1", song_id);
    let resp = client.get(&url).send().await?.json::<LrcResponse>().await?;
    
    let mut lyrics = Vec::new();
    if let Some(lrc_data) = resp.lrc {
        for line in lrc_data.lyric.lines() {
            if line.starts_with('[') {
                if let Some(end_idx) = line.find(']') {
                    let time_str = &line[1..end_idx];
                    let text = line[end_idx + 1..].trim().to_string();
                    
                    let mut parts = time_str.split(':');
                    if let (Some(m), Some(s)) = (parts.next(), parts.next()) {
                        if let (Ok(minutes), Ok(seconds)) = (m.parse::<f64>(), s.parse::<f64>()) {
                            let time = minutes * 60.0 + seconds;
                            lyrics.push(LyricLine { time, text });
                        }
                    }
                }
            }
        }
    }
    
    lyrics.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    Ok(lyrics)
}

/**
 * 接收解析后的歌词数组并结合播放进度持续在控制台输出当前实时歌词
 */
async fn sync_lyrics_to_console(lyrics: Vec<LyricLine>, session: GlobalSystemMediaTransportControlsSession) {
    let mut current_idx = usize::MAX;
    let manual_offset_sec: f64 = 0.2; 
    
    loop {
        let mut position = -1.0;

        if let Ok(timeline) = session.GetTimelineProperties() {
            if let (Ok(pos), Ok(last_updated)) = (timeline.Position(), timeline.LastUpdatedTime()) {
                let mut pos_100ns = pos.Duration;
                
                if let Ok(playback_info) = session.GetPlaybackInfo() {
                    if let Ok(status) = playback_info.PlaybackStatus() {
                        if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
                            if let Ok(now_sys) = SystemTime::now().duration_since(UNIX_EPOCH) {
                                let now_100ns = (now_sys.as_secs() * 10_000_000) as i64 + (now_sys.subsec_nanos() / 100) as i64;
                                let epoch_diff = 11644473600 * 10_000_000i64; 
                                let current_universal_time = now_100ns + epoch_diff;
                                
                                let elapsed_100ns = current_universal_time - last_updated.UniversalTime;
                                
                                if elapsed_100ns > 0 {
                                    pos_100ns += elapsed_100ns;
                                }
                            }
                        }
                    }
                }
                
                position = (pos_100ns as f64 / 10_000_000.0) + manual_offset_sec;
            }
        }

        if position >= 0.0 {
            let target_idx = lyrics.partition_point(|line| line.time <= position).saturating_sub(1);

            if target_idx != current_idx && target_idx < lyrics.len() {
                if !lyrics[target_idx].text.is_empty() {
                    println!("{}", lyrics[target_idx].text);
                }
                current_idx = target_idx;
            }
        }
        
        sleep(Duration::from_millis(20)).await;
    }
}

fn create_media_props_handler(tx: tokio::sync::mpsc::UnboundedSender<String>, handle: tokio::runtime::Handle) -> TypedEventHandler<GlobalSystemMediaTransportControlsSession, MediaPropertiesChangedEventArgs> {
    TypedEventHandler::<GlobalSystemMediaTransportControlsSession, MediaPropertiesChangedEventArgs>::new(move |session_ref, _| {
        let tx_inner = tx.clone();
        if let Some(session) = session_ref.clone() {
            let session_clone = session.clone();
            handle.spawn(async move {
                if let Ok(op) = session_clone.TryGetMediaPropertiesAsync() {
                    if let Ok(properties) = op.await {
                        let props: GlobalSystemMediaTransportControlsSessionMediaProperties = properties;
                        if let Ok(genres) = props.Genres() {
                            for genre in genres {
                                let genre_str: String = genre.to_string();
                                if genre_str.starts_with("NCM-") {
                                    let _ = tx_inner.send(genre_str.replace("NCM-", ""));
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }
        Ok(())
    })
}

/**
 * 通过 Windows API 提供对媒体信息的访问并监听当前播放歌曲的变更
 */
pub async fn listen_smtc_and_sync() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    println!("正在连接 SMTC");
    
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.await?;
    let handle = tokio::runtime::Handle::current();

    let tx_first = tx.clone();
    if let Ok(session) = manager.GetCurrentSession() {
        let _ = session.MediaPropertiesChanged(&create_media_props_handler(tx.clone(), handle.clone()));
        if let Ok(op) = session.TryGetMediaPropertiesAsync() {
            if let Ok(properties) = op.await {
                let props: GlobalSystemMediaTransportControlsSessionMediaProperties = properties;
                if let Ok(genres) = props.Genres() {
                    for genre in genres {
                        let genre_str: String = genre.to_string();
                        if genre_str.starts_with("NCM-") {
                            let _ = tx_first.send(genre_str.replace("NCM-", ""));
                            break;
                        }
                    }
                }
            }
        }
    }

    let tx_for_event = tx.clone();
    let handle_for_event = handle.clone();
    manager.CurrentSessionChanged(&TypedEventHandler::<GlobalSystemMediaTransportControlsSessionManager, CurrentSessionChangedEventArgs>::new(move |manager_ref, _| {
        if let Some(mgr) = manager_ref.clone() {
            if let Ok(session) = mgr.GetCurrentSession() {
                 let _ = session.MediaPropertiesChanged(&create_media_props_handler(tx_for_event.clone(), handle_for_event.clone()));
                 
                 // Trigger once when session changes
                 let tx_inner = tx_for_event.clone();
                 let session_clone = session.clone();
                 handle_for_event.spawn(async move {
                     if let Ok(op) = session_clone.TryGetMediaPropertiesAsync() {
                         if let Ok(properties) = op.await {
                             let props: GlobalSystemMediaTransportControlsSessionMediaProperties = properties;
                             if let Ok(genres) = props.Genres() {
                                 for genre in genres {
                                     let genre_str: String = genre.to_string();
                                     if genre_str.starts_with("NCM-") {
                                         let _ = tx_inner.send(genre_str.replace("NCM-", ""));
                                         break;
                                     }
                                 }
                             }
                         }
                     }
                 });
            }
        }
        Ok(())
    }))?;

    let mut current_task: Option<tokio::task::JoinHandle<()>> = None;
    let client = reqwest::Client::new();

    let mut last_id = String::new();
    while let Some(song_id) = rx.recv().await {
        if song_id != last_id {
            println!("提取到网易云歌曲 ID: {}", song_id);
            last_id = song_id.clone();
            
            if let Some(task) = current_task.take() {
                task.abort();
            }

            if let Ok(session) = manager.GetCurrentSession() {
                match fetch_and_parse_lrc(&client, &song_id).await {
                    Ok(lyrics) => {
                        println!("成功获取歌词，开始同步");
                        current_task = Some(tokio::spawn(async move {
                            sync_lyrics_to_console(lyrics, session).await;
                        }));
                    },
                    Err(e) => println!("获取歌词失败: {}", e)
                }
            }
        }
    }

    Ok(())
}
