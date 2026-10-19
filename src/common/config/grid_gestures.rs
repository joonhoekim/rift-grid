//! Trackpad swipes on the workspace grid, `[settings.grid_gestures]`. They take
//! their finger counts from macOS (and from upstream's swipes) while the grid is
//! on. Distance, vertical tolerance and haptics come from `[settings.gestures]`.

use serde::{Deserialize, Serialize};

use super::ConfigSchema;

fn yes() -> bool { true }
fn three() -> usize { 3 }
fn four() -> usize { 4 }

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct GridGestureSettings {
    #[serde(default = "yes")]
    /// Rift handles these swipes, before macOS: its own swipes between Spaces,
    /// Mission Control and App Exposé don't come with the same finger count.
    #[setting(
        label = "Grid swipes",
        order = 0,
        aliases = "trackpad gestures swipe fingers grid cell mission control spaces"
    )]
    pub enabled: bool,
    #[serde(default = "three")]
    /// Swiping with this many fingers goes to the neighboring cell, any way.
    /// The content follows the fingers: swiping left shows the cell to the right.
    /// 0 turns it off.
    #[setting(label = "Fingers for cells", enabled_by = "enabled")]
    pub cell_fingers: usize,
    #[serde(default = "four")]
    /// Swiping left or right with this many fingers goes through the grids on
    /// the focused display: left to the next one, right to the one before.
    /// 0 turns it off.
    #[setting(label = "Fingers for grids", enabled_by = "enabled")]
    pub grid_fingers: usize,
    #[serde(default = "yes")]
    /// Swiping up with the fingers for grids opens the Overview, in place of
    /// Mission Control. Down stays App Exposé.
    #[setting(
        label = "Swipe up for Overview",
        enabled_by = "enabled",
        aliases = "mission control"
    )]
    pub overview: bool,
}

impl Default for GridGestureSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            cell_fingers: 3,
            grid_fingers: 4,
            overview: true,
        }
    }
}

impl GridGestureSettings {
    pub(super) fn validate(&self, issues: &mut Vec<String>) {
        for (key, fingers) in [
            ("cell_fingers", self.cell_fingers),
            ("grid_fingers", self.grid_fingers),
        ] {
            if fingers != 0 && !(2..=5).contains(&fingers) {
                issues.push(format!(
                    "grid_gestures.{key} must be 0 or from 2 to 5, got {fingers}"
                ));
            }
        }
        if self.cell_fingers != 0 && self.cell_fingers == self.grid_fingers {
            issues.push(format!(
                "grid_gestures.cell_fingers and grid_fingers are both {}",
                self.cell_fingers
            ));
        }
    }
}
