//! The workspace grid's settings, `[settings.grid]`, and its checks of
//! upstream's `[virtual_workspaces]`.

use serde::{Deserialize, Serialize};

use super::{
    ConfigEnum, ConfigSchema, GridGestureSettings, GridMotionSettings, MAX_WORKSPACES,
    VirtualWorkspaceSettings,
};

/// Grids per display, one per number key.
pub const GRID_COUNT: usize = 9;

fn yes() -> bool { true }

/// `[settings.grid]`. The grid's layout itself, `grid_columns`, is in
/// `[virtual_workspaces]` with the other workspace settings.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct GridSettings {
    #[serde(default)]
    #[setting(ignore)]
    pub overview: GridOverviewSettings,
    #[serde(default)]
    #[setting(ignore)]
    pub moves: GridMotionSettings,
    #[serde(default)]
    #[setting(ignore)]
    pub gestures: GridGestureSettings,
    #[serde(default)]
    /// How much rift-grid's own animations move: the slides between cells, the
    /// grid map, the edge light and the window borders. System follows Reduce
    /// motion in the Accessibility settings.
    #[setting(label = "Motion", choices, aliases = "reduce reduced animation accessibility")]
    pub motion: MotionLevel,
    #[serde(default)]
    /// Which grids get a map in the menu bar.
    #[setting(label = "Grid map", choices, aliases = "grid map 3x3 menu bar")]
    pub grid_map: GridMaps,
}

/// `[settings.grid.overview]`: the grid Overview, over upstream's Overview.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct GridOverviewSettings {
    #[serde(default)]
    /// Show the whole grid as soon as the Overview opens. Off, it first shows the
    /// current cell at the zoom below with its neighbours around it, and opening
    /// it again steps back to the whole grid.
    #[setting(label = "Fit the whole grid", aliases = "grid overview stages whole at a glance")]
    pub fit: bool,
    #[serde(default = "default_zoom")]
    /// Size of a cell in the Overview's first stage, as a fraction of the
    /// display. The whole grid is never shown larger than this.
    #[setting(
        label = "Overview size",
        unit = "%",
        scale = 100.0,
        aliases = "grid overview cell size first stage"
    )]
    pub zoom: f64,
    #[serde(default)]
    /// How the first stage moves when the current cell changes.
    #[setting(
        label = "Following focus",
        choices,
        aliases = "grid overview follow focus reveal centre center"
    )]
    pub follow: GridFollow,
    #[serde(default = "default_gap")]
    /// Space between cells, as a fraction of the display's shorter side, so it
    /// looks the same at any size and scale.
    #[setting(
        label = "Overview cell gap",
        unit = "%",
        scale = 100.0,
        aliases = "grid overview spacing cells"
    )]
    pub gap: f64,
    #[serde(default = "yes")]
    /// Show each window's app and title over its tile.
    #[setting(label = "Tile title bars", aliases = "grid overview titles icons names")]
    pub labels: bool,
    #[serde(default)]
    /// Capture every preview at the size it shows at, at the display's
    /// resolution, rather than at most 1024×768 for all but the selected
    /// window: sharp previews, for more memory and capture work while the
    /// Overview is open.
    #[setting(
        label = "Full-resolution previews",
        aliases = "quality sharp retina resolution blurry soft previews capture"
    )]
    pub full_resolution: bool,
}

fn default_zoom() -> f64 { 0.5 }

fn default_gap() -> f64 { 0.01 }

impl Default for GridOverviewSettings {
    fn default() -> Self {
        Self {
            fit: false,
            zoom: default_zoom(),
            follow: GridFollow::default(),
            gap: default_gap(),
            labels: true,
            full_resolution: false,
        }
    }
}

impl VirtualWorkspaceSettings {
    pub(super) fn validate_grid(&self, issues: &mut Vec<String>) {
        let columns = self.grid_columns;
        if columns == 0 {
            return;
        }
        if GRID_COUNT * columns * columns > MAX_WORKSPACES {
            issues.push(format!(
                "grid_columns ({columns}) makes {} workspaces a display, more than \
                 {MAX_WORKSPACES}; it can be at most 3",
                GRID_COUNT * columns * columns
            ));
        }
        for (index, rule) in self.app_rules.iter().enumerate() {
            if rule.grid.is_some_and(|grid| !(1..=GRID_COUNT).contains(&grid)) {
                issues.push(format!("App rule {index}'s grid must be from 1 to {GRID_COUNT}"));
            }
        }
    }
}

/// Which grids the menu bar draws a map for.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum GridMaps {
    /// The grid on screen and every grid with windows, side by side.
    #[default]
    #[setting(label = "Every grid in use")]
    All,
    /// The grid on screen alone, after its number when other grids are in use.
    #[setting(label = "The grid on screen")]
    Current,
}

/// How much the grid's animations move.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum MotionLevel {
    /// Full, or reduced while Reduce motion is on in the Accessibility settings.
    #[default]
    #[setting(label = "As the system")]
    System,
    #[setting(label = "Full")]
    Full,
    /// A third of the time, and the screen does not draw back between cells.
    #[setting(label = "Reduced")]
    Reduced,
    /// Everything shows at once.
    #[setting(label = "Off")]
    Off,
}

impl MotionLevel {
    /// `settings` as this level shows them; `reduce` is whether the system asks
    /// for reduced motion.
    pub fn moves(self, settings: &GridMotionSettings, reduce: bool) -> GridMotionSettings {
        let mut shown = settings.clone();
        match self.resolve(reduce) {
            Self::Reduced => {
                shown.duration_ms /= 3.0;
                shown.zoom = 0.0;
            }
            Self::Off => shown.pan = false,
            _ => {}
        }
        shown
    }

    /// How long an animation of `ms` takes at this level.
    pub fn duration_ms(self, ms: f64, reduce: bool) -> f64 { ms * self.time_scale(reduce) }

    /// The share of their time animations take at this level: 0 when off.
    pub fn time_scale(self, reduce: bool) -> f64 {
        match self.resolve(reduce) {
            Self::Reduced => 1.0 / 3.0,
            Self::Off => 0.0,
            _ => 1.0,
        }
    }

    fn resolve(self, reduce: bool) -> Self {
        match self {
            Self::System if reduce => Self::Reduced,
            Self::System => Self::Full,
            level => level,
        }
    }
}

/// How a grid Overview's first stage moves when the current cell changes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum GridFollow {
    /// Only as far as it takes to show the current cell whole.
    #[default]
    #[setting(label = "Just enough to show it")]
    Reveal,
    /// Center the current cell every time.
    #[setting(label = "Center it")]
    Center,
}
