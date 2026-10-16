use std::ffi::c_void;
use std::thread;
use std::time::{Duration, Instant};

use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use tracing::info;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;

    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: *const c_void;
    static kCFBooleanFalse: *const c_void;
}

const AX_POLL_INTERVAL: Duration = Duration::from_millis(250);
const AX_POLL_TIMEOUT: Duration = Duration::from_secs(30);

#[inline]
fn ax_is_trusted() -> bool {
    unsafe {
        autoreleasepool(|_| {
            let keys: [*mut AnyObject; 1] = [kAXTrustedCheckOptionPrompt as *mut AnyObject];
            let vals: [*mut AnyObject; 1] = [kCFBooleanFalse as *mut AnyObject];
            let dict: *mut AnyObject = msg_send![
                class!(NSDictionary),
                dictionaryWithObjects: vals.as_ptr(),
                forKeys:              keys.as_ptr(),
                count:                1usize
            ];

            AXIsProcessTrustedWithOptions(dict.cast())
        })
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn prompt_ax_trust_dialog() {
    autoreleasepool(|_| {
        let keys: [*mut AnyObject; 1] = [kAXTrustedCheckOptionPrompt as *mut AnyObject];
        let vals: [*mut AnyObject; 1] = [kCFBooleanTrue as *mut AnyObject];

        let dict: *mut AnyObject = msg_send![
            class!(NSDictionary),
            dictionaryWithObjects: vals.as_ptr(),
            forKeys:              keys.as_ptr(),
            count:                1usize
        ];

        let _ = AXIsProcessTrustedWithOptions(dict.cast());
    });
}

pub fn ensure_accessibility_permission() {
    if ax_is_trusted() {
        return;
    }

    info!("Accessibility permission is not granted; prompting user for permission now.");

    unsafe { prompt_ax_trust_dialog() };

    let start = Instant::now();
    loop {
        if ax_is_trusted() {
            info!("Accessibility permission granted");
            return;
        }

        if start.elapsed() >= AX_POLL_TIMEOUT {
            break;
        }

        thread::sleep(AX_POLL_INTERVAL);
    }

    println!(
        "Rift still does not have accessibility permission. Enable it in System Settings > Privacy & Security > Accessibility, then restart Rift."
    );

    std::process::exit(1);
}

/// Screen Recording backs the overview previews and the grid motion snapshots.
/// Both only preflight it and quietly fall back without it, so nothing would
/// ever make macOS list Rift under Screen Recording. Ask once at startup
/// instead. Unlike accessibility this does not block: Rift works without it,
/// and macOS applies the grant only after a restart anyway.
pub fn request_screen_capture_permission() {
    if CGPreflightScreenCaptureAccess() {
        return;
    }

    info!("Screen Recording permission is not granted; prompting user for permission now.");

    if !CGRequestScreenCaptureAccess() {
        println!(
            "Rift has no Screen Recording permission, so overview previews and the slide between cells are off. Enable it in System Settings > Privacy & Security > Screen & System Audio Recording, then restart Rift."
        );
    }
}
