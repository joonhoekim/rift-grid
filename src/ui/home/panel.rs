//! Home's window: a non-activating `NSPanel` over the display with the search
//! box and the grid (docs/home.md, "The window", "Styles", "Search",
//! "Launching", "The app menu").
//!
//! AppKit calls back into the controller while it changes windows (hiding the
//! panel resigns its key window, for one), so the controller's state is only
//! borrowed for short stretches that make no such calls.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAnimationContext, NSApplicationActivationOptions, NSBackingStoreType, NSColor, NSControl,
    NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags, NSFont, NSFontWeightLight,
    NSImage, NSImageScaling, NSImageView, NSLineBreakMode, NSMenu, NSMenuItem, NSPanel,
    NSRunningApplication, NSScreen, NSScrollView, NSStatusWindowLevel, NSTextAlignment,
    NSTextField, NSTextFieldDelegate, NSTextView, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowDelegate, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceOpenConfiguration,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSArray, NSDate, NSDateFormatter, NSDistributedNotificationCenter, NSError, NSNotification,
    NSNotificationCenter, NSString, NSURL, ns_string,
};
use objc2_quartz_core::CAMediaTiming;

use super::apps::Catalog;
use super::grid::{self, GRID_TOP, HEAD_H, ICON, LINE_GAP, PAD, Pick, SLOT_H, SLOT_W, Shown, Step};
use super::launches::{self, Launches};
use super::{categories, search};
use crate::common::config::{HomeCategory, HomeSettings, HomeStyle};
use crate::sys::screen::NSScreenExt;

/// The grid is never wider than this many slots, so a wide display keeps it
/// together in the middle.
const MOST_COLUMNS: f64 = 9.0;
const SEARCH_W: f64 = 560.0;
const SEARCH_H: f64 = 36.0;
const PANEL_RADIUS: f64 = 16.0;
/// How dark the windows around the panel get.
const DIM: f64 = 0.5;
const FADE_IN: f64 = 0.15;
const FADE_OUT: f64 = 0.1;
const RISE: f64 = 12.0;
/// How far a category's box reaches past its slots each side.
const SECTION_OUT: f64 = 8.0;
/// A category menu item's tag: the app's index times this, plus the category's.
const CATEGORY_TAG: usize = 16;

/// A change home asks of the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeEdit {
    /// Leave the app out (`hidden`).
    Hide(String),
    /// Put the app in this category (`categories`).
    Category(String, HomeCategory),
}

/// Why home closed: by itself (`Esc`, the command again, a click on its
/// empty part), when it gives the keyboard back to the app it came from; or
/// because something else took the keyboard, which is left where it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    Itself,
    Elsewhere,
}

struct State {
    settings: HomeSettings,
    catalog: Catalog,
    launches: Launches,
    launches_path: PathBuf,
    open: bool,
    /// Counts openings and closings, so a fade's end that comes after the next
    /// opening does nothing.
    generation: u64,
    /// The app in front when home opened.
    front: Option<Retained<NSRunningApplication>>,
    query: String,
    shown: Shown,
    pick: Option<Pick>,
    hover: Option<Pick>,
    columns: usize,
    /// Where the first column starts in the grid view.
    grid_x: f64,
}

struct Views {
    panel: Retained<HomePanel>,
    root: Retained<RootView>,
    card: Retained<CardView>,
    clock: Retained<NSTextField>,
    date: Retained<NSTextField>,
    field: Retained<NSTextField>,
    scroll: Retained<NSScrollView>,
    grid: Retained<GridView>,
    empty: Retained<NSTextField>,
    plate: Retained<NSView>,
    hover: Retained<NSView>,
    line: Retained<NSView>,
}

/// A slot's views: where it is in the grid view, its icon and its name.
struct Slot {
    view: Retained<NSView>,
}

pub struct ControllerIvars {
    state: RefCell<State>,
    views: OnceCell<Views>,
    /// Slot views by (in the frequent row, app index), made once per app list.
    slots: RefCell<HashMap<(bool, usize), Slot>>,
    icons: RefCell<HashMap<PathBuf, Retained<NSImage>>>,
    edit: Box<dyn Fn(HomeEdit)>,
    /// The categories' titles and boxes, made again with each fill.
    sections: RefCell<Vec<Retained<NSView>>>,
    /// Asked to read the apps again (they changed on disk).
    reread: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftHomePanel"]
    pub struct HomePanel;

    impl HomePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }

        /// Rift's own process has no main menu, where a text field's editing
        /// keys come from; they are sent to the field editor here.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.edit_key(event) || unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }
    }
);

impl HomePanel {
    fn edit_key(&self, event: &NSEvent) -> bool {
        let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        let shift = flags.contains(NSEventModifierFlags::Shift);
        if !(flags - NSEventModifierFlags::Shift).eq(&NSEventModifierFlags::Command) {
            return false;
        }
        // By the key's place, not its character: with a Korean layout Cmd+A
        // gives "ㅁ".
        let action = match (event.keyCode(), shift) {
            (0, false) => sel!(selectAll:),
            (8, false) => sel!(copy:),
            (7, false) => sel!(cut:),
            (9, false) => sel!(paste:),
            (6, false) => sel!(undo:),
            (6, true) => sel!(redo:),
            _ => return false,
        };
        self.firstResponder()
            .is_some_and(|responder| unsafe { responder.tryToPerform_with(action, None) })
    }
}

pub struct ViewIvars {
    controller: Weak<Controller>,
}

define_class!(
    /// The whole display: in the `window` style the darkened part around the
    /// panel. A click on it closes home.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftHomeRoot"]
    #[ivars = ViewIvars]
    struct RootView;

    impl RootView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool { true }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.close(Closed::Itself);
            }
        }
    }
);

define_class!(
    /// The grid's slots, inside the scroll view. It takes every click in it:
    /// the slots are drawn by views that do not.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftHomeGrid"]
    #[ivars = ViewIvars]
    struct GridView;

    impl GridView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool { true }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: CGPoint) -> *mut NSView {
            let inside = unsafe { self.superview() }.is_some_and(|parent| {
                let local = self.convertPoint_fromView(point, Some(&parent));
                self.mouse_inRect(local, self.bounds())
            });
            if inside { self as *const Self as *mut NSView } else { std::ptr::null_mut() }
        }

        /// Taken here, so it does not go up to the views behind, which close
        /// home; the click acts on mouse-up.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let Some(controller) = self.ivars().controller.load() else { return };
            if event.modifierFlags().contains(NSEventModifierFlags::Control) {
                controller.menu_at(self.local(event), event);
            } else {
                controller.clicked(self.local(event));
            }
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.menu_at(self.local(event), event);
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.hovered(Some(self.local(event)));
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.hovered(None);
            }
        }
    }
);

define_class!(
    /// What home shows on: the whole display in the `full` style, the panel in
    /// the `window` style. A click on an empty part of it closes home only in
    /// the `full` style.
    #[unsafe(super(NSVisualEffectView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftHomeCard"]
    #[ivars = ViewIvars]
    struct CardView;

    impl CardView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.clicked_empty();
            }
        }
    }
);

impl GridView {
    fn local(&self, event: &NSEvent) -> CGPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RiftHomeController"]
    #[ivars = ControllerIvars]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSControlTextEditingDelegate for Controller {
        #[unsafe(method(controlTextDidChange:))]
        fn text_did_change(&self, _note: &NSNotification) { self.search_changed(); }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, view: &NSTextView, command: Sel) -> bool {
            self.command(view, command)
        }
    }

    unsafe impl NSTextFieldDelegate for Controller {}

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowDidResignKey:))]
        fn did_resign_key(&self, _note: &NSNotification) { self.close(Closed::Elsewhere); }
    }

    impl Controller {
        /// The active Space changed, the screens changed, the Mac is going to
        /// sleep, the screen locked.
        #[unsafe(method(systemChanged:))]
        fn system_changed(&self, _note: &NSNotification) { self.close(Closed::Elsewhere); }

        #[unsafe(method(menuOpen:))]
        fn menu_open(&self, item: &NSMenuItem) { self.launch(item.tag() as usize); }

        #[unsafe(method(menuReveal:))]
        fn menu_reveal(&self, item: &NSMenuItem) { self.reveal(item.tag() as usize); }

        #[unsafe(method(menuHide:))]
        fn menu_hide(&self, item: &NSMenuItem) { self.hide_app(item.tag() as usize); }

        #[unsafe(method(menuCategory:))]
        fn menu_category(&self, item: &NSMenuItem) {
            let tag = item.tag() as usize;
            if let Some(&category) = HomeCategory::ALL.get(tag % CATEGORY_TAG) {
                self.set_category(tag / CATEGORY_TAG, category);
            }
        }

        #[unsafe(method(menuQuit:))]
        fn menu_quit(&self, item: &NSMenuItem) { self.quit(item.tag() as usize); }
    }
);

/// Home's window and what it shows.
pub struct Home {
    controller: Retained<Controller>,
}

impl Home {
    /// Made when rift starts, hidden. `hide` puts a bundle id in the config's
    /// `hidden`; `reread` reads the apps again.
    pub fn new(
        mtm: MainThreadMarker,
        settings: HomeSettings,
        edit: impl Fn(HomeEdit) + 'static,
        reread: impl Fn() + 'static,
    ) -> Self {
        let launches_path = launches::path();
        let controller: Retained<Controller> = {
            let this = Controller::alloc(mtm).set_ivars(ControllerIvars {
                state: RefCell::new(State {
                    settings,
                    catalog: Catalog::default(),
                    launches: Launches::load(&launches_path),
                    launches_path,
                    open: false,
                    generation: 0,
                    front: None,
                    query: String::new(),
                    shown: Shown::default(),
                    pick: None,
                    hover: None,
                    columns: 1,
                    grid_x: 0.0,
                }),
                views: OnceCell::new(),
                slots: RefCell::new(HashMap::new()),
                icons: RefCell::new(HashMap::new()),
                edit: Box::new(edit),
                sections: RefCell::new(Vec::new()),
                reread: Box::new(reread),
            });
            unsafe { msg_send![super(this), init] }
        };
        let views = make_views(mtm, &controller);
        super::set_window(views.panel.windowNumber().max(0) as u32);
        let _ = controller.ivars().views.set(views);
        controller.observe();
        Self { controller }
    }

    pub fn is_open(&self) -> bool { self.controller.ivars().state.borrow().open }

    /// Open on the display `display` (a `CGDirectDisplayID`). The apps are
    /// read again after if they changed.
    pub fn open(&self, display: u32) { self.controller.open(display); }

    pub fn close(&self, why: Closed) { self.controller.close(why); }

    pub fn configure(&self, settings: HomeSettings) { self.controller.configure(settings); }

    pub fn set_catalog(&self, catalog: Catalog) { self.controller.set_catalog(catalog); }

    /// Lay home out for `frame` (a display's, in AppKit's coordinates) and draw
    /// it into a PNG at `path`, without showing a window: to look at it while
    /// working on it (`examples/home_render.rs`). The blur is not drawn.
    pub fn render(&self, frame: CGRect, usable: CGRect, query: &str, path: &str) -> bool {
        self.controller.render(frame, usable, query, path)
    }
}

fn label(mtm: MainThreadMarker, text: &str, size: f64) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(size)));
    label.setAlignment(NSTextAlignment::Center);
    label.setTextColor(Some(&NSColor::labelColor()));
    label
}

fn plate(mtm: MainThreadMarker, color: &NSColor) -> Retained<NSView> {
    let view = NSView::initWithFrame(NSView::alloc(mtm), CGRect::ZERO);
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setBackgroundColor(Some(&color.CGColor()));
        layer.setCornerRadius(12.0);
    }
    view.setHidden(true);
    view
}

fn make_views(mtm: MainThreadMarker, controller: &Retained<Controller>) -> Views {
    let frame = CGRect::new(CGPoint::ZERO, CGSize::new(800.0, 600.0));
    let panel: Retained<HomePanel> = unsafe {
        msg_send![
            HomePanel::alloc(mtm),
            initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            backing: NSBackingStoreType::Buffered,
            defer: false,
        ]
    };
    panel.setLevel(NSStatusWindowLevel);
    panel.setHidesOnDeactivate(false);
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setDelegate(Some(ProtocolObject::from_ref(&**controller)));

    let weak = Weak::from_retained(controller);
    let root: Retained<RootView> = {
        let this = RootView::alloc(mtm).set_ivars(ViewIvars { controller: weak.clone() });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    };
    root.setWantsLayer(true);
    panel.setContentView(Some(&root));

    let card: Retained<CardView> = {
        let this = CardView::alloc(mtm).set_ivars(ViewIvars { controller: weak.clone() });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    };
    card.setMaterial(NSVisualEffectMaterial::HUDWindow);
    card.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    card.setState(NSVisualEffectState::Active);
    card.setWantsLayer(true);
    root.addSubview(&card);

    let clock = label(mtm, "", 64.0);
    clock.setFont(Some(&NSFont::systemFontOfSize_weight(64.0, unsafe {
        NSFontWeightLight
    })));
    let date = label(mtm, "", 18.0);
    date.setTextColor(Some(&NSColor::secondaryLabelColor()));
    card.addSubview(&clock);
    card.addSubview(&date);

    let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), CGRect::ZERO);
    field.setFont(Some(&NSFont::systemFontOfSize(20.0)));
    field.setPlaceholderString(Some(ns_string!("Search")));
    field.setBezeled(true);
    field.setBezelStyle(objc2_app_kit::NSTextFieldBezelStyle::RoundedBezel);
    unsafe { field.setDelegate(Some(ProtocolObject::from_ref(&**controller))) };
    card.addSubview(&field);

    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), CGRect::ZERO);
    scroll.setDrawsBackground(false);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    // As subline's wheel: no going past the ends.
    scroll.setVerticalScrollElasticity(objc2_app_kit::NSScrollElasticity::None);
    scroll.setHorizontalScrollElasticity(objc2_app_kit::NSScrollElasticity::None);
    let grid: Retained<GridView> = {
        let this = GridView::alloc(mtm).set_ivars(ViewIvars { controller: weak });
        unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] }
    };
    grid.setWantsLayer(true);
    let tracking = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            CGRect::ZERO,
            NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(&grid),
            None,
        )
    };
    grid.addTrackingArea(&tracking);
    scroll.setDocumentView(Some(&grid));
    card.addSubview(&scroll);

    let accent = NSColor::controlAccentColor();
    let dark = accent
        .blendedColorWithFraction_ofColor(0.4, &NSColor::blackColor())
        .unwrap_or(accent);
    let plate_view = plate(mtm, &dark.colorWithAlphaComponent(0.6));
    let hover = plate(mtm, &NSColor::whiteColor().colorWithAlphaComponent(0.1));
    let line = plate(mtm, &NSColor::separatorColor());
    if let Some(layer) = line.layer() {
        layer.setCornerRadius(0.0);
    }
    grid.addSubview(&hover);
    grid.addSubview(&plate_view);
    grid.addSubview(&line);

    let empty = label(mtm, "No apps found", 16.0);
    empty.setTextColor(Some(&NSColor::secondaryLabelColor()));
    empty.setHidden(true);
    card.addSubview(&empty);

    Views {
        panel,
        root,
        card,
        clock,
        date,
        field,
        scroll,
        grid,
        empty,
        plate: plate_view,
        hover,
        line,
    }
}

impl Controller {
    fn views(&self) -> &Views { self.ivars().views.get().expect("home's views are made with it") }

    fn observe(&self) {
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let local = NSNotificationCenter::defaultCenter();
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        unsafe {
            for name in [
                objc2_app_kit::NSWorkspaceActiveSpaceDidChangeNotification,
                objc2_app_kit::NSWorkspaceWillSleepNotification,
                objc2_app_kit::NSWorkspaceSessionDidResignActiveNotification,
            ] {
                workspace.addObserver_selector_name_object(
                    self,
                    sel!(systemChanged:),
                    Some(name),
                    None,
                );
            }
            local.addObserver_selector_name_object(
                self,
                sel!(systemChanged:),
                Some(objc2_app_kit::NSApplicationDidChangeScreenParametersNotification),
                None,
            );
            distributed.addObserver_selector_name_object(
                self,
                sel!(systemChanged:),
                Some(ns_string!("com.apple.screenIsLocked")),
                None,
            );
        }
    }

    fn open(&self, display: u32) {
        let wanted = display;
        let mtm = MainThreadMarker::from(self);
        let Some(screen) = NSScreen::screens(mtm)
            .iter()
            .find(|screen| screen.get_number().is_ok_and(|id| id.as_u32() == display))
        else {
            tracing::debug!(screen = wanted, "home: no such screen");
            return;
        };
        let stale = {
            let mut state = self.ivars().state.borrow_mut();
            if state.open {
                return;
            }
            state.open = true;
            state.generation += 1;
            state.front = NSWorkspace::sharedWorkspace().frontmostApplication();
            state.query.clear();
            state.pick = None;
            state.hover = None;
            state.catalog.stale()
        };
        if stale {
            (self.ivars().reread)();
        }
        super::set_open(true);
        let views = self.views();
        views.field.setStringValue(ns_string!(""));
        let frame = screen.frame();
        views.panel.setFrame_display(frame, false);
        let notch = screen.safeAreaInsets().top;
        self.place(frame, screen.visibleFrame(), notch);
        self.set_clock();
        self.refill();
        self.scroll_to_top();
        let panel = &views.panel;
        panel.setIgnoresMouseEvents(false);
        panel.setAlphaValue(0.0);
        panel.makeKeyAndOrderFront(None);
        panel.makeFirstResponder(Some(&views.field));
        self.fade_in();
    }

    fn fade_in(&self) {
        let views = self.views();
        let scale = crate::ui::grid_motion::time_scale();
        if scale <= 0.0 {
            views.panel.setAlphaValue(1.0);
            return;
        }
        let panel = views.panel.clone();
        let group = RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
            unsafe { context.as_ref() }.setDuration(FADE_IN * scale);
            let animator: Retained<HomePanel> = unsafe { msg_send![&*panel, animator] };
            animator.setAlphaValue(1.0);
        });
        NSAnimationContext::runAnimationGroup(&group);
        // The content rises into place.
        if let Some(layer) = views.card.layer() {
            let rise = objc2_quartz_core::CABasicAnimation::animationWithKeyPath(Some(ns_string!(
                "transform.translation.y"
            )));
            unsafe {
                rise.setFromValue(Some(&objc2_foundation::NSNumber::new_f64(-RISE)));
                rise.setToValue(Some(&objc2_foundation::NSNumber::new_f64(0.0)));
            }
            rise.setDuration(FADE_IN * scale * 1.5);
            rise.setTimingFunction(Some(
                &objc2_quartz_core::CAMediaTimingFunction::functionWithName(unsafe {
                    objc2_quartz_core::kCAMediaTimingFunctionEaseOut
                }),
            ));
            layer.addAnimation_forKey(&rise, Some(ns_string!("rise")));
        }
    }

    fn close(&self, why: Closed) {
        let (generation, front) = {
            let mut state = self.ivars().state.borrow_mut();
            if !state.open {
                return;
            }
            state.open = false;
            state.generation += 1;
            (state.generation, state.front.take())
        };
        super::set_open(false);
        let views = self.views();
        views.panel.setIgnoresMouseEvents(true);
        let give_back = move || {
            // macOS gives the keyboard back to the app in front; if it did
            // not, ask that app to take it.
            if why == Closed::Itself
                && let Some(front) = &front
                && !front.isActive()
            {
                #[allow(deprecated)]
                front.activateWithOptions(NSApplicationActivationOptions::empty());
            }
        };
        let scale = crate::ui::grid_motion::time_scale();
        if scale <= 0.0 {
            views.panel.orderOut(None);
            give_back();
            return;
        }
        let panel = views.panel.clone();
        let group = RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
            unsafe { context.as_ref() }.setDuration(FADE_OUT * scale);
            let animator: Retained<HomePanel> = unsafe { msg_send![&*panel, animator] };
            animator.setAlphaValue(0.0);
        });
        let weak = Weak::from_retained(&self.retain());
        let done = RcBlock::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().state.borrow().generation != generation {
                return;
            }
            this.views().panel.orderOut(None);
            give_back();
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&group, Some(&done));
    }

    fn configure(&self, settings: HomeSettings) {
        let close = {
            let state = self.ivars().state.borrow();
            state.open
                && (state.settings.style != settings.style
                    || state.settings.panel_share() != settings.panel_share())
        };
        if close {
            self.close(Closed::Elsewhere);
        }
        self.ivars().state.borrow_mut().settings = settings;
        if self.ivars().state.borrow().open {
            self.refill();
        }
    }

    fn set_catalog(&self, catalog: Catalog) {
        // The slots are by index into the old list.
        for (_, slot) in self.ivars().slots.borrow_mut().drain() {
            slot.view.removeFromSuperview();
        }
        let (open, count) = {
            let mut state = self.ivars().state.borrow_mut();
            state.catalog = catalog;
            state.pick = None;
            state.hover = None;
            (state.open, state.catalog.apps.len())
        };
        if open {
            self.refill();
        } else {
            // Made now, while home is closed, so opening does not wait for
            // the icons.
            let mtm = MainThreadMarker::from(self);
            for index in 0..count {
                let slot = self.make_slot(mtm, index);
                slot.view.setHidden(true);
                self.views().grid.addSubview(&slot.view);
                self.ivars().slots.borrow_mut().insert((false, index), slot);
            }
        }
    }

    /// Set the views' frames for a display of `frame` (AppKit's coordinates),
    /// `usable` its part without the menu bar and the Dock.
    fn place(&self, frame: CGRect, usable: CGRect, notch: f64) {
        let views = self.views();
        let size = frame.size;
        let bounds = CGRect::new(CGPoint::ZERO, size);
        views.root.setFrame(bounds);
        let style = self.ivars().state.borrow().settings.style;
        let share = self.ivars().state.borrow().settings.panel_share();
        let card = match style {
            HomeStyle::Full => bounds,
            HomeStyle::Window => {
                // The usable area in the root view's (flipped) coordinates.
                let usable = grid::Rect {
                    x: usable.origin.x - frame.origin.x,
                    y: size.height - (usable.origin.y - frame.origin.y) - usable.size.height,
                    w: usable.size.width,
                    h: usable.size.height,
                };
                let panel = grid::panel(usable, share);
                CGRect::new(CGPoint::new(panel.x, panel.y), CGSize::new(panel.w, panel.h))
            }
        };
        views.card.setFrame(card);
        if let Some(root) = views.root.layer() {
            let dim = (style == HomeStyle::Window)
                .then(|| NSColor::blackColor().colorWithAlphaComponent(DIM).CGColor());
            root.setBackgroundColor(dim.as_deref());
        }
        if let Some(layer) = views.card.layer() {
            let radius = if style == HomeStyle::Window {
                PANEL_RADIUS
            } else {
                0.0
            };
            layer.setCornerRadius(radius);
            layer.setMasksToBounds(radius > 0.0);
        }
        // The card's own coordinates are not flipped: y from its bottom.
        let (w, h) = (card.size.width, card.size.height);
        let mut top = h;
        let full = style == HomeStyle::Full;
        views.clock.setHidden(!full);
        views.date.setHidden(!full);
        if full {
            top -= notch.max(0.0) + 72.0;
            let clock_h = 78.0;
            views.clock.setFrame(CGRect::new(
                CGPoint::new(0.0, top - clock_h),
                CGSize::new(w, clock_h),
            ));
            top -= clock_h;
            views
                .date
                .setFrame(CGRect::new(CGPoint::new(0.0, top - 26.0), CGSize::new(w, 26.0)));
            top -= 26.0 + 28.0;
        } else {
            top -= 20.0;
        }
        let search_w = SEARCH_W.min(w - 2.0 * PAD);
        views.field.setFrame(CGRect::new(
            CGPoint::new((w - search_w) / 2.0, top - SEARCH_H),
            CGSize::new(search_w, SEARCH_H),
        ));
        top -= SEARCH_H + 20.0;
        let bottom = if full { 24.0 } else { 12.0 };
        let grid_w = (MOST_COLUMNS * SLOT_W + 2.0 * PAD).min(w);
        let scroll = CGRect::new(
            CGPoint::new((w - grid_w) / 2.0, bottom),
            CGSize::new(grid_w, (top - bottom).max(SLOT_H)),
        );
        views.scroll.setFrame(scroll);
        views
            .empty
            .setFrame(CGRect::new(CGPoint::new(0.0, top - 60.0), CGSize::new(w, 24.0)));
        // The room inside the scroll view, without a scroller that takes some.
        let inner = views.scroll.contentSize().width;
        let columns = grid::columns(inner);
        let mut state = self.ivars().state.borrow_mut();
        state.columns = columns;
        state.grid_x = ((inner - columns as f64 * SLOT_W) / 2.0).max(0.0);
    }

    fn set_clock(&self) {
        let views = self.views();
        let now = NSDate::now();
        let format = |template: &NSString| {
            let formatter = NSDateFormatter::new();
            formatter.setLocalizedDateFormatFromTemplate(template);
            formatter.stringFromDate(&now)
        };
        views.clock.setStringValue(&format(ns_string!("jmm")));
        views.date.setStringValue(&format(ns_string!("EEEEMMMMd")));
    }

    fn scroll_to_top(&self) {
        let views = self.views();
        views.grid.scrollPoint(CGPoint::ZERO);
    }

    /// The rows for what is searched, and the slot views in their places.
    fn refill(&self) {
        let (shown, columns, grid_x) = {
            let mut state = self.ivars().state.borrow_mut();
            let shown = shown_for(&state);
            if !state.query.trim().is_empty() {
                state.pick = shown.first();
            } else if state.pick.is_some_and(|pick| shown.get(pick).is_none()) {
                state.pick = None;
            }
            state.hover = None;
            state.shown = shown.clone();
            (shown, state.columns, state.grid_x)
        };
        let views = self.views();
        let mtm = MainThreadMarker::from(self);
        let scroll_w = views.scroll.contentSize().width;
        let rows = shown.rows.len();
        let height = (shown.row_top(rows) + GRID_TOP).max(views.scroll.contentSize().height);
        views.grid.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(scroll_w, height)));

        let mut slots = self.ivars().slots.borrow_mut();
        let mut wanted = std::collections::HashSet::new();
        for (row, apps) in shown.rows.iter().enumerate() {
            let frequent = shown.frequent_row && row == 0;
            for (col, &index) in apps.iter().enumerate() {
                let key = (frequent, index);
                wanted.insert(key);
                let slot = slots.entry(key).or_insert_with(|| self.make_slot(mtm, index));
                let rect = slot_rect(Pick { row, col }, &shown, grid_x);
                slot.view.setFrame(rect);
                if unsafe { slot.view.superview() }.is_none() {
                    views.grid.addSubview(&slot.view);
                }
                slot.view.setHidden(false);
            }
        }
        for (key, slot) in slots.iter() {
            if !wanted.contains(key) {
                slot.view.setHidden(true);
            }
        }
        drop(slots);
        views.line.setHidden(!shown.frequent_row);
        if shown.frequent_row {
            let y = GRID_TOP + SLOT_H + LINE_GAP / 2.0;
            views.line.setFrame(CGRect::new(
                CGPoint::new(grid_x + 8.0, y),
                CGSize::new(
                    (shown.rows[0].len().max(1) as f64 * SLOT_W - 16.0).max(SLOT_W),
                    1.0,
                ),
            ));
        }
        self.draw_sections(&shown, grid_x, columns);
        let none = rows == 0 && !self.ivars().state.borrow().query.trim().is_empty();
        views.empty.setHidden(!none);
        self.show_pick();
    }

    /// Each category's title, and a rounded box round its rows, behind them.
    fn draw_sections(&self, shown: &Shown, grid_x: f64, columns: usize) {
        let views = self.views();
        let mtm = MainThreadMarker::from(self);
        let mut sections = self.ivars().sections.borrow_mut();
        for view in sections.drain(..) {
            view.removeFromSuperview();
        }
        let width = columns as f64 * SLOT_W;
        for (title, start, end) in shown.sections() {
            let top = shown.row_top(start);
            let bottom = shown.row_top(end - 1) + SLOT_H;
            let frame = CGRect::new(
                CGPoint::new(grid_x - SECTION_OUT, top - 6.0),
                CGSize::new(width + 2.0 * SECTION_OUT, bottom - top + 10.0),
            );
            let section = NSView::initWithFrame(NSView::alloc(mtm), frame);
            section.setWantsLayer(true);
            if let Some(layer) = section.layer() {
                layer.setCornerRadius(14.0);
                layer.setBorderWidth(1.0);
                layer.setBorderColor(Some(&NSColor::separatorColor().CGColor()));
            }
            views.grid.addSubview_positioned_relativeTo(
                &section,
                objc2_app_kit::NSWindowOrderingMode::Below,
                None,
            );
            sections.push(section);
            let name = NSTextField::labelWithString(&NSString::from_str(title), mtm);
            name.setFont(Some(&NSFont::boldSystemFontOfSize(16.0)));
            name.setTextColor(Some(&NSColor::labelColor()));
            name.setFrame(CGRect::new(
                CGPoint::new(grid_x + 6.0, top - HEAD_H + 6.0),
                CGSize::new(width, 22.0),
            ));
            views.grid.addSubview(&name);
            sections.push(Retained::into_super(Retained::into_super(name)));
        }
    }

    fn make_slot(&self, mtm: MainThreadMarker, index: usize) -> Slot {
        let (name, path) = {
            let state = self.ivars().state.borrow();
            let app = &state.catalog.apps[index];
            (app.name.clone(), app.path.clone())
        };
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            CGRect::new(CGPoint::ZERO, CGSize::new(SLOT_W, SLOT_H)),
        );
        let icon = NSImageView::initWithFrame(
            NSImageView::alloc(mtm),
            CGRect::new(
                CGPoint::new((SLOT_W - ICON) / 2.0, SLOT_H - 8.0 - ICON),
                CGSize::new(ICON, ICON),
            ),
        );
        icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        icon.setImage(Some(&self.icon(&path)));
        view.addSubview(&icon);
        let name_view = NSTextField::wrappingLabelWithString(&NSString::from_str(&name), mtm);
        name_view.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        name_view.setAlignment(NSTextAlignment::Center);
        name_view.setTextColor(Some(&NSColor::labelColor()));
        name_view.setMaximumNumberOfLines(2);
        // Wrapped onto two lines, the second cut with an ellipsis.
        name_view.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        if let Some(cell) = name_view.cell() {
            cell.setTruncatesLastVisibleLine(true);
        }
        name_view.setFrame(CGRect::new(
            CGPoint::new(4.0, 6.0),
            CGSize::new(SLOT_W - 8.0, 34.0),
        ));
        view.addSubview(&name_view);
        Slot { view }
    }

    fn icon(&self, path: &PathBuf) -> Retained<NSImage> {
        self.ivars()
            .icons
            .borrow_mut()
            .entry(path.clone())
            .or_insert_with(|| {
                NSWorkspace::sharedWorkspace()
                    .iconForFile(&NSString::from_str(&path.to_string_lossy()))
            })
            .clone()
    }

    fn show_pick(&self) {
        let views = self.views();
        let (pick, hover, shown, grid_x) = {
            let state = self.ivars().state.borrow();
            (state.pick, state.hover, state.shown.clone(), state.grid_x)
        };
        let put = |view: &NSView, at: Option<Pick>| match at.filter(|at| shown.get(*at).is_some()) {
            Some(at) => {
                let rect = slot_rect(at, &shown, grid_x);
                view.setFrame(inset(rect, 6.0));
                view.setHidden(false);
            }
            None => view.setHidden(true),
        };
        put(&views.plate, pick);
        put(&views.hover, hover.filter(|hover| Some(*hover) != pick));
        if let Some(pick) = pick.filter(|pick| shown.get(*pick).is_some()) {
            let mut rect = slot_rect(pick, &shown, grid_x);
            // A category's first row brings its title into view with it.
            if shown.heads.iter().any(|(start, _)| *start == pick.row) {
                rect.origin.y -= HEAD_H;
                rect.size.height += HEAD_H;
            }
            views.grid.scrollRectToVisible(rect);
        }
    }

    fn search_changed(&self) {
        let text = self.views().field.stringValue().to_string();
        {
            let mut state = self.ivars().state.borrow_mut();
            if state.query == text {
                return;
            }
            state.query = text;
            state.pick = None;
        }
        self.refill();
        self.scroll_to_top();
        self.show_pick();
    }

    fn command(&self, view: &NSTextView, command: Sel) -> bool {
        let composing: bool = unsafe { msg_send![view, hasMarkedText] };
        if composing {
            return false;
        }
        let step = if command == sel!(moveUp:) {
            Some(Step::Up)
        } else if command == sel!(moveDown:) {
            Some(Step::Down)
        } else if command == sel!(moveLeft:) {
            Some(Step::Left)
        } else if command == sel!(moveRight:) {
            Some(Step::Right)
        } else {
            None
        };
        if let Some(step) = step {
            {
                let mut state = self.ivars().state.borrow_mut();
                state.pick = grid::step(&state.shown, state.pick, step);
            }
            self.show_pick();
            return true;
        }
        if command == sel!(insertNewline:) {
            let picked = {
                let state = self.ivars().state.borrow();
                state.pick.and_then(|pick| state.shown.get(pick))
            };
            if let Some(index) = picked {
                self.launch(index);
            }
            return true;
        }
        if command == sel!(cancelOperation:) {
            let searching = !self.ivars().state.borrow().query.is_empty();
            if searching {
                self.views().field.setStringValue(ns_string!(""));
                self.search_changed();
            } else {
                self.close(Closed::Itself);
            }
            return true;
        }
        false
    }

    fn slot_at(&self, point: CGPoint) -> Option<usize> {
        let state = self.ivars().state.borrow();
        slot_at(point, &state.shown, state.grid_x).and_then(|pick| state.shown.get(pick))
    }

    fn clicked(&self, point: CGPoint) {
        match self.slot_at(point) {
            Some(index) => self.launch(index),
            None => self.clicked_empty(),
        }
    }

    /// A click on home's empty part: closes it in the `full` style, where
    /// that part is the whole display; in the `window` style only a click
    /// outside the panel does.
    fn clicked_empty(&self) {
        if self.ivars().state.borrow().settings.style == HomeStyle::Full {
            self.close(Closed::Itself);
        }
    }

    fn hovered(&self, point: Option<CGPoint>) {
        {
            let mut state = self.ivars().state.borrow_mut();
            let hover = point.and_then(|point| slot_at(point, &state.shown, state.grid_x));
            if state.hover == hover {
                return;
            }
            state.hover = hover;
        }
        self.show_pick();
    }

    fn menu_at(&self, point: CGPoint, event: &NSEvent) {
        let Some(index) = self.slot_at(point) else { return };
        let (name, running) = {
            let state = self.ivars().state.borrow();
            let app = &state.catalog.apps[index];
            (app.name.clone(), !running_apps(&app.bundle_id).is_empty())
        };
        let mtm = MainThreadMarker::from(self);
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&name));
        menu.setAutoenablesItems(false);
        let add = |title: &str, action: Sel| {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    ns_string!(""),
                )
            };
            unsafe { item.setTarget(Some(self)) };
            item.setTag(index as isize);
            menu.addItem(&item);
        };
        add("Open", sel!(menuOpen:));
        add("Show in Finder", sel!(menuReveal:));
        let current = {
            let state = self.ivars().state.borrow();
            categories::category(&state.catalog.apps[index], &state.settings)
        };
        let kinds = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Category"));
        kinds.setAutoenablesItems(false);
        for (at, category) in HomeCategory::ALL.iter().enumerate() {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(category.label()),
                    Some(sel!(menuCategory:)),
                    ns_string!(""),
                )
            };
            unsafe { item.setTarget(Some(self)) };
            item.setTag((index * CATEGORY_TAG + at) as isize);
            item.setState(if *category == current { 1 } else { 0 });
            kinds.addItem(&item);
        }
        let category = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                ns_string!("Category"),
                None,
                ns_string!(""),
            )
        };
        category.setSubmenu(Some(&kinds));
        menu.addItem(&category);
        add("Hide from Home", sel!(menuHide:));
        if running {
            add("Quit", sel!(menuQuit:));
        }
        NSMenu::popUpContextMenu_withEvent_forView(&menu, event, &self.views().grid);
    }

    fn app(&self, index: usize) -> Option<(String, PathBuf)> {
        let state = self.ivars().state.borrow();
        state
            .catalog
            .apps
            .get(index)
            .map(|app| (app.bundle_id.clone(), app.path.clone()))
    }

    fn launch(&self, index: usize) {
        let Some((bundle_id, path)) = self.app(index) else {
            return;
        };
        {
            let mut state = self.ivars().state.borrow_mut();
            state.launches.record(&bundle_id, launches::now());
            if let Err(error) = state.launches.save(&state.launches_path) {
                tracing::warn!(%error, "home: could not write the opened apps");
            }
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let config = NSWorkspaceOpenConfiguration::configuration();
        config.setActivates(true);
        let done = RcBlock::new(move |_app: *mut NSRunningApplication, error: *mut NSError| {
            if let Some(error) = unsafe { error.as_ref() } {
                tracing::error!(
                    app = %bundle_id,
                    error = %error.localizedDescription(),
                    "home: could not open the app"
                );
            }
        });
        NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
            &url,
            &config,
            Some(&done),
        );
        self.close(Closed::Elsewhere);
    }

    fn reveal(&self, index: usize) {
        let Some((_, path)) = self.app(index) else { return };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSWorkspace::sharedWorkspace()
            .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
        self.close(Closed::Elsewhere);
    }

    fn hide_app(&self, index: usize) {
        let Some((bundle_id, _)) = self.app(index) else { return };
        {
            let mut state = self.ivars().state.borrow_mut();
            if !state.settings.hides(&bundle_id) {
                state.settings.hidden.push(bundle_id.clone());
            }
            state.pick = None;
        }
        (self.ivars().edit)(HomeEdit::Hide(bundle_id));
        self.refill();
    }

    fn set_category(&self, index: usize, category: HomeCategory) {
        let Some((bundle_id, _)) = self.app(index) else { return };
        {
            let mut state = self.ivars().state.borrow_mut();
            let categories = &mut state.settings.categories;
            categories.retain(|id, _| !id.eq_ignore_ascii_case(&bundle_id));
            categories.insert(bundle_id.clone(), category);
            // The rows move: what was picked is somewhere else now.
            state.pick = None;
        }
        (self.ivars().edit)(HomeEdit::Category(bundle_id, category));
        self.refill();
    }

    fn quit(&self, index: usize) {
        let Some((bundle_id, _)) = self.app(index) else { return };
        for app in running_apps(&bundle_id) {
            app.terminate();
        }
    }

    fn render(&self, frame: CGRect, usable: CGRect, query: &str, path: &str) -> bool {
        let views = self.views();
        views.panel.setFrame_display(frame, false);
        self.place(frame, usable, 0.0);
        self.set_clock();
        {
            let mut state = self.ivars().state.borrow_mut();
            state.query = query.to_owned();
            state.pick = None;
        }
        views.field.setStringValue(&NSString::from_str(query));
        self.refill();
        let root = &views.root;
        root.layoutSubtreeIfNeeded();
        let bounds = root.bounds();
        let Some(bitmap) = root.bitmapImageRepForCachingDisplayInRect(bounds) else {
            return false;
        };
        root.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
        let png = unsafe {
            bitmap.representationUsingType_properties(
                objc2_app_kit::NSBitmapImageFileType::PNG,
                &objc2_foundation::NSDictionary::new(),
            )
        };
        png.is_some_and(|png| png.writeToFile_atomically(&NSString::from_str(path), true))
    }
}

fn running_apps(bundle_id: &str) -> Vec<Retained<NSRunningApplication>> {
    NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id))
        .to_vec()
}

/// What the grid shows for `state`'s search: with none, the frequent row and
/// every app; with one, its results. Hidden apps are left out of both.
fn shown_for(state: &State) -> Shown {
    let apps = &state.catalog.apps;
    let settings = &state.settings;
    let visible = |index: &usize| !settings.hides(&apps[*index].bundle_id);
    if state.query.trim().is_empty() {
        let all: Vec<usize> = (0..apps.len()).filter(visible).collect();
        let frequent = if settings.frequent {
            let ids = all.iter().map(|&index| (index, apps[index].bundle_id.as_str()));
            state.launches.frequent(ids, launches::now(), state.columns)
        } else {
            Vec::new()
        };
        let groups: Vec<(String, Vec<usize>)> = if settings.categorize {
            HomeCategory::ALL
                .iter()
                .map(|&category| {
                    let members = all
                        .iter()
                        .copied()
                        .filter(|&index| categories::category(&apps[index], settings) == category)
                        .collect();
                    (category.label().to_owned(), members)
                })
                .collect()
        } else {
            vec![(String::new(), all)]
        };
        Shown::grouped(&frequent, &groups, state.columns)
    } else {
        let found: Vec<usize> =
            search::search(apps, &state.query).into_iter().filter(visible).collect();
        Shown::new(&[], &found, state.columns)
    }
}

fn slot_rect(pick: Pick, shown: &Shown, grid_x: f64) -> CGRect {
    CGRect::new(
        CGPoint::new(grid_x + pick.col as f64 * SLOT_W, shown.row_top(pick.row)),
        CGSize::new(SLOT_W, SLOT_H),
    )
}

fn slot_at(point: CGPoint, shown: &Shown, grid_x: f64) -> Option<Pick> {
    let col = ((point.x - grid_x) / SLOT_W).floor();
    if col < 0.0 {
        return None;
    }
    let row = (0..shown.rows.len()).find(|&row| {
        let top = shown.row_top(row);
        point.y >= top && point.y < top + SLOT_H
    })?;
    let pick = Pick { row, col: col as usize };
    shown.get(pick).map(|_| pick)
}

fn inset(rect: CGRect, by: f64) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x + by, rect.origin.y + by),
        CGSize::new(rect.size.width - 2.0 * by, rect.size.height - 2.0 * by),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_finds_the_slot_under_it() {
        let shown = Shown::new(&[3], &[0, 1, 2, 3], 3);
        let at = |x: f64, y: f64| slot_at(CGPoint::new(x, y), &shown, 10.0);
        assert_eq!(at(15.0, GRID_TOP + 1.0), Some(Pick { row: 0, col: 0 }));
        // The frequent row holds one app: nothing beside it.
        assert_eq!(at(15.0 + SLOT_W, GRID_TOP + 1.0), None);
        // Below the line.
        let second = shown.row_top(1) + 1.0;
        assert_eq!(at(15.0 + 2.0 * SLOT_W, second), Some(Pick { row: 1, col: 2 }));
        assert_eq!(at(5.0, second), None);
        assert_eq!(at(15.0, shown.row_top(1) - 2.0), None);
    }
}
