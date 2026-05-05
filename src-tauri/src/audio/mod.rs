pub mod capture;

use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Serialize, Clone)]
pub struct AppInfo {
    pub bundle_id: String,
    pub name: String,
    pub pid: i32,
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
