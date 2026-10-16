//! Moves between grid cells and grids, shown: the screen slides to the cell moved
//! to, or fades to another grid, and a small map of the grid shows where the
//! active cell went. The slide plays on an overlay over the display while rift
//! moves the windows under it: the cell left is a capture of the display taken
//! before they move, the cell arrived at is the wallpaper with a capture of each
//! of its windows where the layout puts them.

use std::fmt;
use std::time::{Duration, Instant};

use block2::RcBlock;
use cgs::{CgsWindow, CgsWindowError, WindowSurface, release_layer_tree, with_disabled_actions};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSColor, NSFont, NSPopUpMenuWindowLevel};
use objc2_core_foundation::{CFRetained, CFString, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGImage};
use objc2_foundation::{NSArray, NSError, NSNumber, NSString, NSValue};
use objc2_quartz_core::{
    CABasicAnimation, CAFrameRateRange, CALayer, CAMediaTiming, CAMediaTimingFunction,
    CATextLayer, CATransaction,
};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
    SCWindow,
};

use crate::common::collections::HashMap;

pub type Image = CFRetained<CGImage>;

/// Carries ScreenCaptureKit's results from its queue to the reactor.
pub struct SendBox<T>(pub T);
unsafe impl<T> Send for SendBox<T> {}

impl<T> fmt::Debug for SendBox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("SendBox") }
}

/// What the captures and timers report back to the reactor.
#[derive(Debug)]
pub enum Note {
    Content(SendBox<Option<Retained<SCShareableContent>>>),
    Wallpaper { display: u32, rect: CGRect, image: Option<SendBox<Image>> },
    Window { generation: u64, window: u32, image: Option<SendBox<Image>> },
    /// Start the slide with whatever captures have arrived.
    Start(u64),
    /// The slide is over: the windows are in place under it.
    End(u64),
    HudFade(u64),
    HudGone(u64),
}

static OVERVIEW_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Note whether the Overview is open: it shows moves itself, and a slide would
/// cover it.
pub fn set_overview_open(open: bool) {
    OVERVIEW_OPEN.store(open, std::sync::atomic::Ordering::Relaxed);
}

pub fn overview_open() -> bool { OVERVIEW_OPEN.load(std::sync::atomic::Ordering::Relaxed) }

/// The space between cells as the screen slides past them.
const GAP: f64 = 28.0;

fn color(r: f64, g: f64, b: f64, a: f64) -> CFRetained<CGColor> {
    CGColor::new_generic_rgb(r, g, b, a)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> CGRect {
    CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
}

fn set_contents(layer: &CALayer, image: &CGImage) {
    unsafe { layer.setContents(Some(&*(image as *const CGImage as *const AnyObject))) };
}

/// What ScreenCaptureKit can capture, fetched ahead of time: asking for it takes
/// longer than a slide can wait.
#[derive(Default)]
pub struct Shareable {
    content: Option<Retained<SCShareableContent>>,
    windows: HashMap<u32, Retained<SCWindow>>,
    fetched: Option<Instant>,
    fetching: bool,
    wallpapers: HashMap<u32, (CGRect, Image)>,
}

impl Shareable {
    pub fn has_content(&self) -> bool { self.content.is_some() }

    /// Whether what was fetched is older than `age`, and nothing is on its way.
    pub fn wants(&self, age: Duration) -> bool {
        !self.fetching && self.fetched.is_none_or(|at| at.elapsed() >= age)
    }

    /// Fetch again at the next chance.
    pub fn expire(&mut self) { self.fetched = None; }

    /// Wait out the age before asking again, as after a fetch.
    pub fn defer(&mut self) { self.fetched = Some(Instant::now()); }

    pub fn refresh(&mut self, send: impl Fn(Note) + Send + 'static) {
        self.fetching = true;
        let callback = RcBlock::new(move |content: *mut SCShareableContent, _: *mut NSError| {
            let content = unsafe { Retained::retain(content) };
            send(Note::Content(SendBox(content)));
        });
        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                false, false, &callback,
            );
        }
    }

    pub fn set_content(&mut self, content: Option<Retained<SCShareableContent>>) {
        self.fetching = false;
        self.fetched = Some(Instant::now());
        if let Some(content) = content {
            self.windows = unsafe { content.windows() }
                .iter()
                .map(|w| (unsafe { w.windowID() }, w))
                .collect();
            self.content = Some(content);
        }
    }

    fn display(&self, id: u32) -> Option<Retained<SCDisplay>> {
        let content = self.content.as_ref()?;
        unsafe { content.displays() }.iter().find(|d| unsafe { d.displayID() } == id)
    }

    fn window(&self, id: u32) -> Option<Retained<SCWindow>> { self.windows.get(&id).cloned() }

    pub fn knows_window(&self, id: u32) -> bool { self.window(id).is_some() }

    /// The display's `rect` (in its own points, from its top left) as it is now,
    /// leaving out the windows `leave_out`. Waits at most `wait`.
    pub fn capture_display_now(
        &self,
        display: u32,
        rect: CGRect,
        scale: f64,
        leave_out: &[u32],
        wait: Duration,
    ) -> Option<Image> {
        let display = self.display(display)?;
        let leave_out: Vec<_> = leave_out.iter().filter_map(|&id| self.window(id)).collect();
        let leave_out = NSArray::from_retained_slice(&leave_out);
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &leave_out,
            )
        };
        let config = configuration(rect.size, scale);
        unsafe { config.setSourceRect(rect) };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let callback = RcBlock::new(move |image: *mut CGImage, _: *mut NSError| {
            let image = std::ptr::NonNull::new(image).map(|i| unsafe { CFRetained::retain(i) });
            let _ = tx.try_send(image.map(SendBox));
        });
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&callback),
            );
        }
        rx.recv_timeout(wait).ok().flatten().map(|image| image.0)
    }

    /// The wallpaper behind the display's `rect`, kept from an earlier capture.
    pub fn wallpaper(&self, display: u32, rect: CGRect) -> Option<&Image> {
        self.wallpapers.get(&display).filter(|(r, _)| *r == rect).map(|(_, image)| image)
    }

    pub fn set_wallpaper(&mut self, display: u32, rect: CGRect, image: Option<Image>) {
        if let Some(image) = image {
            self.wallpapers.insert(display, (rect, image));
        }
    }

    /// Capture the wallpaper behind the display's `rect`. False when there is no
    /// wallpaper window to capture.
    pub fn capture_wallpaper(
        &self,
        display: u32,
        rect: CGRect,
        scale: f64,
        send: impl Fn(Note) + Send + 'static,
    ) -> bool {
        let Some(sc_display) = self.display(display) else { return false };
        let Some(content) = &self.content else { return false };
        let frame = unsafe { sc_display.frame() };
        // The Dock draws each display's wallpaper in a window below everything.
        let wallpaper: Vec<_> = unsafe { content.windows() }
            .iter()
            .filter(|w| unsafe {
                w.windowLayer() < 0
                    && w.isOnScreen()
                    && w.frame() == frame
                    && w.title().is_some_and(|t| t.to_string().starts_with("Wallpaper"))
            })
            .collect();
        if wallpaper.is_empty() {
            return false;
        }
        let windows = NSArray::from_retained_slice(&wallpaper);
        let filter = unsafe {
            SCContentFilter::initWithDisplay_includingWindows(
                SCContentFilter::alloc(),
                &sc_display,
                &windows,
            )
        };
        let config = configuration(rect.size, scale);
        unsafe { config.setSourceRect(rect) };
        let callback = RcBlock::new(move |image: *mut CGImage, _: *mut NSError| {
            let image = std::ptr::NonNull::new(image).map(|i| unsafe { CFRetained::retain(i) });
            send(Note::Wallpaper { display, rect, image: image.map(SendBox) });
        });
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&callback),
            );
        }
        true
    }

    /// Capture one window wherever it is, on screen or not. False when the
    /// window is not one ScreenCaptureKit knows yet.
    pub fn capture_window(
        &self,
        window: u32,
        size: CGSize,
        scale: f64,
        generation: u64,
        send: impl Fn(Note) + Send + 'static,
    ) -> bool {
        let Some(sc_window) = self.window(window) else { return false };
        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &sc_window)
        };
        let config = configuration(size, scale);
        unsafe {
            config.setIgnoreShadowsSingleWindow(true);
            config.setScalesToFit(true);
            config.setPreservesAspectRatio(false);
        }
        let callback = RcBlock::new(move |image: *mut CGImage, _: *mut NSError| {
            let image = std::ptr::NonNull::new(image).map(|i| unsafe { CFRetained::retain(i) });
            send(Note::Window { generation, window, image: image.map(SendBox) });
        });
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&callback),
            );
        }
        true
    }
}

fn configuration(size: CGSize, scale: f64) -> Retained<SCStreamConfiguration> {
    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth((size.width * scale).round().max(1.0) as usize);
        config.setHeight((size.height * scale).round().max(1.0) as usize);
        config.setShowsCursor(false);
        config.setCapturesAudio(false);
    }
    config
}

/// Where a move went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// To another cell of the same grid, this many columns and rows away.
    Cells { dx: isize, dy: isize },
    /// To another grid, later or earlier in number.
    Grid { forward: bool },
}

/// A window of the cell moved to, in the overlay's coordinates.
pub struct Tile {
    pub frame: CGRect,
    pub image: Option<Image>,
    /// The window's size when it was captured.
    pub captured: CGSize,
}

/// A window carried along into the cell moved to: from where it was to where it
/// lands, in the overlay's coordinates. It stays put while the cells slide.
pub struct Carried {
    pub from: CGRect,
    pub to: CGRect,
    pub image: Option<Image>,
    pub captured: CGSize,
}

/// The overlay a slide plays on, covering the display below the menu bar.
pub struct Pan {
    generation: u64,
    frame: CGRect,
    root: Retained<CALayer>,
    world: Retained<CALayer>,
    /// Where the cell last moved to lies in `world`; the slide heads there.
    offset: CGPoint,
    /// The last move's carried windows, replaced by the next move's.
    carried: Vec<Retained<CALayer>>,
    size: CGSize,
    scale: f64,
    // The surface must be detached before its WindowServer window drops.
    _surface: WindowSurface,
    _window: CgsWindow,
}

impl Drop for Pan {
    fn drop(&mut self) {
        release_layer_tree(&self.root);
        CATransaction::flush();
    }
}

impl Pan {
    /// Cover `frame` with `from`, the screen as it was, before the windows move.
    pub fn show(
        generation: u64,
        frame: CGRect,
        scale: f64,
        from: &CGImage,
    ) -> Result<Self, CgsWindowError> {
        let size = frame.size;
        let root = CALayer::layer();
        let world = CALayer::layer();
        let from_layer = CALayer::layer();
        with_disabled_actions(|| {
            root.setFrame(rect(0.0, 0.0, size.width, size.height));
            root.setContentsScale(scale);
            root.setGeometryFlipped(true);
            root.setMasksToBounds(true);
            root.setBackgroundColor(Some(&color(0.03, 0.03, 0.04, 1.0)));
            world.setAnchorPoint(CGPoint::new(0.0, 0.0));
            world.setPosition(CGPoint::new(0.0, 0.0));
            world.setBounds(rect(0.0, 0.0, size.width, size.height));
            from_layer.setFrame(rect(0.0, 0.0, size.width, size.height));
            from_layer.setContentsScale(scale);
            set_contents(&from_layer, from);
            world.addSublayer(&from_layer);
            root.addSublayer(&world);
        });
        let window = CgsWindow::new_compositor(frame, 0.0)?;
        window.set_resolution(scale)?;
        window.set_level(NSPopUpMenuWindowLevel as i32)?;
        let surface = WindowSurface::new_scaled(window.id(), root.bounds(), &root, scale)?;
        surface.flush();
        window.order_above(None)?;
        CATransaction::flush();
        Ok(Self {
            generation,
            frame,
            root,
            world,
            offset: CGPoint::new(0.0, 0.0),
            carried: Vec::new(),
            size,
            scale,
            _surface: surface,
            _window: window,
        })
    }

    pub fn generation(&self) -> u64 { self.generation }

    /// Whether a move on `frame` can go on from this slide.
    pub fn covers(&self, frame: CGRect) -> bool { self.frame == frame }

    /// Take over the slide for move `generation`, which goes on from where this
    /// one has got to.
    pub fn continue_as(&mut self, generation: u64) { self.generation = generation; }

    /// Slide (or fade) to the cell made of `wallpaper` and `tiles`, back to
    /// front, over `duration` seconds, from wherever an earlier slide has got to.
    pub fn run(
        &mut self,
        step: Step,
        wallpaper: Option<&CGImage>,
        tiles: &[Tile],
        carried: &[Carried],
        duration: f64,
    ) {
        let (w, h) = (self.size.width, self.size.height);
        if let Step::Cells { dx, dy } = step {
            self.offset.x += dx as f64 * (w + GAP);
            self.offset.y += dy as f64 * (h + GAP);
        }
        let offset = self.offset;
        let to = CALayer::layer();
        let mut moving = Vec::new();
        // One transaction: the layers must not reach the screen at their end
        // frames before their animations do.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        with_disabled_actions(|| {
            to.setFrame(rect(offset.x, offset.y, w, h));
            to.setMasksToBounds(true);
            to.setBackgroundColor(Some(&color(0.08, 0.08, 0.1, 1.0)));
            if let Some(image) = wallpaper {
                let back = CALayer::layer();
                back.setFrame(rect(0.0, 0.0, w, h));
                back.setContentsScale(self.scale);
                set_contents(&back, image);
                to.addSublayer(&back);
            }
            for tile in tiles {
                to.addSublayer(&self.tile_layer(tile.frame, tile.image.as_deref(), tile.captured));
            }
            for layer in self.carried.drain(..) {
                layer.removeFromSuperlayer();
            }
            for c in carried {
                let layer = self.tile_layer(c.from, c.image.as_deref(), c.captured);
                layer.setZPosition(10.0);
                self.root.addSublayer(&layer);
                layer.setFrame(c.to);
                moving.push((layer, c.from));
            }
        });
        match step {
            Step::Cells { .. } => {
                let shown = unsafe { self.world.presentationLayer() }
                    .map_or_else(|| self.world.position(), |shown| shown.position());
                self.world.addSublayer(&to);
                self.world.setPosition(CGPoint::new(-offset.x, -offset.y));
                animate(&self.world, "position", &point_value(shown), duration);
            }
            Step::Grid { forward } => {
                // Another context: the next grid comes forward over this one,
                // or an earlier one settles back.
                self.world.addSublayer(&to);
                let lean = if forward { 0.94 } else { 1.06 };
                animate(&to, "opacity", &NSNumber::new_f64(0.0), duration);
                animate(&to, "transform.scale", &NSNumber::new_f64(lean), duration);
            }
        }
        for (layer, from) in moving {
            glide(&layer, from, duration);
            self.carried.push(layer);
        }
        CATransaction::commit();
        CATransaction::flush();
    }

    fn tile_layer(&self, frame: CGRect, image: Option<&CGImage>, captured: CGSize) -> Retained<CALayer> {
        let layer = CALayer::layer();
        layer.setFrame(frame);
        layer.setContentsScale(self.scale);
        let same_size = (captured.width - frame.size.width).abs() < 2.0
            && (captured.height - frame.size.height).abs() < 2.0;
        match image {
            Some(image) if same_size => set_contents(&layer, image),
            _ => {
                // Not captured in time, or captured at another size while hidden:
                // a plain card where the window will be, what was captured at its
                // own size in its corner, as the window shows it until it redraws.
                layer.setBackgroundColor(Some(&color(0.16, 0.16, 0.19, 0.95)));
                layer.setCornerRadius(12.0);
                layer.setMasksToBounds(true);
                if let Some(image) = image {
                    let inner = CALayer::layer();
                    inner.setFrame(rect(0.0, 0.0, captured.width, captured.height));
                    inner.setContentsScale(self.scale);
                    set_contents(&inner, image);
                    layer.addSublayer(&inner);
                }
            }
        }
        layer
    }
}

fn point_value(point: CGPoint) -> Retained<NSValue> { unsafe { NSValue::valueWithPoint(point) } }

fn rect_value(rect: CGRect) -> Retained<NSValue> { unsafe { NSValue::valueWithRect(rect) } }

fn timing() -> Retained<CAMediaTimingFunction> {
    // Quick to start, settling gently: the screen is pushed, not dragged.
    CAMediaTimingFunction::functionWithControlPoints(0.3, 0.0, 0.2, 1.0)
}

/// Animate `key` of `layer`, already at its new value, from `from`.
fn animate(layer: &CALayer, key: &str, from: &AnyObject, duration: f64) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(key)));
    unsafe { animation.setFromValue(Some(from)) };
    animation.setDuration(duration);
    animation.setTimingFunction(Some(&timing()));
    animation.setPreferredFrameRateRange(CAFrameRateRange::new(60.0, 120.0, 120.0));
    layer.addAnimation_forKey(&animation, Some(&NSString::from_str(key)));
}

/// Move `layer`, already at its new frame, from `from`.
fn glide(layer: &CALayer, from: CGRect, duration: f64) {
    let center = CGPoint::new(
        from.origin.x + from.size.width / 2.0,
        from.origin.y + from.size.height / 2.0,
    );
    animate(layer, "position", &point_value(center), duration);
    animate(
        layer,
        "bounds",
        &rect_value(rect(0.0, 0.0, from.size.width, from.size.height)),
        duration,
    );
}

/// What the grid map shows.
#[derive(Clone, Debug, PartialEq)]
pub struct HudView {
    /// 0-based.
    pub grid: usize,
    pub rows: usize,
    pub columns: usize,
    /// The active cell, row and column.
    pub cell: (usize, usize),
    /// Which cells have windows, row by row.
    pub occupied: Vec<bool>,
    /// The cell the move left, when it was in the same grid.
    pub from: Option<(usize, usize)>,
}

const HUD_CELL: f64 = 34.0;
const HUD_GAP: f64 = 5.0;
const HUD_PAD: f64 = 14.0;
const HUD_NUMBER: f64 = 26.0;

/// The grid map: the grid's number beside its cells, the active one lit, shown a
/// moment after each move.
pub struct Hud {
    generation: u64,
    display: u32,
    root: Retained<CALayer>,
    content: Retained<CALayer>,
    number: Retained<CATextLayer>,
    cells: Vec<Retained<CALayer>>,
    highlight: Retained<CALayer>,
    shape: (usize, usize),
    grid: usize,
    // The surface must be detached before its WindowServer window drops.
    _surface: WindowSurface,
    _window: CgsWindow,
}

impl Drop for Hud {
    fn drop(&mut self) {
        release_layer_tree(&self.root);
        CATransaction::flush();
    }
}

impl Hud {
    /// Show the map on the display with id `display`, whose usable area is `screen`.
    pub fn show(
        generation: u64,
        display: u32,
        screen: CGRect,
        scale: f64,
        view: &HudView,
    ) -> Result<Self, CgsWindowError> {
        let aspect = screen.size.width / screen.size.height.max(1.0);
        let cell_size = CGSize::new(HUD_CELL, (HUD_CELL / aspect).round());
        let (rows, columns) = (view.rows as f64, view.columns as f64);
        let map_w = columns * cell_size.width + (columns - 1.0) * HUD_GAP;
        let map_h = rows * cell_size.height + (rows - 1.0) * HUD_GAP;
        let size = CGSize::new(HUD_PAD * 2.0 + HUD_NUMBER + 8.0 + map_w, HUD_PAD * 2.0 + map_h);
        let frame = rect(
            (screen.origin.x + (screen.size.width - size.width) / 2.0).round(),
            (screen.origin.y + screen.size.height * 0.82 - size.height).round(),
            size.width,
            size.height,
        );
        let root = CALayer::layer();
        let content = CALayer::layer();
        let number = CATextLayer::layer();
        let highlight = CALayer::layer();
        let mut cells = Vec::new();
        with_disabled_actions(|| {
            root.setFrame(rect(0.0, 0.0, size.width, size.height));
            root.setContentsScale(scale);
            root.setGeometryFlipped(true);
            root.setBackgroundColor(Some(&color(0.07, 0.07, 0.09, 0.62)));
            root.setCornerRadius(14.0);
            content.setFrame(rect(0.0, 0.0, size.width, size.height));
            number.setFrame(rect(HUD_PAD, (size.height - 30.0) / 2.0, HUD_NUMBER, 30.0));
            number.setContentsScale(scale);
            let font = NSFont::boldSystemFontOfSize(22.0);
            unsafe {
                number.setFont(Some(&*(Retained::as_ptr(&font) as *const CFType)));
                number.setAlignmentMode(objc2_quartz_core::kCAAlignmentCenter);
            }
            number.setFontSize(22.0);
            number.setForegroundColor(Some(&color(1.0, 1.0, 1.0, 0.92)));
            content.addSublayer(&number);
            let left = HUD_PAD + HUD_NUMBER + 8.0;
            for row in 0..view.rows {
                for column in 0..view.columns {
                    let cell = CALayer::layer();
                    cell.setFrame(rect(
                        left + column as f64 * (cell_size.width + HUD_GAP),
                        HUD_PAD + row as f64 * (cell_size.height + HUD_GAP),
                        cell_size.width,
                        cell_size.height,
                    ));
                    cell.setCornerRadius(3.0);
                    content.addSublayer(&cell);
                    cells.push(cell);
                }
            }
            highlight.setCornerRadius(3.0);
            highlight.setBackgroundColor(Some(&NSColor::controlAccentColor().CGColor()));
            content.addSublayer(&highlight);
            root.addSublayer(&content);
        });
        let window = CgsWindow::new_compositor(frame, 14.0)?;
        window.set_resolution(scale)?;
        window.set_level(NSPopUpMenuWindowLevel as i32 + 1)?;
        window.set_blur(20, None)?;
        let surface = WindowSurface::new_scaled(window.id(), root.bounds(), &root, scale)?;
        let mut hud = Self {
            generation,
            display,
            root,
            content,
            number,
            cells,
            highlight,
            shape: (view.rows, view.columns),
            grid: view.grid,
            _surface: surface,
            _window: window,
        };
        hud.draw(view, true);
        hud._surface.flush();
        hud._window.order_above(None)?;
        CATransaction::flush();
        Ok(hud)
    }

    pub fn generation(&self) -> u64 { self.generation }

    /// Hide the map, or show it again, at once: a capture of the screen must not
    /// show it.
    pub fn set_visible(&self, visible: bool) {
        let _ = self._window.set_alpha(if visible { 1.0 } else { 0.0 });
    }

    /// Whether this map can show `view` on `display` in place.
    pub fn fits(&self, display: u32, view: &HudView) -> bool {
        self.display == display && self.shape == (view.rows, view.columns)
    }

    /// Show `view`, the move it made played on the map.
    pub fn update(&mut self, generation: u64, view: &HudView) {
        self.generation = generation;
        with_disabled_actions(|| {
            self.root.removeAllAnimations();
            self.root.setOpacity(1.0);
        });
        self.draw(view, false);
        CATransaction::flush();
    }

    fn cell_frame(&self, (row, column): (usize, usize)) -> CGRect {
        self.cells
            .get(row * self.shape.1 + column)
            .map(|cell| cell.frame())
            .unwrap_or_default()
    }

    fn draw(&mut self, view: &HudView, first: bool) {
        let grid_changed = view.grid != self.grid;
        let forward = view.grid > self.grid;
        self.grid = view.grid;
        let to = self.cell_frame(view.cell);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let text = CFString::from_str(&(view.grid + 1).to_string());
        unsafe { self.number.setString(Some(&*(text.as_ref() as *const _ as *const AnyObject))) };
        for (cell, &occupied) in self.cells.iter().zip(&view.occupied) {
            let alpha = if occupied { 0.34 } else { 0.12 };
            cell.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, alpha)));
        }
        let from = unsafe { self.highlight.presentationLayer() }.map(|l| l.frame());
        self.highlight.setFrame(to);
        if grid_changed && !first {
            // Another grid: the map comes in from the side it lies on.
            let shift = if forward { 14.0 } else { -14.0 };
            let at = self.content.position();
            animate(
                &self.content,
                "position",
                &point_value(CGPoint::new(at.x + shift, at.y)),
                0.2,
            );
            animate(&self.content, "opacity", &NSNumber::new_f64(0.2), 0.2);
        } else {
            let from = match (first, view.from) {
                (true, Some(cell)) => Some(self.cell_frame(cell)),
                (true, None) => None,
                (false, _) => from.filter(|f| f.size.width > 0.0),
            };
            if let Some(from) = from {
                glide(&self.highlight, from, 0.2);
            }
        }
        CATransaction::commit();
    }

    /// Fade the map out; it is dropped after.
    pub fn fade(&self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.root.setOpacity(0.0);
        animate(&self.root, "opacity", &NSNumber::new_f64(1.0), 0.18);
        CATransaction::commit();
        CATransaction::flush();
    }
}
