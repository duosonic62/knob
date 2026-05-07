pub mod capture;
pub mod devices;
pub mod resampler;
pub mod router;

use serde::Serialize;
use std::sync::Mutex;

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
    pub session: Mutex<Option<capture::CaptureSession>>,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }
}

pub struct RoutingState {
    pub active: parking_lot::Mutex<Option<(String, router::RouterHandle)>>,
}

impl Default for RoutingState {
    fn default() -> Self {
        Self {
            active: parking_lot::Mutex::new(None),
        }
    }
}
