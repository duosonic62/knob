pub mod capture;
pub mod devices;
pub mod resampler;
pub mod router;

use arc_swap::ArcSwap;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;

#[derive(Debug, Serialize, Clone)]
pub struct AppInfo {
    pub bundle_id: String,
    pub name: String,
    pub pid: i32,
}

#[derive(Debug, Serialize, Clone)]
pub struct AudioDeviceInfo {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct BlackHoleStatus {
    pub installed: bool,
    pub devices: Vec<AudioDeviceInfo>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AudioLevel {
    pub bundle_id: String,
    pub rms: f32,
}

pub struct CaptureState {
    pub sessions: parking_lot::Mutex<HashMap<String, capture::CaptureSession>>,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            sessions: parking_lot::Mutex::new(HashMap::new()),
        }
    }
}

/// Per-active-route entry. gain_bits / muted are shared with the SCK push closure.
pub struct SourceEntry {
    pub gain_bits: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    pub mix_source: Arc<router::MixSource>,
}

pub struct RoutingState {
    pub sources: parking_lot::Mutex<HashMap<String, SourceEntry>>,
    pub mix_snapshot: Arc<ArcSwap<Vec<Arc<router::MixSource>>>>,
    pub io_proc: parking_lot::Mutex<Option<router::IoProcHandle>>,

    pub master_gain_bits: Arc<AtomicU32>,
    pub master_muted: Arc<AtomicBool>,
}

impl Default for RoutingState {
    fn default() -> Self {
        Self {
            sources: parking_lot::Mutex::new(HashMap::new()),
            mix_snapshot: Arc::new(ArcSwap::new(Arc::new(vec![]))),
            io_proc: parking_lot::Mutex::new(None),
            master_gain_bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            master_muted: Arc::new(AtomicBool::new(false)),
        }
    }
}
