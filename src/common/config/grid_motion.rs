//! How moving between grid cells and grids is shown, `[settings.ui.grid_motion]`.

use serde::{Deserialize, Serialize};

use super::ConfigSchema;

fn yes() -> bool { true }

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct GridMotionSettings {
    #[serde(default = "yes")]
    /// Slide the screen to the cell moved to, and fade between grids.
    #[setting(
        label = "Slide between cells",
        order = 0,
        aliases = "animation pan slide transition move cell grid motion"
    )]
    pub pan: bool,
    #[serde(default = "default_duration_ms")]
    /// How long the slide or fade takes.
    #[setting(label = "Slide duration", unit = "ms", enabled_by = "pan", aliases = "speed")]
    pub duration_ms: f64,
    #[serde(default = "yes")]
    /// Show a small map of the grid for a moment, its active cell moving to the
    /// cell moved to.
    #[setting(label = "Grid map", aliases = "hud indicator minimap overlay osd")]
    pub hud: bool,
    #[serde(default = "default_hud_ms")]
    /// How long the map stays after the last move.
    #[setting(label = "Map time", unit = "ms", enabled_by = "hud")]
    pub hud_ms: f64,
}

fn default_duration_ms() -> f64 { 240.0 }
fn default_hud_ms() -> f64 { 700.0 }

impl Default for GridMotionSettings {
    fn default() -> Self {
        Self {
            pan: true,
            duration_ms: default_duration_ms(),
            hud: true,
            hud_ms: default_hud_ms(),
        }
    }
}
