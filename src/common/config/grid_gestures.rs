//! Trackpad swipes on the workspace grid, `[settings.grid.gestures]`, as
//! subline's `[gestures]`: a table of what each swipe of three and of four
//! fingers does. Rift takes those finger counts before macOS and upstream's
//! swipes while a swipe there has an action. Distance and haptics come from
//! `[settings.gestures]`.

use serde::{Deserialize, Serialize};

use super::{ConfigEnum, ConfigSchema};
use crate::layout_engine::Direction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, ConfigSchema)]
#[serde(deny_unknown_fields)]
pub struct GridGestureSettings {
    #[serde(default = "SwipeActions::three", deserialize_with = "three_over_defaults")]
    #[setting(ignore)]
    pub three: SwipeActions,
    #[serde(default = "SwipeActions::four", deserialize_with = "four_over_defaults")]
    #[setting(ignore)]
    pub four: SwipeActions,
}

impl Default for GridGestureSettings {
    fn default() -> Self {
        Self {
            three: SwipeActions::three(),
            four: SwipeActions::four(),
        }
    }
}

/// A hand's table as written: the ways it gives, over that hand's defaults, so
/// that a file (or the settings app) giving one way keeps the others.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GivenSwipes {
    left: Option<GestureAction>,
    right: Option<GestureAction>,
    up: Option<GestureAction>,
    down: Option<GestureAction>,
}

fn over(defaults: SwipeActions, given: GivenSwipes) -> SwipeActions {
    SwipeActions {
        left: given.left.unwrap_or(defaults.left),
        right: given.right.unwrap_or(defaults.right),
        up: given.up.unwrap_or(defaults.up),
        down: given.down.unwrap_or(defaults.down),
    }
}

fn three_over_defaults<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SwipeActions, D::Error> {
    Ok(over(SwipeActions::three(), GivenSwipes::deserialize(d)?))
}

fn four_over_defaults<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SwipeActions, D::Error> {
    Ok(over(SwipeActions::four(), GivenSwipes::deserialize(d)?))
}

/// What a swipe each way does, `[settings.grid.gestures.three]` and `.four`.
/// The content follows the fingers: swiping left shows what is to the right.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub struct SwipeActions {
    pub left: GestureAction,
    pub right: GestureAction,
    pub up: GestureAction,
    pub down: GestureAction,
}

impl SwipeActions {
    /// Three fingers go to the neighbouring cell, any way.
    pub fn three() -> Self {
        Self {
            left: GestureAction::GoToCellRight,
            right: GestureAction::GoToCellLeft,
            up: GestureAction::GoToCellDown,
            down: GestureAction::GoToCellUp,
        }
    }

    /// Four fingers go through the display's grids, and up into the Overview;
    /// down stays macOS's App Exposé.
    pub fn four() -> Self {
        Self {
            left: GestureAction::NextGrid,
            right: GestureAction::PreviousGrid,
            up: GestureAction::Overview,
            down: GestureAction::None,
        }
    }

    pub fn get(&self, way: Direction) -> GestureAction {
        match way {
            Direction::Left => self.left,
            Direction::Right => self.right,
            Direction::Up => self.up,
            Direction::Down => self.down,
        }
    }

    pub fn set(&mut self, way: Direction, action: GestureAction) {
        match way {
            Direction::Left => self.left = action,
            Direction::Right => self.right = action,
            Direction::Up => self.up = action,
            Direction::Down => self.down = action,
        }
    }

    /// Whether any swipe has an action.
    pub fn any(&self) -> bool {
        [self.left, self.right, self.up, self.down]
            .iter()
            .any(|a| *a != GestureAction::None)
    }
}

/// What a swipe can do, as subline's actions that take a gesture.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default, ConfigEnum)]
#[serde(rename_all = "snake_case")]
pub enum GestureAction {
    /// Nothing: the swipe stays macOS's.
    #[default]
    #[setting(label = "Nothing")]
    None,
    #[setting(label = "Cell to the left")]
    GoToCellLeft,
    #[setting(label = "Cell to the right")]
    GoToCellRight,
    #[setting(label = "Cell above")]
    GoToCellUp,
    #[setting(label = "Cell below")]
    GoToCellDown,
    /// The tile beside, or past the cell's edge the next cell, as `focus`.
    #[setting(label = "Tile or cell to the left")]
    FocusLeft,
    #[setting(label = "Tile or cell to the right")]
    FocusRight,
    #[setting(label = "Tile or cell above")]
    FocusUp,
    #[setting(label = "Tile or cell below")]
    FocusDown,
    /// The next grid on the display, round the end.
    #[setting(label = "Next grid")]
    NextGrid,
    #[setting(label = "Previous grid")]
    PreviousGrid,
    /// Opens the Overview and steps out a stage; the opposite swipe steps back
    /// in and leaves it.
    #[setting(label = "Overview")]
    Overview,
    #[setting(label = "Default cell")]
    FocusDefaultCell,
    #[setting(label = "Previous window")]
    FocusRecentWindow,
    #[setting(label = "Display to the left")]
    FocusDisplayLeft,
    #[setting(label = "Display to the right")]
    FocusDisplayRight,
    #[setting(label = "Display above")]
    FocusDisplayUp,
    #[setting(label = "Display below")]
    FocusDisplayDown,
}
