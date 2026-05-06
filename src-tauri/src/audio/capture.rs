use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavSpec, WavWriter};
use screencapturekit::prelude::*;

use super::AppInfo;
use super::router::{self, SampleProducer};

pub enum CaptureSink {
    Wav {
        path: String,
        writer: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>,
    },
    Route {
        gain_bits: Arc<AtomicU32>,
        muted: Arc<AtomicBool>,
        overrun_count: Arc<AtomicU64>,
    },
}

pub struct CaptureSession {
    stream: SCStream,
    pub sink: CaptureSink,
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

pub fn start_capture(bundle_id: &str, state: &super::CaptureState) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    if session.is_some() {
        return Err("Capture already running. Stop it first.".to_string());
    }

    let (filter, config) = build_filter_and_config(bundle_id)?;

    let safe_name = bundle_id.replace(['/', '.', ' '], "_");
    let wav_path = format!("/tmp/knob_poc_{}.wav", safe_name);

    let spec = WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let wav = WavWriter::create(&wav_path, spec).map_err(|e| e.to_string())?;
    let writer: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>> =
        Arc::new(Mutex::new(Some(wav)));
    let writer_for_handler = writer.clone();

    let callback_count = Arc::new(AtomicU64::new(0));
    let callback_count_for_handler = callback_count.clone();

    let mut stream = SCStream::new(&filter, &config);
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
                    if bytes.is_empty() || bytes.len() % 4 != 0 {
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

            let count = callback_count_for_handler.fetch_add(1, Ordering::Relaxed);
            if count % 50 == 0 {
                let all_samples: Vec<f32> =
                    channels.iter().flat_map(|c| c.iter().copied()).collect();
                let rms = {
                    let sum_sq: f64 =
                        all_samples.iter().map(|&s| (s as f64).powi(2)).sum();
                    (sum_sq / all_samples.len() as f64).sqrt()
                };
                log::info!(
                    "[knob] audio RMS={:.6}  frames={} buffers={}",
                    rms,
                    channels[0].len(),
                    channels.len()
                );
            }

            if let Ok(mut guard) = writer_for_handler.lock() {
                if let Some(ref mut wav) = *guard {
                    let frame_count = channels[0].len();
                    for frame in 0..frame_count {
                        for ch in &channels {
                            if frame < ch.len() {
                                let _ = wav.write_sample(ch[frame]);
                            }
                        }
                    }
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    stream.start_capture().map_err(|e| e.to_string())?;
    log::info!("[knob] wav capture started for '{}'", bundle_id);

    *session = Some(CaptureSession {
        stream,
        sink: CaptureSink::Wav {
            path: wav_path,
            writer,
        },
    });
    Ok(())
}

pub fn stop_capture(state: &super::CaptureState) -> Result<String, String> {
    let mut session = state.session.lock().unwrap();
    let sess = session.take().ok_or("No capture running")?;

    sess.stream.stop_capture().map_err(|e| e.to_string())?;

    let path = match sess.sink {
        CaptureSink::Wav { path, writer } => {
            let mut guard = writer.lock().unwrap();
            if let Some(wav) = guard.take() {
                wav.finalize().map_err(|e| e.to_string())?;
            }
            path
        }
        CaptureSink::Route { .. } => String::new(),
    };

    log::info!("[knob] capture stopped. WAV saved to {}", path);
    Ok(path)
}

fn build_stream_route(
    bundle_id: &str,
    producer: SampleProducer,
    gain_bits: Arc<AtomicU32>,
    muted: Arc<AtomicBool>,
    overrun_count: Arc<AtomicU64>,
) -> Result<SCStream, String> {
    let (filter, config) = build_filter_and_config(bundle_id)?;

    let g_arc = gain_bits;
    let m_arc = muted;
    let oc = overrun_count;
    // SCK requires Fn (not FnMut); wrap producer in Mutex for interior mutability.
    // SCK calls from a single dispatch queue so this is always uncontended.
    let producer = parking_lot::Mutex::new(producer);

    let mut stream = SCStream::new(&filter, &config);
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
                    if bytes.is_empty() || bytes.len() % 4 != 0 {
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
            let g = if m_arc.load(Ordering::Relaxed) { 0.0 } else { g_raw };

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
                if prev % 50 == 0 {
                    log::warn!(
                        "[knob] ring overrun: dropped {} samples",
                        scratch.len() - pushed
                    );
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    Ok(stream)
}

pub fn start_routing(
    bundle_id: &str,
    initial_volume: f32,
    initial_muted: bool,
    cap: &super::CaptureState,
    rt: &super::RoutingState,
) -> Result<(), String> {
    let mut session = cap.session.lock().unwrap();
    if session.is_some() {
        return Err("Capture already running. Stop it first.".to_string());
    }
    let mut handle_lock = rt.handle.lock();
    if handle_lock.is_some() {
        return Err("Routing already active. Stop it first.".to_string());
    }

    // Get BlackHole device id
    let bh = super::devices::check_blackhole()?;
    if !bh.installed {
        return Err("BlackHole is not installed".to_string());
    }
    let device_id = bh.devices[0].id;

    // Open HAL IOProc route
    let (mut handle, producer) = router::open_blackhole_route(device_id)?;

    // Apply initial volume/mute before SCK starts (prevents audio jump race)
    router::set_gain(&handle, initial_volume);
    router::set_muted(&handle, initial_muted);

    let gain_bits = handle.gain_bits.clone();
    let muted = handle.muted.clone();
    let overrun_count = Arc::new(AtomicU64::new(0));

    let stream = match build_stream_route(
        bundle_id,
        producer,
        gain_bits.clone(),
        muted.clone(),
        overrun_count.clone(),
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = router::close_route(handle);
            return Err(e);
        }
    };

    match stream.start_capture() {
        Ok(_) => {}
        Err(e) => {
            let _ = router::close_route(handle);
            return Err(e.to_string());
        }
    }

    log::info!("[knob] routing started for '{}'", bundle_id);

    *session = Some(CaptureSession {
        stream,
        sink: CaptureSink::Route {
            gain_bits,
            muted,
            overrun_count,
        },
    });
    *handle_lock = Some(handle);
    Ok(())
}

pub fn stop_routing(
    cap: &super::CaptureState,
    rt: &super::RoutingState,
) -> Result<(), String> {
    let mut session = cap.session.lock().unwrap();
    let sess = session.take().ok_or("No routing active")?;

    // Stop SCK first so producer stops pushing before IOProc consumer is torn down
    sess.stream.stop_capture().map_err(|e| e.to_string())?;

    let mut handle_lock = rt.handle.lock();
    if let Some(handle) = handle_lock.take() {
        router::close_route(handle)?;
    }

    log::info!("[knob] routing stopped");
    Ok(())
}
