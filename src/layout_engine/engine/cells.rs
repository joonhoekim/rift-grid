//! Which cell each window is in, kept on disk while rift runs, so that a
//! restarted rift puts windows back in the cells they were in.
//!
//! Without it, rift starts with no assignments: windows parked in other cells
//! are put back on screen when rift exits, and the next rift takes every window
//! it sees into its display's active cell. The file is written whenever an
//! assignment changes, so a crash leaves it as current as a clean exit does.
//!
//! A window is known by its process and window number, and a cell by its macOS
//! Space and its place among the Space's workspaces. Both hold for as long as
//! the login session: after a reboot nothing matches, and windows go where a
//! new window would, as before. A remembered cell is used as a departure is
//! (`departures.rs`), for a window that comes with no assignment.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::LayoutEngine;
use crate::actor::app::WindowId;
use crate::common::collections::HashMap;
use crate::model::WindowStore;
use crate::model::virtual_workspace::VirtualWorkspaceId;
use crate::sys::screen::SpaceId;

/// What the file holds: where each window is, and which workspace each
/// Space shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Saved {
    windows: Vec<Entry>,
    #[serde(default)]
    shown: Vec<Shown>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Shown {
    space: u64,
    index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Entry {
    pid: i32,
    window: u32,
    space: u64,
    /// The workspace's place among its Space's workspaces.
    index: usize,
}

#[derive(Debug, Default)]
pub(super) struct Cells {
    /// Windows not seen yet since rift started, and where they were.
    remembered: HashMap<WindowId, (SpaceId, usize)>,
    /// What the file holds.
    saved: Saved,
}

impl Cells {
    pub(super) fn forget(&mut self, wid: WindowId) { self.remembered.remove(&wid); }
}

fn cells_file() -> PathBuf { crate::common::config::data_dir().join("cells.json") }

fn window_id(pid: i32, window: u32) -> Option<WindowId> {
    (pid > 0 && window > 0).then(|| WindowId::new(pid, window))
}

impl LayoutEngine {
    /// Read where windows were when rift last ran.
    pub fn load_cells(&mut self) { self.load_cells_from(&cells_file()); }

    fn load_cells_from(&mut self, path: &std::path::Path) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let Ok(saved) = serde_json::from_str::<Saved>(&text) else {
            tracing::warn!(?path, "Unreadable cells file; windows start in the active cell");
            return;
        };
        self.workspaces.shown_before =
            saved.shown.iter().map(|s| (SpaceId::new(s.space), s.index)).collect();
        let cells = &mut self.departures.cells;
        cells.remembered = saved
            .windows
            .iter()
            .filter_map(|e| Some((window_id(e.pid, e.window)?, (SpaceId::new(e.space), e.index))))
            .collect();
        cells.saved = saved;
    }

    /// Write where windows are now, when it changed.
    pub fn save_cells(&mut self, window_store: &WindowStore) {
        // Tests run the reactor too, and must not write the user's file.
        if cfg!(test) {
            return;
        }
        if let Some(saved) = self.cells_changed(window_store)
            && let Err(error) = write_atomically(&cells_file(), &saved)
        {
            tracing::warn!(%error, "Could not save the windows' cells");
        }
    }

    /// What to save when it differs from the file, noting it saved.
    fn cells_changed(&mut self, window_store: &WindowStore) -> Option<Saved> {
        let workspaces = &self.workspaces;
        let place = |workspace: VirtualWorkspaceId| {
            let space = workspaces.workspace_space(workspace)?;
            let index = workspaces.workspace_ids(space).iter().position(|&id| id == workspace)?;
            Some((space, index))
        };
        let assigned: HashMap<WindowId, (SpaceId, usize)> = window_store
            .iter_workspace_assignments()
            .filter_map(|(wid, workspace)| Some((wid, place(workspace)?)))
            .collect();
        let cells = &mut self.departures.cells;
        // A window placed since is no longer waiting for its cell.
        cells.remembered.retain(|wid, _| !assigned.contains_key(wid));
        let mut entries: Vec<Entry> = assigned
            .iter()
            .chain(&cells.remembered)
            .map(|(wid, (space, index))| Entry {
                pid: wid.pid,
                window: wid.idx.get(),
                space: space.get(),
                index: *index,
            })
            .collect();
        entries.sort_unstable();
        // Spaces not made yet keep what they showed before.
        let mut shown: Vec<Shown> = workspaces
            .initialized_spaces()
            .into_iter()
            .filter_map(|space| {
                let active = workspaces.active_workspace(space)?;
                Some(Shown { space: space.get(), index: place(active)?.1 })
            })
            .chain(workspaces.shown_before.iter().map(|(space, &index)| Shown {
                space: space.get(),
                index,
            }))
            .collect();
        shown.sort_unstable();
        shown.dedup_by_key(|s| s.space);
        let saved = Saved { windows: entries, shown };
        if saved == cells.saved {
            return None;
        }
        cells.saved = saved.clone();
        Some(saved)
    }

    /// Make `space`'s workspaces when `wid` is remembered there, so that its
    /// cell can be found before the Space has been shown.
    pub(super) fn ready_remembered_cell(&mut self, wid: WindowId, space: SpaceId) {
        if self.departures.cells.remembered.get(&wid).is_some_and(|&(was, _)| was == space) {
            self.workspaces.ensure_space_initialized(space);
        }
    }

    /// The workspace `wid` was in on `space` when rift last ran.
    pub(super) fn remembered_cell(
        &self,
        wid: WindowId,
        space: SpaceId,
    ) -> Option<VirtualWorkspaceId> {
        let &(was, index) = self.departures.cells.remembered.get(&wid)?;
        (was == space).then(|| self.workspaces.workspace_ids(space).get(index).copied()).flatten()
    }
}

fn write_atomically(path: &std::path::Path, saved: &Saved) -> std::io::Result<()> {
    let text = serde_json::to_string(saved).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.new");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::CGSize;

    use super::*;
    use crate::common::config::{LayoutSettings, VirtualWorkspaceSettings};
    use crate::layout_engine::LayoutEvent;
    use crate::layout_engine::engine::WindowLayoutInfo;

    fn engine(space: SpaceId) -> (LayoutEngine, WindowStore) {
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut engine = LayoutEngine::new(&settings, &LayoutSettings::default(), None);
        let mut store = WindowStore::default();
        let size = CGSize::new(1000.0, 800.0);
        let _ = engine.handle_event(&mut store, LayoutEvent::SpaceExposed(space, size));
        (engine, store)
    }

    fn observe(engine: &mut LayoutEngine, store: &mut WindowStore, space: SpaceId, w: &[WindowId]) {
        let info = w
            .iter()
            .map(|&window_id| WindowLayoutInfo {
                window_id,
                title: None,
                ax_role: None,
                ax_subrole: None,
                is_resizable: true,
                current_size: CGSize::new(0.0, 0.0),
                min_size: None,
                max_size: None,
            })
            .collect();
        let _ = engine.handle_event(store, LayoutEvent::windows_observed(space, 1, info, None));
    }

    #[test]
    fn a_restarted_rift_puts_windows_back_in_their_cells() {
        let space = SpaceId::new(7);
        let (a, b, c) = (WindowId::new(10, 1), WindowId::new(10, 2), WindowId::new(11, 3));
        let path = std::env::temp_dir().join(format!("rift-grid-cells-{}.json", std::process::id()));

        // The rift before: a in cell 6 of grid 1, b in cell 4 of grid 2.
        let (mut before, mut store) = engine(space);
        observe(&mut before, &mut store, space, &[a, b]);
        let ids = before.workspaces.workspace_ids(space);
        store.assign_window_to_workspace(a, ids[5]);
        store.assign_window_to_workspace(b, ids[12]);
        before.workspaces.set_active_workspace(space, ids[12]);
        let saved = before.cells_changed(&store).expect("first save");
        assert!(before.cells_changed(&store).is_none(), "unchanged, not written again");
        write_atomically(&path, &saved).unwrap();

        // The rift after: windows come with no assignment.
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut after = LayoutEngine::new(&settings, &LayoutSettings::default(), None);
        let mut store = WindowStore::default();
        after.load_cells_from(&path);
        let size = CGSize::new(1000.0, 800.0);
        let _ = after.handle_event(&mut store, LayoutEvent::SpaceExposed(space, size));
        observe(&mut after, &mut store, space, &[c, a, b]);
        let ids = after.workspaces.workspace_ids(space);
        assert_eq!(store.workspace_for_window(a), Some(ids[5]));
        assert_eq!(store.workspace_for_window(b), Some(ids[12]));
        let active = after.workspaces.active_workspace(space);
        assert_eq!(active, Some(ids[12]), "the Space shows what it showed");
        assert_eq!(store.workspace_for_window(c), active, "a new window, as before");
        assert!(after.departures.cells.remembered.is_empty(), "both placed");

        // Another Space, or a window it never saw, is left alone.
        let (mut other, mut store) = engine(SpaceId::new(8));
        other.load_cells_from(&path);
        observe(&mut other, &mut store, SpaceId::new(8), &[a]);
        assert_eq!(store.workspace_for_window(a), other.workspaces.active_workspace(SpaceId::new(8)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_window_seen_before_its_space_is_shown_goes_back_too() {
        let path = std::env::temp_dir().join(format!("rift-grid-cells-u-{}.json", std::process::id()));
        let a = WindowId::new(10, 1);
        let saved = Saved { windows: vec![Entry { pid: 10, window: 1, space: 9, index: 12 }], shown: Vec::new() };
        write_atomically(&path, &saved).unwrap();
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut engine = LayoutEngine::new(&settings, &LayoutSettings::default(), None);
        let mut store = WindowStore::default();
        engine.load_cells_from(&path);
        observe(&mut engine, &mut store, SpaceId::new(9), &[a]);
        let ids = engine.workspaces.workspace_ids(SpaceId::new(9));
        assert_eq!(store.workspace_for_window(a), Some(ids[12]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn windows_not_seen_yet_stay_in_the_file() {
        let space = SpaceId::new(7);
        let path = std::env::temp_dir().join(format!("rift-grid-cells-k-{}.json", std::process::id()));
        let saved = Saved {
            windows: vec![Entry { pid: 10, window: 1, space: 7, index: 5 }],
            shown: vec![Shown { space: 7, index: 13 }],
        };
        write_atomically(&path, &saved).unwrap();
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let mut engine = LayoutEngine::new(&settings, &LayoutSettings::default(), None);
        let mut store = WindowStore::default();
        engine.load_cells_from(&path);
        assert!(engine.cells_changed(&store).is_none(), "as read");
        let _ = engine.handle_event(&mut store, LayoutEvent::SpaceExposed(space, CGSize::new(1000.0, 800.0)));
        assert!(engine.cells_changed(&store).is_none(), "the Space shows what it showed");
        let _ = std::fs::remove_file(&path);
    }
}
