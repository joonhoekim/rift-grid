//! The edge of the screen lit for a moment when a move has no cell beyond the
//! grid's end, as subline's `moves.edge_glow`.

use cgs::compositor::{
    CgsWindow, CgsWindowError, WindowSurface, release_layer_tree, with_disabled_actions,
};
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSPopUpMenuWindowLevel};
use objc2_core_foundation::CGRect;
use objc2_foundation::NSNumber;
use objc2_quartz_core::{CALayer, CATransaction};

use super::{animate, rect, time_scale};
use crate::layout_engine::Direction;

/// How far the light reaches in from the edge.
const DEPTH: f64 = 44.0;
/// The lit line at the edge itself.
const LINE: f64 = 2.0;
/// The light's bands, and how strong it is next to the line.
const BAND: f64 = 2.0;
const PEAK: f64 = 0.45;
const FADE_IN: f64 = 0.06;
/// How long the light takes to go out.
pub const FADE_OUT: f64 = 0.32;

/// The lit edge of one display.
pub struct EdgeGlow {
    generation: u64,
    frame: CGRect,
    root: Retained<CALayer>,
    // The surface must be detached before its WindowServer window drops.
    _surface: WindowSurface,
    _window: CgsWindow,
}

impl Drop for EdgeGlow {
    fn drop(&mut self) {
        release_layer_tree(&self.root);
        CATransaction::flush();
    }
}

impl EdgeGlow {
    /// Light the `side` edge of `screen`, the display's usable area.
    pub fn show(
        generation: u64,
        screen: CGRect,
        scale: f64,
        side: Direction,
    ) -> Result<Self, CgsWindowError> {
        let frame = edge_frame(screen, side);
        let size = frame.size;
        // The light fades inward in thin bands, strongest at the edge.
        let band = |from_edge: f64, thick: f64| match side {
            Direction::Left => rect(from_edge, 0.0, thick, size.height),
            Direction::Right => rect(size.width - from_edge - thick, 0.0, thick, size.height),
            Direction::Up => rect(0.0, from_edge, size.width, thick),
            Direction::Down => rect(0.0, size.height - from_edge - thick, size.width, thick),
        };
        let root = CALayer::layer();
        let accent = NSColor::controlAccentColor();
        let tint = |alpha: f64| accent.colorWithAlphaComponent(alpha).CGColor();
        with_disabled_actions(|| {
            root.setFrame(rect(0.0, 0.0, size.width, size.height));
            root.setContentsScale(scale);
            root.setGeometryFlipped(true);
            let bands = (DEPTH / BAND) as usize;
            for i in 0..bands {
                let left = 1.0 - i as f64 / bands as f64;
                let layer = CALayer::layer();
                layer.setFrame(band(i as f64 * BAND, BAND));
                layer.setBackgroundColor(Some(&tint(PEAK * left * left)));
                root.addSublayer(&layer);
            }
            let line = CALayer::layer();
            line.setFrame(band(0.0, LINE));
            line.setBackgroundColor(Some(&tint(0.9)));
            root.addSublayer(&line);
        });
        let window = CgsWindow::new_compositor(frame, 0.0)?;
        window.set_resolution(scale)?;
        window.set_level(NSPopUpMenuWindowLevel as i32 + 1)?;
        let surface = WindowSurface::new_scaled(window.id(), root.bounds(), &root, scale)?;
        let glow = Self { generation, frame, root, _surface: surface, _window: window };
        glow.fade_in();
        glow._surface.flush();
        glow._window.order_above(None)?;
        CATransaction::flush();
        Ok(glow)
    }

    pub fn generation(&self) -> u64 { self.generation }

    /// Whether this light is the `side` edge of `screen`.
    pub fn lights(&self, screen: CGRect, side: Direction) -> bool {
        self.frame == edge_frame(screen, side)
    }

    /// Light it again, as a move of generation `generation` ran into it once more.
    pub fn relight(&mut self, generation: u64) {
        self.generation = generation;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.root.removeAllAnimations();
        self.root.setOpacity(1.0);
        CATransaction::commit();
        CATransaction::flush();
    }

    fn fade_in(&self) {
        // A zero duration would be Core Animation's default, not none.
        if time_scale() <= 0.0 {
            return;
        }
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        animate(&self.root, "opacity", &NSNumber::new_f64(0.0), FADE_IN * time_scale());
        CATransaction::commit();
    }

    /// Put the light out; it is dropped after.
    pub fn fade(&self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.root.setOpacity(0.0);
        if time_scale() > 0.0 {
            animate(&self.root, "opacity", &NSNumber::new_f64(1.0), FADE_OUT * time_scale());
        }
        CATransaction::commit();
        CATransaction::flush();
    }
}

/// The strip along the `side` edge of `screen` that the light covers.
fn edge_frame(screen: CGRect, side: Direction) -> CGRect {
    let (x, y, w, h) = (screen.origin.x, screen.origin.y, screen.size.width, screen.size.height);
    match side {
        Direction::Left => rect(x, y, DEPTH, h),
        Direction::Right => rect(x + w - DEPTH, y, DEPTH, h),
        Direction::Up => rect(x, y, w, DEPTH),
        Direction::Down => rect(x, y + h - DEPTH, w, DEPTH),
    }
}
