//! Window border settings, `[settings.ui.borders]`.

use serde::{Deserialize, Serialize};

use super::ConfigSchema;

fn yes() -> bool { true }

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct BorderSettings {
    #[serde(default)]
    /// Draw a border around each window, in place of a separate program such as
    /// JankyBorders.
    #[setting(label = "Enabled", order = 0, aliases = "window borders outline focus ring")]
    pub enabled: bool,
    #[serde(default = "default_border_width")]
    /// Thickness of the border, drawn just outside the window's edge.
    #[setting(label = "Width", unit = "pt", enabled_by = "enabled")]
    pub width: f64,
    #[serde(default = "default_border_radius")]
    /// Corner radius of the windows the border follows; 0 for square corners.
    #[setting(label = "Corner radius", unit = "pt", enabled_by = "enabled")]
    pub radius: f64,
    #[serde(default = "default_border_active_color")]
    /// The focused window's border, as 0xAARRGGBB or #RRGGBB[AA].
    #[setting(label = "Focused color", custom, aliases = "active colour")]
    pub active_color: String,
    #[serde(default = "default_border_inactive_color")]
    /// The other windows' borders, as 0xAARRGGBB or #RRGGBB[AA]. Empty draws none.
    #[setting(label = "Unfocused color", custom, aliases = "inactive colour")]
    pub inactive_color: String,
    #[serde(default = "yes")]
    /// A soft glow around the focused window's border.
    #[setting(label = "Glow", enabled_by = "enabled")]
    pub glow: bool,
    #[serde(default = "default_border_glow_radius")]
    /// How far the glow spreads out from the border.
    #[setting(label = "Glow size", unit = "pt", enabled_by = "glow")]
    pub glow_radius: f64,
    #[serde(default = "default_border_animation_ms")]
    /// How long the focused border takes to slide to the next focused window.
    /// 0 moves it at once.
    #[setting(
        label = "Focus slide",
        unit = "ms",
        enabled_by = "enabled",
        aliases = "animation move glide focus border"
    )]
    pub animation_ms: f64,
}

fn default_border_width() -> f64 { 2.0 }
// macOS 26's plain windows; windows with a toolbar round more.
fn default_border_radius() -> f64 { 16.0 }
fn default_border_glow_radius() -> f64 { 12.0 }
fn default_border_animation_ms() -> f64 { 180.0 }
fn default_border_active_color() -> String { "0xffb4befe".into() }
fn default_border_inactive_color() -> String { "0xff45475a".into() }

impl Default for BorderSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            width: default_border_width(),
            radius: default_border_radius(),
            active_color: default_border_active_color(),
            inactive_color: default_border_inactive_color(),
            glow: true,
            glow_radius: default_border_glow_radius(),
            animation_ms: default_border_animation_ms(),
        }
    }
}
