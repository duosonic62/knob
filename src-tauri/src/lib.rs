mod audio;
mod settings;

use audio::{
    AppInfo, AudioDeviceInfo, BlackHoleStatus, CaptureState, DriverIpcState, DriverStatus,
    RoutingState,
};
use settings::{Settings, SettingsState};
use tauri::{Manager, State};

#[tauri::command]
async fn list_audio_apps() -> Result<Vec<AppInfo>, String> {
    audio::capture::list_audio_apps()
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
    audio::capture::add_route(&bundle_id, volume, muted, &cap, &rt, app)
}

#[tauri::command]
async fn stop_routing(
    app: tauri::AppHandle,
    bundle_id: String,
    cap: State<'_, CaptureState>,
    rt: State<'_, RoutingState>,
) -> Result<(), String> {
    audio::capture::remove_route(&bundle_id, &cap, &rt, app)
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

    let sources = rt.sources.lock();
    if let Some(entry) = sources.get(&bundle_id) {
        entry.gain_bits.store(volume.to_bits(), std::sync::atomic::Ordering::Relaxed);
        entry.muted.store(muted, std::sync::atomic::Ordering::Relaxed);
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

/// Debug-only (#31 PoC): feed a sine tone to the Knob driver via shared memory so we can verify the
/// host→driver IPC path end-to-end (record the Knob input and confirm the tone).
#[tauri::command]
async fn debug_driver_sine(
    enable: bool,
    freq: f32,
    ipc: State<'_, DriverIpcState>,
) -> Result<(), String> {
    ipc.set_sine(enable, freq)
}

/// Debug-only (#31 PoC): returns true if the driver currently has an active host-override mapping
/// (i.e. it successfully opened the host's shared memory). Used to tell a sandbox/shm failure apart
/// from a data-plane bug.
#[tauri::command]
async fn debug_driver_status(ipc: State<'_, DriverIpcState>) -> Result<DriverStatus, String> {
    ipc.status()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CaptureState::default())
        .manage(RoutingState::default())
        .manage(DriverIpcState::default())
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

            let routed = loaded.routed.clone();
            app.manage(SettingsState {
                inner: parking_lot::Mutex::new(loaded),
                path,
            });

            // Restore previously routed apps after SettingsState is managed
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                for bundle_id in routed {
                    let cap = app_handle.state::<CaptureState>();
                    let rt = app_handle.state::<RoutingState>();
                    let ss = app_handle.state::<settings::SettingsState>();
                    let vol = ss.inner.lock().apps
                        .get(&bundle_id)
                        .map(|a| a.volume)
                        .unwrap_or(0.75);
                    let muted = ss.inner.lock().apps
                        .get(&bundle_id)
                        .map(|a| a.muted)
                        .unwrap_or(false);
                    if let Err(e) = audio::capture::add_route(
                        &bundle_id, vol, muted, &cap, &rt, app_handle.clone(),
                    ) {
                        log::warn!("[knob] restore route '{}' failed: {}", bundle_id, e);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_apps,
            list_audio_output_devices,
            check_blackhole,
            start_routing,
            stop_routing,
            get_settings,
            set_app_settings,
            set_master_volume,
            debug_driver_sine,
            debug_driver_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
