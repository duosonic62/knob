#!/usr/bin/swift
// Read the Knob driver's 'Kibp' IPC diagnostics from any terminal, without the Tauri app.
//
// Usage:
//   swift knob-driver/scripts/driver-status.swift            # one-shot
//   swift knob-driver/scripts/driver-status.swift --watch   # poll every second (Ctrl-C to stop)

import CoreAudio
import Foundation

let KNOB_BUNDLE_ID = "com.duosonic62.knob.driver"
let SELECTOR_SHM_CONFIG: AudioObjectPropertySelector = 0x4B69_6270  // 'Kibp'

// Mirrors C `KnobShmHeader` / Rust `DriverStatus` — must match exactly.
struct KnobIpcStatus {
    var override_active: UInt32
    var last_stage: Int32
    var last_errno: Int32
    var last_config_len: UInt32
    var xrun_count: UInt32
}

// Find the Knob driver's PlugIn AudioObjectID by matching its bundle ID.
// Mirrors driver_ipc.rs `resolve_plugin_id()`.
func resolvePluginID() -> AudioObjectID? {
    var listAddr = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyPlugInList,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(
        AudioObjectID(kAudioObjectSystemObject), &listAddr, 0, nil, &size
    ) == 0, size > 0 else { return nil }

    let count = Int(size) / MemoryLayout<AudioObjectID>.size
    var ids = [AudioObjectID](repeating: 0, count: count)
    guard AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &listAddr, 0, nil, &size, &ids
    ) == 0 else { return nil }

    var bundleAddr = AudioObjectPropertyAddress(
        mSelector: kAudioPlugInPropertyBundleID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    for pid in ids {
        var cfRef: Unmanaged<CFString>? = nil
        var bsize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        guard AudioObjectGetPropertyData(pid, &bundleAddr, 0, nil, &bsize, &cfRef) == 0,
              let ref = cfRef?.takeRetainedValue() else { continue }
        if (ref as String) == KNOB_BUNDLE_ID { return pid }
    }
    return nil
}

// Read the driver's 'Kibp' diagnostics.
// Mirrors driver_ipc.rs `read_status()`.
func readStatus(pluginID: AudioObjectID) -> KnobIpcStatus? {
    var addr = AudioObjectPropertyAddress(
        mSelector: SELECTOR_SHM_CONFIG,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var cfRef: Unmanaged<CFData>? = nil
    var size = UInt32(MemoryLayout<Unmanaged<CFData>?>.size)
    guard AudioObjectGetPropertyData(pluginID, &addr, 0, nil, &size, &cfRef) == 0,
          let ref = cfRef?.takeRetainedValue() else { return nil }
    let data = ref as Data
    guard data.count >= MemoryLayout<KnobIpcStatus>.size else {
        fputs("error: payload too short (\(data.count) bytes) — rebuild and reinstall the driver\n", stderr)
        return nil
    }
    return data.withUnsafeBytes { $0.load(as: KnobIpcStatus.self) }
}

// --- main ---

guard let pluginID = resolvePluginID() else {
    fputs("error: Knob driver not found — is it installed? (sudo knob-driver/scripts/install-driver.sh)\n", stderr)
    exit(1)
}

let watchMode = CommandLine.arguments.contains("--watch")

if !watchMode {
    guard let s = readStatus(pluginID: pluginID) else {
        fputs("error: failed to read 'Kibp' status from driver\n", stderr)
        exit(1)
    }
    print("override_active: \(s.override_active)")
    print("last_stage:      \(s.last_stage)")
    print("last_errno:      \(s.last_errno)")
    print("last_config_len: \(s.last_config_len)")
    print("xrun_count:      \(s.xrun_count)")
} else {
    let fmt = DateFormatter()
    fmt.dateFormat = "HH:mm:ss"
    print("Watching 'Kibp' every second — Ctrl-C to stop")
    while true {
        let ts = fmt.string(from: Date())
        if let s = readStatus(pluginID: pluginID) {
            print("[\(ts)] active=\(s.override_active) stage=\(s.last_stage) errno=\(s.last_errno) xrun=\(s.xrun_count)")
        } else {
            print("[\(ts)] error: failed to read status")
        }
        sleep(1)
    }
}
