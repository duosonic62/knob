//! Host → Knob driver IPC (Issue #31 / #A2, PoC spike).
//!
//! Control plane: a CoreAudio custom property (`'Kibp'`) on the Knob driver's PlugIn object, used to
//! hand the driver the name of a POSIX shared-memory region. Data plane: that shared-memory SPSC ring
//! of interleaved stereo f32, which the driver's ReadInput reads while the host override is active.
//!
//! This PoC only feeds a debug sine tone. The real per-app mix is wired in later issues (#A3/#A4).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::data::CFData;
use core_foundation::string::CFString;
use coreaudio_sys::{
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData,
};

const KNOB_BUNDLE_ID: &str = "com.duosonic62.knob.driver";
const KNOB_IPC_MAGIC: u32 = 0x4B4E_4F42; // 'KNOB'
const KNOB_IPC_PROTOCOL_VERSION: u32 = 1;
const KNOB_SHM_NAME_MAX: usize = 64;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u32 = 2;
const RING_FRAMES: u32 = 16_384;
const ELEMENT_MAIN: u32 = 0;

// 'Kibp' custom property selector on the driver's PlugIn object.
const SELECTOR_SHM_CONFIG: u32 = u32::from_be_bytes(*b"Kibp");
// kAudioHardwarePropertyPlugInList / kAudioPlugInPropertyBundleID as 4cc (coreaudio-sys exposes some
// of these inconsistently across versions, so spell them out to stay independent of the binding).
const PROP_PLUGIN_LIST: u32 = u32::from_be_bytes(*b"plg#");
const PROP_PLUGIN_BUNDLE_ID: u32 = u32::from_be_bytes(*b"piid");

/// Header at the start of the shared-memory region. C ABI shared with the driver (`KnobShmHeader`).
#[repr(C)]
struct ShmHeader {
    magic: u32,
    protocol_version: u32,
    sample_rate: u32,
    channels: u32,
    ring_frames: u32,
    _pad: u32,
    write_index: AtomicU64,
    read_index: AtomicU64,
    heartbeat: AtomicU64,
}

/// Payload carried by the custom property. C ABI shared with the driver (`KnobShmConfig`).
#[repr(C)]
struct ShmConfig {
    magic: u32,
    protocol_version: u32,
    name: [u8; KNOB_SHM_NAME_MAX],
}

/// Diagnostics read back from the driver's 'Kibp' Get (mirrors C `KnobIpcStatus`).
/// `last_stage`: 0 none, 1 bad/none CFData, 2 bad magic/ver/name, 3 shm_open fail, 4 fstat/size fail,
/// 5 mmap fail, 6 bad header, 7 success.
#[repr(C)]
#[derive(serde::Serialize, Clone, Copy, Debug)]
pub struct DriverStatus {
    pub override_active: u32,
    pub last_stage: i32,
    pub last_errno: i32,
    pub last_config_len: u32,
    pub xrun_count: u32,
}

const HEADER_SIZE: usize = std::mem::size_of::<ShmHeader>();

fn shm_total_size() -> usize {
    HEADER_SIZE + (RING_FRAMES as usize) * (CHANNELS as usize) * std::mem::size_of::<f32>()
}

/// A running debug producer + its shared-memory region.
struct DriverIpc {
    name: std::ffi::CString,
    base: *mut c_void,
    len: usize,
    plugin_id: AudioObjectID,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

// The raw pointer is only touched by the producer thread (which owns it for its lifetime) and during
// teardown on the command thread after the producer has joined.
unsafe impl Send for DriverIpc {}

#[derive(Default)]
pub struct DriverIpcState {
    inner: parking_lot::Mutex<Option<DriverIpc>>,
}

impl DriverIpcState {
    /// Read the driver's diagnostics (override flag + last failure stage/errno).
    pub fn status(&self) -> Result<DriverStatus, String> {
        read_status()
    }

    /// Enable/disable the debug sine tone fed to the Knob driver via shared memory.
    pub fn set_sine(&self, enable: bool, freq: f32) -> Result<(), String> {
        let mut guard = self.inner.lock();

        // Always tear down any previous session first.
        if let Some(mut ipc) = guard.take() {
            teardown(&mut ipc);
        }
        if !enable {
            return Ok(());
        }

        let ipc = start_sine(freq)?;
        *guard = Some(ipc);
        Ok(())
    }
}

/// Create the shm region, hand its name to the driver, and start the producer thread.
fn start_sine(freq: f32) -> Result<DriverIpc, String> {
    let plugin_id = resolve_plugin_id()
        .ok_or_else(|| "Knob driver not found (is it installed?)".to_string())?;

    let pid = std::process::id();
    let name = format!("/knob.{}", pid);
    let cname = std::ffi::CString::new(name.clone()).map_err(|e| e.to_string())?;
    let total = shm_total_size();

    let base = unsafe {
        // Fresh region: unlink any stale one, then create.
        libc::shm_unlink(cname.as_ptr());
        let fd = libc::shm_open(
            cname.as_ptr(),
            libc::O_CREAT | libc::O_RDWR,
            0o666 as libc::c_uint,
        );
        if fd < 0 {
            return Err(format!("shm_open failed (errno {})", errno()));
        }
        // The host runs as the login user, but coreaudiod (and our driver) runs as `_coreaudiod`.
        // Force group/other read-write so the driver can shm_open it O_RDWR. fchmod bypasses umask,
        // which would otherwise strip the write bits from the shm_open mode above.
        libc::fchmod(fd, 0o666);
        if libc::ftruncate(fd, total as libc::off_t) != 0 {
            libc::close(fd);
            libc::shm_unlink(cname.as_ptr());
            return Err(format!("ftruncate failed (errno {})", errno()));
        }
        let p = libc::mmap(
            std::ptr::null_mut(),
            total,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        );
        libc::close(fd);
        if p == libc::MAP_FAILED {
            libc::shm_unlink(cname.as_ptr());
            return Err(format!("mmap failed (errno {})", errno()));
        }
        p
    };

    // Initialize the header.
    unsafe {
        let h = base as *mut ShmHeader;
        std::ptr::write(
            h,
            ShmHeader {
                magic: KNOB_IPC_MAGIC,
                protocol_version: KNOB_IPC_PROTOCOL_VERSION,
                sample_rate: SAMPLE_RATE,
                channels: CHANNELS,
                ring_frames: RING_FRAMES,
                _pad: 0,
                write_index: AtomicU64::new(0),
                read_index: AtomicU64::new(0),
                heartbeat: AtomicU64::new(0),
            },
        );
    }

    // Hand the shm name to the driver via the custom property.
    if let Err(e) = send_config(plugin_id, Some(&name)) {
        unsafe {
            libc::munmap(base, total);
            libc::shm_unlink(cname.as_ptr());
        }
        return Err(e);
    }

    // Start the producer thread.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let base_addr = base as usize;
    let thread = std::thread::spawn(move || {
        run_sine_producer(base_addr, freq, stop_thread);
    });

    log::info!("[knob] driver_ipc: sine override started ({} Hz) via {}", freq, name);
    Ok(DriverIpc {
        name: cname,
        base,
        len: total,
        plugin_id,
        stop,
        thread: Some(thread),
    })
}

fn teardown(ipc: &mut DriverIpc) {
    // Tell the driver to drop the override (back to loopback).
    let _ = send_config(ipc.plugin_id, None);
    // Stop and join the producer.
    ipc.stop.store(true, Ordering::Release);
    if let Some(t) = ipc.thread.take() {
        let _ = t.join();
    }
    // Unmap and remove the region.
    unsafe {
        libc::munmap(ipc.base, ipc.len);
        libc::shm_unlink(ipc.name.as_ptr());
    }
    log::info!("[knob] driver_ipc: sine override stopped");
}

/// Generate a 440-ish Hz stereo sine into the ring, keeping it filled, until `stop` is set.
fn run_sine_producer(base_addr: usize, freq: f32, stop: Arc<AtomicBool>) {
    let base = base_addr as *mut c_void;
    let header = base as *const ShmHeader;
    let data = unsafe { (base as *mut u8).add(HEADER_SIZE) as *mut f32 };
    let ring_frames = RING_FRAMES as u64;

    let phase_inc = std::f32::consts::TAU * freq / SAMPLE_RATE as f32;
    let mut phase = 0.0f32;
    let mut produced: u64 = 0;
    const CHUNK: u64 = 1024;
    const AMPLITUDE: f32 = 0.2;

    unsafe {
        let write_index = &(*header).write_index;
        let read_index = &(*header).read_index;
        let heartbeat = &(*header).heartbeat;

        while !stop.load(Ordering::Acquire) {
            let read = read_index.load(Ordering::Acquire);
            let used = produced - read;
            let free = ring_frames.saturating_sub(used);
            let chunk = free.min(CHUNK);

            for _ in 0..chunk {
                let pos = (produced % ring_frames) as usize;
                let s = phase.sin() * AMPLITUDE;
                *data.add(pos * 2) = s;
                *data.add(pos * 2 + 1) = s;
                phase += phase_inc;
                if phase > std::f32::consts::TAU {
                    phase -= std::f32::consts::TAU;
                }
                produced += 1;
            }

            write_index.store(produced, Ordering::Release);
            heartbeat.fetch_add(1, Ordering::Relaxed);

            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Find the Knob driver's PlugIn AudioObjectID by matching its bundle id.
fn resolve_plugin_id() -> Option<AudioObjectID> {
    unsafe {
        let list_addr = AudioObjectPropertyAddress {
            mSelector: PROP_PLUGIN_LIST,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: ELEMENT_MAIN,
        };
        let mut size: u32 = 0;
        if AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject,
            &list_addr,
            0,
            std::ptr::null(),
            &mut size,
        ) != 0
            || size == 0
        {
            return None;
        }
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids: Vec<AudioObjectID> = vec![0; count];
        if AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &list_addr,
            0,
            std::ptr::null(),
            &mut size,
            ids.as_mut_ptr() as *mut c_void,
        ) != 0
        {
            return None;
        }

        let id_addr = AudioObjectPropertyAddress {
            mSelector: PROP_PLUGIN_BUNDLE_ID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: ELEMENT_MAIN,
        };
        for pid in ids {
            let mut cfref: *const c_void = std::ptr::null();
            let mut bsize = std::mem::size_of::<*const c_void>() as u32;
            let st = AudioObjectGetPropertyData(
                pid,
                &id_addr,
                0,
                std::ptr::null(),
                &mut bsize,
                &mut cfref as *mut _ as *mut c_void,
            );
            if st == 0 && !cfref.is_null() {
                let bundle = CFString::wrap_under_create_rule(cfref as _).to_string();
                if bundle == KNOB_BUNDLE_ID {
                    return Some(pid);
                }
            }
        }
        None
    }
}

/// Read the driver's 'Kibp' diagnostics struct.
fn read_status() -> Result<DriverStatus, String> {
    let plugin_id = resolve_plugin_id()
        .ok_or_else(|| "Knob driver not found (is it installed?)".to_string())?;
    let addr = AudioObjectPropertyAddress {
        mSelector: SELECTOR_SHM_CONFIG,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: ELEMENT_MAIN,
    };
    unsafe {
        let mut cfref: *const c_void = std::ptr::null();
        let mut size = std::mem::size_of::<*const c_void>() as u32;
        let st = AudioObjectGetPropertyData(
            plugin_id,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            &mut cfref as *mut _ as *mut c_void,
        );
        if st != 0 {
            return Err(format!("AudioObjectGetPropertyData('Kibp') failed: {}", st));
        }
        if cfref.is_null() {
            return Err("Get('Kibp') returned null".to_string());
        }
        let data = CFData::wrap_under_create_rule(cfref as _);
        let bytes = data.bytes();
        if bytes.len() < std::mem::size_of::<DriverStatus>() {
            return Err(format!("status payload too short: {} bytes", bytes.len()));
        }
        let mut status = DriverStatus {
            override_active: 0,
            last_stage: 0,
            last_errno: 0,
            last_config_len: 0,
            xrun_count: 0,
        };
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            &mut status as *mut DriverStatus as *mut u8,
            std::mem::size_of::<DriverStatus>(),
        );
        Ok(status)
    }
}

/// Send (or clear) the shm config to the driver's custom property.
/// `Some(name)` publishes the region; `None` sends an empty payload to disconnect.
fn send_config(plugin_id: AudioObjectID, name: Option<&str>) -> Result<(), String> {
    let cfdata = match name {
        Some(n) => {
            let mut cfg = ShmConfig {
                magic: KNOB_IPC_MAGIC,
                protocol_version: KNOB_IPC_PROTOCOL_VERSION,
                name: [0u8; KNOB_SHM_NAME_MAX],
            };
            let bytes = n.as_bytes();
            if bytes.len() >= KNOB_SHM_NAME_MAX {
                return Err("shm name too long".to_string());
            }
            cfg.name[..bytes.len()].copy_from_slice(bytes);
            let raw = unsafe {
                std::slice::from_raw_parts(
                    &cfg as *const ShmConfig as *const u8,
                    std::mem::size_of::<ShmConfig>(),
                )
            };
            CFData::from_buffer(raw)
        }
        None => CFData::from_buffer(&[]),
    };

    let addr = AudioObjectPropertyAddress {
        mSelector: SELECTOR_SHM_CONFIG,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: ELEMENT_MAIN,
    };
    let data_ref = cfdata.as_concrete_TypeRef();
    let status = unsafe {
        AudioObjectSetPropertyData(
            plugin_id,
            &addr,
            0,
            std::ptr::null(),
            std::mem::size_of::<*const c_void>() as u32,
            &data_ref as *const _ as *const c_void,
        )
    };
    if status != 0 {
        return Err(format!("AudioObjectSetPropertyData('Kibp') failed: {}", status));
    }
    Ok(())
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}
