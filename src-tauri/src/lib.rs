mod audio;

use audio::{AppInfo, CaptureState};
use tauri::State;

#[tauri::command]
async fn list_audio_apps(state: State<'_, CaptureState>) -> Result<Vec<AppInfo>, String> {
    let _ = state; // CaptureState not needed for listing, but keeps command signature consistent
    audio::capture::list_audio_apps()
}

#[tauri::command]
async fn start_capture(bundle_id: String, state: State<'_, CaptureState>) -> Result<(), String> {
    audio::capture::start_capture(&bundle_id, &state)
}

#[tauri::command]
async fn stop_capture(state: State<'_, CaptureState>) -> Result<String, String> {
    audio::capture::stop_capture(&state)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CaptureState::default())
        .invoke_handler(tauri::generate_handler![
            list_audio_apps,
            start_capture,
            stop_capture,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
