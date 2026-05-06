mod audio;

use audio::{AppInfo, AudioDeviceInfo, BlackHoleStatus, CaptureState, RoutingState};
use tauri::State;

#[tauri::command]
async fn list_audio_apps(state: State<'_, CaptureState>) -> Result<Vec<AppInfo>, String> {
    let _ = state;
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

#[tauri::command]
async fn list_audio_output_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    audio::devices::list_output_devices()
}

#[tauri::command]
async fn check_blackhole() -> Result<BlackHoleStatus, String> {
    audio::devices::check_blackhole()
}

#[tauri::command]
async fn start_routing(
    bundle_id: String,
    volume: f32,
    muted: bool,
    cap: State<'_, CaptureState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    audio::capture::start_routing(&bundle_id, volume, muted, &cap, &rt)
}

#[tauri::command]
async fn stop_routing(
    cap: State<'_, CaptureState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    audio::capture::stop_routing(&cap, &rt)
}

#[tauri::command]
async fn set_routing_volume(volume: f32, rt: State<'_, RoutingState>) -> Result<(), String> {
    let handle = rt.handle.lock();
    match &*handle {
        Some(h) => {
            audio::router::set_gain(h, volume);
            Ok(())
        }
        None => Err("No routing active".to_string()),
    }
}

#[tauri::command]
async fn set_routing_mute(muted: bool, rt: State<'_, RoutingState>) -> Result<(), String> {
    let handle = rt.handle.lock();
    match &*handle {
        Some(h) => {
            audio::router::set_muted(h, muted);
            Ok(())
        }
        None => Err("No routing active".to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CaptureState::default())
        .manage(RoutingState::default())
        .invoke_handler(tauri::generate_handler![
            list_audio_apps,
            start_capture,
            stop_capture,
            list_audio_output_devices,
            check_blackhole,
            start_routing,
            stop_routing,
            set_routing_volume,
            set_routing_mute,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
