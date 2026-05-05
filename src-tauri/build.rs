fn main() {
    tauri_build::build();

    // screencapturekit links Swift runtime via FFI. Apple's linker (ld-1167+)
    // strips @rpath entries that don't resolve to loose dylib files on disk;
    // on macOS 26 Swift Concurrency lives in the dyld shared cache only, so
    // screencapturekit's own -rpath,/usr/lib/swift gets pruned and the binary
    // fails to start.
    //
    // Workaround: add the CommandLineTools back-deployment library path as a
    // fallback rpath. The CLT ships libswift_Concurrency.dylib under
    // usr/lib/swift-5.5/macosx/ which the dyld can load if the cache lookup
    // fails. This produces an ObjC duplicate-class warning at runtime but does
    // not crash the process.
    //
    // TODO: remove once screencapturekit or upstream linker handles this.
    #[cfg(target_os = "macos")]
    {
        use std::path::Path;
        use std::process::Command;

        // Prefer the active developer dir (xcode-select -p), but Xcode 26+ does not
        // ship swift-5.5 back-deployment libs. Fall back to CommandLineTools, which
        // always carries them.
        let candidates = {
            let mut v = Vec::new();
            if let Ok(out) = Command::new("xcode-select").arg("-p").output() {
                if out.status.success() {
                    let base = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    v.push(format!("{base}/usr/lib/swift-5.5/macosx"));
                }
            }
            v.push(
                "/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx".to_string(),
            );
            v
        };

        for path in candidates {
            if Path::new(&path).join("libswift_Concurrency.dylib").exists() {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
                break;
            }
        }
    }
}
