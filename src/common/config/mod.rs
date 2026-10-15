//! Source documents are editable; runtime configs contain expanded hotkeys.
mod borders;
mod document;
mod grid;
mod parse;
mod schema;
mod types;
pub use borders::BorderSettings;
pub use grid::{GridFollow, GridMaps};
pub use document::ConfigDocument;
pub use schema::*;
pub use types::*;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod grid_tests;
