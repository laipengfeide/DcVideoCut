use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::commands::AppState;
use crate::models::{ProgressPayload, VideoMeta};

#[derive(Clone, Debug)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub version: String,
    pub source: String,
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| "未知版本".into())
}

fn version_of(exe: &Path) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.arg("-version").stdin(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(first_line(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut Command) {}

fn from_dir(dir: &Path, source: &str) -> Option<Ffmpeg> {
    let ffmpeg = dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
    let ffprobe = dir.join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    if !ffmpeg.is_file() || !ffprobe.is_file() {
        return None;
    }
    let version = version_of(&ffmpeg).unwrap_or_else(|| "未知版本".into());
    Some(Ffmpeg {
        ffmpeg,
        ffprobe,
        version,
        source: source.to_string(),
    })
}

/// 依次尝试：环境变量 → 打包资源目录 → 可执行文件旁 → 系统 PATH
pub fn detect(exe_dir: &Path, resource_dir: Option<&Path>) -> (Option<Ffmpeg>, String) {
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    if let Ok(dir) = std::env::var("DCVC_FFMPEG_DIR") {
        let p = PathBuf::from(dir);
        candidates.push((p.clone(), format!("环境变量 DCVC_FFMPEG_DIR ({})", p.display())));
        candidates.push((p.join("bin"), "环境变量 DCVC_FFMPEG_DIR/bin".into()));
    }
    if let Some(res) = resource_dir {
        candidates.push((res.join("ffmpeg").join("bin"), "内置资源 ffmpeg/bin".into()));
        candidates.push((res.join("ffmpeg"), "内置资源 ffmpeg".into()));
        candidates.push((res.join("bin"), "内置资源 bin".into()));
    }
    candidates.push((exe_dir.join("ffmpeg").join("bin"), "程序目录 ffmpeg/bin".into()));
    candidates.push((exe_dir.join("bin"), "程序目录 bin".into()));
    candidates.push((exe_dir.join("ffmpeg"), "程序目录 ffmpeg".into()));

    for (dir, label) in candidates {
        if let Some(ff) = from_dir(&dir, &label) {
            return (Some(ff), String::new());
        }
    }

    // 系统 PATH
    if let Some(version) = version_of(Path::new(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })) {
        if version_of(Path::new(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" })).is_some() {
            return (
                Some(Ffmpeg {
                    ffmpeg: PathBuf::from(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }),
                    ffprobe: PathBuf::from(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }),
                    version,
                    source: "系统 PATH".into(),
                }),
                String::new(),
            );
        }
    }

    (
        None,
        "未检测到 ffmpeg / ffprobe。请安装 ffmpeg 并加入系统 PATH，\
         或将 ffmpeg.exe、ffprobe.exe 放到程序目录下的 ffmpeg/bin 文件夹后重启程序。"
            .to_string(),
    )
}

fn parse_rate(rate: &str) -> f64 {
    if let Some((num, den)) = rate.split_once('/') {
        let n: f64 = num.parse().unwrap_or(0.0);
        let d: f64 = den.parse().unwrap_or(0.0);
        if d > 0.0 {
            return n / d;
        }
    }
    rate.parse().unwrap_or(0.0)
}

pub fn probe(ff: &Ffmpeg, path: &str, size_bytes: u64) -> Result<VideoMeta, String> {
    let mut cmd = Command::new(&ff.ffprobe);
    cmd.args([
        "-v",
        "error",
        "-show_format",
        "-show_streams",
        "-print_format",
        "json",
        path,
    ])
    .stdin(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| format!("执行 ffprobe 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "无法解析视频文件：{}",
            tail(&String::from_utf8_lossy(&out.stderr), 2)
        ));
    }
    let root: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("解析视频信息失败: {e}"))?;

    let streams = root
        .get("streams")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();

    let video = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("video"))
        .ok_or_else(|| "该文件不包含视频轨道".to_string())?;

    let audio_codec = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("audio"))
        .and_then(|s| s.get("codec_name").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    let width = video.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let height = video.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let fps = video
        .get("avg_frame_rate")
        .and_then(|v| v.as_str())
        .map(parse_rate)
        .filter(|f| *f > 0.0)
        .unwrap_or(0.0);

    let duration_ms = root
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            video
                .get("duration")
                .and_then(|d| d.as_str())
                .and_then(|d| d.parse::<f64>().ok())
        })
        .map(|secs| (secs * 1000.0).round() as u64)
        .unwrap_or(0);

    if duration_ms == 0 {
        return Err("视频时长无法识别（可能是纯图片序列或文件损坏）".into());
    }

    let bitrate = root
        .get("format")
        .and_then(|f| f.get("bit_rate"))
        .and_then(|b| b.as_str())
        .and_then(|b| b.parse::<u64>().ok())
        .unwrap_or(0);

    let file_name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let real_size = root
        .get("format")
        .and_then(|f| f.get("size"))
        .and_then(|s| s.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(size_bytes);

    Ok(VideoMeta {
        path: path.to_string(),
        file_name,
        size_bytes: real_size,
        duration_ms,
        width,
        height,
        fps,
        video_codec: video
            .get("codec_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        audio_codec,
        bitrate,
    })
}

pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join(" | ")
}

/// 执行 ffmpeg 并解析 `-progress pipe:1` 输出，向前端推送进度事件。
#[allow(clippy::too_many_arguments)]
pub fn run_progress(
    app: &AppHandle,
    state: &AppState,
    job: &str,
    program: &Path,
    args: &[String],
    duration_ms: f64,
    base_percent: f64,
    span_percent: f64,
    label: &str,
) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 ffmpeg 失败: {e}"))?;

    let stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stderr {
            let _ = s.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).to_string()
    });

    let stdout = child.stdout.take();
    state.jobs.lock().unwrap().insert(job.to_string(), child);

    let emit = |percent: f64| {
        let _ = app.emit(
            "job-progress",
            ProgressPayload {
                job: job.to_string(),
                percent: percent.clamp(0.0, 100.0),
                message: label.to_string(),
            },
        );
    };
    emit(base_percent);

    let mut last_emitted = base_percent;
    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        let mut current_us: f64 = -1.0;
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if let Some(v) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = v.parse::<f64>() {
                    current_us = us;
                }
            } else if let Some(v) = line.strip_prefix("out_time_ms=") {
                if current_us < 0.0 {
                    if let Ok(us) = v.parse::<f64>() {
                        current_us = us;
                    }
                }
            } else if line == "progress=continue" || line == "progress=end" {
                let frac = if duration_ms > 0.0 {
                    (current_us / 1000.0 / duration_ms).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let pct = base_percent + span_percent * frac;
                if pct - last_emitted >= 0.6 {
                    last_emitted = pct;
                    emit(pct);
                }
                current_us = -1.0;
            }
        }
    }

    let mut child = match state.jobs.lock().unwrap().remove(job) {
        Some(c) => c,
        None => {
            let _ = err_thread.join();
            return Err("任务已取消".into());
        }
    };

    let status = child
        .wait()
        .map_err(|e| format!("等待 ffmpeg 结束失败: {e}"))?;
    let stderr_text = err_thread.join().unwrap_or_default();

    if !status.success() {
        return Err(if stderr_text.trim().is_empty() {
            "ffmpeg 执行失败（没有输出错误信息）".into()
        } else {
            format!("ffmpeg 执行失败：{}", tail(&stderr_text, 3))
        });
    }

    emit(base_percent + span_percent);
    Ok(())
}
