//! Settings of the workspace grid that live in upstream's settings structs.

use serde::{Deserialize, Serialize};

use super::ConfigEnum;

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
