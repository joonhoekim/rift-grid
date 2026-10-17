//! A text field saves its value when editing ends. AppKit ends it on Return or
//! Tab only, so a click elsewhere in the window, or the window going to the
//! back, ends it here as well.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Weak;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSControl, NSEvent, NSEventMask, NSTextView, NSWindow, NSWindowDidResignKeyNotification,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSNotification, NSNotificationCenter};

pub(super) fn end_editing_elsewhere(window: &NSWindow) {
    let clicked = Weak::new(window);
    let monitor = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let event_ref = unsafe { event.as_ref() };
        if let Some(window) = clicked.load()
            && event_ref.windowNumber() == window.windowNumber()
            && let Some(frame) = editing(&window)
        {
            let at = event_ref.locationInWindow();
            let inside = at.x >= frame.origin.x
                && at.x <= frame.origin.x + frame.size.width
                && at.y >= frame.origin.y
                && at.y <= frame.origin.y + frame.size.height;
            if !inside {
                window.makeFirstResponder(None);
            }
        }
        event.as_ptr()
    });
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseDown, &monitor) };

    let resigned = Weak::new(window);
    let observer = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(window) = resigned.load()
            && editing(&window).is_some()
        {
            window.makeFirstResponder(None);
        }
    });
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidResignKeyNotification),
            Some(window),
            None,
            &observer,
        )
    };
}

/// Where the control whose text is being edited in `window` is, in the
/// window's coordinates.
fn editing(window: &NSWindow) -> Option<CGRect> {
    let editor = window.firstResponder()?.downcast::<NSTextView>().ok()?;
    if !editor.isFieldEditor() {
        return None;
    }
    let delegate = editor.delegate()?;
    let control = AsRef::<AnyObject>::as_ref(&*delegate).downcast_ref::<NSControl>()?;
    Some(control.convertRect_toView(control.bounds(), None))
}
