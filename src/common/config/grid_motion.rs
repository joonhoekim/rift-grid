//! How moving between grid cells and grids is shown, `[settings.ui.grid_motion]`.

use serde::{Deserialize, Serialize};

use super::{ConfigEnum, ConfigSchema};

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
    #[serde(default)]
    /// How the slide gathers speed and slows down.
    #[setting(
        label = "Easing",
        choices,
        enabled_by = "pan",
        aliases = "curve bezier timing spring ease acceleration"
    )]
    pub easing: MotionEasing,
    #[serde(default)]
    /// The custom easing: a cubic Bézier curve `x1, y1, x2, y2`, as in CSS's
    /// `cubic-bezier()`, with x1 and x2 from 0 to 1.
    #[setting(label = "Custom curve", enabled_by = "pan", aliases = "cubic bezier easing")]
    pub curve: String,
    #[serde(default)]
    /// How far the screen draws back halfway through a slide between cells, as a
    /// fraction of its size. 0 keeps it flat.
    #[setting(
        label = "Zoom out",
        unit = "%",
        scale = 100.0,
        enabled_by = "pan",
        aliases = "zoom scale depth pull back"
    )]
    pub zoom: f64,
    #[serde(default = "default_zoom_out_time")]
    /// The share of the slide's time spent drawing back; the slide across
    /// starts near its end.
    #[setting(label = "Zoom out time", unit = "%", scale = 100.0, enabled_by = "pan")]
    pub zoom_out_time: f64,
    #[serde(default = "default_zoom_in_time")]
    /// The share of the slide's time spent coming in again; the slide across
    /// ends halfway through it.
    #[setting(label = "Zoom in time", unit = "%", scale = 100.0, enabled_by = "pan")]
    pub zoom_in_time: f64,
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
fn default_zoom_out_time() -> f64 { 0.25 }
fn default_zoom_in_time() -> f64 { 0.3 }

impl Default for GridMotionSettings {
    fn default() -> Self {
        Self {
            pan: true,
            duration_ms: default_duration_ms(),
            easing: MotionEasing::default(),
            curve: String::new(),
            zoom: 0.0,
            zoom_out_time: default_zoom_out_time(),
            zoom_in_time: default_zoom_in_time(),
            hud: true,
            hud_ms: default_hud_ms(),
        }
    }
}

/// How a slide gathers speed and slows down.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum MotionEasing {
    /// Quick to start, settling gently.
    #[default]
    #[setting(label = "Glide")]
    Glide,
    /// Slow at both ends, quickest halfway.
    #[setting(label = "Smooth")]
    Smooth,
    /// Off at once, then a long soft landing.
    #[setting(label = "Snappy")]
    Snappy,
    /// Runs a little past the cell and comes back.
    #[setting(label = "Overshoot")]
    Overshoot,
    /// A spring that settles with a small bounce.
    #[setting(label = "Spring")]
    Spring,
    /// The curve in `curve`.
    #[setting(label = "Custom curve")]
    Custom,
}

impl MotionEasing {
    /// The cubic Bézier curve's control points, `custom` for `Custom` (Glide's when
    /// it does not parse); None for the spring.
    pub fn curve(self, custom: &str) -> Option<[f64; 4]> {
        Some(match self {
            Self::Glide => [0.3, 0.0, 0.2, 1.0],
            Self::Smooth => [0.65, 0.0, 0.35, 1.0],
            Self::Snappy => [0.05, 0.7, 0.1, 1.0],
            Self::Overshoot => [0.34, 1.4, 0.64, 1.0],
            Self::Spring => return None,
            Self::Custom => parse_curve(custom).unwrap_or([0.3, 0.0, 0.2, 1.0]),
        })
    }
}

/// `x1, y1, x2, y2`, separated by commas or spaces, with x1 and x2 from 0 to 1.
pub fn parse_curve(text: &str) -> Option<[f64; 4]> {
    let numbers: Vec<f64> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().ok().filter(|n: &f64| n.is_finite()))
        .collect::<Option<_>>()?;
    let curve: [f64; 4] = numbers.try_into().ok()?;
    ((0.0..=1.0).contains(&curve[0]) && (0.0..=1.0).contains(&curve[2])).then_some(curve)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_parse_like_css() {
        assert_eq!(parse_curve("0.25, 0.1, 0.25, 1"), Some([0.25, 0.1, 0.25, 1.0]));
        assert_eq!(parse_curve(" .3 0 .2 1 "), Some([0.3, 0.0, 0.2, 1.0]));
        assert_eq!(parse_curve("0.3, 0, 0.2"), None, "four numbers");
        assert_eq!(parse_curve("1.2, 0, 0.2, 1"), None, "x within 0..1");
        assert_eq!(MotionEasing::Custom.curve("nope"), MotionEasing::Glide.curve(""));
        assert_eq!(MotionEasing::Spring.curve(""), None);
    }
}
