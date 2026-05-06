use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

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
use ringbuf::traits::Split;
use ringbuf::{HeapProd, HeapRb};

const ELEMENT_MAIN: u32 = 0;

pub type SampleProducer = HeapProd<f32>;

struct IoProcCtx {
    consumer: ringbuf::HeapCons<f32>,
    interleaved: bool,
    scratch: Vec<f32>,
}

pub struct RouterHandle {
    pub gain_bits: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    device_id: AudioDeviceID,
    proc_id: AudioDeviceIOProcID,
    _ctx: Box<IoProcCtx>,
}

unsafe impl Send for RouterHandle {}
unsafe impl Sync for RouterHandle {}

pub fn open_blackhole_route(device_id: AudioDeviceID) -> Result<(RouterHandle, SampleProducer), String> {
    unsafe {
        // 1. 48kHz check
        let rate_addr = coreaudio_sys::AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeOutput,
            mElement: ELEMENT_MAIN,
        };
        let mut rate: f64 = 0.0;
        let mut rate_size = std::mem::size_of::<f64>() as u32;
        let status = AudioObjectGetPropertyData(
            device_id,
            &rate_addr,
            0,
            std::ptr::null(),
            &mut rate_size,
            &mut rate as *mut f64 as *mut c_void,
        );
        if status != 0 {
            return Err(format!("CoreAudio error reading sample rate: {}", status));
        }
        if (rate - 48000.0).abs() > 0.5 {
            return Err(format!(
                "BlackHole 16ch のサンプルレートが 48 kHz ではありません (現在: {} Hz)。\
                Audio MIDI Setup → BlackHole 16ch → フォーマットを 48000 Hz に変更してください。",
                rate as u32
            ));
        }

        // 2. Get ASBD to determine interleaved vs non-interleaved
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

        let interleaved = (asbd.mFormatFlags & coreaudio_sys::kAudioFormatFlagIsNonInterleaved) == 0;

        // 3. Ring buffer: 16384 f32 slots = 8192 stereo frames @ 170ms @48kHz
        let rb = HeapRb::<f32>::new(16384);
        let (prod, cons) = rb.split();

        // 4. Build IoProcCtx on heap (address must be stable after Box::new)
        let ctx = Box::new(IoProcCtx {
            consumer: cons,
            interleaved,
            scratch: vec![0f32; 4096],
        });
        let ctx_ptr = &*ctx as *const IoProcCtx as *mut c_void;

        // 5. Register IOProc
        let mut proc_id: AudioDeviceIOProcID = None;
        let status = AudioDeviceCreateIOProcID(
            device_id,
            Some(io_proc_trampoline),
            ctx_ptr,
            &mut proc_id,
        );
        if status != 0 {
            return Err(format!("AudioDeviceCreateIOProcID failed: {}", status));
        }

        // 6. Start device
        let status = AudioDeviceStart(device_id, proc_id);
        if status != 0 {
            AudioDeviceDestroyIOProcID(device_id, proc_id);
            return Err(format!("AudioDeviceStart failed: {}", status));
        }

        let gain_bits = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let muted = Arc::new(AtomicBool::new(false));

        let handle = RouterHandle {
            gain_bits,
            muted,
            device_id,
            proc_id,
            _ctx: ctx,
        };

        Ok((handle, prod))
    }
}

pub fn close_route(h: RouterHandle) -> Result<(), String> {
    unsafe {
        let stop_status = AudioDeviceStop(h.device_id, h.proc_id);
        let destroy_status = AudioDeviceDestroyIOProcID(h.device_id, h.proc_id);
        // _ctx is dropped here after IOProc is torn down — consumer lifetime ends safely
        drop(h._ctx);
        if stop_status != 0 {
            return Err(format!("AudioDeviceStop failed: {}", stop_status));
        }
        if destroy_status != 0 {
            return Err(format!("AudioDeviceDestroyIOProcID failed: {}", destroy_status));
        }
        Ok(())
    }
}

pub fn set_gain(h: &RouterHandle, gain: f32) {
    h.gain_bits.store(gain.to_bits(), Ordering::Relaxed);
}

pub fn set_muted(h: &RouterHandle, m: bool) {
    h.muted.store(m, Ordering::Relaxed);
}

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

    // Determine frame count from first buffer
    let first_buf = &*buffers_base;
    let bytes_per_frame = if ctx.interleaved {
        // interleaved: mNumberChannels == total channels (16), each frame = 16 floats
        (first_buf.mNumberChannels as usize) * std::mem::size_of::<f32>()
    } else {
        // non-interleaved: each buffer is 1 channel, each frame = 1 float
        std::mem::size_of::<f32>()
    };
    let frame_count = if bytes_per_frame > 0 {
        first_buf.mDataByteSize as usize / bytes_per_frame
    } else {
        0
    };

    // Zero-fill all buffers
    for i in 0..n_buffers {
        let buf = &mut *buffers_base.add(i);
        let data = buf.mData as *mut u8;
        std::ptr::write_bytes(data, 0, buf.mDataByteSize as usize);
    }

    if frame_count == 0 {
        return 0;
    }

    // Pop stereo interleaved samples from ring: [L0, R0, L1, R1, ...]
    let needed = frame_count * 2;
    let scratch_len = ctx.scratch.len();
    if scratch_len < needed {
        ctx.scratch.resize(needed, 0.0);
    }
    let scratch = &mut ctx.scratch[..needed];
    let popped = ringbuf::traits::Consumer::pop_slice(&mut ctx.consumer, scratch);
    // Any frames not popped stay zero (underrun → silence)
    let frames_popped = popped / 2;

    if ctx.interleaved {
        // mBuffers[0], mNumberChannels == n_channels (typically 16)
        let buf = &mut *buffers_base;
        let n_ch = buf.mNumberChannels as usize;
        let data = buf.mData as *mut f32;
        for i in 0..frames_popped {
            *data.add(i * n_ch) = scratch[i * 2];       // ch 1 = L
            *data.add(i * n_ch + 1) = scratch[i * 2 + 1]; // ch 2 = R
        }
    } else {
        // mBuffers[0] = ch 1, mBuffers[1] = ch 2
        if n_buffers >= 1 {
            let buf0 = &mut *buffers_base;
            let data0 = buf0.mData as *mut f32;
            for i in 0..frames_popped {
                *data0.add(i) = scratch[i * 2];
            }
        }
        if n_buffers >= 2 {
            let buf1 = &mut *buffers_base.add(1);
            let data1 = buf1.mData as *mut f32;
            for i in 0..frames_popped {
                *data1.add(i) = scratch[i * 2 + 1];
            }
        }
    }

    0 // noErr
}
