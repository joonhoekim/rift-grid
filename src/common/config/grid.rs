//! The workspace grid's settings, `[settings.grid]`, and its checks of
//! upstream's `[virtual_workspaces]`.

use serde::{Deserialize, Serialize};

use super::{
    ConfigEnum, ConfigSchema, GridGestureSettings, GridMotionSettings,
    VirtualWorkspaceSettings,
};

/// Grids per display, one per number key.
pub const GRID_COUNT: usize = 9;

fn yes() -> bool { true }

/// The most rows, and columns, a grid can have.
pub const MAX_GRID_SIDE: usize = 9;

/// `[settings.grid]`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct GridSettings {
    #[serde(default)]
    /// Rows of cells in every grid, odd numbers only: an even number reads as the
    /// next odd one. 0, with columns 0, keeps upstream's line of workspaces.
    #[setting(label = "Rows", aliases = "grid cells size 3x3 layout matrix height")]
    pub rows: usize,
    #[serde(default)]
    /// Cells in a row of every grid, odd numbers only: an even number reads as
    /// the next odd one.
    #[setting(label = "Columns", aliases = "grid cells size 3x3 layout matrix width")]
    pub columns: usize,
    #[serde(default)]
    /// The cell a display starts on when rift-grid starts.
    #[setting(label = "Starting cell", choices, aliases = "start login launch default last")]
    pub start: GridStart,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Grid 1's cell names, row by row; `names_2` to `names_9` name the other
    /// grids'. A cell without a name is its number.
    #[setting(ignore)]
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_2: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_3: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_4: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_5: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_6: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_7: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_8: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[setting(ignore)]
    pub names_9: Vec<String>,
    #[serde(default)]
    #[setting(ignore)]
    pub overview: GridOverviewSettings,
    #[serde(default)]
    #[setting(ignore)]
    pub focus: super::BorderSettings,
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
    #[setting(
        label = "Motion",
        choices,
        group = "cells",
        aliases = "reduce reduced animation accessibility"
    )]
    pub motion: MotionLevel,
    #[serde(default)]
    /// Which grids get a map in the menu bar.
    #[setting(label = "Grid map", choices, group = "menu_bar", aliases = "grid map 3x3 menu bar")]
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

/// What the workspace store takes from `[settings.grid]`.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct GridLayout {
    pub rows: usize,
    pub columns: usize,
    /// Each grid's cell names, grid 1's first.
    pub names: Vec<Vec<String>>,
    pub start: GridStart,
}

impl GridLayout {
    pub fn new(rows: usize, columns: usize) -> Self {
        Self { rows, columns, ..Self::default() }
    }

    /// The name of cell `cell` (0-based) of grid `grid` (0-based): its own,
    /// else its number.
    pub fn cell_name(&self, grid: usize, cell: usize) -> String {
        self.names
            .get(grid)
            .and_then(|names| names.get(cell))
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| (cell + 1).to_string())
    }
}

/// The cell a display starts on.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum GridStart {
    /// The default cell, grid 1's center.
    #[default]
    #[setting(label = "The default cell")]
    Default,
    /// The cell the display was on when rift-grid last ran.
    #[setting(label = "The last cell")]
    Last,
}

impl GridSettings {
    /// What the workspace store takes: none without a grid.
    pub fn layout(&self) -> Option<GridLayout> {
        let (rows, columns) = self.shape()?;
        let names = [
            &self.names, &self.names_2, &self.names_3, &self.names_4, &self.names_5,
            &self.names_6, &self.names_7, &self.names_8, &self.names_9,
        ]
        .map(Clone::clone)
        .to_vec();
        Some(GridLayout { rows, columns, names, start: self.start })
    }

    /// The grids' rows and columns, each odd and at most `MAX_GRID_SIDE`; none
    /// without a grid.
    pub fn shape(&self) -> Option<(usize, usize)> {
        let side = |n: usize| (n | 1).min(MAX_GRID_SIDE);
        (self.rows > 0 && self.columns > 0).then(|| (side(self.rows), side(self.columns)))
    }

    pub(super) fn validate(&self, issues: &mut Vec<String>) {
        if (self.rows == 0) != (self.columns == 0) {
            issues.push("[settings.grid] needs both rows and columns, or neither".into());
        }
        for (name, n) in [("rows", self.rows), ("columns", self.columns)] {
            if n > MAX_GRID_SIDE {
                issues.push(format!("[settings.grid] {name} can be at most {MAX_GRID_SIDE}"));
            }
        }
        // Two cells of a grid with one name would be two workspaces with one.
        if let Some(layout) = self.layout() {
            let cells = layout.rows * layout.columns;
            for (grid, names) in layout.names.iter().enumerate() {
                let key = if grid == 0 { "names".to_string() } else { format!("names_{}", grid + 1) };
                if names.len() > cells {
                    issues.push(format!(
                        "[settings.grid] {key} has {} names for {cells} cells",
                        names.len()
                    ));
                }
                let mut seen = std::collections::HashSet::new();
                if let Some(name) =
                    (0..cells).map(|cell| layout.cell_name(grid, cell)).find(|name| !seen.insert(name.clone()))
                {
                    issues.push(format!("[settings.grid] {key} names two cells \"{name}\""));
                }
            }
        }
    }
}

impl VirtualWorkspaceSettings {
    /// These settings with the grid from `[settings.grid]`, which the
    /// workspace store reads here.
    pub fn with_grid(mut self, grid: &GridSettings) -> Self {
        // Said once for each shape written, not on every parse.
        static WARNED: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));
        let written = (grid.rows, grid.columns);
        if let Ok(mut warned) = WARNED.lock()
            && *warned != written
        {
            *warned = written;
            for (name, n) in [("rows", grid.rows), ("columns", grid.columns)] {
                if n % 2 == 0 && (1..MAX_GRID_SIDE).contains(&n) {
                    tracing::warn!(
                        "[settings.grid] {name} = {n} reads as {}: odd numbers only",
                        n + 1
                    );
                }
            }
        }
        self.grid_layout = grid.layout();
        self
    }

    /// The grids' rows and columns, none without a grid.
    pub fn grid_shape(&self) -> Option<(usize, usize)> {
        self.grid_layout.as_ref().map(|grid| (grid.rows, grid.columns))
    }

    pub(super) fn validate_grid(&self, issues: &mut Vec<String>) {
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
