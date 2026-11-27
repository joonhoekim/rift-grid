//! Source documents are editable; runtime configs contain expanded hotkeys.
pub mod borders;
mod document;
mod grid;
mod grid_gestures;
mod grid_motion;
mod parse;
mod schema;
mod types;
pub use borders::BorderSettings;
pub use grid::{
    GRID_COUNT, GridFollow, GridLayout, GridMaps, GridOverviewSettings, GridSettings, GridStart,
    MotionLevel,
};
pub use grid_gestures::{GestureAction, GridGestureSettings, SwipeActions};
pub use grid_motion::GridMotionSettings;
pub use document::ConfigDocument;
pub use schema::*;
pub use types::*;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod grid_tests;
