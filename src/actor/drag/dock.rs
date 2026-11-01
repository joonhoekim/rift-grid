//! Dock-style drop targeting, as in VS Code's editor groups: the tile under the
//! pointer, and which of its five zones (the middle or an edge) the pointer is in.
//!
//! Nothing outside a tile is a target: no nearest-tile fallback across gaps, no
//! sticky zones, no moves onto the dragged window's own tile. Where the window
//! then lands is the layout's business, and the reactor previews exactly that
//! frame (`LayoutEngine::drop_preview_frame`), so every layout shows the truth.

use objc2_core_foundation::{CGPoint, CGRect};

use super::{DragScene, DropIntent, classify_zone, resolve_action};
use crate::actor::app::WindowId;
use crate::common::config::MouseDropAction;

/// Dropping on the middle of a tile trades places with its window.
const CENTER: MouseDropAction = MouseDropAction::Swap;

/// The zone of the first tile in `scene` under `point`, skipping `excluded`.
///
/// A tile owns its left and top edges but not its right and bottom ones, so a
/// point on a shared edge belongs to one tile, and a column the scrolling layout
/// parks just off the screen's left edge is not hit at the screen's first pixel.
pub(super) fn target(
    scene: &DragScene,
    point: CGPoint,
    fraction: f64,
    excluded: Option<WindowId>,
) -> Option<DropIntent> {
    let owns = |frame: CGRect| {
        point.x < frame.origin.x + frame.size.width && point.y < frame.origin.y + frame.size.height
    };
    scene.targets.iter().filter(|target| Some(target.window) != excluded).find_map(|target| {
        if !owns(target.frame) {
            return None;
        }
        let zone = classify_zone(target.frame, point, fraction)?;
        Some(DropIntent {
            window: target.window,
            space: target.space,
            frame: target.frame,
            zone,
            action: scene.action_override.unwrap_or_else(|| resolve_action(zone, CENTER)),
        })
    })
}
