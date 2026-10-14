//! Window borders: one click-through WindowServer window per bordered window,
//! ordered just above it, drawing a ring outside its edge.

use cgs::{CgsWindow, CgsWindowError, WindowSurface, WindowTransaction, with_disabled_actions};
use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGColor;
use objc2_quartz_core::CALayer;
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderStyle {
    pub width: f64,
    pub radius: f64,
    pub active: Rgba,
    /// None leaves unfocused windows without a border.
    pub inactive: Option<Rgba>,
    pub glow: bool,
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
        }
    }

    fn color(&self, focused: bool) -> Option<Rgba> {
        if focused { Some(self.active) } else { self.inactive }
    }

    /// Room around the ring for the glow to fade out in.
    fn pad(&self) -> f64 { if self.glow { self.width * 2.0 } else { 0.0 } }

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

struct Border {
    // The surface must be detached before its WindowServer window drops.
    surface: WindowSurface,
    window: CgsWindow,
    root: Retained<CALayer>,
    ring: Retained<CALayer>,
    target: BorderTarget,
}

impl Border {
    fn new(style: &BorderStyle, target: BorderTarget) -> Result<Self, CgsWindowError> {
        let canvas = style.canvas(target.frame);
        let window = CgsWindow::new_compositor(canvas, 0.0)?;
        window.set_alpha(1.0)?;
        // No WindowServer shadow of its own.
        window.set_tags(1 << 3)?;
        let root = CALayer::layer();
        let ring = CALayer::layer();
        with_disabled_actions(|| {
            root.setAnchorPoint(CGPoint::new(0.0, 0.0));
            root.setPosition(CGPoint::new(0.0, 0.0));
            root.setContentsScale(2.0);
            root.setOpaque(false);
            root.addSublayer(&ring);
        });
        let bounds = CGRect::new(CGPoint::new(0.0, 0.0), canvas.size);
        let surface = WindowSurface::new(window.id(), bounds, &root)?;
        let border = Self { surface, window, root, ring, target };
        border.draw(style);
        border.window.order_above(Some(target.window.as_u32()))?;
        Ok(border)
    }

    fn draw(&self, style: &BorderStyle) {
        let canvas = style.canvas(self.target.frame);
        let color = style.color(self.target.focused).unwrap_or([0.0; 4]);
        with_disabled_actions(|| {
            self.root.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), canvas.size));
            self.ring.setFrame(style.ring(self.target.frame));
            self.ring.setCornerRadius(if style.radius > 0.0 {
                style.radius + style.width
            } else {
                0.0
            });
            self.ring.setBorderWidth(style.width);
            self.ring.setBorderColor(Some(&cg_color(color)));
            let glow = style.glow && self.target.focused;
            self.ring.setShadowColor(Some(&cg_color(color)));
            self.ring.setShadowOffset(CGSize::new(0.0, 0.0));
            self.ring.setShadowRadius(style.width);
            self.ring.setShadowOpacity(if glow { 0.9 } else { 0.0 });
        });
        self.surface.flush();
    }

    fn update(
        &mut self,
        style: &BorderStyle,
        target: BorderTarget,
        reorder: bool,
    ) -> Result<(), CgsWindowError> {
        let previous = self.target;
        self.target = target;
        let resized = previous.frame.size != target.frame.size;
        if previous.frame != target.frame {
            let canvas = style.canvas(target.frame);
            let transaction = WindowTransaction::new()?;
            if resized {
                transaction.set_rounded_frame(&self.window, canvas, 0.0)?;
                self.surface
                    .set_bounds_in(&transaction, CGRect::new(CGPoint::new(0.0, 0.0), canvas.size));
            } else {
                transaction.move_window(&self.window, canvas.origin);
            }
            transaction.commit();
        }
        if resized || previous.focused != target.focused {
            self.draw(style);
        }
        if reorder || previous.focused != target.focused {
            self.window.order_above(Some(target.window.as_u32()))?;
        }
        Ok(())
    }
}

/// The borders on screen, owned by the reactor like the drag preview.
#[derive(Default)]
pub struct Borders {
    style: Option<BorderStyle>,
    borders: HashMap<WindowServerId, Border>,
}

// Owned by the reactor and only touched on its thread, as DragPreview is.
unsafe impl Send for Borders {}

impl Borders {
    /// Show borders for exactly `targets`. `reorder` re-asserts each border just
    /// above its window, for when windows may have been raised over their borders.
    pub fn update(&mut self, style: BorderStyle, targets: &[BorderTarget], reorder: bool) {
        if self.style != Some(style) {
            self.borders.clear();
            self.style = Some(style);
        }
        let wanted: Vec<_> =
            targets.iter().filter(|t| style.color(t.focused).is_some()).copied().collect();
        let keep: HashSet<_> = wanted.iter().map(|t| t.window).collect();
        self.borders.retain(|window, _| keep.contains(window));
        for target in wanted {
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
    }

    pub fn len(&self) -> usize { self.borders.len() }
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
        for bad in ["", "red", "0xfff", "#12345", "0xgg000000"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_ring_sits_outside_the_window_with_room_for_the_glow() {
        let settings = BorderSettings { enabled: true, ..BorderSettings::default() };
        let style = BorderStyle::new(&settings);
        let frame = CGRect::new(CGPoint::new(100.0, 50.0), CGSize::new(800.0, 600.0));
        // Width 5 and a glow of twice that around it.
        assert_eq!(style.canvas(frame), CGRect::new(CGPoint::new(85.0, 35.0), CGSize::new(830.0, 630.0)));
        assert_eq!(style.ring(frame), CGRect::new(CGPoint::new(10.0, 10.0), CGSize::new(810.0, 610.0)));
        // The ring's inner edge is the window's edge.
        let ring = style.ring(frame);
        let canvas = style.canvas(frame);
        assert_eq!(canvas.origin.x + ring.origin.x + style.width, frame.origin.x);
        assert_eq!(canvas.origin.y + ring.origin.y + style.width, frame.origin.y);

        let plain = BorderStyle::new(&BorderSettings { glow: false, ..settings.clone() });
        assert_eq!(plain.canvas(frame), CGRect::new(CGPoint::new(95.0, 45.0), CGSize::new(810.0, 610.0)));

        assert_eq!(style.color(true), Some(style.active));
        let none = BorderStyle::new(&BorderSettings { inactive_color: String::new(), ..settings });
        assert_eq!(none.color(false), None, "no unfocused borders");
    }
}
