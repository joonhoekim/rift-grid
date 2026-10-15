//! Window borders: click-through WindowServer windows around each bordered
//! window, ordered just above it, drawing a ring outside its edge. A border is
//! four bands around the window rather than one window over it: a WindowServer
//! window costs its whole area at the display's scale, and the middle of a
//! border is empty.

use cgs::{CgsWindow, CgsWindowError, WindowSurface, WindowTransaction, with_disabled_actions};
use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2::runtime::AnyObject;
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColor, CGColorSpace, CGContext,
    CGImage, CGImageAlphaInfo, CGMutablePath,
};
use objc2_quartz_core::{
    CALayer, CAMediaTimingFunction, CAShapeLayer, CATransaction,
};
use tracing::warn;

use crate::common::collections::{HashMap, HashSet};
use crate::common::config::BorderSettings;
use crate::sys::window_server::WindowServerId;

/// A window to draw a border around, in WindowServer coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderTarget {
    pub window: WindowServerId,
    pub frame: CGRect,
    pub focused: bool,
    /// The frames of the windows above this one. A border window cannot be ordered
    /// just above a window of another app, so it sits above them all and leaves out
    /// what those windows cover instead.
    pub covered_by: Vec<CGRect>,
}

/// `base` without `holes`, as rectangles that do not overlap.
pub fn subtract(base: CGRect, holes: &[CGRect]) -> Vec<CGRect> {
    let mut parts = vec![base];
    for hole in holes {
        let mut next = Vec::with_capacity(parts.len() + 3);
        for part in parts {
            let (px0, py0) = (part.origin.x, part.origin.y);
            let (px1, py1) = (px0 + part.size.width, py0 + part.size.height);
            let hx0 = hole.origin.x.max(px0);
            let hy0 = hole.origin.y.max(py0);
            let hx1 = (hole.origin.x + hole.size.width).min(px1);
            let hy1 = (hole.origin.y + hole.size.height).min(py1);
            if hx0 >= hx1 || hy0 >= hy1 {
                next.push(part);
                continue;
            }
            let mut keep = |x0: f64, y0: f64, x1: f64, y1: f64| {
                if x1 > x0 && y1 > y0 {
                    next.push(CGRect::new(CGPoint::new(x0, y0), CGSize::new(x1 - x0, y1 - y0)));
                }
            };
            keep(px0, py0, px1, hy0);
            keep(px0, hy1, px1, py1);
            keep(px0, hy0, hx0, hy1);
            keep(hx1, hy0, px1, hy1);
        }
        parts = next;
    }
    parts
}

pub use crate::common::config::borders::{Rgba, format_color, parse_color};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderStyle {
    pub width: f64,
    pub radius: f64,
    pub active: Rgba,
    /// None leaves unfocused windows without a border.
    pub inactive: Option<Rgba>,
    pub glow: bool,
    pub glow_radius: f64,
    /// How long the focused border slides; zero jumps.
    pub animation: std::time::Duration,
}

impl BorderStyle {
    pub fn new(settings: &BorderSettings) -> Self {
        let defaults = BorderSettings::default();
        let active = parse_color(&settings.active_color).unwrap_or_else(|| {
            warn!(color = %settings.active_color, "Unreadable border color; using the default");
            parse_color(&defaults.active_color).unwrap()
        });
        let inactive = match settings.inactive_color.trim() {
            "" => None,
            text => parse_color(text).or_else(|| {
                warn!(color = %text, "Unreadable border color; drawing no unfocused borders");
                None
            }),
        };
        Self {
            width: settings.width.clamp(0.5, 20.0),
            radius: settings.radius.clamp(0.0, 40.0),
            active,
            inactive,
            glow: settings.glow,
            glow_radius: settings.glow_radius.clamp(1.0, 40.0),
            animation: std::time::Duration::from_secs_f64(
                settings.animation_ms.clamp(0.0, 1000.0) / 1000.0,
            ),
        }
    }

    fn color(&self, focused: bool) -> Option<Rgba> {
        if focused { Some(self.active) } else { self.inactive }
    }

    /// Room around the ring for the glow to fade out in.
    pub(crate) fn pad(&self) -> f64 { if self.glow { self.glow_radius * 2.0 } else { 0.0 } }

    /// The ring around a window, in the same coordinates as the window.
    pub fn ring_around(&self, window: CGRect) -> CGRect {
        let w = self.width;
        CGRect::new(
            CGPoint::new(window.origin.x - w, window.origin.y - w),
            CGSize::new(window.size.width + 2.0 * w, window.size.height + 2.0 * w),
        )
    }

    /// The window inside the canvas.
    pub fn hole(&self, frame: CGRect) -> CGRect {
        let inset = self.pad() + self.width;
        CGRect::new(CGPoint::new(inset, inset), frame.size)
    }

    /// The overlay window's frame for a window at `frame`.
    pub fn canvas(&self, frame: CGRect) -> CGRect {
        let out = self.width + self.pad();
        CGRect::new(
            CGPoint::new(frame.origin.x - out, frame.origin.y - out),
            CGSize::new(frame.size.width + 2.0 * out, frame.size.height + 2.0 * out),
        )
    }

    /// The ring inside the canvas: the window's frame grown by the width.
    pub fn ring(&self, frame: CGRect) -> CGRect {
        let pad = self.pad();
        CGRect::new(
            CGPoint::new(pad, pad),
            CGSize::new(frame.size.width + 2.0 * self.width, frame.size.height + 2.0 * self.width),
        )
    }
}

fn cg_color([r, g, b, a]: Rgba) -> CFRetained<CGColor> { CGColor::new_generic_rgb(r, g, b, a) }

/// A rounded rectangle's corner radius, kept within what CoreGraphics accepts.
fn corner(rect: CGRect, radius: f64) -> f64 {
    radius.min(rect.size.width / 2.0).min(rect.size.height / 2.0).max(0.0)
}

/// The glow around a ring as a nine-slice image: corners `corner` points square,
/// a stretchable point between them. Drawn once rather than by a CoreAnimation
/// shadow, which renders from what lies in each band's window and so would differ
/// at the bands' ends.
fn glow_image(style: &BorderStyle, color: Rgba) -> Option<(CFRetained<CGImage>, f64)> {
    thread_local! {
        static CACHE: std::cell::RefCell<Option<(String, CFRetained<CGImage>, f64)>> =
            const { std::cell::RefCell::new(None) };
    }
    let key = format!("{:?}", (color, style.width, style.radius, style.glow_radius));
    if let Some(hit) = CACHE.with_borrow(|c| {
        c.as_ref().filter(|(k, ..)| *k == key).map(|(_, image, corner)| (image.clone(), *corner))
    }) {
        return Some(hit);
    }
    const SCALE: f64 = 2.0;
    let pad = style.pad();
    // The corner holds its curve and everything the blur spreads from it, so the
    // stretched edges carry only the straight sides' glow.
    let corner_size = (2.0 * pad + style.width + style.radius).ceil() + 1.0;
    let side = 2.0 * corner_size + 1.0;
    let pixels = (side * SCALE) as usize;
    let space = CGColorSpace::new_device_rgb()?;
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            pixels,
            pixels,
            8,
            pixels * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    let cg = Some(&*context);
    CGContext::scale_ctm(cg, SCALE, SCALE);
    let all = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(side, side));
    let ring = grown(all, -pad);
    let hole = grown(ring, -style.width);
    // Never inside the window.
    CGContext::add_path(cg, Some(&outside(all.size, hole, style.radius)));
    CGContext::eo_clip(cg);
    let outer = Ring::outer_radius(style, ring);
    let plate = CGMutablePath::new();
    unsafe { CGMutablePath::add_rounded_rect(Some(&plate), std::ptr::null(), ring, outer, outer) };
    CGContext::set_shadow_with_color(
        cg,
        CGSize::new(0.0, 0.0),
        3.0 * style.glow_radius,
        Some(&cg_color(color)),
    );
    CGContext::set_rgb_fill_color(cg, color[0], color[1], color[2], color[3]);
    CGContext::add_path(cg, Some(&plate));
    CGContext::fill_path(cg);
    let image = CGBitmapContextCreateImage(cg)?;
    CACHE.with_borrow_mut(|c| *c = Some((key, image.clone(), corner_size)));
    Some((image, corner_size))
}

/// Everything in `canvas` but the rounded `hole`, so the glow never reaches into
/// the window it surrounds.
fn outside(canvas: CGSize, hole: CGRect, radius: f64) -> CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    let r = corner(hole, radius);
    unsafe {
        CGMutablePath::add_rect(
            Some(&path),
            std::ptr::null(),
            CGRect::new(CGPoint::new(0.0, 0.0), canvas),
        );
        CGMutablePath::add_rounded_rect(Some(&path), std::ptr::null(), hole, r, r);
    }
    path
}

/// The layers that draw one ring. A group the size of the ring and its glow holds
/// the ring and, below it, the glow, a nine-slice image stretched over the group.
/// Masks render offscreen at their layer's size, so they sit on the group, never
/// on a whole display's root.
struct Ring {
    root: Retained<CALayer>,
    group: Retained<CALayer>,
    glow: Retained<CALayer>,
    ring: Retained<CALayer>,
    /// Leaves out what windows above the target cover.
    clip: Retained<CAShapeLayer>,
}

fn grown(rect: CGRect, by: f64) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x - by, rect.origin.y - by),
        CGSize::new(rect.size.width + 2.0 * by, rect.size.height + 2.0 * by),
    )
}

impl Ring {
    fn new(size: CGSize) -> Self {
        let root = CALayer::layer();
        let group = CALayer::layer();
        let glow = CALayer::layer();
        let ring = CALayer::layer();
        let clip = CAShapeLayer::layer();
        with_disabled_actions(|| {
            root.setAnchorPoint(CGPoint::new(0.0, 0.0));
            root.setPosition(CGPoint::new(0.0, 0.0));
            root.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), size));
            root.setContentsScale(2.0);
            root.setOpaque(false);
            // Top-left origin, like the WindowServer frames everything is placed by.
            // Unflipped, anything not symmetric about the window (what covers it, the
            // ends of a slide) lands mirrored top to bottom.
            root.setGeometryFlipped(true);
            glow.setContentsScale(2.0);
            glow.setHidden(true);
            group.addSublayer(&glow);
            group.addSublayer(&ring);
            root.addSublayer(&group);
        });
        Self { root, group, glow, ring, clip }
    }

    fn outer_radius(style: &BorderStyle, ring: CGRect) -> f64 {
        if style.radius > 0.0 { corner(ring, style.radius + style.width) } else { 0.0 }
    }

    /// Draw the ring around `window`, in this surface's coordinates. With `slide`,
    /// the ring moves there over that many seconds and the glow waits.
    fn draw(
        &self,
        style: &BorderStyle,
        window: CGRect,
        color: Rgba,
        glow: bool,
        slide: Option<f64>,
    ) {
        let pad = style.pad();
        let ring = style.ring_around(window);
        let group = grown(ring, pad);
        let inner = CGRect::new(CGPoint::new(pad, pad), ring.size);
        let outer = Self::outer_radius(style, ring);
        with_disabled_actions(|| {
            self.group.setHidden(false);
            self.ring.setBorderWidth(style.width);
            self.ring.setBorderColor(Some(&cg_color(color)));
            if slide.is_none() {
                self.group.setFrame(group);
                self.ring.setFrame(inner);
                self.ring.setCornerRadius(outer);
            }
            let image = (glow && style.glow && slide.is_none())
                .then(|| glow_image(style, color))
                .flatten();
            self.glow.setHidden(image.is_none());
            if let Some((image, corner)) = image {
                let side = 2.0 * corner + 1.0;
                self.glow.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), group.size));
                unsafe {
                    self.glow.setContents(Some(&*(&*image as *const CGImage as *const AnyObject)));
                }
                self.glow.setContentsCenter(CGRect::new(
                    CGPoint::new(corner / side, corner / side),
                    CGSize::new(1.0 / side, 1.0 / side),
                ));
            }
        });
        if let Some(duration) = slide {
            CATransaction::begin();
            CATransaction::setAnimationDuration(duration);
            // Fast out, gentle landing, as a spring that does not overshoot.
            CATransaction::setAnimationTimingFunction(Some(
                &CAMediaTimingFunction::functionWithControlPoints(0.2, 0.9, 0.25, 1.0),
            ));
            self.group.setFrame(group);
            self.ring.setFrame(inner);
            self.ring.setCornerRadius(outer);
            CATransaction::commit();
        }
    }

    /// Put the ring at `ring` (surface coordinates) at once, to slide from there.
    fn jump(&self, style: &BorderStyle, ring: CGRect) {
        let pad = style.pad();
        with_disabled_actions(|| {
            self.group.removeAllAnimations();
            self.ring.removeAllAnimations();
            self.group.setFrame(grown(ring, pad));
            self.ring.setFrame(CGRect::new(CGPoint::new(pad, pad), ring.size));
        });
    }

    /// Show only what `covered_by` (in this surface's coordinates) leaves visible;
    /// all of it when nothing covers it.
    fn clip(&self, covered_by: &[CGRect]) {
        let group = self.group.frame();
        let bounds = CGRect::new(CGPoint::new(0.0, 0.0), group.size);
        let covers: Vec<_> = covered_by.iter().map(|r| local(*r, group.origin)).collect();
        let visible = subtract(bounds, &covers);
        with_disabled_actions(|| {
            if visible.len() == 1 && visible[0] == bounds {
                unsafe { self.group.setMask(None) };
                return;
            }
            let path = CGMutablePath::new();
            for rect in &visible {
                unsafe { CGMutablePath::add_rect(Some(&path), std::ptr::null(), *rect) };
            }
            self.clip.setFrame(bounds);
            self.clip.setPath(Some(&path));
            unsafe { self.group.setMask(Some(&self.clip)) };
        });
    }

    /// Where the ring is right now, mid-slide included: the presentation layers have
    /// the animation's current values.
    fn on_screen(&self, style: &BorderStyle) -> Option<CGRect> {
        if self.group.isHidden() {
            return None;
        }
        let group =
            unsafe { self.group.presentationLayer() }.map_or(self.group.frame(), |l| l.frame());
        Some(grown(group, -style.pad()))
    }
}

/// The parts of a canvas the ring and its glow reach: bands `depth` deep along
/// each edge, or the whole canvas when the bands would meet.
fn bands(canvas: CGSize, depth: f64) -> Vec<CGRect> {
    let rect = |x, y, w, h| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
    let (w, h) = (canvas.width, canvas.height);
    if 2.0 * depth >= w || 2.0 * depth >= h {
        return vec![rect(0.0, 0.0, w, h)];
    }
    vec![
        rect(0.0, 0.0, w, depth),
        rect(0.0, h - depth, w, depth),
        rect(0.0, depth, depth, h - 2.0 * depth),
        rect(w - depth, depth, depth, h - 2.0 * depth),
    ]
}

impl BorderStyle {
    /// How far into the canvas the ring and its glow reach from each edge: the
    /// glow's room, the ring, and the window's corners, which the ring follows in.
    fn depth(&self) -> f64 { (self.pad() + self.width + self.radius).ceil() + 1.0 }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGSWindowSetShadowProperties(window: u32, properties: *const AnyObject) -> i32;
}

/// Turn off a window's WindowServer shadow. The no-shadow tag does not, and with a
/// border in bands each band's shadow falls on the next.
fn without_shadow(window: &CgsWindow) {
    let key = objc2_foundation::NSString::from_str("com.apple.WindowShadowDensity");
    let none = objc2_foundation::NSNumber::new_i64(0);
    let properties = objc2_foundation::NSDictionary::from_slices(&[&*key], &[&*none]);
    let result = unsafe {
        CGSWindowSetShadowProperties(window.id(), Retained::as_ptr(&properties).cast())
    };
    if result != 0 {
        warn!(result, "Border window keeps its shadow");
    }
}

/// One band of a border: an overlay window showing that part of the ring.
struct Band {
    // The surface must be detached before its WindowServer window drops.
    surface: WindowSurface,
    window: CgsWindow,
    ring: Ring,
    /// Where the band is in the canvas.
    rect: CGRect,
}

impl Band {
    fn new(canvas: CGRect, rect: CGRect) -> Result<Self, CgsWindowError> {
        let window = CgsWindow::new_compositor(shifted(rect, canvas.origin), 0.0)?;
        window.set_alpha(1.0)?;
        window.set_tags(1 << 3)?;
        without_shadow(&window);
        let ring = Ring::new(rect.size);
        let surface = WindowSurface::new(
            window.id(),
            CGRect::new(CGPoint::new(0.0, 0.0), rect.size),
            &ring.root,
        )?;
        let band = Self { surface, window, ring, rect };
        band.show_rect();
        Ok(band)
    }

    /// The band's layers draw in canvas coordinates; its root shows its part.
    fn show_rect(&self) {
        with_disabled_actions(|| self.ring.root.setBounds(self.rect));
    }
}

/// An unfocused window's border: bands around the window, ordered just above it.
struct Border {
    bands: Vec<Band>,
    canvas: CGRect,
    target: BorderTarget,
    visible: bool,
}

fn shifted(rect: CGRect, by: CGPoint) -> CGRect {
    CGRect::new(CGPoint::new(rect.origin.x + by.x, rect.origin.y + by.y), rect.size)
}

fn local(rect: CGRect, origin: CGPoint) -> CGRect {
    shifted(rect, CGPoint::new(-origin.x, -origin.y))
}

impl Border {
    fn new(style: &BorderStyle, target: BorderTarget) -> Result<Self, CgsWindowError> {
        let canvas = style.canvas(target.frame);
        let bands = bands(canvas.size, style.depth())
            .into_iter()
            .map(|rect| Band::new(canvas, rect))
            .collect::<Result<_, _>>()?;
        let border = Self { bands, canvas, target, visible: true };
        border.draw(style);
        border.order()?;
        Ok(border)
    }

    fn order(&self) -> Result<(), CgsWindowError> {
        for band in &self.bands {
            band.window.order_above(Some(self.target.window.as_u32()))?;
        }
        Ok(())
    }

    fn set_visible(&mut self, visible: bool) -> Result<(), CgsWindowError> {
        if self.visible != visible {
            for band in &self.bands {
                band.window.set_alpha(if visible { 1.0 } else { 0.0 })?;
            }
            self.visible = visible;
        }
        Ok(())
    }

    fn covers(&self) -> Vec<CGRect> {
        self.target.covered_by.iter().map(|r| local(*r, self.canvas.origin)).collect()
    }

    fn draw(&self, style: &BorderStyle) {
        let color = style.color(self.target.focused).unwrap_or([0.0; 4]);
        let window = local(self.target.frame, self.canvas.origin);
        let covers = self.covers();
        for band in &self.bands {
            band.show_rect();
            band.ring.draw(style, window, color, self.target.focused, None);
            band.ring.clip(&covers);
        }
        CATransaction::flush();
    }

    fn update(
        &mut self,
        style: &BorderStyle,
        target: BorderTarget,
        reorder: bool,
    ) -> Result<(), CgsWindowError> {
        let previous = std::mem::replace(&mut self.target, target);
        let canvas = style.canvas(self.target.frame);
        let resized = canvas.size != self.canvas.size;
        let moved = canvas.origin != self.canvas.origin;
        if resized {
            let rects = bands(canvas.size, style.depth());
            if rects.len() == self.bands.len() {
                let transaction = WindowTransaction::new()?;
                for (band, rect) in self.bands.iter_mut().zip(rects) {
                    band.rect = rect;
                    transaction.set_rounded_frame(&band.window, shifted(rect, canvas.origin), 0.0)?;
                    band.surface.set_bounds_in(
                        &transaction,
                        CGRect::new(CGPoint::new(0.0, 0.0), rect.size),
                    );
                }
                transaction.commit();
            } else {
                // The bands meet or part: new ones, shown as the old ones were.
                self.bands = rects
                    .into_iter()
                    .map(|rect| Band::new(canvas, rect))
                    .collect::<Result<_, _>>()?;
                if !self.visible {
                    for band in &self.bands {
                        band.window.set_alpha(0.0)?;
                    }
                }
                self.canvas = canvas;
                self.draw(style);
                return self.order();
            }
        } else if moved {
            // Only moved: the layers keep their place in the bands.
            let transaction = WindowTransaction::new()?;
            for band in &self.bands {
                transaction.move_window(&band.window, shifted(band.rect, canvas.origin).origin);
            }
            transaction.commit();
        }
        self.canvas = canvas;
        if resized || previous.focused != self.target.focused {
            self.draw(style);
        } else if moved || previous.covered_by != self.target.covered_by {
            let covers = self.covers();
            for band in &self.bands {
                band.ring.clip(&covers);
            }
            CATransaction::flush();
        }
        if reorder {
            self.order()?;
        }
        Ok(())
    }
}

/// An overlay that only exists to carry the focused ring from one window to the
/// next. It is hidden between slides and sized to both ends at the start of each.
struct Slider {
    // The surface must be detached before its WindowServer window drops.
    surface: WindowSurface,
    window: CgsWindow,
    canvas: CGRect,
    ring: Ring,
}

impl Slider {
    fn new(canvas: CGRect) -> Result<Self, CgsWindowError> {
        let window = CgsWindow::new_compositor(canvas, 0.0)?;
        window.set_alpha(0.0)?;
        window.set_tags(1 << 3)?;
        without_shadow(&window);
        let ring = Ring::new(canvas.size);
        let bounds = CGRect::new(CGPoint::new(0.0, 0.0), canvas.size);
        let surface = WindowSurface::new(window.id(), bounds, &ring.root)?;
        Ok(Self { surface, window, canvas, ring })
    }

    /// Slide from `from` (a ring, global) to around `to`, over `duration`. Set up
    /// while hidden, then shown, so it never appears anywhere but `from`.
    fn start(
        &mut self,
        style: &BorderStyle,
        from: CGRect,
        to: &BorderTarget,
        duration: f64,
    ) -> Result<(), CgsWindowError> {
        let canvas = union(grown(from, style.pad()), style.canvas(to.frame));
        if canvas != self.canvas {
            let transaction = WindowTransaction::new()?;
            transaction.set_rounded_frame(&self.window, canvas, 0.0)?;
            self.surface
                .set_bounds_in(&transaction, CGRect::new(CGPoint::new(0.0, 0.0), canvas.size));
            transaction.commit();
            self.canvas = canvas;
            with_disabled_actions(|| {
                self.ring.root.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), canvas.size));
            });
        }
        let color = style.color(true).unwrap_or([0.0; 4]);
        self.ring.draw(style, local(to.frame, canvas.origin), color, true, Some(0.0));
        self.ring.jump(style, local(from, canvas.origin));
        self.ring.clip(&[]);
        self.surface.flush();
        self.window.set_alpha(1.0)?;
        self.window.order_above(Some(to.window.as_u32()))?;
        self.ring.draw(style, local(to.frame, canvas.origin), color, true, Some(duration));
        self.surface.flush();
        Ok(())
    }

    /// Where the ring is on screen, global, while a slide is shown.
    fn on_screen(&self, style: &BorderStyle) -> Option<CGRect> {
        self.ring.on_screen(style).map(|r| shifted(r, self.canvas.origin))
    }

    fn hide(&self) {
        let _ = self.window.set_alpha(0.0);
    }
}

fn union(a: CGRect, b: CGRect) -> CGRect {
    let x = a.origin.x.min(b.origin.x);
    let y = a.origin.y.min(b.origin.y);
    let right = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
    let bottom = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
    CGRect::new(CGPoint::new(x, y), CGSize::new(right - x, bottom - y))
}

/// The focused window's border, an overlay like any other window's, and the slider
/// that carries the ring when focus moves on. While it slides the border waits,
/// hidden, at the window focus moved to, and shows when the slide lands.
struct FocusBorder {
    border: Option<Border>,
    slider: Option<Slider>,
    sliding: Option<u64>,
    generation: u64,
}

impl FocusBorder {
    fn new() -> Self { Self { border: None, slider: None, sliding: None, generation: 0 } }

    fn target(&self) -> Option<&BorderTarget> { self.border.as_ref().map(|b| &b.target) }

    fn update(
        &mut self,
        style: &BorderStyle,
        target: Option<BorderTarget>,
        reorder: bool,
    ) -> Result<Option<(u64, std::time::Duration)>, CgsWindowError> {
        let Some(target) = target else {
            self.border = None;
            if let Some(slider) = self.slider.take() {
                slider.hide();
            }
            self.sliding = None;
            return Ok(None);
        };
        let previous = self.target().cloned();
        let moved_on = previous.as_ref().is_none_or(|p| p.window != target.window);
        let duration = style.animation.as_secs_f64();
        // Where the ring is now: mid-slide on the slider, otherwise on the border.
        let from = match (self.sliding, &self.slider, &previous) {
            (Some(_), Some(slider), _) => slider.on_screen(style),
            (_, _, Some(previous)) => Some(style.ring_around(previous.frame)),
            _ => None,
        };
        if moved_on && duration > 0.0 && let Some(from) = from {
            let canvas = style.canvas(target.frame);
            let slider = match &mut self.slider {
                Some(slider) => slider,
                None => self.slider.insert(Slider::new(canvas)?),
            };
            slider.start(style, from, &target, duration)?;
            // Hide the border only once the slider shows the ring where it was.
            if let Some(border) = &mut self.border {
                border.set_visible(false)?;
                border.update(style, target, true)?;
            } else {
                let mut border = Border::new(style, target)?;
                border.set_visible(false)?;
                self.border = Some(border);
            }
            self.generation = self.generation.wrapping_add(1);
            self.sliding = Some(self.generation);
            // A little past the slide, so the last frame has landed.
            return Ok(Some((self.generation, style.animation + std::time::Duration::from_millis(40))));
        }
        match &mut self.border {
            Some(border) => border.update(style, target, reorder || moved_on)?,
            None => self.border = Some(Border::new(style, target)?),
        }
        if self.sliding.is_some() && !moved_on {
            // Mid-slide: the border keeps up with its window, hidden, until it lands.
            return Ok(None);
        }
        self.land()?;
        Ok(None)
    }

    /// Show the border where the slide ends, then hide the slider: for a moment both
    /// draw the same ring, never neither.
    fn land(&mut self) -> Result<(), CgsWindowError> {
        self.sliding = None;
        if let Some(border) = &mut self.border {
            border.set_visible(true)?;
        }
        // The slider spans both ends of a slide, as large as a display; it is made
        // again for the next one rather than kept.
        if let Some(slider) = self.slider.take() {
            slider.hide();
        }
        Ok(())
    }

    fn settle(&mut self, generation: u64) -> Result<(), CgsWindowError> {
        if self.sliding == Some(generation) { self.land() } else { Ok(()) }
    }
}

/// Whether any border is on screen, for the input thread.
static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shown() -> bool { SHOWN.load(std::sync::atomic::Ordering::Relaxed) }

/// The borders on screen, owned by the reactor like the drag preview: one per
/// unfocused window, and the focused border, which slides between windows.
pub struct Borders {
    style: Option<BorderStyle>,
    borders: HashMap<WindowServerId, Border>,
    focus: FocusBorder,
}

impl Default for Borders {
    fn default() -> Self { Self { style: None, borders: HashMap::default(), focus: FocusBorder::new() } }
}

// Owned by the reactor and only touched on its thread, as DragPreview is.
unsafe impl Send for Borders {}

impl Drop for Borders {
    fn drop(&mut self) { SHOWN.store(false, std::sync::atomic::Ordering::Relaxed); }
}

impl Borders {
    /// Show borders for exactly `targets`. `reorder` re-asserts each border above
    /// its window. When the focused border starts to slide, returns the slide's
    /// generation and how long to wait before calling `settle` with it.
    pub fn update(
        &mut self,
        style: BorderStyle,
        targets: &[BorderTarget],
        reorder: bool,
    ) -> Option<(u64, std::time::Duration)> {
        if self.style != Some(style) {
            self.borders.clear();
            self.focus = FocusBorder::new();
            self.style = Some(style);
        }
        let unfocused: Vec<_> = targets
            .iter()
            .filter(|t| !t.focused && style.inactive.is_some())
            .cloned()
            .collect();
        let keep: HashSet<_> = unfocused.iter().map(|t| t.window).collect();
        self.borders.retain(|window, _| keep.contains(window));
        // Unfocused borders never glow, so they keep no room for it: each overlay's
        // surface is the size of its window at the display's scale.
        let plain = BorderStyle { glow: false, ..style };
        for target in unfocused {
            let window = target.window;
            let result = match self.borders.get_mut(&window) {
                Some(border) => border.update(&plain, target, reorder),
                None => Border::new(&plain, target).map(|border| {
                    self.borders.insert(window, border);
                }),
            };
            if let Err(error) = result {
                warn!(?error, ?window, "Window border failed");
                self.borders.remove(&window);
            }
        }
        let focused = targets.iter().find(|t| t.focused).cloned();
        let settle = self.focus.update(&style, focused, reorder).unwrap_or_else(|error| {
            warn!(?error, "Focused window border failed");
            self.focus = FocusBorder::new();
            None
        });
        SHOWN.store(self.len() > 0, std::sync::atomic::Ordering::Relaxed);
        settle
    }

    /// Land the slide of generation `generation`, if it is still the latest.
    pub fn settle(&mut self, generation: u64) {
        if let Err(error) = self.focus.settle(generation) {
            warn!(?error, "Focused window border failed");
            self.focus = FocusBorder::new();
        }
    }

    pub fn len(&self) -> usize { self.borders.len() + usize::from(self.focus.border.is_some()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_read_as_janky_borders_and_css_write_them() {
        assert_eq!(parse_color("0xffb4befe"), Some([180.0 / 255.0, 190.0 / 255.0, 254.0 / 255.0, 1.0]));
        assert_eq!(parse_color("0x80000000"), Some([0.0, 0.0, 0.0, 128.0 / 255.0]));
        assert_eq!(parse_color("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color("#00ff0080"), Some([0.0, 1.0, 0.0, 128.0 / 255.0]));
        assert_eq!(format_color([180.0 / 255.0, 190.0 / 255.0, 254.0 / 255.0, 1.0]), "0xffb4befe");
        for text in ["0xffb4befe", "0x80123456"] {
            assert_eq!(format_color(parse_color(text).unwrap()), text, "a picked color reads back");
        }
        for bad in ["", "red", "0xfff", "#12345", "0xgg000000"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_ring_sits_outside_the_window_with_room_for_the_glow() {
        let settings = BorderSettings { enabled: true, width: 5.0, ..BorderSettings::default() };
        let style = BorderStyle::new(&settings);
        let frame = CGRect::new(CGPoint::new(100.0, 50.0), CGSize::new(800.0, 600.0));
        // Width 5, and twice the glow's 12 around it for the glow to fade out.
        assert_eq!(style.canvas(frame), CGRect::new(CGPoint::new(71.0, 21.0), CGSize::new(858.0, 658.0)));
        assert_eq!(style.ring(frame), CGRect::new(CGPoint::new(24.0, 24.0), CGSize::new(810.0, 610.0)));
        // The ring's inner edge, and the hole the glow leaves, are the window's edge.
        let (ring, canvas, hole) = (style.ring(frame), style.canvas(frame), style.hole(frame));
        assert_eq!(canvas.origin.x + ring.origin.x + style.width, frame.origin.x);
        assert_eq!(canvas.origin.y + ring.origin.y + style.width, frame.origin.y);
        assert_eq!((canvas.origin.x + hole.origin.x, hole.size), (frame.origin.x, frame.size));

        let plain = BorderStyle::new(&BorderSettings { glow: false, ..settings.clone() });
        assert_eq!(plain.canvas(frame), CGRect::new(CGPoint::new(95.0, 45.0), CGSize::new(810.0, 610.0)));

        assert_eq!(style.color(true), Some(style.active));
        let none = BorderStyle::new(&BorderSettings { inactive_color: String::new(), ..settings });
        assert_eq!(none.color(false), None, "no unfocused borders");

        // Corners never exceed half a side, which CoreGraphics rejects.
        let thin = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(20.0, 400.0));
        assert_eq!(corner(thin, 16.0), 10.0);
        assert_eq!(corner(frame, 16.0), 16.0);
    }

    #[test]
    fn the_focused_ring_wraps_the_window_in_any_coordinates() {
        let rect = |x, y, w, h| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        let style = BorderStyle::new(&BorderSettings::default());
        assert_eq!(style.width, 2.0);
        assert_eq!(style.ring_around(rect(10.0, 20.0, 100.0, 50.0)), rect(8.0, 18.0, 104.0, 54.0));
        // A window on a display left of the main one, drawn in that display's overlay.
        let display = rect(-1440.0, 0.0, 1440.0, 900.0);
        let window = local(rect(-1000.0, 100.0, 400.0, 300.0), display.origin);
        assert_eq!(window, rect(440.0, 100.0, 400.0, 300.0));
        assert_eq!(style.animation, std::time::Duration::from_millis(180));
        let still = BorderStyle::new(&BorderSettings { animation_ms: 0.0, ..BorderSettings::default() });
        assert!(still.animation.is_zero(), "0 moves the border at once");
    }

    #[test]
    fn a_border_is_bands_around_the_window() {
        let rect = |x, y, w, h| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        let style = BorderStyle::new(&BorderSettings::default());
        // Glow room 24, ring 2, corners 16, and a point to spare.
        assert_eq!(style.depth(), 43.0);
        let canvas = CGSize::new(1000.0, 600.0);
        let parts = bands(canvas, 43.0);
        assert_eq!(parts.len(), 4);
        let area: f64 = parts.iter().map(|r| r.size.width * r.size.height).sum();
        assert_eq!(area, 1000.0 * 600.0 - 914.0 * 514.0, "everything but the middle, once");
        assert_eq!(parts[0], rect(0.0, 0.0, 1000.0, 43.0));
        assert_eq!(parts[3], rect(957.0, 43.0, 43.0, 514.0));
        // A window too small for bands to part is one band.
        assert_eq!(bands(CGSize::new(80.0, 600.0), 43.0), vec![rect(0.0, 0.0, 80.0, 600.0)]);
    }

    #[test]
    fn what_windows_above_cover_is_left_out() {
        let rect = |x, y, w, h| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        let area = |rs: &[CGRect]| rs.iter().map(|r| r.size.width * r.size.height).sum::<f64>();
        let base = rect(0.0, 0.0, 100.0, 100.0);
        assert_eq!(subtract(base, &[]), vec![base]);
        assert_eq!(subtract(base, &[rect(200.0, 0.0, 10.0, 10.0)]), vec![base], "apart");
        assert!(subtract(base, &[rect(-10.0, -10.0, 200.0, 200.0)]).is_empty(), "all covered");
        let hole = subtract(base, &[rect(25.0, 25.0, 50.0, 50.0)]);
        assert_eq!(area(&hole), 10000.0 - 2500.0);
        // Overlapping holes are not counted twice, and the pieces never overlap.
        let two = subtract(base, &[rect(0.0, 0.0, 60.0, 60.0), rect(40.0, 40.0, 60.0, 60.0)]);
        assert_eq!(area(&two), 10000.0 - (3600.0 + 3600.0 - 400.0));
        for (i, a) in two.iter().enumerate() {
            for b in &two[i + 1..] {
                let overlap_w = (a.origin.x + a.size.width).min(b.origin.x + b.size.width) - a.origin.x.max(b.origin.x);
                let overlap_h = (a.origin.y + a.size.height).min(b.origin.y + b.size.height) - a.origin.y.max(b.origin.y);
                assert!(overlap_w <= 0.0 || overlap_h <= 0.0);
            }
        }
    }
}
