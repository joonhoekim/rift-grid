//! The workspace grid in the menu bar: a small 3x3 map per grid instead of a
//! label per workspace.

use std::borrow::Cow;

use objc2_app_kit::NSColor;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGContext;

use objc2::Message;
use objc2_app_kit::NSMenuItem;

use super::{
    CachedTextLine, MenuActionHandler, MenuIcon, MenuIconLayout, SEPARATOR_SPACING,
    WorkspaceRenderInput, add_rounded_rect, make_menu_item, set_menu_item_checked,
    set_menu_item_hotkey,
};
use crate::actor::reactor;
use crate::actor::wm_controller::WmCommand;
use crate::layout_engine::LayoutCommand;
use crate::sys::hotkey::Hotkey;
use crate::actor::menu_bar::{DisplayWorkspaces, Workspace};
use crate::common::config::{GridMaps, MenuBarSettings};

const GRID_MAP_CELL_WIDTH: f64 = 5.0;
const GRID_MAP_CELL_HEIGHT: f64 = 4.0;
const GRID_MAP_GAP: f64 = 1.5;
const GRID_MAP_EMPTY_ALPHA: f64 = 0.25;
const GRID_MAP_OCCUPIED_ALPHA: f64 = 0.55;
const GRID_MAP_ACTIVE_ALPHA: f64 = 1.0;
/// Grids not on screen are drawn at this share of their strength.
const GRID_MAP_DIMMED: f64 = 0.45;
/// Between two grids' maps on one display.
const GRID_MAP_SPACING: f64 = 5.0;
/// Between a grid's number and its map.
const GRID_LABEL_SPACING: f64 = 3.0;

fn grid_map_alpha(workspace: &Workspace) -> f64 {
    if workspace.is_active {
        GRID_MAP_ACTIVE_ALPHA
    } else if workspace.window_count > 0 {
        GRID_MAP_OCCUPIED_ALPHA
    } else {
        GRID_MAP_EMPTY_ALPHA
    }
}

fn active_grid(display: &DisplayWorkspaces) -> Option<usize> {
    display.workspaces.iter().find(|w| w.is_active).and_then(|w| w.grid)
}

fn in_use(display: &DisplayWorkspaces, grid: Option<usize>) -> bool {
    display.workspaces.iter().any(|w| w.grid == grid && w.window_count > 0)
}

/// Whether a cell's grid gets a map: the grid on screen always, the grids in use
/// too unless only the grid on screen is asked for.
pub(super) fn shown(
    display: &DisplayWorkspaces,
    workspace: &Workspace,
    settings: &MenuBarSettings,
) -> bool {
    workspace.grid == active_grid(display)
        || (settings.grid_maps == GridMaps::All && in_use(display, workspace.grid))
}

/// The number before a cell's map: every map's, as on subline's strip, or a lone
/// map's when other grids are in use and it would not be clear which grid is on
/// screen.
pub(super) fn label(
    display: &DisplayWorkspaces,
    workspace: &Workspace,
    settings: &MenuBarSettings,
) -> Cow<'static, str> {
    let active = active_grid(display);
    let others = display
        .workspaces
        .iter()
        .any(|w| w.grid != active && w.window_count > 0);
    let numbered = match settings.grid_maps {
        GridMaps::All => workspace.grid,
        GridMaps::Active => active.filter(|_| others),
    };
    numbered.map_or(Cow::Borrowed(""), |grid| Cow::Owned((grid + 1).to_string()))
}

/// A map per grid, a cell per workspace at its row and column, shaded by whether
/// it is empty, holds windows, or is the one on screen; grids not on screen are
/// dimmed. Displays are set apart as elsewhere in the menu bar.
pub(super) fn build_grid_layout(
    inputs: &[WorkspaceRenderInput<'_>],
    separator: impl FnOnce() -> Option<CachedTextLine>,
    text: impl Fn(&str) -> Option<CachedTextLine>,
) -> MenuIconLayout {
    let mut groups: Vec<Vec<&WorkspaceRenderInput<'_>>> = Vec::new();
    for input in inputs {
        match groups.last_mut() {
            Some(group)
                if (group[0].display_uuid, group[0].space, group[0].grid)
                    == (input.display_uuid, input.space, input.grid) =>
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
    let displays =
        groups.windows(2).filter(|w| (w[0][0].display_uuid, w[0][0].space) != (w[1][0].display_uuid, w[1][0].space)).count();
    let separator_line = (displays > 0).then(separator).flatten();
    let mut separators = Vec::new();
    let mut grid_cells = Vec::with_capacity(inputs.len());
    let mut grid_labels = Vec::new();
    let mut width = 0.0;
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            let previous = groups[i - 1][0];
            if (previous.display_uuid, previous.space) == (group[0].display_uuid, group[0].space) {
                width += GRID_MAP_SPACING;
            } else {
                separators.push(width + SEPARATOR_SPACING);
                width += 2.0 * SEPARATOR_SPACING + separator_line.as_ref().map_or(0.0, |l| l.width);
            }
        }
        let strength =
            if group.iter().any(|input| input.workspace.is_active) { 1.0 } else { GRID_MAP_DIMMED };
        if let Some(line) = Some(group[0].label.as_ref()).filter(|l| !l.is_empty()).and_then(&text) {
            let line_width = line.width;
            grid_labels.push((width, line, strength));
            width += line_width + GRID_LABEL_SPACING;
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
                grid_map_alpha(input.workspace) * strength,
            ));
        }
        width += extent(columns, GRID_MAP_CELL_WIDTH);
    }
    MenuIconLayout {
        size: CGSize::new(width, height),
        grid_cells,
        grid_labels,
        separators,
        separator_line,
        ..MenuIconLayout::default()
    }
}

/// The maps' cells, and the numbers before lone maps.
pub(super) fn draw(cg: &CGContext, layout: &MenuIconLayout, y_offset: f64, foreground: &NSColor) {
    for &(cell, alpha) in &layout.grid_cells {
        add_rounded_rect(
            cg,
            cell.origin.x,
            cell.origin.y + y_offset,
            cell.size.width,
            cell.size.height,
            1.0,
        );
        CGContext::set_rgb_fill_color(Some(cg), 1.0, 1.0, 1.0, alpha);
        CGContext::fill_path(Some(cg));
    }
    for (x, line, strength) in &layout.grid_labels {
        foreground.setFill();
        let baseline = y_offset + (layout.size.height - (line.ascent + line.descent)) / 2.0
            + line.descent;
        CGContext::save_g_state(Some(cg));
        CGContext::set_alpha(Some(cg), *strength);
        CGContext::set_text_position(Some(cg), *x, baseline);
        unsafe { line.line.draw(cg) };
        CGContext::restore_g_state(Some(cg));
    }
}

/// The Workspace menu's entry for a grid: its number, and how many windows it
/// holds when any.
fn grid_menu_title(grid: usize, windows: usize) -> String {
    match windows {
        0 => format!("Grid {}", grid + 1),
        1 => format!("Grid {} · 1 window", grid + 1),
        n => format!("Grid {} · {n} windows", grid + 1),
    }
}

/// The key bound to showing grid N (1-based), for the menu to show.
fn grid_hotkey(hotkeys: &[(Hotkey, WmCommand)], grid: usize) -> Option<&Hotkey> {
    hotkeys.iter().find_map(|(hotkey, command)| {
        matches!(
            command,
            WmCommand::ReactorCommand(reactor::Command::Layout(LayoutCommand::SwitchToGrid(n)))
                if *n == grid
        )
        .then_some(hotkey)
    })
}

impl MenuIcon {
    /// With workspaces in grids, the Workspace menu lists the grids rather than
    /// every cell, the one on screen checked, and stepping through workspaces one
    /// by one is left out. False when the workspaces are not a grid.
    pub(super) fn sync_grid_menu(
        &mut self,
        workspaces: &[Workspace],
        hotkeys: &[(Hotkey, WmCommand)],
    ) -> bool {
        let grid = !workspaces.is_empty() && workspaces.iter().all(|w| w.grid.is_some());
        self.next_workspace_item.setHidden(grid);
        self.prev_workspace_item.setHidden(grid);
        for item in self.grid_items.drain(..) {
            self.workspace_submenu.remove(&item);
        }
        if !grid {
            return false;
        }
        for workspace in self.workspace_items.drain(..) {
            self.workspace_submenu.remove(&workspace.item);
        }
        let grids = workspaces.iter().filter_map(|w| w.grid).max().map_or(0, |g| g + 1);
        for grid in 0..grids {
            let windows =
                workspaces.iter().filter(|w| w.grid == Some(grid)).map(|w| w.window_count).sum();
            let item = make_menu_item(
                self.mtm,
                &grid_menu_title(grid, windows),
                Some(MenuActionHandler::on_switch_grid),
                Some(&self.menu_handler),
            );
            let native = item.ns_menu_item().retain();
            native.setTag(grid as isize);
            set_menu_item_checked(
                &native,
                workspaces.iter().any(|w| w.is_active && w.grid == Some(grid)),
            );
            set_menu_item_hotkey(&native, grid_hotkey(hotkeys, grid + 1));
            self.workspace_submenu.add(item);
            self.grid_items.push(native);
        }
        self.workspace_item.setEnabled(true);
        true
    }
}

impl MenuActionHandler {
    fn on_switch_grid(&self, sender: Option<&NSMenuItem>) {
        if let Some(tag) = sender.map(|sender| sender.tag()).filter(|&tag| tag >= 0) {
            self.emit(super::MenuAction::SwitchToGrid(tag as usize + 1));
        }
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
            grid: None,
        }
    }

    #[test]
    fn the_workspace_menu_names_grids_with_their_windows_and_keys() {
        assert_eq!(grid_menu_title(0, 0), "Grid 1");
        assert_eq!(grid_menu_title(1, 1), "Grid 2 · 1 window");
        assert_eq!(grid_menu_title(8, 3), "Grid 9 · 3 windows");
        let config = crate::common::config::Config::parse(
            r#"
                [settings]
                [keys]
                "Alt + 2" = { switch_to_grid = 2 }
            "#,
        )
        .unwrap();
        let hotkeys: Vec<_> = config
            .binding_mode_specs
            .iter()
            .find(|(mode, _)| mode == "default")
            .unwrap()
            .1
            .iter()
            .filter_map(|(spec, command)| Some((spec.parse::<Hotkey>().ok()?, command.clone())))
            .collect();
        assert!(grid_hotkey(&hotkeys, 2).is_some());
        assert!(grid_hotkey(&hotkeys, 1).is_none());
    }

    #[test]
    fn a_map_per_grid_in_use_and_the_grid_on_screen() {
        // Grid 1 on screen at its centre with a window in its top-left cell; grid 2
        // has a window; grid 3 has none.
        let workspaces: Vec<_> = (0..27)
            .map(|index| {
                let mut w = workspace(&index.to_string(), index, "", index == 4, "bsp");
                w.grid = Some(index / 9);
                w.grid_cell = Some(((index % 9) / 3, index % 3));
                w.window_count = usize::from(index == 0 || index == 10);
                w
            })
            .collect();
        let displays = vec![DisplayWorkspaces {
            display_uuid: "a".into(),
            space: SpaceId::new(1),
            is_active_context: true,
            workspaces,
        }];
        let settings = MenuBarSettings {
            show_empty: false,
            mode: MenuBarDisplayMode::Active,
            ..MenuBarSettings::default()
        };
        let inputs = render_inputs(&displays, &settings);
        assert_eq!(inputs.len(), 18, "grids 1 and 2, every cell; not grid 3");
        assert!(inputs.iter().all(|input| !input.show_windows));
        assert!(inputs[..9].iter().all(|input| input.label == "1"), "every map has its number");
        assert!(inputs[9..].iter().all(|input| input.label == "2"));

        let layout = build_grid_layout(&inputs, || None, |_| None);
        let cells = &layout.grid_cells;
        assert_eq!(cells.len(), 18);
        let map = 3.0 * GRID_MAP_CELL_WIDTH + 2.0 * GRID_MAP_GAP;
        assert_eq!(
            layout.size,
            CGSize::new(2.0 * map + GRID_MAP_SPACING, 3.0 * GRID_MAP_CELL_HEIGHT + 2.0 * GRID_MAP_GAP)
        );
        // Row 0 is on top in the unflipped canvas, columns run left to right.
        assert_eq!(cells[0].0.max().y, layout.size.height);
        assert!(cells[1].0.origin.x > cells[0].0.max().x);
        assert!(cells[3].0.max().y < cells[0].0.origin.y);
        assert_eq!(cells[0].1, GRID_MAP_OCCUPIED_ALPHA);
        assert_eq!(cells[4].1, GRID_MAP_ACTIVE_ALPHA);
        assert_eq!(cells[1].1, GRID_MAP_EMPTY_ALPHA);
        // Grid 2's map comes after, dimmed.
        assert_eq!(cells[9].0.origin.x, map + GRID_MAP_SPACING);
        assert_eq!(cells[10].1, GRID_MAP_OCCUPIED_ALPHA * GRID_MAP_DIMMED);

        // Only the grid on screen, after its number since grid 2 is in use.
        let lone = MenuBarSettings { grid_maps: GridMaps::Active, ..settings.clone() };
        let inputs = render_inputs(&displays, &lone);
        assert_eq!(inputs.len(), 9);
        assert!(inputs.iter().all(|input| input.grid == Some(0) && input.label == "1"));

        // A cell gaining its first window redraws the maps.
        let key = MenuIconRenderKey::from_inputs(&render_inputs(&displays, &settings));
        let mut changed = displays.clone();
        changed[0].workspaces[5].window_count = 1;
        assert!(!key.matches_inputs(&render_inputs(&changed, &settings)));
    }
}
