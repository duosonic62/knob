use std::ffi::c_void;
use std::sync::Arc;

use arc_swap::ArcSwap;
use coreaudio_sys::{
    kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyStreams,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput,
    kAudioStreamPropertyVirtualFormat,
    AudioBufferList,
    AudioDeviceCreateIOProcID,
    AudioDeviceDestroyIOProcID,
    AudioDeviceID,
    AudioDeviceIOProcID,
    AudioDeviceStart,
    AudioDeviceStop,
    AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize,
    AudioObjectID,
    AudioStreamBasicDescription,
    AudioStreamID,
    AudioTimeStamp,
    OSStatus,
};
use parking_lot::Mutex;
use ringbuf::traits::Split;
use ringbuf::{HeapProd, HeapRb};

use super::resampler::StereoResampler;

const ELEMENT_MAIN: u32 = 0;

pub type SampleProducer = HeapProd<f32>;

/// Audio source owned by IOProc. Mutex は IOProc スレッドからしか触らないため競合不可。
pub struct MixSource {
    pub consumer: Mutex<ringbuf::HeapCons<f32>>,
    pub resampler: Mutex<Option<StereoResampler>>,
}

struct IoProcCtx {
    snapshot: Arc<ArcSwap<Vec<Arc<MixSource>>>>,
    interleaved: bool,
    mix_scratch: Vec<f32>,
    out_scratch: Vec<f32>,
}

pub struct IoProcHandle {
    device_id: AudioDeviceID,
    proc_id: AudioDeviceIOProcID,
    _ctx: Box<IoProcCtx>,
}

unsafe impl Send for IoProcHandle {}
unsafe impl Sync for IoProcHandle {}

// -------------------------------------------------------
// Public API — new multi-source
// -------------------------------------------------------

/// Get the nominal sample rate of a CoreAudio device.
pub fn query_device_sample_rate(device_id: AudioDeviceID) -> Result<f64, String> {
    unsafe {
        let addr = coreaudio_sys::AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeOutput,
            mElement: ELEMENT_MAIN,
        };
        let mut rate: f64 = 0.0;
        let mut size = std::mem::size_of::<f64>() as u32;
        let status = AudioObjectGetPropertyData(
            device_id,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            &mut rate as *mut f64 as *mut c_void,
        );
        if status != 0 {
            return Err(format!("CoreAudio error reading sample rate: {}", status));
        }
        Ok(rate)
    }
}

/// Build a per-source ring buffer + optional resampler.
/// Returns (MixSource for IOProc snapshot, SampleProducer for SCK push).
pub fn build_source(device_sample_rate: f64) -> Result<(Arc<MixSource>, SampleProducer), String> {
    const SCK_INPUT_RATE: f64 = 48000.0;
    const RESAMPLER_OUTPUT_CHUNK: usize = 1024;
    const RING_SIZE: usize = 16384;

    let resampler = if (device_sample_rate - SCK_INPUT_RATE).abs() > 0.5 {
        log::info!(
            "[knob] router: {} Hz → {} Hz, resampler=enabled",
            SCK_INPUT_RATE as u32,
            device_sample_rate as u32
        );
        Some(
            StereoResampler::new(SCK_INPUT_RATE, device_sample_rate, RESAMPLER_OUTPUT_CHUNK)
                .map_err(|e| format!("resampler init failed: {}", e))?,
        )
    } else {
        None
    };

    let rb = HeapRb::<f32>::new(RING_SIZE);
    let (prod, cons) = rb.split();

    let src = Arc::new(MixSource {
        consumer: Mutex::new(cons),
        resampler: Mutex::new(resampler),
    });
    Ok((src, prod))
}

/// Open a single IOProc against `device_id`. The IOProc fires continuously;
/// it mixes whatever is currently in `snapshot`.
pub fn open_io_proc(
    device_id: AudioDeviceID,
    snapshot: Arc<ArcSwap<Vec<Arc<MixSource>>>>,
) -> Result<IoProcHandle, String> {
    unsafe {
        let interleaved = query_interleaved(device_id)?;

        let ctx = Box::new(IoProcCtx {
            snapshot,
            interleaved,
            mix_scratch: vec![0f32; 4096],
            out_scratch: vec![0f32; 4096],
        });
        let ctx_ptr = &*ctx as *const IoProcCtx as *mut c_void;

        let mut proc_id: AudioDeviceIOProcID = None;
        let status =
            AudioDeviceCreateIOProcID(device_id, Some(io_proc_trampoline), ctx_ptr, &mut proc_id);
        if status != 0 {
            return Err(format!("AudioDeviceCreateIOProcID failed: {}", status));
        }

        let status = AudioDeviceStart(device_id, proc_id);
        if status != 0 {
            AudioDeviceDestroyIOProcID(device_id, proc_id);
            return Err(format!("AudioDeviceStart failed: {}", status));
        }

        log::info!("[knob] router: IOProc opened on device {}", device_id);

        Ok(IoProcHandle { device_id, proc_id, _ctx: ctx })
    }
}

/// Stop and destroy the IOProc. Must be called only after all SCK streams are stopped.
pub fn close_io_proc(h: IoProcHandle) -> Result<(), String> {
    unsafe {
        let stop = AudioDeviceStop(h.device_id, h.proc_id);
        let destroy = AudioDeviceDestroyIOProcID(h.device_id, h.proc_id);
        drop(h._ctx);
        log::info!("[knob] router: IOProc closed on device {}", h.device_id);
        if stop != 0 {
            return Err(format!("AudioDeviceStop failed: {}", stop));
        }
        if destroy != 0 {
            return Err(format!("AudioDeviceDestroyIOProcID failed: {}", destroy));
        }
        Ok(())
    }
}

// -------------------------------------------------------
// Helpers
// -------------------------------------------------------

fn query_interleaved(device_id: AudioDeviceID) -> Result<bool, String> {
    unsafe {
        let streams_addr = coreaudio_sys::AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreams,
            mScope: kAudioObjectPropertyScopeOutput,
            mElement: ELEMENT_MAIN,
        };
        let mut streams_size: u32 = 0;
        let status = AudioObjectGetPropertyDataSize(
            device_id,
            &streams_addr,
            0,
            std::ptr::null(),
            &mut streams_size,
        );
        if status != 0 || streams_size == 0 {
            return Err("BlackHole 16ch has no output streams".to_string());
        }
        let n_streams = streams_size as usize / std::mem::size_of::<AudioStreamID>();
        let mut stream_ids: Vec<AudioStreamID> = vec![0u32; n_streams];
        let status = AudioObjectGetPropertyData(
            device_id,
            &streams_addr,
            0,
            std::ptr::null(),
            &mut streams_size,
            stream_ids.as_mut_ptr() as *mut c_void,
        );
        if status != 0 {
            return Err(format!("CoreAudio error fetching stream ids: {}", status));
        }

        let fmt_addr = coreaudio_sys::AudioObjectPropertyAddress {
            mSelector: kAudioStreamPropertyVirtualFormat,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: ELEMENT_MAIN,
        };
        let mut asbd: AudioStreamBasicDescription = std::mem::zeroed();
        let mut asbd_size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
        let status = AudioObjectGetPropertyData(
            stream_ids[0],
            &fmt_addr,
            0,
            std::ptr::null(),
            &mut asbd_size,
            &mut asbd as *mut AudioStreamBasicDescription as *mut c_void,
        );
        if status != 0 {
            return Err(format!("CoreAudio error reading stream format: {}", status));
        }

        Ok((asbd.mFormatFlags & coreaudio_sys::kAudioFormatFlagIsNonInterleaved) == 0)
    }
}

// -------------------------------------------------------
// IOProc callback — mixes N sources into ch 1-2
// -------------------------------------------------------

unsafe extern "C" fn io_proc_trampoline(
    _device: AudioObjectID,
    _now: *const AudioTimeStamp,
    _input_data: *const AudioBufferList,
    _input_time: *const AudioTimeStamp,
    output_data: *mut AudioBufferList,
    _output_time: *const AudioTimeStamp,
    client_data: *mut c_void,
) -> OSStatus {
    let ctx = &mut *(client_data as *mut IoProcCtx);
    let abl = &mut *output_data;
    let n_buffers = abl.mNumberBuffers as usize;
    let buffers_base = abl.mBuffers.as_mut_ptr();

    let first_buf = &*buffers_base;
    let bytes_per_frame = if ctx.interleaved {
        (first_buf.mNumberChannels as usize) * std::mem::size_of::<f32>()
    } else {
        std::mem::size_of::<f32>()
    };
    let frame_count = if bytes_per_frame > 0 {
        first_buf.mDataByteSize as usize / bytes_per_frame
    } else {
        0
    };

    for i in 0..n_buffers {
        let buf = &mut *buffers_base.add(i);
        let data = buf.mData as *mut u8;
        std::ptr::write_bytes(data, 0, buf.mDataByteSize as usize);
    }

    if frame_count == 0 {
        return 0;
    }

    let needed = frame_count * 2;
    if ctx.mix_scratch.len() < needed {
        ctx.mix_scratch.resize(needed, 0.0);
    }
    if ctx.out_scratch.len() < needed {
        ctx.out_scratch.resize(needed, 0.0);
    }
    ctx.out_scratch[..needed].fill(0.0);

    // Lock-free snapshot load: Arc clone (1 atomic inc, no alloc)
    let guard = ctx.snapshot.load();
    let mut frames_to_write = 0usize;

    for src in guard.iter() {
        ctx.mix_scratch[..needed].fill(0.0);

        // Mutex は IOProc 専用。競合不可なので即取得。
        let mut cons = src.consumer.lock();
        let mut rs = src.resampler.lock();

        #[allow(clippy::explicit_auto_deref)]
        let frames_from_src = match rs.as_mut() {
            None => {
                let popped = ringbuf::traits::Consumer::pop_slice(
                    &mut *cons,
                    &mut ctx.mix_scratch[..needed],
                );
                popped / 2
            }
            Some(r) => {
                r.process(&mut ctx.mix_scratch[..needed], &mut *cons);
                frame_count
            }
        };

        frames_to_write = frames_to_write.max(frames_from_src);
        let n = frames_from_src * 2;
        for i in 0..n {
            ctx.out_scratch[i] += ctx.mix_scratch[i];
        }
    }

    // Write mixed output to BlackHole ch 1-2
    if ctx.interleaved {
        let buf = &mut *buffers_base;
        let n_ch = buf.mNumberChannels as usize;
        let data = buf.mData as *mut f32;
        for i in 0..frames_to_write {
            *data.add(i * n_ch) = ctx.out_scratch[i * 2];
            *data.add(i * n_ch + 1) = ctx.out_scratch[i * 2 + 1];
        }
    } else {
        if n_buffers >= 1 {
            let buf0 = &mut *buffers_base;
            let data0 = buf0.mData as *mut f32;
            for i in 0..frames_to_write {
                *data0.add(i) = ctx.out_scratch[i * 2];
            }
        }
        if n_buffers >= 2 {
            let buf1 = &mut *buffers_base.add(1);
            let data1 = buf1.mData as *mut f32;
            for i in 0..frames_to_write {
                *data1.add(i) = ctx.out_scratch[i * 2 + 1];
            }
        }
    }

    0
}

// -------------------------------------------------------
// Unit test — snapshot swap (no CoreAudio deps)
// -------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::HeapRb;

    fn make_mix_source() -> Arc<MixSource> {
        let rb = HeapRb::<f32>::new(64);
        let (_prod, cons) = rb.split();
        Arc::new(MixSource {
            consumer: Mutex::new(cons),
            resampler: Mutex::new(None),
        })
    }

    #[test]
    fn snapshot_swap_len() {
        let snap: Arc<ArcSwap<Vec<Arc<MixSource>>>> = Arc::new(ArcSwap::new(Arc::new(vec![])));

        let s1 = make_mix_source();
        let s2 = make_mix_source();

        // add two sources
        {
            let mut v = snap.load().as_ref().clone();
            v.push(s1.clone());
            snap.store(Arc::new(v));
        }
        {
            let mut v = snap.load().as_ref().clone();
            v.push(s2.clone());
            snap.store(Arc::new(v));
        }
        assert_eq!(snap.load().len(), 2);

        // remove first source
        {
            let ptr = Arc::as_ptr(&s1);
            let new: Vec<_> = snap.load().iter()
                .filter(|a| Arc::as_ptr(a) != ptr)
                .cloned()
                .collect();
            snap.store(Arc::new(new));
        }
        assert_eq!(snap.load().len(), 1);
        assert!(Arc::ptr_eq(&snap.load()[0], &s2));
    }
}
