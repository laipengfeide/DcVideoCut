use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::auth::{self, Identity};
use crate::ffmpeg::{self, Ffmpeg};
use crate::models::*;

pub struct FfmpegCache {
    pub tried: bool,
    pub value: Option<Ffmpeg>,
    pub error: String,
}

pub struct AppState {
    pub db: Mutex<Connection>,
    pub identity: Mutex<Identity>,
    pub jobs: Mutex<HashMap<String, Child>>,
    pub ffmpeg: Mutex<FfmpegCache>,
    pub cache_dir: PathBuf,
    pub exe_dir: PathBuf,
    pub resource_dir: Option<PathBuf>,
}

impl AppState {
    pub fn new(
        db: Connection,
        cache_dir: PathBuf,
        exe_dir: PathBuf,
        resource_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            db: Mutex::new(db),
            identity: Mutex::new(Identity::Guest),
            jobs: Mutex::new(HashMap::new()),
            ffmpeg: Mutex::new(FfmpegCache {
                tried: false,
                value: None,
                error: String::new(),
            }),
            cache_dir,
            exe_dir,
            resource_dir,
        }
    }
}

fn get_ffmpeg(state: &AppState) -> Result<Ffmpeg, String> {
    let mut cache = state.ffmpeg.lock().unwrap();
    if !cache.tried {
        let (value, error) = ffmpeg::detect(&state.exe_dir, state.resource_dir.as_deref());
        cache.value = value;
        cache.error = error;
        cache.tried = true;
    }
    cache
        .value
        .clone()
        .ok_or_else(|| cache.error.clone())
}

fn emit_progress(app: &AppHandle, job: &str, percent: f64, message: &str) {
    use tauri::Emitter;
    let _ = app.emit(
        "job-progress",
        ProgressPayload {
            job: job.to_string(),
            percent: percent.clamp(0.0, 100.0),
            message: message.to_string(),
        },
    );
}

pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn build_session_view(state: &AppState) -> SessionView {
    let identity = state.identity.lock().unwrap().clone();
    let guest = identity.is_guest();
    let (ffmpeg_ok, ffmpeg_message) = match get_ffmpeg(state) {
        Ok(f) => (
            true,
            format!("ffmpeg 已就绪（{}）· {}", f.source, f.version),
        ),
        Err(e) => (false, e),
    };
    let last_out_dir = state
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT value FROM settings WHERE key = 'out_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten();
    SessionView {
        is_guest: guest,
        username: identity.username(),
        max_video_bytes: auth::MAX_VIDEO_BYTES,
        guest_frame_limit_bytes: auth::GUEST_FRAME_LIMIT_BYTES,
        frame_limit_bytes: if guest {
            Some(auth::GUEST_FRAME_LIMIT_BYTES)
        } else {
            None
        },
        max_cut_points: if guest {
            Some(auth::GUEST_MAX_CUT_POINTS)
        } else {
            None
        },
        ffmpeg_ok,
        ffmpeg_message,
        last_out_dir,
    }
}

fn stat_video(path: &str) -> Result<(PathBuf, u64), String> {
    let p = PathBuf::from(path);
    if !p.exists() {
        return Err("文件不存在或已被移动".into());
    }
    let meta = fs::metadata(&p).map_err(|e| format!("读取文件信息失败: {e}"))?;
    if !meta.is_file() {
        return Err("请选择一个视频文件".into());
    }
    Ok((p, meta.len()))
}

fn check_video_limits(state: &AppState, size: u64, for_frames: bool) -> Result<(), String> {
    if size > auth::MAX_VIDEO_BYTES {
        return Err(format!(
            "视频大小 {} 超过 2GB 上限（处理耗时过久），请先压缩或截取后再导入。",
            human_size(size)
        ));
    }
    if for_frames {
        let guest = state.identity.lock().unwrap().is_guest();
        if guest && size > auth::GUEST_FRAME_LIMIT_BYTES {
            return Err(format!(
                "游客模式仅支持不超过 30MB 的视频抽帧（当前 {}），注册登录后可解除该限制。",
                human_size(size)
            ));
        }
    }
    Ok(())
}

fn time_label(ms: u64) -> String {
    let total = ms / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h:02}h{m:02}m{s:02}s")
    } else {
        format!("{m:02}m{s:02}s")
    }
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\n' | '\r' | '\t' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == ' ');
    if trimmed.is_empty() {
        "video".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn stem_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| sanitize(&s.to_string_lossy()))
        .unwrap_or_else(|| "video".into())
}

fn secs(ms: u64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

fn cleanup_older_frames(root: &Path, keep: &str) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy() != keep {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

// ---------------------------------------------------------------- 会话 / 账号

#[tauri::command]
pub fn app_bootstrap(state: State<'_, AppState>) -> SessionView {
    {
        let conn = state.db.lock().unwrap();
        if let Some(identity) = auth::restore_session(&conn) {
            *state.identity.lock().unwrap() = identity;
        }
    }
    build_session_view(&state)
}

#[tauri::command]
pub fn auth_register(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<SessionView, String> {
    let identity = {
        let conn = state.db.lock().unwrap();
        auth::register(&conn, &username, &password)?
    };
    *state.identity.lock().unwrap() = identity;
    Ok(build_session_view(&state))
}

#[tauri::command]
pub fn auth_login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<SessionView, String> {
    let identity = {
        let conn = state.db.lock().unwrap();
        auth::login(&conn, &username, &password)?
    };
    *state.identity.lock().unwrap() = identity;
    Ok(build_session_view(&state))
}

#[tauri::command]
pub fn auth_guest(state: State<'_, AppState>) -> SessionView {
    *state.identity.lock().unwrap() = Identity::Guest;
    build_session_view(&state)
}

#[tauri::command]
pub fn auth_logout(state: State<'_, AppState>) -> SessionView {
    {
        let conn = state.db.lock().unwrap();
        let _ = auth::logout(&conn);
    }
    *state.identity.lock().unwrap() = Identity::Guest;
    build_session_view(&state)
}

#[tauri::command]
pub fn remember_out_dir(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('out_dir', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job: String) -> Result<(), String> {
    let mut jobs = state.jobs.lock().unwrap();
    if job.trim().is_empty() {
        for child in jobs.values_mut() {
            let _ = child.kill();
        }
        jobs.clear();
    } else if let Some(child) = jobs.get_mut(&job) {
        let _ = child.kill();
        jobs.remove(&job);
    }
    Ok(())
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    if !Path::new(&path).exists() {
        return Err("路径不存在".into());
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let mut cmd = if Path::new(&path).is_dir() {
            let mut c = Command::new("explorer");
            c.arg(&path);
            c
        } else {
            let mut c = Command::new("explorer");
            c.arg("/select,").arg(&path);
            c
        };
        ffmpeg::hide_console(&mut cmd);
        cmd.spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    Ok(())
}

// ---------------------------------------------------------------- 视频信息

#[tauri::command]
pub async fn probe_video(app: AppHandle, path: String) -> Result<VideoMeta, String> {
    tauri::async_runtime::spawn_blocking(move || probe_video_blocking(&app, path))
        .await
        .map_err(|e| format!("任务异常: {e}"))?
}

fn probe_video_blocking(app: &AppHandle, path: String) -> Result<VideoMeta, String> {
    let state = app.state::<AppState>();
    let (_file, size) = stat_video(&path)?;
    check_video_limits(&state, size, false)?;
    let ff = get_ffmpeg(&state)?;
    ffmpeg::probe(&ff, &path, size)
}

// ---------------------------------------------------------------- 抽帧

#[tauri::command]
pub async fn extract_frames(
    app: AppHandle,
    path: String,
    interval_ms: u64,
) -> Result<FramesResult, String> {
    tauri::async_runtime::spawn_blocking(move || extract_frames_blocking(&app, path, interval_ms))
        .await
        .map_err(|e| format!("任务异常: {e}"))?
}

fn extract_frames_blocking(
    app: &AppHandle,
    path: String,
    interval_ms: u64,
) -> Result<FramesResult, String> {
    let state = app.state::<AppState>();
    let (_file, size) = stat_video(&path)?;
    check_video_limits(&state, size, true)?;
    let ff = get_ffmpeg(&state)?;
    let meta = ffmpeg::probe(&ff, &path, size)?;

    if interval_ms < 100 {
        return Err("抽帧间隔不能小于 0.1 秒".into());
    }
    let estimate = ((meta.duration_ms as f64 / interval_ms as f64).ceil() as u64).max(1);
    if estimate > 3000 {
        return Err(format!(
            "按当前间隔预计生成约 {estimate} 张图片（单次上限 3000 张），请加大间隔"
        ));
    }

    let job = Uuid::new_v4().simple().to_string();
    let frames_root = state.cache_dir.join("frames");
    fs::create_dir_all(&frames_root).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    cleanup_older_frames(&frames_root, &job);

    let job_dir = frames_root.join(&job);
    let full_dir = job_dir.join("full");
    let thumb_dir = job_dir.join("thumb");
    fs::create_dir_all(&full_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&thumb_dir).map_err(|e| e.to_string())?;

    let full_pattern = full_dir.join("%05d.jpg").to_string_lossy().to_string();
    let thumb_pattern = thumb_dir.join("%05d.jpg").to_string_lossy().to_string();
    let interval_sec = format!("{:.3}", interval_ms as f64 / 1000.0);

    let extract_args = vec![
        "-hide_banner".to_string(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-i".into(),
        path.clone(),
        "-vf".into(),
        format!("fps=1/{interval_sec}"),
        "-q:v".into(),
        "2".into(),
        "-pix_fmt".into(),
        "yuvj420p".into(),
        full_pattern.clone(),
    ];
    ffmpeg::run_progress(
        app,
        &state,
        &job,
        &ff.ffmpeg,
        &extract_args,
        meta.duration_ms as f64,
        0.0,
        85.0,
        "正在抽取画面",
    )?;

    let thumb_args = vec![
        "-hide_banner".to_string(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        full_pattern,
        "-vf".into(),
        "scale=-2:240".into(),
        "-q:v".into(),
        "4".into(),
        thumb_pattern,
    ];
    let _ = ffmpeg::run_progress(
        app,
        &state,
        &job,
        &ff.ffmpeg,
        &thumb_args,
        0.0,
        85.0,
        15.0,
        "正在生成缩略图",
    );

    let mut items: Vec<(u32, PathBuf)> = Vec::new();
    let entries = fs::read_dir(&full_dir).map_err(|e| format!("读取帧目录失败: {e}"))?;
    for entry in entries.flatten() {
        let path_buf = entry.path();
        if path_buf.extension().and_then(|e| e.to_str()) != Some("jpg") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(idx) = digits.parse::<u32>() {
            items.push((idx, path_buf));
        }
    }
    items.sort_by_key(|(idx, _)| *idx);

    if items.is_empty() {
        return Err("没有抽取到任何画面，视频可能损坏或时长异常".into());
    }

    let frames: Vec<FrameItem> = items
        .into_iter()
        .map(|(idx, full)| {
            let thumb = thumb_dir.join(format!("{idx:05}.jpg"));
            FrameItem {
                index: idx,
                time_ms: ((idx as u64 - 1) * interval_ms) as u64,
                file: full.to_string_lossy().to_string(),
                thumb: if thumb.exists() {
                    thumb.to_string_lossy().to_string()
                } else {
                    full.to_string_lossy().to_string()
                },
            }
        })
        .collect();

    Ok(FramesResult {
        job_id: job,
        video: meta,
        frames,
        interval_ms,
    })
}

// ---------------------------------------------------------------- 时间轴缩略图

#[tauri::command]
pub async fn timeline_thumbs(
    app: AppHandle,
    path: String,
    count: u32,
) -> Result<Vec<FrameItem>, String> {
    tauri::async_runtime::spawn_blocking(move || timeline_thumbs_blocking(&app, path, count))
        .await
        .map_err(|e| format!("任务异常: {e}"))?
}

fn timeline_thumbs_blocking(
    app: &AppHandle,
    path: String,
    count: u32,
) -> Result<Vec<FrameItem>, String> {
    let state = app.state::<AppState>();
    let (_file, size) = stat_video(&path)?;
    check_video_limits(&state, size, false)?;
    let ff = get_ffmpeg(&state)?;
    let meta = ffmpeg::probe(&ff, &path, size)?;

    let count = count.clamp(4, 40);
    let job = Uuid::new_v4().simple().to_string();
    let root = state.cache_dir.join("thumbs");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    cleanup_older_frames(&root, &job);
    let dir = root.join(&job);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    let last_ms = (meta.duration_ms as f64 * 0.985) as u64;
    for i in 0..count {
        let t_ms = if count <= 1 {
            0
        } else {
            (i as f64 / (count - 1) as f64 * last_ms as f64).round() as u64
        };
        let out = dir.join(format!("{:03}.jpg", i + 1));
        let args = vec![
            "-hide_banner".to_string(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-progress".into(),
            "pipe:1".into(),
            "-nostats".into(),
            "-ss".into(),
            secs(t_ms),
            "-i".into(),
            path.clone(),
            "-frames:v".into(),
            "1".into(),
            "-vf".into(),
            "scale=-2:120".into(),
            "-q:v".into(),
            "5".into(),
            out.to_string_lossy().to_string(),
        ];
        let base_pct = (i as f64 / count as f64) * 100.0;
        let span = 100.0 / count as f64;
        let result = ffmpeg::run_progress(
            app,
            &state,
            &job,
            &ff.ffmpeg,
            &args,
            0.0,
            base_pct,
            span,
            "正在生成时间轴缩略图",
        );
        match result {
            Ok(()) => {
                if out.exists() {
                    let file = out.to_string_lossy().to_string();
                    items.push(FrameItem {
                        index: i + 1,
                        time_ms: t_ms,
                        file: file.clone(),
                        thumb: file,
                    });
                }
            }
            Err(e) if e == "任务已取消" => return Err(e),
            Err(_) => {}
        }
    }

    if items.is_empty() {
        return Err("无法生成时间轴缩略图".into());
    }
    emit_progress(app, &job, 100.0, "缩略图生成完成");
    Ok(items)
}

// ---------------------------------------------------------------- 保存图片

#[tauri::command]
pub async fn save_frames(
    app: AppHandle,
    picks: Vec<FramePick>,
    format: String,
    quality: u8,
    out_dir: String,
    base_name: String,
) -> Result<SaveResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_frames_blocking(&app, picks, format, quality, out_dir, base_name)
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

fn save_frames_blocking(
    app: &AppHandle,
    picks: Vec<FramePick>,
    format: String,
    quality: u8,
    out_dir: String,
    base_name: String,
) -> Result<SaveResult, String> {
    let state = app.state::<AppState>();
    if picks.is_empty() {
        return Err("请先选择要保存的图片".into());
    }
    let fmt = match format.to_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        "png" => "png",
        _ => return Err("仅支持保存为 JPG 或 PNG".into()),
    };
    let quality = quality.clamp(1, 10);
    let dir = PathBuf::from(&out_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建输出目录: {e}"))?;

    let job = Uuid::new_v4().simple().to_string();
    let base = if base_name.trim().is_empty() {
        "frame".to_string()
    } else {
        sanitize(&base_name)
    };
    let ff = get_ffmpeg(&state)?;

    let total = picks.len() as f64;
    let mut saved = Vec::new();

    for (i, pick) in picks.iter().enumerate() {
        let src = PathBuf::from(&pick.file);
        if !src.exists() {
            return Err("部分源帧已被清理，请重新抽帧后再保存".into());
        }
        let file_name = format!(
            "{}_{:04}_{}.{}",
            base,
            pick.index,
            time_label(pick.time_ms),
            fmt
        );
        let target = dir.join(file_name);
        let base_pct = (i as f64 / total) * 100.0;
        let span = 100.0 / total;

        if fmt == "jpg" && quality == 2 {
            fs::copy(&src, &target)
                .map_err(|e| format!("保存 {} 失败: {e}", target.display()))?;
            let _ = emit_progress(app, &job, base_pct, "正在保存图片");
        } else {
            let mut args = vec![
                "-hide_banner".to_string(),
                "-loglevel".into(),
                "error".into(),
                "-y".into(),
                "-progress".into(),
                "pipe:1".into(),
                "-nostats".into(),
                "-i".into(),
                pick.file.clone(),
            ];
            if fmt == "jpg" {
                args.push("-q:v".into());
                args.push(quality.to_string());
            }
            args.push(target.to_string_lossy().to_string());
            ffmpeg::run_progress(
                app,
                &state,
                &job,
                &ff.ffmpeg,
                &args,
                0.0,
                base_pct,
                span,
                "正在保存图片",
            )?;
        }
        saved.push(target.to_string_lossy().to_string());
    }

    emit_progress(app, &job, 100.0, "保存完成");
    let _ = state
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO settings (key, value) VALUES ('out_dir', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![out_dir.clone()],
        );

    Ok(SaveResult {
        saved,
        out_dir: out_dir.clone(),
    })
}

// ---------------------------------------------------------------- 切割导出

#[tauri::command]
pub async fn export_segments(
    app: AppHandle,
    source: String,
    segments: Vec<SegmentInput>,
    mode: String,
    out_dir: String,
) -> Result<ExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        export_segments_blocking(&app, source, segments, mode, out_dir)
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

fn export_segments_blocking(
    app: &AppHandle,
    source: String,
    segments: Vec<SegmentInput>,
    mode: String,
    out_dir: String,
) -> Result<ExportResult, String> {
    let state = app.state::<AppState>();
    let (_file, size) = stat_video(&source)?;
    check_video_limits(&state, size, false)?;
    let ff = get_ffmpeg(&state)?;

    if segments.is_empty() {
        return Err("没有可导出的片段".into());
    }

    let guest = state.identity.lock().unwrap().is_guest();
    let max_segments = if guest {
        auth::GUEST_MAX_CUT_POINTS + 1
    } else {
        auth::MAX_EXPORT_SEGMENTS
    };
    if segments.len() as u32 > max_segments {
        return Err(if guest {
            format!(
                "游客模式最多只能切成 {} 段（最多插入 {} 个切割点），注册登录后可解锁无限切割。",
                max_segments,
                auth::GUEST_MAX_CUT_POINTS
            )
        } else {
            format!("单次最多导出 {} 个片段", max_segments)
        });
    }

    let meta = ffmpeg::probe(&ff, &source, size)?;
    let mode = match mode.to_lowercase().as_str() {
        "copy" => "copy",
        "encode" => "encode",
        _ => return Err("未知的切割模式".into()),
    };

    let dir = PathBuf::from(&out_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建输出目录: {e}"))?;

    let source_ext = Path::new(&source)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "mp4".into());
    let ext = if mode == "encode" {
        match source_ext.as_str() {
            "mp4" | "m4v" | "mov" | "mkv" | "avi" => source_ext.clone(),
            _ => "mp4".to_string(),
        }
    } else {
        source_ext.clone()
    };

    let stem = stem_of(&source);
    let job = Uuid::new_v4().simple().to_string();
    let total = segments.len() as f64;
    let mut outputs = Vec::new();

    for (i, seg) in segments.iter().enumerate() {
        if seg.end_ms <= seg.start_ms {
            return Err(format!("第 {} 段的时间范围无效（结束时间应大于开始时间）", i + 1));
        }
        let start = seg.start_ms;
        let mut end = seg.end_ms;
        if end > meta.duration_ms {
            end = meta.duration_ms;
        }
        if end <= start || end - start < 20 {
            return Err(format!("第 {} 段过短，无法导出", i + 1));
        }
        let duration = end - start;

        let name = match &seg.name {
            Some(n) if !n.trim().is_empty() => sanitize(n),
            _ => format!(
                "{stem}_{:02}_{}-{}",
                i + 1,
                time_label(start),
                time_label(end)
            ),
        };
        let target = dir.join(format!("{name}.{ext}"));
        let target = if target.exists() {
            dir.join(format!(
                "{}_{:02}_{}-{}.{}",
                name,
                i + 1,
                time_label(start),
                time_label(end),
                ext
            ))
        } else {
            target
        };

        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-progress".into(),
            "pipe:1".into(),
            "-nostats".into(),
            "-ss".into(),
            secs(start),
            "-i".into(),
            source.clone(),
            "-t".into(),
            secs(duration),
            "-map".into(),
            "0:v:0".into(),
        ];
        if meta.audio_codec.is_some() {
            args.push("-map".into());
            args.push("0:a:0".into());
        }

        if mode == "copy" {
            args.push("-c".into());
            args.push("copy".into());
            args.push("-avoid_negative_ts".into());
            args.push("make_zero".into());
        } else {
            args.extend([
                "-c:v".to_string(),
                "libx264".into(),
                "-preset".into(),
                "veryfast".into(),
                "-crf".into(),
                "20".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "160k".into(),
            ]);
            if ext == "mp4" || ext == "m4v" || ext == "mov" {
                args.push("-movflags".into());
                args.push("+faststart".into());
            }
        }
        args.push(target.to_string_lossy().to_string());

        let base_pct = (i as f64 / total) * 100.0;
        let span = 100.0 / total;
        ffmpeg::run_progress(
            app,
            &state,
            &job,
            &ff.ffmpeg,
            &args,
            duration as f64,
            base_pct,
            span,
            &format!("正在导出第 {}/{} 段", i + 1, total as u32),
        )?;
        outputs.push(target.to_string_lossy().to_string());
    }

    emit_progress(app, &job, 100.0, "导出完成");
    let _ = state
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO settings (key, value) VALUES ('out_dir', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![out_dir.clone()],
        );

    Ok(ExportResult {
        outputs,
        out_dir: out_dir.clone(),
        mode: mode.to_string(),
    })
}
