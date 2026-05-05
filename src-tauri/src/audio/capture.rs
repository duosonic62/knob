use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavSpec, WavWriter};
use screencapturekit::prelude::*;

use super::AppInfo;

pub struct CaptureSession {
    stream: SCStream,
    pub wav_path: String,
    writer: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>,
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

pub fn start_capture(
    bundle_id: &str,
    state: &super::CaptureState,
) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    if session.is_some() {
        return Err("Capture already running. Stop it first.".to_string());
    }

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

            // Collect per-channel f32 slices (ScreenCaptureKit delivers non-interleaved PCM)
            let channels: Vec<&[f32]> = buf_list
                .iter()
                .filter_map(|ab| {
                    let bytes = ab.data();
                    if bytes.is_empty() || bytes.len() % 4 != 0 {
                        return None;
                    }
                    // Safety: ScreenCaptureKit guarantees 4-byte aligned float32 PCM data
                    Some(unsafe {
                        std::slice::from_raw_parts(
                            bytes.as_ptr().cast::<f32>(),
                            bytes.len() / 4,
                        )
                    })
                })
                .collect();

            if channels.is_empty() {
                return;
            }

            let count = callback_count_for_handler.fetch_add(1, Ordering::Relaxed);
            if count % 50 == 0 {
                let all_samples: Vec<f32> = channels.iter().flat_map(|c| c.iter().copied()).collect();
                let rms = {
                    let sum_sq: f64 = all_samples.iter().map(|&s| (s as f64).powi(2)).sum();
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
    log::info!("[knob] capture started for '{}'", bundle_id);

    *session = Some(CaptureSession {
        stream,
        wav_path,
        writer,
    });
    Ok(())
}

pub fn stop_capture(state: &super::CaptureState) -> Result<String, String> {
    let mut session = state.session.lock().unwrap();
    let sess = session.take().ok_or("No capture running")?;

    sess.stream.stop_capture().map_err(|e| e.to_string())?;

    // Finalize WAV after stream is stopped (no more callbacks)
    let mut guard = sess.writer.lock().unwrap();
    if let Some(wav) = guard.take() {
        wav.finalize().map_err(|e| e.to_string())?;
    }

    log::info!("[knob] capture stopped. WAV saved to {}", sess.wav_path);
    Ok(sess.wav_path)
}
