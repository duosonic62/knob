mod audio;
mod settings;

use audio::{AppInfo, AudioDeviceInfo, BlackHoleStatus, CaptureState, RoutingState};
use settings::{Settings, SettingsState};
use tauri::{Manager, State};

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
    app: tauri::AppHandle,
    bundle_id: String,
    volume: f32,
    muted: bool,
    cap: State<'_, CaptureState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    audio::capture::start_routing(&bundle_id, volume, muted, &cap, &rt, app)
}

#[tauri::command]
async fn stop_routing(
    cap: State<'_, CaptureState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    audio::capture::stop_routing(&cap, &rt)
}

#[tauri::command]
async fn get_settings(s: State<'_, SettingsState>) -> Result<Settings, String> {
    Ok(s.inner.lock().clone())
}

#[tauri::command]
async fn set_app_settings(
    bundle_id: String,
    volume: f32,
    muted: bool,
    s: State<'_, SettingsState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    let volume = volume.clamp(0.0, 1.0);

    let snapshot = {
        let mut g = s.inner.lock();
        g.apps.insert(bundle_id.clone(), settings::AppSettings { volume, muted });
        g.clone()
    };
    settings::save(&snapshot, &s.path)?;

    let active = rt.active.lock();
    if let Some((rb, h)) = active.as_ref() {
        if rb == &bundle_id {
            audio::router::set_gain(h, volume);
            audio::router::set_muted(h, muted);
        }
    }
    Ok(())
}

#[tauri::command]
async fn set_master_volume(
    volume: f32,
    muted: bool,
    s: State<'_, SettingsState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    let volume = volume.clamp(0.0, 1.0);

    let snapshot = {
        let mut g = s.inner.lock();
        g.master = settings::MasterSettings { volume, muted };
        g.clone()
    };
    settings::save(&snapshot, &s.path)?;

    rt.master_gain_bits
        .store(volume.to_bits(), std::sync::atomic::Ordering::Relaxed);
    rt.master_muted
        .store(muted, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CaptureState::default())
        .manage(RoutingState::default())
        .setup(|app| {
            let dir = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("app_config_dir failed: {}", e))?;
            let path = dir.join("settings.json");
            let loaded = settings::load(&path);
            let rt = app.state::<RoutingState>();
            rt.master_gain_bits.store(
                loaded.master.volume.to_bits(),
                std::sync::atomic::Ordering::Relaxed,
            );
            rt.master_muted
                .store(loaded.master.muted, std::sync::atomic::Ordering::Relaxed);
            app.manage(SettingsState {
                inner: parking_lot::Mutex::new(loaded),
                path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_apps,
            start_capture,
            stop_capture,
            list_audio_output_devices,
            check_blackhole,
            start_routing,
            stop_routing,
            get_settings,
            set_app_settings,
            set_master_volume,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
