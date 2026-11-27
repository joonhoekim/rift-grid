//! Workspaces as a grid of cells, read row by row from the top left. Where a
//! direction leads comes from `common::grid_nav`.

use super::{Direction, SpaceId, VirtualWorkspaceId, WorkspaceStore};
pub use crate::common::config::GRID_COUNT;
use crate::common::config::{VirtualWorkspaceSettings, WorkspaceMode};
use crate::common::grid_nav;

/// The settings the store works with. When workspaces form grids, each display
/// gets every cell of every grid, named by grid and cell: "2·5", or "2·Mail"
/// when grid 2 names its cell 5. A display starts on grid 1's center.
pub(super) fn effective_settings(config: &VirtualWorkspaceSettings) -> VirtualWorkspaceSettings {
    let mut settings = config.clone();
    let Some(grid) = &config.grid_layout else {
        return settings;
    };
    let (rows, columns) = (grid.rows, grid.columns);
    // Grids are a display's own; `grid_displays.rs` gives each grid one display.
    settings.mode = WorkspaceMode::PerDisplay;
    let cells = rows * columns;
    settings.default_workspace_count = GRID_COUNT * cells;
    settings.workspace_names = (0..settings.default_workspace_count)
        .map(|index| format!("{}·{}", index / cells + 1, grid.cell_name(index / cells, index % cells)))
        .collect();
    settings.default_workspace = cells / 2;
    settings
}

/// The most workspaces a display holds: upstream's limit, or every cell of
/// every grid when that is more.
pub(super) fn workspace_cap(settings: &VirtualWorkspaceSettings) -> usize {
    let cells = settings.grid_shape().map_or(0, |(rows, columns)| GRID_COUNT * rows * columns);
    crate::common::config::MAX_WORKSPACES.max(cells)
}

pub(crate) fn grid_dir(dir: Direction) -> grid_nav::Dir {
    match dir {
        Direction::Left => grid_nav::Dir::Left,
        Direction::Right => grid_nav::Dir::Right,
        Direction::Up => grid_nav::Dir::Up,
        Direction::Down => grid_nav::Dir::Down,
    }
}

/// `count` cells read row by row into `columns` columns; the last row may be short.
pub(crate) fn grid_neighbor_index(
    index: usize,
    count: usize,
    columns: usize,
    dir: Direction,
) -> Option<usize> {
    if columns == 0 || index >= count {
        return None;
    }
    grid_nav::Grid::new(count.div_ceil(columns), columns)
        .towards(index, grid_dir(dir))
        .filter(|&target| target < count)
}

impl WorkspaceStore {
    pub fn grid_enabled(&self) -> bool { self.settings.grid_layout.is_some() }

    /// Whether a display starts on the cell it showed when rift-grid last ran.
    pub(crate) fn grid_starts_last(&self) -> bool {
        self.settings
            .grid_layout
            .as_ref()
            .is_none_or(|grid| grid.start == crate::common::config::GridStart::Last)
    }

    /// The grids' rows and columns; (0, 0) without a grid.
    pub(crate) fn grid_shape(&self) -> (usize, usize) { self.settings.grid_shape().unwrap_or_default() }

    /// The grid's shape can change only when rift-grid starts: the workspaces
    /// a display has, and what each one's place means, follow from it.
    pub(super) fn check_grid_shape(
        &self,
        config: &VirtualWorkspaceSettings,
        restoring: bool,
    ) -> Result<(), String> {
        let started = !self.initialized_spaces().is_empty();
        if restoring {
            let Some((rows, columns)) = config.grid_shape() else { return Ok(()) };
            let cells = GRID_COUNT * rows * columns;
            if let Some(space) =
                self.initialized_spaces().into_iter().find(|&s| self.workspace_ids(s).len() != cells)
            {
                return Err(format!(
                    "the saved layout has {} workspaces on space {}, not the {cells} of nine \
                     {rows}x{columns} grids: the grid's shape changed since it was saved",
                    self.workspace_ids(space).len(),
                    space.get()
                ));
            }
        } else if started && config.grid_shape() != self.settings.grid_shape() {
            return Err("[settings.grid] rows and columns take effect when rift-grid starts again"
                .into());
        }
        Ok(())
    }

    fn cells_per_grid(&self) -> usize {
        let (rows, columns) = self.grid_shape();
        rows * columns
    }

    /// The grid (0-based) the workspace at `index` is a cell of.
    pub fn grid_of(&self, index: usize) -> Option<usize> {
        self.grid_enabled()
            .then(|| index / self.cells_per_grid())
            .filter(|&grid| grid < GRID_COUNT)
    }

    /// Row and column of the workspace at `index` within its grid.
    pub fn grid_cell(&self, index: usize) -> Option<(usize, usize)> {
        self.grid_of(index).map(|_| {
            let cell = index % self.cells_per_grid();
            let columns = self.grid_shape().1;
            (cell / columns, cell % columns)
        })
    }

    /// Where an app rule puts a window: its `grid`'s current cell, else its
    /// `workspace`.
    pub(super) fn rule_workspace<'a>(
        &self,
        context: impl Into<super::WorkspaceContext<'a>>,
        selector: Option<&crate::common::config::WorkspaceSelector>,
        grid: Option<usize>,
    ) -> Option<VirtualWorkspaceId> {
        let context = context.into();
        grid.and_then(|grid| self.grid_current_cell(context.space, grid.checked_sub(1)?))
            .or_else(|| {
                selector
                    .and_then(|selector| self.resolve_workspace(context, selector))
                    .map(|target| target.id)
            })
    }

    pub(super) fn remember_grid_cell(&mut self, space: SpaceId, workspace: VirtualWorkspaceId) {
        if let Some(grid) = self.grid_of_workspace(space, workspace) {
            self.grid_current.insert((space, grid), workspace);
        }
    }

    /// Grid `grid`'s (0-based) current cell on `space`: the one it was last on, its
    /// center until then.
    pub fn grid_current_cell(&self, space: SpaceId, grid: usize) -> Option<VirtualWorkspaceId> {
        if !self.grid_enabled() || grid >= GRID_COUNT {
            return None;
        }
        self.grid_current
            .get(&(space, grid))
            .copied()
            .filter(|&id| self.grid_of_workspace(space, id) == Some(grid))
            .or_else(|| self.grid_center_cell(space, grid))
    }

    /// Whether a cell of grid `grid` (0-based) on `space` holds a window.
    pub fn grid_has_windows(
        &self,
        window_store: &crate::model::WindowStore,
        space: SpaceId,
        grid: usize,
    ) -> bool {
        let cells = self.cells_per_grid();
        self.grid_enabled()
            && self
                .workspace_ids(space)
                .iter()
                .skip(grid * cells)
                .take(cells)
                .any(|&id| window_store.workspace_window_count(id) > 0)
    }

    /// Hand grid `grid` (0-based) from `source` to `target`: each cell's layouts,
    /// last focus and windows go to the same cell there, and the grid's current
    /// cell with them. The target's cells, empty, come back in their place.
    /// Returns the windows moved and each cell as (source, target), or `None`
    /// when the target's cells of the grid hold windows.
    pub(crate) fn move_grid(
        &mut self,
        window_store: &mut crate::model::WindowStore,
        source: SpaceId,
        target: SpaceId,
        grid: usize,
    ) -> Option<(
        Vec<crate::actor::app::WindowId>,
        Vec<(VirtualWorkspaceId, VirtualWorkspaceId)>,
    )> {
        if !self.grid_enabled() || grid >= GRID_COUNT || source == target {
            return None;
        }
        self.ensure_space_initialized(source);
        self.ensure_space_initialized(target);
        if self.grid_has_windows(window_store, target, grid) {
            return None;
        }
        let cells = self.cells_per_grid();
        let current = self
            .active_workspace(source)
            .filter(|&id| self.grid_of_workspace(source, id) == Some(grid))
            .or_else(|| self.grid_current_cell(source, grid));
        let mut moved = Vec::new();
        let mut pairs = Vec::new();
        for index in grid * cells..(grid + 1) * cells {
            let (Some(&from), Some(&to)) = (
                self.workspace_ids(source).get(index),
                self.workspace_ids(target).get(index),
            ) else {
                continue;
            };
            let [a, b] = self.workspaces.get_disjoint_mut([from, to])?;
            std::mem::swap(&mut a.layout_system, &mut b.layout_system);
            std::mem::swap(&mut a.layout_mode, &mut b.layout_mode);
            std::mem::swap(&mut a.layout_state, &mut b.layout_state);
            std::mem::swap(&mut a.last_focused, &mut b.last_focused);
            pairs.push((from, to));
            for wid in window_store.workspace_windows(from) {
                window_store.assign_window_to_workspace(wid, to);
                moved.push(wid);
            }
            if current == Some(from) {
                self.grid_current.insert((target, grid), to);
            }
        }
        self.grid_current.remove(&(source, grid));
        Some((moved, pairs))
    }

    fn grid_of_workspace(&self, space: SpaceId, workspace: VirtualWorkspaceId) -> Option<usize> {
        let index = self.workspace_ids(space).iter().position(|&id| id == workspace)?;
        self.grid_of(index)
    }

    /// Move each grid's current cell from `old_space` to `new_space`.
    pub(super) fn remap_grid_cells(&mut self, old_space: SpaceId, new_space: SpaceId) {
        self.grid_current.retain(|&(space, _), _| space != new_space);
        let moved: Vec<_> = self
            .grid_current
            .extract_if(|&(space, _), _| space == old_space)
            .map(|((_, grid), id)| ((new_space, grid), id))
            .collect();
        self.grid_current.extend(moved);
    }

    /// Grid `grid`'s (0-based) center cell on `space`.
    pub fn grid_center_cell(&self, space: SpaceId, grid: usize) -> Option<VirtualWorkspaceId> {
        if !self.grid_enabled() || grid >= GRID_COUNT {
            return None;
        }
        let cells = self.cells_per_grid();
        self.workspace_ids(space).get(grid * cells + cells / 2).copied()
    }

    /// The cell next to `current` in its grid. Grids do not wrap, and cells never
    /// lead into another grid.
    pub fn grid_neighbor(
        &self,
        space: SpaceId,
        current: VirtualWorkspaceId,
        dir: Direction,
    ) -> Option<VirtualWorkspaceId> {
        let index = self.workspace_ids(space).iter().position(|&id| id == current)?;
        let cells = self.cells_per_grid();
        let start = self.grid_of(index)? * cells;
        let count = cells.min(self.workspace_ids(space).len().saturating_sub(start));
        grid_neighbor_index(index - start, count, self.grid_shape().1, dir)
            .map(|cell| self.workspace_ids(space)[start + cell])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_neighbors_stop_at_edges() {
        use Direction::*;
        // 0 1 2
        // 3 4 5
        // 6 7 8
        let n = |index, dir| grid_neighbor_index(index, 9, 3, dir);
        assert_eq!([n(4, Left), n(4, Right), n(4, Up), n(4, Down)], [
            Some(3),
            Some(5),
            Some(1),
            Some(7)
        ]);
        assert_eq!([n(0, Left), n(0, Up), n(2, Right), n(8, Down)], [None; 4]);
        assert_eq!(n(3, Left), None, "rows do not wrap into each other");
        assert_eq!(n(5, Right), None);
        assert_eq!(grid_neighbor_index(7, 8, 3, Right), None, "short last row");
        assert_eq!(grid_neighbor_index(5, 8, 3, Down), None);
        assert_eq!(n(4, Right).and(grid_neighbor_index(4, 9, 0, Right)), None, "off");
    }

    #[test]
    fn a_grid_of_three_rows_and_five_columns() {
        use crate::common::config::LayoutSettings;
        let settings = VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 5)),
            ..Default::default()
        };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let space = SpaceId::new(7);
        store.ensure_space_initialized(space);
        let ids = store.workspace_ids(space).to_vec();
        assert_eq!(ids.len(), 9 * 15, "nine grids of fifteen cells");
        assert_eq!((store.grid_of(14), store.grid_of(15)), (Some(0), Some(1)));
        assert_eq!(store.grid_cell(7), Some((1, 2)), "row by row, five a row");
        assert_eq!(store.grid_cell(15 + 9), Some((1, 4)));
        // 0  1  2  3  4
        // 5  6  7  8  9
        // 10 11 12 13 14
        assert_eq!(store.grid_center_cell(space, 0), Some(ids[7]));
        assert_eq!(store.grid_center_cell(space, 1), Some(ids[22]));
        let next = |index: usize, dir| {
            store.grid_neighbor(space, ids[index], dir).map(|id| ids.iter().position(|&i| i == id).unwrap())
        };
        assert_eq!(next(7, Direction::Right), Some(8));
        assert_eq!(next(7, Direction::Down), Some(12));
        assert_eq!(next(9, Direction::Right), None, "the row's end");
        assert_eq!(next(14, Direction::Right), None, "not into grid 2");
        assert_eq!(next(15, Direction::Up), None);
        assert_eq!(store.list_workspaces(space)[22].1, "2·8");

        // The largest grids: nine of 9x9 on a display.
        let settings = VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(9, 9)),
            ..Default::default()
        };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        store.ensure_space_initialized(space);
        assert_eq!(store.workspace_ids(space).len(), 729);
        assert_eq!(store.grid_center_cell(space, 8), Some(store.workspace_ids(space)[8 * 81 + 40]));
    }

    #[test]
    fn cells_take_their_grids_names_and_displays_start_on_the_default_cell() {
        let config = crate::common::config::Config::parse(
            "[settings.grid]\nrows = 3\ncolumns = 3\nnames = [\"Mail\", \"\", \"Web\"]\n\
             names_3 = [\"a\"]\n[virtual_workspaces]\ndefault_workspace = 7\n[keys]\n",
        )
        .unwrap();
        let settings = effective_settings(&config.virtual_workspaces);
        let names = &settings.workspace_names;
        assert_eq!([&names[0], &names[1], &names[2], &names[3]], ["1·Mail", "1·2", "1·Web", "1·4"]);
        assert_eq!([&names[9], &names[18], &names[19]], ["2·1", "3·a", "3·2"]);
        assert_eq!(settings.default_workspace, 4, "grid 1's center, not upstream's setting");
    }

    #[test]
    fn the_grids_shape_changes_only_on_a_restart() {
        use crate::common::config::LayoutSettings;
        let three = VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut store = WorkspaceStore::new_with_config(&three, &LayoutSettings::default());
        let fresh = |shape: Option<(usize, usize)>| VirtualWorkspaceSettings {
            grid_layout: shape.map(|(rows, columns)| crate::common::config::GridLayout::new(rows, columns)),
            ..Default::default()
        };
        assert!(store.plan_settings_names(&fresh(Some((5, 5))), false).is_ok(), "nothing made yet");
        store.ensure_space_initialized(SpaceId::new(3));
        assert!(store.plan_settings_names(&three, false).is_ok(), "the same shape");
        let Err(error) = store.plan_settings_names(&fresh(Some((1, 3))), false) else { panic!() };
        assert!(error.contains("starts again"), "{error}");
        let Err(error) = store.plan_settings_names(&fresh(Some((5, 3))), true) else { panic!() };
        assert!(error.contains("shape changed"), "{error}");
    }

    #[test]
    fn an_app_rule_grid_is_that_grids_current_cell() {
        use crate::common::config::{LayoutSettings, WorkspaceSelector};
        let settings = VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let space = SpaceId::new(5);
        store.ensure_space_initialized(space);
        let ids = store.workspace_ids(space).to_vec();
        let resolve = |store: &WorkspaceStore, selector: Option<&WorkspaceSelector>, grid| {
            store.rule_workspace(space, selector, grid).unwrap()
        };
        assert_eq!(
            resolve(&store, None, Some(2)),
            ids[13],
            "grid 2's center at first"
        );
        store.set_active_workspace(space, ids[10]);
        assert_eq!(
            resolve(&store, None, Some(2)),
            ids[10],
            "then where grid 2 was left"
        );
        let cell = WorkspaceSelector::Index(0);
        assert_eq!(
            resolve(&store, Some(&cell), Some(2)),
            ids[10],
            "grid wins over workspace"
        );
        assert_eq!(resolve(&store, Some(&cell), None), ids[0]);
    }

    #[test]
    fn a_grids_current_cell_follows_its_workspaces_to_a_new_space() {
        use crate::common::config::LayoutSettings;
        use crate::model::WindowStore;
        let settings = VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let (old, new) = (SpaceId::new(5), SpaceId::new(6));
        store.ensure_space_initialized(old);
        store.ensure_space_initialized(new);
        let ids = store.workspace_ids(old).to_vec();
        store.set_active_workspace(old, ids[10]);
        store.remap_space(&mut WindowStore::default(), old, new);
        assert_eq!(
            store.grid_current_cell(new, 1),
            Some(ids[10]),
            "not new's own center"
        );
        assert_eq!(store.grid_current_cell(new, 0), Some(ids[4]));
    }

    #[test]
    fn a_remembered_cell_outside_its_grid_is_not_used() {
        use crate::common::config::LayoutSettings;
        let settings = VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let space = SpaceId::new(5);
        store.ensure_space_initialized(space);
        let ids = store.workspace_ids(space).to_vec();
        store.grid_current.insert((space, 1), ids[2]);
        assert_eq!(store.grid_current_cell(space, 1), Some(ids[13]));
    }
}
