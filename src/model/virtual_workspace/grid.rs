//! Workspaces as a grid of cells, read row by row from the top left. Where a
//! direction leads comes from subline-grid.

use super::{Direction, SpaceId, VirtualWorkspaceId, WorkspaceStore};

pub(crate) fn grid_dir(dir: Direction) -> subline_grid::Dir {
    match dir {
        Direction::Left => subline_grid::Dir::Left,
        Direction::Right => subline_grid::Dir::Right,
        Direction::Up => subline_grid::Dir::Up,
        Direction::Down => subline_grid::Dir::Down,
    }
}

/// `count` workspaces read row by row into `columns` columns; the last row may be short.
pub(crate) fn grid_neighbor_index(
    index: usize,
    count: usize,
    columns: usize,
    dir: Direction,
) -> Option<usize> {
    if columns == 0 || index >= count {
        return None;
    }
    subline_grid::Grid::new(count.div_ceil(columns), columns)
        .towards(index, grid_dir(dir))
        .filter(|&target| target < count)
}

impl WorkspaceStore {
    pub fn grid_enabled(&self) -> bool { self.grid_columns > 0 }

    /// Row and column of the workspace at `index` when workspaces form a grid.
    pub fn grid_cell(&self, index: usize) -> Option<(usize, usize)> {
        self.grid_enabled().then(|| (index / self.grid_columns, index % self.grid_columns))
    }

    /// The workspace next to `current` in the grid, read row by row from the top
    /// left. The grid does not wrap: there is no neighbor past an edge.
    pub fn grid_neighbor(
        &self,
        space: SpaceId,
        current: VirtualWorkspaceId,
        dir: Direction,
    ) -> Option<VirtualWorkspaceId> {
        grid_neighbor_index(
            self.workspace_ids(space).iter().position(|&id| id == current)?,
            self.workspace_ids(space).len(),
            self.grid_columns,
            dir,
        )
        .map(|index| self.workspace_ids(space)[index])
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
}
