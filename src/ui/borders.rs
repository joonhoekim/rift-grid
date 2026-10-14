//! Window borders: one click-through WindowServer window per bordered window,
//! ordered just above it, drawing a ring outside its edge.

use cgs::{CgsWindow, CgsWindowError, WindowSurface, WindowTransaction, with_disabled_actions};
use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath};
use objc2_quartz_core::{
    CALayer, CAMediaTimingFunction, CAShapeLayer, CATransaction, kCAFillRuleEvenOdd,
};
use tracing::warn;

use crate::common::collections::{HashMap, HashSet};
use crate::common::config::BorderSettings;
use crate::sys::window_server::WindowServerId;

/// A window to draw a border around, in WindowServer coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderTarget {
    pub window: WindowServerId,
    pub frame: CGRect,
    pub focused: bool,
}

/// Red, green, blue, alpha in 0..1.
pub type Rgba = [f64; 4];

/// `0xAARRGGBB`, `#RRGGBB` or `#RRGGBBAA`.
pub fn parse_color(text: &str) -> Option<Rgba> {
    let text = text.trim();
    let channel = |v: u32, shift: u32| ((v >> shift) & 0xff) as f64 / 255.0;
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        let v = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 8)?;
        return Some([channel(v, 16), channel(v, 8), channel(v, 0), channel(v, 24)]);
    }
    let hex = text.strip_prefix('#')?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some([channel(v, 16), channel(v, 8), channel(v, 0), 1.0]),
        8 => Some([channel(v, 24), channel(v, 16), channel(v, 8), channel(v, 0)]),
        _ => None,
    }
}

/// `0xAARRGGBB`, the form JankyBorders and the config's defaults use.
pub fn format_color([r, g, b, a]: Rgba) -> String {
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    format!("0x{:02x}{:02x}{:02x}{:02x}", byte(a), byte(r), byte(g), byte(b))
}

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
    fn pad(&self) -> f64 { if self.glow { self.glow_radius * 2.0 } else { 0.0 } }

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

struct Border {
    // The surface must be detached before its WindowServer window drops.
    surface: WindowSurface,
    window: CgsWindow,
    /// Where the overlay window is. Normally the target's canvas; while the focused
    /// border slides, the room for both ends of the slide.
    canvas: CGRect,
    root: Retained<CALayer>,
    /// A filled plate the size of the ring, whose shadow is the glow; masked to
    /// what lies outside the window.
    glow: Retained<CALayer>,
    glow_mask: Retained<CAShapeLayer>,
    ring: Retained<CALayer>,
    target: BorderTarget,
}

fn union(a: CGRect, b: CGRect) -> CGRect {
    let x = a.origin.x.min(b.origin.x);
    let y = a.origin.y.min(b.origin.y);
    let right = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
    let bottom = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
    CGRect::new(CGPoint::new(x, y), CGSize::new(right - x, bottom - y))
}

fn shifted(rect: CGRect, by: CGPoint) -> CGRect {
    CGRect::new(CGPoint::new(rect.origin.x + by.x, rect.origin.y + by.y), rect.size)
}

impl Border {
    fn new(style: &BorderStyle, target: BorderTarget) -> Result<Self, CgsWindowError> {
        let canvas = style.canvas(target.frame);
        let window = CgsWindow::new_compositor(canvas, 0.0)?;
        window.set_alpha(1.0)?;
        // No WindowServer shadow of its own.
        window.set_tags(1 << 3)?;
        let root = CALayer::layer();
        let glow = CALayer::layer();
        let glow_mask = CAShapeLayer::layer();
        let ring = CALayer::layer();
        with_disabled_actions(|| {
            root.setAnchorPoint(CGPoint::new(0.0, 0.0));
            root.setPosition(CGPoint::new(0.0, 0.0));
            root.setContentsScale(2.0);
            root.setOpaque(false);
            glow_mask.setFillRule(unsafe { kCAFillRuleEvenOdd });
            unsafe { glow.setMask(Some(&glow_mask)) };
            root.addSublayer(&glow);
            root.addSublayer(&ring);
        });
        let bounds = CGRect::new(CGPoint::new(0.0, 0.0), canvas.size);
        let surface = WindowSurface::new(window.id(), bounds, &root)?;
        let border = Self { surface, window, canvas, root, glow, glow_mask, ring, target };
        border.place(style, true);
        border.window.order_above(Some(target.window.as_u32()))?;
        Ok(border)
    }

    /// Where the target's ring goes inside the overlay window.
    fn ring_in_canvas(&self, style: &BorderStyle) -> CGRect {
        let at = style.canvas(self.target.frame).origin;
        let by = CGPoint::new(at.x - self.canvas.origin.x, at.y - self.canvas.origin.y);
        shifted(style.ring(self.target.frame), by)
    }

    fn outer_radius(&self, style: &BorderStyle, ring: CGRect) -> f64 {
        if style.radius > 0.0 { corner(ring, style.radius + style.width) } else { 0.0 }
    }

    /// Lay the layers out for the target, without animation. The glow is left off
    /// while the border slides.
    fn place(&self, style: &BorderStyle, glow_allowed: bool) {
        let ring = self.ring_in_canvas(style);
        let outer = self.outer_radius(style, ring);
        let color = style.color(self.target.focused).unwrap_or([0.0; 4]);
        with_disabled_actions(|| {
            self.root.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), self.canvas.size));
            self.ring.setFrame(ring);
            self.ring.setCornerRadius(outer);
            self.ring.setBorderWidth(style.width);
            self.ring.setBorderColor(Some(&cg_color(color)));

            let glowing = glow_allowed && style.glow && self.target.focused;
            self.glow.setHidden(!glowing);
            if glowing {
                self.glow.setFrame(ring);
                self.glow.setCornerRadius(outer);
                self.glow.setBackgroundColor(Some(&cg_color(color)));
                self.glow.setShadowColor(Some(&cg_color(color)));
                self.glow.setShadowOffset(CGSize::new(0.0, 0.0));
                self.glow.setShadowRadius(style.glow_radius);
                self.glow.setShadowOpacity(1.0);
                // The mask lives in the glow layer's space and covers the window.
                self.glow_mask.setFrame(CGRect::new(
                    CGPoint::new(-ring.origin.x, -ring.origin.y),
                    self.canvas.size,
                ));
                let hole = CGRect::new(
                    CGPoint::new(ring.origin.x + style.width, ring.origin.y + style.width),
                    self.target.frame.size,
                );
                self.glow_mask.setPath(Some(&outside(self.canvas.size, hole, style.radius)));
            }
        });
        self.surface.flush();
    }

    /// Move or resize the overlay window to `canvas` in one WindowServer transaction.
    fn reframe(&mut self, canvas: CGRect) -> Result<(), CgsWindowError> {
        if canvas == self.canvas {
            return Ok(());
        }
        let transaction = WindowTransaction::new()?;
        if canvas.size != self.canvas.size {
            transaction.set_rounded_frame(&self.window, canvas, 0.0)?;
            self.surface
                .set_bounds_in(&transaction, CGRect::new(CGPoint::new(0.0, 0.0), canvas.size));
        } else {
            transaction.move_window(&self.window, canvas.origin);
        }
        transaction.commit();
        self.canvas = canvas;
        Ok(())
    }

    fn update(
        &mut self,
        style: &BorderStyle,
        target: BorderTarget,
        reorder: bool,
    ) -> Result<(), CgsWindowError> {
        let previous = self.target;
        self.target = target;
        if previous.window == target.window && previous.frame.size == target.frame.size {
            // Only moved: carry the overlay along and leave the layers alone. Moving
            // the window and re-placing the layers reach the screen separately, and
            // doing both shows the ring out of place for a frame.
            let by = CGPoint::new(
                target.frame.origin.x - previous.frame.origin.x,
                target.frame.origin.y - previous.frame.origin.y,
            );
            self.reframe(shifted(self.canvas, by))?;
            if previous.focused != target.focused {
                self.place(style, true);
            }
        } else {
            self.reframe(style.canvas(target.frame))?;
            self.place(style, true);
        }
        if reorder || previous.focused != target.focused || previous.window != target.window {
            self.window.order_above(Some(target.window.as_u32()))?;
        }
        Ok(())
    }

    /// Where the ring is on screen right now, mid-slide included: the presentation
    /// layer has the animation's current value.
    fn ring_on_screen(&self, style: &BorderStyle) -> CGRect {
        let frame = unsafe { self.ring.presentationLayer() }
            .map(|layer| layer.frame())
            .unwrap_or_else(|| self.ring_in_canvas(style));
        shifted(frame, self.canvas.origin)
    }

    /// Slide the ring from where it is on screen to `target`, over `duration`. The
    /// window grows to hold both ends; `settle` shrinks it back once it has landed.
    fn slide(
        &mut self,
        style: &BorderStyle,
        target: BorderTarget,
        duration: f64,
    ) -> Result<(), CgsWindowError> {
        let from = self.ring_on_screen(style);
        let at = style.canvas(target.frame).origin;
        let to = shifted(style.ring(target.frame), at);
        // Room for the glow at the far end, so landing needs no resize.
        let canvas = union(from, style.canvas(target.frame));
        self.reframe(canvas)?;
        let local = |r: CGRect| shifted(r, CGPoint::new(-canvas.origin.x, -canvas.origin.y));
        let color = style.color(true).unwrap_or([0.0; 4]);
        with_disabled_actions(|| {
            self.ring.removeAllAnimations();
            self.root.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), canvas.size));
            self.glow.setHidden(true);
            self.ring.setFrame(local(from));
            self.ring.setCornerRadius(self.outer_radius(style, from));
            self.ring.setBorderWidth(style.width);
            self.ring.setBorderColor(Some(&cg_color(color)));
        });
        self.surface.flush();
        self.target = target;
        self.window.order_above(Some(target.window.as_u32()))?;
        CATransaction::begin();
        CATransaction::setAnimationDuration(duration);
        // Fast out, gentle landing, as a spring that does not overshoot.
        CATransaction::setAnimationTimingFunction(Some(
            &CAMediaTimingFunction::functionWithControlPoints(0.2, 0.9, 0.25, 1.0),
        ));
        self.ring.setFrame(local(to));
        self.ring.setCornerRadius(self.outer_radius(style, to));
        CATransaction::commit();
        self.surface.flush();
        Ok(())
    }

    /// Bring the glow back once a slide has landed. The overlay keeps the size it
    /// slid in, which already has room for the glow, so nothing moves.
    fn settle(&mut self, style: &BorderStyle) -> Result<(), CgsWindowError> {
        self.place(style, true);
        Ok(())
    }
}

/// The borders on screen, owned by the reactor like the drag preview: one per
/// unfocused window, and one focused border that slides between windows.
#[derive(Default)]
pub struct Borders {
    style: Option<BorderStyle>,
    borders: HashMap<WindowServerId, Border>,
    focus: Option<Border>,
    /// The slide in flight, by generation.
    sliding: Option<u64>,
    generation: u64,
}

// Owned by the reactor and only touched on its thread, as DragPreview is.
unsafe impl Send for Borders {}

impl Borders {
    /// Show borders for exactly `targets`. `reorder` re-asserts each border just
    /// above its window, for when windows may have been raised over their borders.
    /// When the focused border starts to slide, returns the slide's generation and
    /// how long to wait before calling `settle` with it.
    pub fn update(
        &mut self,
        style: BorderStyle,
        targets: &[BorderTarget],
        reorder: bool,
    ) -> Option<(u64, std::time::Duration)> {
        if self.style != Some(style) {
            self.borders.clear();
            self.focus = None;
            self.sliding = None;
            self.style = Some(style);
        }
        let unfocused: Vec<_> = targets
            .iter()
            .filter(|t| !t.focused && style.inactive.is_some())
            .copied()
            .collect();
        let keep: HashSet<_> = unfocused.iter().map(|t| t.window).collect();
        self.borders.retain(|window, _| keep.contains(window));
        for target in unfocused {
            let result = match self.borders.get_mut(&target.window) {
                Some(border) => border.update(&style, target, reorder),
                None => Border::new(&style, target).map(|border| {
                    self.borders.insert(target.window, border);
                }),
            };
            if let Err(error) = result {
                warn!(?error, window = ?target.window, "Window border failed");
                self.borders.remove(&target.window);
            }
        }
        self.update_focus(style, targets.iter().find(|t| t.focused).copied(), reorder)
    }

    fn update_focus(
        &mut self,
        style: BorderStyle,
        target: Option<BorderTarget>,
        reorder: bool,
    ) -> Option<(u64, std::time::Duration)> {
        let Some(target) = target else {
            self.focus = None;
            self.sliding = None;
            return None;
        };
        let Some(ring) = &mut self.focus else {
            match Border::new(&style, target) {
                Ok(border) => self.focus = Some(border),
                Err(error) => warn!(?error, "Focused window border failed"),
            }
            return None;
        };
        let duration = style.animation.as_secs_f64();
        let moved_on = ring.target.window != target.window;
        let result = if moved_on && duration > 0.0 {
            ring.slide(&style, target, duration).map(|()| {
                self.generation = self.generation.wrapping_add(1);
                self.sliding = Some(self.generation);
                // A little past the slide, so the last frame has landed.
                Some((self.generation, style.animation + std::time::Duration::from_millis(40)))
            })
        } else if self.sliding.is_some() && !moved_on {
            // Mid-slide: land wherever the window is by the time it settles.
            ring.target = target;
            Ok(None)
        } else {
            self.sliding = None;
            ring.update(&style, target, reorder).map(|()| None)
        };
        result.unwrap_or_else(|error| {
            warn!(?error, "Focused window border failed");
            self.focus = None;
            self.sliding = None;
            None
        })
    }

    /// Land the slide of generation `generation`, if it is still the latest.
    pub fn settle(&mut self, generation: u64) {
        if self.sliding != Some(generation) {
            return;
        }
        self.sliding = None;
        if let (Some(style), Some(ring)) = (self.style, &mut self.focus)
            && let Err(error) = ring.settle(&style)
        {
            warn!(?error, "Focused window border failed");
            self.focus = None;
        }
    }

    pub fn len(&self) -> usize { self.borders.len() + usize::from(self.focus.is_some()) }
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
    fn a_slide_has_room_for_both_ends() {
        let rect = |x, y, w, h| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        assert_eq!(
            union(rect(0.0, 0.0, 100.0, 50.0), rect(300.0, -20.0, 50.0, 50.0)),
            rect(0.0, -20.0, 350.0, 70.0)
        );
        let style = BorderStyle::new(&BorderSettings::default());
        assert_eq!(style.animation, std::time::Duration::from_millis(180));
        assert_eq!(style.width, 2.0);
        let still = BorderStyle::new(&BorderSettings { animation_ms: 0.0, ..BorderSettings::default() });
        assert!(still.animation.is_zero(), "0 moves the border at once");
    }
}
