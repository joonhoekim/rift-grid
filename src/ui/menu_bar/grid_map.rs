//! The workspace grid in the menu bar: one small map per display instead of a
//! label per workspace.

use objc2_core_foundation::{CGPoint, CGRect, CGSize};

use super::{CachedTextLine, MenuIconLayout, SEPARATOR_SPACING, WorkspaceRenderInput};
use crate::actor::menu_bar::Workspace;

// The workspace grid as one small map per display, row by row from the top.
const GRID_MAP_CELL_WIDTH: f64 = 5.0;
const GRID_MAP_CELL_HEIGHT: f64 = 4.0;
const GRID_MAP_GAP: f64 = 1.5;
const GRID_MAP_EMPTY_ALPHA: f64 = 0.25;
const GRID_MAP_OCCUPIED_ALPHA: f64 = 0.55;
const GRID_MAP_ACTIVE_ALPHA: f64 = 1.0;

fn grid_map_alpha(workspace: &Workspace) -> f64 {
    if workspace.is_active {
        GRID_MAP_ACTIVE_ALPHA
    } else if workspace.window_count > 0 {
        GRID_MAP_OCCUPIED_ALPHA
    } else {
        GRID_MAP_EMPTY_ALPHA
    }
}

/// One map per display: a cell per workspace at its row and column, shaded by
/// whether it is empty, holds windows, or is the active one.
pub(super) fn build_grid_layout(
    inputs: &[WorkspaceRenderInput<'_>],
    separator: impl FnOnce() -> Option<CachedTextLine>,
) -> MenuIconLayout {
    let mut groups: Vec<Vec<&WorkspaceRenderInput<'_>>> = Vec::new();
    for input in inputs {
        match groups.last_mut() {
            Some(group)
                if (group[0].display_uuid, group[0].space)
                    == (input.display_uuid, input.space) =>
            {
                group.push(input)
            }
            _ => groups.push(vec![input]),
        }
    }
    let shape = |group: &[&WorkspaceRenderInput<'_>]| {
        group.iter().filter_map(|input| input.grid_cell).fold((0, 0), |(rows, columns), (r, c)| {
            (rows.max(r + 1), columns.max(c + 1))
        })
    };
    let extent = |count: usize, cell: f64| {
        count as f64 * cell + count.saturating_sub(1) as f64 * GRID_MAP_GAP
    };
    let height = groups
        .iter()
        .map(|group| extent(shape(group).0, GRID_MAP_CELL_HEIGHT))
        .fold(0.0, f64::max);
    let separator_line = (groups.len() > 1).then(separator).flatten();
    let mut separators = Vec::new();
    let mut grid_cells = Vec::with_capacity(inputs.len());
    let mut width = 0.0;
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            separators.push(width + SEPARATOR_SPACING);
            width += 2.0 * SEPARATOR_SPACING + separator_line.as_ref().map_or(0.0, |l| l.width);
        }
        let (rows, columns) = shape(group);
        let top = (height + extent(rows, GRID_MAP_CELL_HEIGHT)) / 2.0;
        for input in group {
            let Some((row, column)) = input.grid_cell else { continue };
            grid_cells.push((
                CGRect::new(
                    CGPoint::new(
                        width + column as f64 * (GRID_MAP_CELL_WIDTH + GRID_MAP_GAP),
                        top - GRID_MAP_CELL_HEIGHT
                            - row as f64 * (GRID_MAP_CELL_HEIGHT + GRID_MAP_GAP),
                    ),
                    CGSize::new(GRID_MAP_CELL_WIDTH, GRID_MAP_CELL_HEIGHT),
                ),
                grid_map_alpha(input.workspace),
            ));
        }
        width += extent(columns, GRID_MAP_CELL_WIDTH);
    }
    MenuIconLayout {
        size: CGSize::new(width, height),
        grid_cells,
        separators,
        separator_line,
        ..MenuIconLayout::default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MenuIconRenderKey, render_inputs};
    use super::*;
    use crate::actor::menu_bar::DisplayWorkspaces;
    use crate::common::config::{MenuBarDisplayMode, MenuBarSettings};
    use crate::sys::screen::SpaceId;

    fn workspace(id: &str, index: usize, name: &str, active: bool, layout: &str) -> Workspace {
        Workspace {
            id: id.to_owned(),
            index,
            name: name.to_owned(),
            layout_mode: layout.to_owned(),
            is_active: active,
            window_count: 0,
            windows: Vec::new(),
            grid_cell: None,
        }
    }

    #[test]
    fn grid_shows_one_map_per_display_shaded_by_state() {
        let mut workspaces: Vec<_> = (0..9)
            .map(|index| Workspace {
                grid_cell: Some((index / 3, index % 3)),
                ..workspace(&index.to_string(), index, "", index == 4, "bsp")
            })
            .collect();
        workspaces[0].window_count = 2;
        let displays = vec![DisplayWorkspaces {
            display_uuid: "a".into(),
            space: SpaceId::new(1),
            is_active_context: true,
            workspaces,
        }];
        // Empty cells stay, whatever the mode says.
        let settings = MenuBarSettings {
            show_empty: false,
            mode: MenuBarDisplayMode::Active,
            ..MenuBarSettings::default()
        };
        let inputs = render_inputs(&displays, &settings);
        assert_eq!(inputs.len(), 9);
        assert!(inputs.iter().all(|input| input.label.is_empty() && !input.show_windows));

        let layout = build_grid_layout(&inputs, || None);
        let cells = &layout.grid_cells;
        assert_eq!(cells.len(), 9);
        assert!(layout.workspaces.is_empty());
        assert_eq!(
            layout.size,
            CGSize::new(
                3.0 * GRID_MAP_CELL_WIDTH + 2.0 * GRID_MAP_GAP,
                3.0 * GRID_MAP_CELL_HEIGHT + 2.0 * GRID_MAP_GAP
            )
        );
        // Row 0 is on top in the unflipped canvas, columns run left to right.
        assert_eq!(cells[0].0.max().y, layout.size.height);
        assert_eq!(cells[8].0.origin, CGPoint::new(layout.size.width - GRID_MAP_CELL_WIDTH, 0.0));
        assert!(cells[1].0.origin.x > cells[0].0.max().x);
        assert!(cells[3].0.max().y < cells[0].0.origin.y);
        assert_eq!(
            cells.iter().map(|&(_, alpha)| alpha).collect::<Vec<_>>(),
            [
                GRID_MAP_OCCUPIED_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
                GRID_MAP_ACTIVE_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
                GRID_MAP_EMPTY_ALPHA,
            ]
        );

        // A cell gaining its first window redraws the map.
        let key = MenuIconRenderKey::from_inputs(&inputs);
        let mut changed = displays.clone();
        changed[0].workspaces[5].window_count = 1;
        assert!(!key.matches_inputs(&render_inputs(&changed, &settings)));
        changed[0].workspaces[5].window_count = 0;
        changed[0].workspaces[0].window_count = 3;
        assert!(key.matches_inputs(&render_inputs(&changed, &settings)));
    }
}
