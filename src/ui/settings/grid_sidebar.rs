//! The sidebar in subline's order: its sections Input, Workspace, Shell and
//! System, each a group of pages here. `Category`'s own order stays upstream's,
//! since pages are kept by it.

use super::CATEGORIES;
use super::Category::{self, *};

/// The pages in the sidebar's order: General; Input (Keyboard, Mouse);
/// Workspace (Grid to Rules); Shell (Menu Bar, Stack Tabs); System (Advanced,
/// About).
const ORDER: [Category; CATEGORIES.len()] = [
    General, Keyboard, Mouse, Grid, Cells, Borders, Overview, Workspaces, Layouts, Rules, MenuBar,
    StackTabs, Advanced, About,
];

/// Each page with its title and symbol, in the sidebar's order.
pub(super) fn ordered() -> impl Iterator<Item = (Category, &'static str, &'static str)> {
    ORDER.into_iter().map(|id| CATEGORIES[id.index()])
}

/// Whether the page starts one of subline's sections.
pub(super) fn starts_group(id: Category) -> bool {
    matches!(id, Keyboard | Grid | MenuBar | Advanced)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidebar_shows_every_page_once() {
        let mut shown: Vec<_> = ordered().map(|(id, _, _)| id.index()).collect();
        shown.sort_unstable();
        assert_eq!(shown, (0..CATEGORIES.len()).collect::<Vec<_>>());
    }
}
