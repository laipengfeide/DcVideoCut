mod auth;
mod commands;
mod ffmpeg;
mod models;

use std::fs;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            fs::create_dir_all(&cache_dir)?;

            let db = auth::open_database(&data_dir)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let resource_dir = app.path().resource_dir().ok();

            app.manage(commands::AppState::new(db, cache_dir, exe_dir, resource_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_bootstrap,
            commands::auth_register,
            commands::auth_login,
            commands::auth_guest,
            commands::auth_logout,
            commands::remember_out_dir,
            commands::cancel_job,
            commands::reveal_path,
            commands::probe_video,
            commands::extract_frames,
            commands::timeline_thumbs,
            commands::save_frames,
            commands::export_segments,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Dc Video Cut 时发生错误");
}
