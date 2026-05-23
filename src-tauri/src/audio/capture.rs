use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use screencapturekit::prelude::*;
use screencapturekit::stream::delegate_trait::StreamCallbacks;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "macos")]
extern crate libc;

use super::AppInfo;
use super::router::{self, SampleProducer};
use crate::settings::SettingsState;

pub struct CaptureSession {
    stream: SCStream,
}

pub fn list_audio_apps() -> Result<Vec<AppInfo>, String> {
    let content = SCShareableContent::get().map_err(|e| e.to_string())?;
    let apps = content
        .applications()
        .into_iter()
        .map(|a| AppInfo {
            bundle_id: a.bundle_identifier(),
            name: a.application_name(),
            pid: a.process_id(),
        })
        .collect();
    Ok(apps)
}

fn build_filter_and_config(
    bundle_id: &str,
) -> Result<(SCContentFilter, SCStreamConfiguration), String> {
    let content = SCShareableContent::get().map_err(|e| e.to_string())?;
    let displays = content.displays();
    let display = displays.first().ok_or("No display available")?;
    let apps = content.applications();
    let target_app = apps
        .iter()
        .find(|a| a.bundle_identifier() == bundle_id)
        .ok_or_else(|| format!("App '{}' not found in running applications", bundle_id))?;

    let filter = SCContentFilter::create()
        .with_display(display)
        .with_including_applications(&[target_app], &[])
        .build();

    let config = SCStreamConfiguration::new()
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(48000)
        .with_channel_count(2)
        .with_width(2)
        .with_height(2);

    Ok((filter, config))
}

fn start_exit_watch(app: AppHandle, bundle_id: String, pid: i32) {
    std::thread::Builder::new()
        .name(format!("exit-watch-{}", bundle_id))
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));

                {
                    let rt = app.state::<super::RoutingState>();
                    if !rt.sources.lock().contains_key(&bundle_id) {
                        break;
                    }
                }

                let process_exists = unsafe { libc::kill(pid, 0) == 0 };
                if !process_exists {
                    log::info!("[knob] process exit detected: bundle={} pid={}", bundle_id, pid);
                    handle_sck_termination(app.clone(), bundle_id.clone());
                    break;
                }
            }
        })
        .ok();
}

fn handle_sck_termination(app: AppHandle, bundle_id: String) {
    tauri::async_runtime::spawn(async move {
        remove_route_internal(&bundle_id, app.clone());
        if let Err(e) = app.emit("routing-stopped", &bundle_id) {
            log::warn!("[knob] emit routing-stopped failed: {}", e);
        }
        log::info!("[knob] routing auto-stopped for '{}'", bundle_id);
    });
}

/// Shared teardown: stop SCK → update snapshot → close IOProc if last.
fn remove_route_internal(bundle_id: &str, app: AppHandle) {
    let cap = app.state::<super::CaptureState>();
    let rt = app.state::<super::RoutingState>();

    // 1. Take the SCK session for this bundle (no lock held while stopping)
    let session = cap.sessions.lock().remove(bundle_id);

    // 2. Stop SCK so producer stops pushing before consumer is removed
    if let Some(sess) = session {
        if let Err(e) = sess.stream.stop_capture() {
            log::warn!("[knob] stop_capture for '{}' failed: {}", bundle_id, e);
        }
    }

    // 3. Remove SourceEntry from map
    let removed_entry = rt.sources.lock().remove(bundle_id);

    // 4. Update snapshot: rebuild Vec excluding this source
    if let Some(entry) = removed_entry {
        let ptr = Arc::as_ptr(&entry.mix_source);
        let new_vec: Vec<_> = rt
            .mix_snapshot
            .load()
            .iter()
            .filter(|a| Arc::as_ptr(a) != ptr)
            .cloned()
            .collect();
        let sources_empty = new_vec.is_empty();
        rt.mix_snapshot.store(Arc::new(new_vec));

        // 5. Close IOProc when last source is removed
        if sources_empty {
            if let Some(h) = rt.io_proc.lock().take() {
                if let Err(e) = router::close_io_proc(h) {
                    log::warn!("[knob] close_io_proc after '{}' failed: {}", bundle_id, e);
                }
            }
        }
    }

    // Persist routed set
    if let Some(ss) = app.try_state::<SettingsState>() {
        let snapshot = {
            let mut g = ss.inner.lock();
            g.routed.retain(|b| b != bundle_id);
            g.clone()
        };
        if let Err(e) = crate::settings::save(&snapshot, &ss.path) {
            log::warn!("[knob] remove_route: failed to save settings: {}", e);
        }
    }

    log::info!(
        "[knob] remove_route: '{}' done, sources_remaining={}",
        bundle_id,
        rt.sources.lock().len()
    );
}

#[allow(clippy::too_many_arguments)]
fn build_stream_route(
    bundle_id: &str,
    producer: SampleProducer,
    gain_bits: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    master_gain_bits: Arc<AtomicU32>,
    master_muted: Arc<AtomicBool>,
    overrun_count: Arc<AtomicU64>,
    app: AppHandle,
    pid: i32,
) -> Result<SCStream, String> {
    let (filter, config) = build_filter_and_config(bundle_id)?;

    let app_for_stop = app.clone();
    let bid_for_stop = bundle_id.to_string();
    let app_for_err = app.clone();
    let bid_for_err = bundle_id.to_string();
    let app_for_inactive = app.clone();
    let bid_for_inactive = bundle_id.to_string();
    let delegate = StreamCallbacks::new()
        .on_stop(move |err| {
            log::info!("[knob] SCK stream_did_stop bundle={} err={:?}", bid_for_stop, err);
            handle_sck_termination(app_for_stop.clone(), bid_for_stop.clone());
        })
        .on_error(move |err| {
            log::warn!("[knob] SCK did_stop_with_error bundle={} err={}", bid_for_err, err);
            handle_sck_termination(app_for_err.clone(), bid_for_err.clone());
        })
        .on_inactive(move || {
            log::info!("[knob] SCK stream became inactive bundle={}", bid_for_inactive);
            handle_sck_termination(app_for_inactive.clone(), bid_for_inactive.clone());
        });

    let g_arc = gain_bits;
    let m_arc = muted;
    let mg_arc = master_gain_bits;
    let mm_arc = master_muted;
    let oc = overrun_count;
    let app_for_meter = app.clone();
    let bid_for_meter = bundle_id.to_string();
    let meter_count = AtomicU64::new(0);
    let producer = parking_lot::Mutex::new(producer);

    start_exit_watch(app.clone(), bundle_id.to_string(), pid);

    let mut stream = SCStream::new_with_delegate(&filter, &config, delegate);
    stream.add_output_handler(
        move |sample: CMSampleBuffer, of_type: SCStreamOutputType| {
            if of_type != SCStreamOutputType::Audio {
                return;
            }
            let Some(buf_list) = sample.audio_buffer_list() else {
                return;
            };

            let channels: Vec<&[f32]> = buf_list
                .iter()
                .filter_map(|ab| {
                    let bytes = ab.data();
                    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
                        return None;
                    }
                    Some(unsafe {
                        std::slice::from_raw_parts(bytes.as_ptr().cast::<f32>(), bytes.len() / 4)
                    })
                })
                .collect();

            if channels.is_empty() {
                return;
            }

            let n = channels[0].len();
            let g_raw = f32::from_bits(g_arc.load(Ordering::Relaxed));
            let mg = f32::from_bits(mg_arc.load(Ordering::Relaxed));
            let any_muted = m_arc.load(Ordering::Relaxed) || mm_arc.load(Ordering::Relaxed);
            let g = if any_muted { 0.0 } else { g_raw * mg };

            let mut scratch: Vec<f32> = Vec::with_capacity(n * 2);
            for i in 0..n {
                let l = channels[0].get(i).copied().unwrap_or(0.0) * g;
                let r = channels.get(1).and_then(|c| c.get(i)).copied().unwrap_or(l);
                scratch.push(l);
                scratch.push(r * g);
            }

            let pushed = ringbuf::traits::Producer::push_slice(&mut *producer.lock(), &scratch);
            if pushed < scratch.len() {
                let prev = oc.fetch_add(1, Ordering::Relaxed);
                if prev.is_multiple_of(50) {
                    log::warn!(
                        "[knob] ring overrun: dropped {} samples",
                        scratch.len() - pushed
                    );
                }
            }

            let mc = meter_count.fetch_add(1, Ordering::Relaxed);
            if mc.is_multiple_of(10) {
                let sum_sq: f64 = scratch.iter().map(|&s| (s as f64).powi(2)).sum();
                let rms = if scratch.is_empty() {
                    0.0
                } else {
                    (sum_sq / scratch.len() as f64).sqrt() as f32
                };
                let payload = super::AudioLevel {
                    bundle_id: bid_for_meter.clone(),
                    rms,
                };
                if let Err(e) = app_for_meter.emit("audio-level", &payload) {
                    log::warn!("[knob] emit audio-level failed: {}", e);
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    Ok(stream)
}

/// Add a new route for bundle_id. Multiple concurrent routes are supported.
pub fn add_route(
    bundle_id: &str,
    initial_volume: f32,
    initial_muted: bool,
    cap: &super::CaptureState,
    rt: &super::RoutingState,
    app: AppHandle,
) -> Result<(), String> {
    // Reject duplicate routes for the same bundle
    if rt.sources.lock().contains_key(bundle_id) {
        return Err(format!("'{}' is already being routed", bundle_id));
    }

    // Get target app PID
    let pid = {
        let content = SCShareableContent::get().map_err(|e| e.to_string())?;
        content
            .applications()
            .into_iter()
            .find(|a| a.bundle_identifier() == bundle_id)
            .map(|a| a.process_id())
            .ok_or_else(|| format!("App '{}' not found", bundle_id))?
    };

    // Get BlackHole device id
    let bh = super::devices::check_blackhole()?;
    if !bh.installed {
        return Err("BlackHole is not installed".to_string());
    }
    let device_id = bh.devices[0].id;

    // Open IOProc on first route
    let is_first = rt.io_proc.lock().is_none();
    if is_first {
        let io_proc = router::open_io_proc(device_id, rt.mix_snapshot.clone())?;
        *rt.io_proc.lock() = Some(io_proc);
    }

    // Query device sample rate; build per-source ring + optional resampler
    let rate = router::query_device_sample_rate(device_id).unwrap_or(48000.0);
    let (mix_source, producer) = match router::build_source(rate) {
        Ok(v) => v,
        Err(e) => {
            if is_first {
                if let Some(h) = rt.io_proc.lock().take() {
                    let _ = router::close_io_proc(h);
                }
            }
            return Err(e);
        }
    };

    // Register source in snapshot BEFORE starting SCK capture
    {
        let mut v = rt.mix_snapshot.load().as_ref().clone();
        v.push(mix_source.clone());
        rt.mix_snapshot.store(Arc::new(v));
    }

    let gain_bits = Arc::new(AtomicU32::new(initial_volume.to_bits()));
    let muted_arc = Arc::new(AtomicBool::new(initial_muted));
    let overrun_count = Arc::new(AtomicU64::new(0));

    let stream = match build_stream_route(
        bundle_id,
        producer,
        gain_bits.clone(),
        muted_arc.clone(),
        rt.master_gain_bits.clone(),
        rt.master_muted.clone(),
        overrun_count,
        app.clone(),
        pid,
    ) {
        Ok(s) => s,
        Err(e) => {
            rollback_snapshot(rt, &mix_source, is_first);
            return Err(e);
        }
    };

    match stream.start_capture() {
        Ok(_) => {}
        Err(e) => {
            rollback_snapshot(rt, &mix_source, is_first);
            return Err(e.to_string());
        }
    }

    cap.sessions.lock().insert(
        bundle_id.to_string(),
        CaptureSession { stream },
    );
    rt.sources.lock().insert(
        bundle_id.to_string(),
        super::SourceEntry { gain_bits, muted: muted_arc, mix_source },
    );

    // Persist routed set
    if let Some(ss) = app.try_state::<SettingsState>() {
        let snapshot = {
            let mut g = ss.inner.lock();
            if !g.routed.contains(&bundle_id.to_string()) {
                g.routed.push(bundle_id.to_string());
            }
            g.clone()
        };
        if let Err(e) = crate::settings::save(&snapshot, &ss.path) {
            log::warn!("[knob] add_route: failed to save settings: {}", e);
        }
    }

    log::info!(
        "[knob] add_route: '{}' started, sources_total={}",
        bundle_id,
        rt.sources.lock().len()
    );

    Ok(())
}

/// Remove a route by bundle_id. No-op if not active.
pub fn remove_route(
    bundle_id: &str,
    _cap: &super::CaptureState,
    _rt: &super::RoutingState,
    app: AppHandle,
) -> Result<(), String> {
    remove_route_internal(bundle_id, app);
    Ok(())
}

fn rollback_snapshot(
    rt: &super::RoutingState,
    mix_source: &Arc<super::router::MixSource>,
    was_first: bool,
) {
    let ptr = Arc::as_ptr(mix_source);
    let new_vec: Vec<_> = rt
        .mix_snapshot
        .load()
        .iter()
        .filter(|a| Arc::as_ptr(a) != ptr)
        .cloned()
        .collect();
    let now_empty = new_vec.is_empty();
    rt.mix_snapshot.store(Arc::new(new_vec));
    if was_first && now_empty {
        if let Some(h) = rt.io_proc.lock().take() {
            let _ = router::close_io_proc(h);
        }
    }
}
