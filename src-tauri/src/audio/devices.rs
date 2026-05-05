use coreaudio_sys::{
    kAudioDevicePropertyStreams,
    kAudioHardwarePropertyDevices,
    kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput,
    kAudioObjectSystemObject,
    AudioDeviceID,
    AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize,
    AudioObjectPropertyAddress,
};
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use std::mem;

use super::{AudioDeviceInfo, BlackHoleStatus};

// kAudioObjectPropertyElementMain (= kAudioObjectPropertyElementMaster in older SDKs) = 0
const ELEMENT_MAIN: u32 = 0;

pub fn list_output_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    unsafe {
        // Step 1: get size of device list
        let devices_addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: ELEMENT_MAIN,
        };
        let mut data_size: u32 = 0;
        let status = AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject,
            &devices_addr,
            0,
            std::ptr::null(),
            &mut data_size,
        );
        if status != 0 {
            return Err(format!("CoreAudio error getting device list size: {}", status));
        }

        // Step 2: fetch device IDs
        let device_count = data_size as usize / mem::size_of::<AudioDeviceID>();
        let mut device_ids: Vec<AudioDeviceID> = vec![0u32; device_count];
        let status = AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &devices_addr,
            0,
            std::ptr::null(),
            &mut data_size,
            device_ids.as_mut_ptr() as *mut std::ffi::c_void,
        );
        if status != 0 {
            return Err(format!("CoreAudio error fetching device list: {}", status));
        }

        let mut result = Vec::new();

        for &id in &device_ids {
            // Filter: only devices with at least one output stream
            let streams_addr = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyStreams,
                mScope: kAudioObjectPropertyScopeOutput,
                mElement: ELEMENT_MAIN,
            };
            let mut streams_size: u32 = 0;
            let status = AudioObjectGetPropertyDataSize(
                id,
                &streams_addr,
                0,
                std::ptr::null(),
                &mut streams_size,
            );
            if status != 0 || streams_size == 0 {
                continue;
            }

            // Fetch device name (CFStringRef, Create rule — we own it)
            let name_addr = AudioObjectPropertyAddress {
                mSelector: kAudioObjectPropertyName,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: ELEMENT_MAIN,
            };
            let mut name_ref: CFStringRef = std::ptr::null();
            let mut name_size = mem::size_of::<CFStringRef>() as u32;
            let status = AudioObjectGetPropertyData(
                id,
                &name_addr,
                0,
                std::ptr::null(),
                &mut name_size,
                &mut name_ref as *mut CFStringRef as *mut std::ffi::c_void,
            );
            if status != 0 || name_ref.is_null() {
                continue;
            }

            // wrap_under_create_rule transfers ownership and calls CFRelease on Drop
            let name = CFString::wrap_under_create_rule(name_ref).to_string();
            result.push(AudioDeviceInfo { id, name });
        }

        Ok(result)
    }
}

pub fn check_blackhole() -> Result<BlackHoleStatus, String> {
    let all = list_output_devices()?;
    let devices: Vec<AudioDeviceInfo> = all
        .into_iter()
        .filter(|d| d.name.to_lowercase().contains("blackhole"))
        .collect();
    Ok(BlackHoleStatus {
        installed: !devices.is_empty(),
        devices,
    })
}
