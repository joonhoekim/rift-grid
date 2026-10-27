//! Settings of the workspace grid that live in upstream's settings structs.

use serde::{Deserialize, Serialize};

use super::{ConfigEnum, MAX_WORKSPACES, VirtualWorkspaceSettings};

/// Grids per display, one per number key.
pub const GRID_COUNT: usize = 9;

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
    Active,
}

/// How a grid Overview looked; only the cards are left.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum GridStyle {
    /// Cards of the cells' windows, in two stages.
    #[default]
    #[setting(label = "Cards")]
    Cards,
    /// Removed; read as the cards.
    #[setting(label = "Zoom out")]
    Zoom,
}

/// How a grid Overview's first stage moves when the active cell changes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum GridFollow {
    /// Only as far as it takes to show the active cell whole.
    #[default]
    #[setting(label = "Just enough to show it")]
    Reveal,
    /// Center the active cell every time.
    #[setting(label = "Center it")]
    Center,
}
