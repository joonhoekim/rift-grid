//! Workspaces as a grid of cells, read row by row from the top left. Where a
//! direction leads comes from `common::grid_nav`.

use super::{Direction, SpaceId, VirtualWorkspaceId, WorkspaceStore};
use crate::common::config::VirtualWorkspaceSettings;
use crate::common::grid_nav;

/// Grids per display, one per number key.
pub const GRID_COUNT: usize = 9;

/// How many workspaces each display gets: every cell of every grid when
/// workspaces form grids, as the config says otherwise.
pub(super) fn workspace_count(config: &VirtualWorkspaceSettings) -> usize {
    match config.grid_columns {
        0 => config.default_workspace_count,
        columns => GRID_COUNT * columns * columns,
    }
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
    pub fn grid_enabled(&self) -> bool {
        self.grid_columns > 0
    }

    fn cells_per_grid(&self) -> usize {
        self.grid_columns * self.grid_columns
    }

    /// The grid (0-based) the workspace at `index` is a cell of.
    pub fn grid_of(&self, index: usize) -> Option<usize> {
        self.grid_enabled().then(|| index / self.cells_per_grid())
    }

    /// Row and column of the workspace at `index` within its grid.
    pub fn grid_cell(&self, index: usize) -> Option<(usize, usize)> {
        self.grid_enabled().then(|| {
            let cell = index % self.cells_per_grid();
            (cell / self.grid_columns, cell % self.grid_columns)
        })
    }

    /// A workspace's name from the config. In a grid, cell names repeat in every
    /// grid after its number: "2·5", or "2·Mail" with a name for cell 5.
    pub(super) fn configured_name(&self, index: usize) -> Option<String> {
        let Some(grid) = self.grid_of(index) else {
            return self.default_workspace_names.get(index).cloned();
        };
        let cell = index % self.cells_per_grid();
        let name = self.default_workspace_names.get(cell).cloned();
        Some(format!(
            "{}·{}",
            grid + 1,
            name.unwrap_or_else(|| (cell + 1).to_string())
        ))
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
        grid_neighbor_index(index - start, count, self.grid_columns, dir)
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
        assert_eq!(
            [n(4, Left), n(4, Right), n(4, Up), n(4, Down)],
            [Some(3), Some(5), Some(1), Some(7)]
        );
        assert_eq!([n(0, Left), n(0, Up), n(2, Right), n(8, Down)], [None; 4]);
        assert_eq!(n(3, Left), None, "rows do not wrap into each other");
        assert_eq!(n(5, Right), None);
        assert_eq!(grid_neighbor_index(7, 8, 3, Right), None, "short last row");
        assert_eq!(grid_neighbor_index(5, 8, 3, Down), None);
        assert_eq!(n(4, Right).and(grid_neighbor_index(4, 9, 0, Right)), None, "off");
    }

    #[test]
    fn an_app_rule_grid_is_that_grids_current_cell() {
        use crate::common::config::{LayoutSettings, WorkspaceSelector};
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let space = SpaceId::new(5);
        store.ensure_space_initialized(space);
        let ids = store.workspace_ids(space).to_vec();
        let resolve = |store: &mut WorkspaceStore, selector: Option<&WorkspaceSelector>, grid| {
            store.resolve_rule_workspace_with_policy(space, selector, grid, None, false).unwrap()
        };
        assert_eq!(resolve(&mut store, None, Some(2)), ids[13], "grid 2's center at first");
        store.set_active_workspace(space, ids[10]);
        assert_eq!(resolve(&mut store, None, Some(2)), ids[10], "then where grid 2 was left");
        let cell = WorkspaceSelector::Index(0);
        assert_eq!(resolve(&mut store, Some(&cell), Some(2)), ids[10], "grid wins over workspace");
        assert_eq!(resolve(&mut store, Some(&cell), None), ids[0]);
    }

    #[test]
    fn a_grids_current_cell_follows_its_workspaces_to_a_new_space() {
        use crate::common::config::LayoutSettings;
        use crate::model::WindowStore;
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let (old, new) = (SpaceId::new(5), SpaceId::new(6));
        store.ensure_space_initialized(old);
        store.ensure_space_initialized(new);
        let ids = store.workspace_ids(old).to_vec();
        store.set_active_workspace(old, ids[10]);
        store.remap_space(&mut WindowStore::default(), old, new);
        assert_eq!(store.grid_current_cell(new, 1), Some(ids[10]), "not new's own center");
        assert_eq!(store.grid_current_cell(new, 0), Some(ids[4]));
    }

    #[test]
    fn a_remembered_cell_outside_its_grid_is_not_used() {
        use crate::common::config::LayoutSettings;
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut store = WorkspaceStore::new_with_config(&settings, &LayoutSettings::default());
        let space = SpaceId::new(5);
        store.ensure_space_initialized(space);
        let ids = store.workspace_ids(space).to_vec();
        store.grid_current.insert((space, 1), ids[2]);
        assert_eq!(store.grid_current_cell(space, 1), Some(ids[13]));
    }
}
