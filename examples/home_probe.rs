//! A hand check for home's window (docs/home.md): an accessory app shows a
//! borderless non-activating panel with a text field, and logs what home
//! depends on: whether the panel becomes key without activating the app,
//! whether typing (Korean composing included) reaches the field, which keys
//! come as commands while composing, and who has the keyboard after it hides.
//!
//! `cargo run --example home_probe`, then click into another app within 3 s.
//! `Esc` hides the panel; it shows again 4 s later. `Return` opens the app
//! named in the field (`/Applications/<name>.app` or
//! `/System/Applications/<name>.app`). `Ctrl + C` in the terminal ends it.

use std::cell::{Cell, RefCell};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSControl,
    NSControlTextEditingDelegate, NSFont, NSPanel, NSRunningApplication, NSScreen,
    NSStatusWindowLevel, NSTextField, NSTextFieldDelegate, NSTextView, NSWindowCollectionBehavior,
    NSWindowDelegate, NSWindowStyleMask, NSWorkspace, NSWorkspaceOpenConfiguration,
};
use objc2_foundation::{NSError, NSNotification, NSPoint, NSRect, NSSize, NSString, NSURL};

fn front() -> String {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|app| app.localizedName())
        .map(|name| name.to_string())
        .unwrap_or_else(|| "(none)".into())
}

fn log(what: &str) {
    let mtm = MainThreadMarker::new().unwrap();
    let active = NSApplication::sharedApplication(mtm).isActive();
    println!(
        "[home_probe] {what}  | front app: {}, probe active: {active}",
        front()
    );
}

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "HomeProbePanel"]
    struct Panel;

    impl Panel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }
    }
);

struct Ivars {
    panel: RefCell<Option<Retained<Panel>>>,
    field: RefCell<Option<Retained<NSTextField>>>,
    shown: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "HomeProbeDelegate"]
    #[ivars = Ivars]
    struct Probe;

    unsafe impl NSObjectProtocol for Probe {}

    unsafe impl NSControlTextEditingDelegate for Probe {
        #[unsafe(method(controlTextDidChange:))]
        fn text_did_change(&self, _note: &NSNotification) {
            let (text, marked) = self.editor_state();
            log(&format!("controlTextDidChange: {text:?} marked={marked}"));
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, view: &NSTextView, command: Sel) -> bool {
            self.command(view, command)
        }
    }

    unsafe impl NSTextFieldDelegate for Probe {}

    unsafe impl NSWindowDelegate for Probe {
        #[unsafe(method(windowDidBecomeKey:))]
        fn did_become_key(&self, _note: &NSNotification) { log("panel became key"); }

        #[unsafe(method(windowDidResignKey:))]
        fn did_resign_key(&self, _note: &NSNotification) {
            log("panel resigned key");
            self.hide("resign key");
        }
    }

    impl Probe {
        #[unsafe(method(show:))]
        fn show(&self, _sender: Option<&AnyObject>) {
            let panel = self.ivars().panel.borrow().clone().unwrap();
            let field = self.ivars().field.borrow().clone().unwrap();
            log("showing");
            field.setStringValue(&NSString::from_str(""));
            panel.makeKeyAndOrderFront(None);
            panel.makeFirstResponder(Some(&field));
            self.ivars().shown.set(true);
            log(&format!("shown, panel key: {}", panel.isKeyWindow()));
        }

        #[unsafe(method(report:))]
        fn report(&self, _sender: Option<&AnyObject>) { log("0.3 s after hiding"); }
    }
);

impl Probe {
    fn command(&self, view: &NSTextView, command: Sel) -> bool {
        let marked: bool = unsafe { msg_send![view, hasMarkedText] };
        log(&format!(
            "command {} marked={marked}",
            command.name().to_string_lossy()
        ));
        if marked {
            return false;
        }
        if command == sel!(cancelOperation:) {
            self.hide("Esc");
            return true;
        }
        if command == sel!(insertNewline:) {
            self.open(view.string().to_string());
            return true;
        }
        false
    }

    fn editor_state(&self) -> (String, bool) {
        let panel = self.ivars().panel.borrow().clone().unwrap();
        let Some(responder) = panel.firstResponder() else {
            return (String::new(), false);
        };
        match responder.downcast::<NSTextView>() {
            Ok(view) => (view.string().to_string(), unsafe {
                msg_send![&*view, hasMarkedText]
            }),
            Err(_) => (String::new(), false),
        }
    }

    fn hide(&self, why: &str) {
        if !self.ivars().shown.replace(false) {
            return;
        }
        let panel = self.ivars().panel.borrow().clone().unwrap();
        panel.orderOut(None);
        log(&format!("hidden ({why})"));
        unsafe {
            let _: () = msg_send![self, performSelector: sel!(report:), withObject: None::<&AnyObject>, afterDelay: 0.3f64];
            let _: () = msg_send![self, performSelector: sel!(show:), withObject: None::<&AnyObject>, afterDelay: 4.0f64];
        }
    }

    fn open(&self, name: String) {
        let path = ["/Applications", "/System/Applications"]
            .iter()
            .map(|dir| format!("{dir}/{name}.app"))
            .find(|path| std::path::Path::new(path).exists());
        let Some(path) = path else {
            log(&format!("no app named {name:?}"));
            return;
        };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
        let config = NSWorkspaceOpenConfiguration::configuration();
        config.setActivates(true);
        let done = RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
            let app = unsafe { app.as_ref() };
            let error = unsafe { error.as_ref() };
            println!(
                "[home_probe] open finished (off main: {}): active={:?} error={:?}",
                MainThreadMarker::new().is_none(),
                app.map(|a| a.isActive()),
                error.map(|e| e.localizedDescription().to_string()),
            );
        });
        NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
            &url,
            &config,
            Some(&done),
        );
        self.hide("opened an app");
    }
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let screen = NSScreen::mainScreen(mtm).unwrap().visibleFrame();
    let size = NSSize::new(560.0, 72.0);
    let frame = NSRect::new(
        NSPoint::new(
            screen.origin.x + (screen.size.width - size.width) / 2.0,
            screen.origin.y + screen.size.height * 0.6,
        ),
        size,
    );
    let panel: Retained<Panel> = unsafe {
        msg_send![
            Panel::alloc(mtm),
            initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            backing: NSBackingStoreType::Buffered,
            defer: false,
        ]
    };
    panel.setLevel(NSStatusWindowLevel);
    panel.setHidesOnDeactivate(false);
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
    println!("[home_probe] panel window number: {}", panel.windowNumber());

    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(16.0, 16.0), NSSize::new(size.width - 32.0, 40.0)),
    );
    field.setFont(Some(&NSFont::systemFontOfSize(24.0)));
    field.setPlaceholderString(Some(&NSString::from_str("Search")));
    panel.contentView().unwrap().addSubview(&field);

    let probe: Retained<Probe> = {
        let this = Probe::alloc(mtm).set_ivars(Ivars {
            panel: RefCell::new(Some(panel.clone())),
            field: RefCell::new(Some(field.clone())),
            shown: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    };
    unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&*probe))) };
    panel.setDelegate(Some(ProtocolObject::from_ref(&*probe)));

    println!("[home_probe] click into another app now; the panel shows in 3 s");
    unsafe {
        let _: () = msg_send![&*probe, performSelector: sel!(show:), withObject: None::<&AnyObject>, afterDelay: 3.0f64];
    }
    app.run();
}
