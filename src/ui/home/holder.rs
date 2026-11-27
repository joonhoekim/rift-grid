//! The holder: a window of rift-grid's own that takes the keyboard while the
//! current cell is empty, so that typed keys go nowhere rather than to a
//! window in another cell (`actor/reactor/cell_focus.rs`).
//!
//! Taking the keyboard, it brings rift-grid to the front: the app that was in
//! front would otherwise stay there behind it, and a Dock click or Cmd+Tab on
//! that app would then be no activation at all, which rift follows to the
//! app's cell. Spotlight and the like also give the keyboard back to the app in
//! front, which is then rift-grid. The holder is invisible (1x1 pt, alpha 0)
//! and lets the mouse through.

use std::cell::OnceCell;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSPanel, NSScreen, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};

use crate::sys::screen::NSScreenExt;

static KEY: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The display the holder holds, between `hold` and `release`.
    static HELD: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Whether the holder has the keyboard.
pub fn is_key() -> bool { KEY.load(Ordering::Relaxed) }

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftCellHolder"]
    struct HolderPanel;

    impl HolderPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }

        #[unsafe(method(becomeKeyWindow))]
        fn become_key(&self) {
            let _: () = unsafe { msg_send![super(self), becomeKeyWindow] };
            KEY.store(true, Ordering::Relaxed);
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            let _: () = unsafe { msg_send![super(self), resignKeyWindow] };
            KEY.store(false, Ordering::Relaxed);
        }

        /// Typed keys stop here, without the beep for a key nobody takes.
        #[unsafe(method(noResponderFor:))]
        fn no_responder_for(&self, _selector: Sel) {}
    }
);

thread_local! {
    static PANEL: OnceCell<Retained<HolderPanel>> = const { OnceCell::new() };
}

fn panel(mtm: MainThreadMarker) -> Retained<HolderPanel> {
    PANEL.with(|cell| cell.get_or_init(|| make(mtm)).clone())
}

fn make(mtm: MainThreadMarker) -> Retained<HolderPanel> {
    let frame = CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0));
    let panel: Retained<HolderPanel> = unsafe {
        msg_send![
            HolderPanel::alloc(mtm),
            initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Borderless,
            backing: NSBackingStoreType::Buffered,
            defer: false,
        ]
    };
    panel.setHidesOnDeactivate(false);
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setAlphaValue(0.0);
    panel.setHasShadow(false);
    panel.setIgnoresMouseEvents(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel
}

/// Take the keyboard on this display (a `CGDirectDisplayID`). The holder sits
/// at the display's center: macOS's focused display follows the key window.
pub fn hold(mtm: MainThreadMarker, display_id: u32) {
    let Some(screen) = NSScreen::screens(mtm)
        .iter()
        .find(|screen| screen.get_number().is_ok_and(|id| id.as_u32() == display_id))
    else {
        tracing::debug!(display_id, "holder: no such screen");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    if HELD.get() == Some(display_id) && is_key() && app.isActive() {
        // Already holding here. Taking it again, late on the main thread,
        // could undo an app the user has just chosen.
        return;
    }
    HELD.set(Some(display_id));
    let frame = screen.frame();
    let center = CGPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let panel = panel(mtm);
    panel.setFrame_display(CGRect::new(center, CGSize::new(1.0, 1.0)), false);
    take(mtm, &panel);
}

fn take(mtm: MainThreadMarker, panel: &HolderPanel) {
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    panel.makeKeyAndOrderFront(None);
}

/// Take the keyboard back where the holder is, if it still holds a cell.
pub fn take_back(mtm: MainThreadMarker) {
    if HELD.get().is_some() {
        take(mtm, &panel(mtm));
    }
}

/// Put the holder away: the cell has a window again, or shows another.
pub fn release(mtm: MainThreadMarker) {
    HELD.set(None);
    if PANEL.with(|cell| cell.get().is_some()) {
        panel(mtm).orderOut(None);
    }
}
