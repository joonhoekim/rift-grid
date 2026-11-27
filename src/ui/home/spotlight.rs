//! Spotlight's key (Cmd + Space), handed to home while `take_spotlight` is on
//! (docs/home.md, "Opening and closing").
//!
//! macOS reads that key as a system shortcut before any app. Rift turns the
//! shortcut off for the session, so the key reaches rift's keymap, where it is
//! bound to `home`, and turns it on again when the setting is off. The change
//! lasts until the next login; a rift that quits leaves it off until then.
//! A rift that starts with the setting off turns it on again.

use objc2_core_graphics::CGError;

/// "Show Spotlight search" among macOS's system shortcuts.
const SHOW_SPOTLIGHT: i32 = 64;

unsafe extern "C" {
    fn CGSSetSymbolicHotKeyEnabled(hot_key: i32, enabled: bool) -> CGError;
    fn CGSIsSymbolicHotKeyEnabled(hot_key: i32) -> bool;
}

/// Give Spotlight's key to home (`take`), or back to Spotlight. Only a key
/// rift turned off in this session is turned on again: a file in rift's data
/// folder says so, with the time the Mac started, since the key comes back on
/// by itself at the next start. A key the user turned off stays off.
pub fn take_key(take: bool) {
    let marker = crate::common::config::data_dir().join("spotlight-key-taken");
    let enabled = unsafe { CGSIsSymbolicHotKeyEnabled(SHOW_SPOTLIGHT) };
    if take {
        if enabled && set(false) {
            if let Err(error) = std::fs::write(&marker, boot_time().to_string()) {
                tracing::warn!(%error, "home: could not note that Spotlight's key is off");
            }
        }
        return;
    }
    let Ok(noted) = std::fs::read_to_string(&marker) else {
        return;
    };
    if !enabled && noted.trim() == boot_time().to_string() {
        set(true);
    }
    let _ = std::fs::remove_file(&marker);
}

/// When the Mac started, in seconds since 1970.
fn boot_time() -> i64 {
    let mut time = nix::libc::timeval { tv_sec: 0, tv_usec: 0 };
    let mut size = std::mem::size_of::<nix::libc::timeval>();
    let name = c"kern.boottime";
    let result = unsafe {
        nix::libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut time).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 { time.tv_sec } else { 0 }
}

fn set(enabled: bool) -> bool {
    let error = unsafe { CGSSetSymbolicHotKeyEnabled(SHOW_SPOTLIGHT, enabled) };
    if error != CGError::Success {
        tracing::warn!(?error, enabled, "home: could not change Spotlight's key");
    }
    error == CGError::Success
}
