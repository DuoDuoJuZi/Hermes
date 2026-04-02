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
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Deserialize, Debug)]
struct LrcResponse {
    lrc: Option<LrcData>,
    tlyric: Option<LrcData>,
}

#[derive(Deserialize, Debug)]
struct LrcData {
    lyric: String,
}

#[derive(Debug, Clone)]
struct LyricLine {
    time: f64,
    text: String,
    trans: Option<String>,
}

async fn fetch_and_parse_lrc(client: &reqwest::Client, song_id: &str) -> std::result::Result<Vec<LyricLine>, Box<dyn std::error::Error>> {
    let url = format!("http://127.0.0.1:10754/lyric?id={}&realIP=211.161.244.70", song_id);
    let resp = client.get(&url).send().await?.json::<LrcResponse>().await?;

    let mut lyrics = Vec::new();
    if let Some(lrc_data) = resp.lrc {
        for mut line in lrc_data.lyric.lines() {
            line = line.trim();
            let mut times = Vec::new();
            while line.starts_with('[') {
                if let Some(end_idx) = line.find(']') {
                    times.push(&line[1..end_idx]);
                    line = line[end_idx + 1..].trim();
                } else {
                    break;
                }
            }
            
            let text = line.to_string();
            for time_str in times {
                let mut parts = time_str.split(':');
                if let (Some(m), Some(s)) = (parts.next(), parts.next()) {
                    if let (Ok(minutes), Ok(seconds)) = (m.parse::<f64>(), s.parse::<f64>()) {
                        let time = minutes * 60.0 + seconds;
                        lyrics.push(LyricLine { time, text: text.clone(), trans: None });
                    }
                }
            }
        }
    }

    let mut tlyrics = Vec::new();
    if let Some(t_data) = resp.tlyric {
        for mut line in t_data.lyric.lines() {
            line = line.trim();
            let mut times = Vec::new();
            while line.starts_with('[') {
                if let Some(end_idx) = line.find(']') {
                    times.push(&line[1..end_idx]);
                    line = line[end_idx + 1..].trim();
                } else {
                    break;
                }
            }
            
            let text = line.to_string();
            if text.is_empty() { continue; }
            
            for time_str in times {
                let mut parts = time_str.split(':');
                if let (Some(m), Some(s)) = (parts.next(), parts.next()) {
                    if let (Ok(minutes), Ok(seconds)) = (m.parse::<f64>(), s.parse::<f64>()) {
                        let time = minutes * 60.0 + seconds;
                        tlyrics.push((time, text.clone()));
                    }
                }
            }
        }
    }

    // 将翻译合并到主歌词中
    for lyric in &mut lyrics {
        let closest = tlyrics.iter().min_by(|a, b| {
            let diff_a = (a.0 - lyric.time).abs();
            let diff_b = (b.0 - lyric.time).abs();
            diff_a.partial_cmp(&diff_b).unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some(t) = closest {
            if (t.0 - lyric.time).abs() < 0.1 {
                lyric.trans = Some(t.1.clone());
            }
        }
    }

    lyrics.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    Ok(lyrics)
}

async fn sync_lyrics_to_channel(lyrics: Vec<LyricLine>, session: GlobalSystemMediaTransportControlsSession, lyric_tx: tokio::sync::broadcast::Sender<String>) {
    let mut current_idx = usize::MAX;
    let manual_offset_sec: f64 = 0.0;

    let wait_start = tokio::time::Instant::now();
    loop {
        if let Ok(timeline) = session.GetTimelineProperties() {
            if let Ok(pos) = timeline.Position() {
                if pos.Duration < 10_000_000 || wait_start.elapsed().as_millis() > 600 {
                    break;
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }

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
                let current_lyric_obj = &lyrics[target_idx];
                let current_lyric = &current_lyric_obj.text;
                if !current_lyric.is_empty() {
                    let mut lines = Vec::with_capacity(11);

                      let mut display_text = current_lyric.clone();
                      if let Some(trans) = &current_lyric_obj.trans {
                          display_text = format!("{}\n{}", display_text, trans);
                      }                    let start_idx = if target_idx >= 5 { target_idx - 5 } else { 0 };
                    for i in start_idx..target_idx {
                        lines.push(lyrics[i].text.clone());
                    }

                    while lines.len() < 5 { lines.insert(0, String::new()); }
                    lines.push(display_text);

                    for j in (target_idx + 1)..=(target_idx + 5) {
                        if j < lyrics.len() {
                            lines.push(lyrics[j].text.clone());
                        } else {
                            lines.push(String::new());
                        }
                    }

                    while lines.len() < 11 { lines.push(String::new()); }

                    let json_str = serde_json::to_string(&lines).unwrap_or_default();
                    println!("{}", current_lyric);
                    let _ = lyric_tx.send(json_str);
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

pub async fn listen_smtc_and_sync(lyric_tx: tokio::sync::broadcast::Sender<String>, song_tx: tokio::sync::broadcast::Sender<String>, progress_tx: tokio::sync::broadcast::Sender<(u16, u64, u64)>, play_state_tx: tokio::sync::broadcast::Sender<bool>) -> std::result::Result<(), Box<dyn std::error::Error>> {
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
    let current_ncm_id = Arc::new(RwLock::new(None::<String>));
    let progress_manager = manager.clone();
    let progress_tx_clone = progress_tx.clone();
    let current_ncm_id_for_task = current_ncm_id.clone();
    tokio::spawn(async move {
        let mut last_play_state: Option<bool> = None;
        loop {
            if let Ok(session) = progress_manager.GetCurrentSession() {
                let mut matched_ncm: Option<String> = None;
                if let Ok(op_props) = session.TryGetMediaPropertiesAsync() {
                    if let Ok(properties) = op_props.await {
                        let props: GlobalSystemMediaTransportControlsSessionMediaProperties = properties;
                        if let Ok(genres) = props.Genres() {
                            for genre in genres {
                                let genre_str: String = genre.to_string();
                                if genre_str.starts_with("NCM-") {
                                    matched_ncm = Some(genre_str.replace("NCM-", ""));
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Some(expected) = current_ncm_id_for_task.read().await.clone() {
                    if let Some(found) = matched_ncm {
                        if found != expected {
                            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                            continue;
                        }
                    } else {
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                        continue;
                    }
                }

                if let Ok(timeline) = session.GetTimelineProperties() {
                    if let (Ok(pos), Ok(last_updated), Ok(end_time)) = (timeline.Position(), timeline.LastUpdatedTime(), timeline.EndTime()) {
                        let total = end_time.Duration;
                        if total > 0 {
                            let mut pos_100ns = pos.Duration;
                            let mut is_playing = false;
                            if let Ok(playback_info) = session.GetPlaybackInfo() {
                                if let Ok(status) = playback_info.PlaybackStatus() {
                                    if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
                                        is_playing = true;
                                        if let Ok(now_sys) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
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
                            if Some(is_playing) != last_play_state {
                                let _ = play_state_tx.send(is_playing);
                                last_play_state = Some(is_playing);
                            }
                            let progress = (pos_100ns as f64 / total as f64 * 1000.0) as u16;
                            
                            let current_sec = (pos_100ns / 10_000_000).max(0) as u64;
                            let total_sec = (total / 10_000_000).max(0) as u64;
                            
                            let _ = progress_tx_clone.send((progress.min(1000), current_sec, total_sec));
                        }
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    });

    let mut last_id = String::new();
    while let Some(song_id) = rx.recv().await {
        if song_id != last_id {
            println!("提取到网易云歌曲 ID: {}", song_id);
            last_id = song_id.clone();
            let _ = song_tx.send(song_id.clone());
            {
                let mut writer = current_ncm_id.write().await;
                *writer = Some(song_id.clone());
            }
            if let Some(task) = current_task.take() {
                task.abort();
            }
            
            let empty_lyric = "[\"\",\"\",\"\",\"\",\"\",\"加载中...\",\"\",\"\",\"\",\"\",\"\"]";
            let _ = lyric_tx.send(empty_lyric.to_string());

            if let Ok(session) = manager.GetCurrentSession() {
                match fetch_and_parse_lrc(&client, &song_id).await {
                    Ok(lyrics) => {
                        println!("成功获取歌词，开始同步");
                        let tx_clone = lyric_tx.clone();
                        current_task = Some(tokio::spawn(async move {
                            sync_lyrics_to_channel(lyrics, session, tx_clone).await;
                        }));
                    },
                    Err(e) => println!("获取歌词失败: {}", e)
                }
            }
        }
    }

    Ok(())
}