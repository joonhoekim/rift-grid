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
//!
//! Each cell's layouts are kept too. When the first of its windows comes back,
//! they are put back with all its windows departed, so that each returns to
//! its place and size whatever order the apps are found in.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::LayoutEngine;
use crate::actor::app::WindowId;
use crate::common::collections::HashMap;
use crate::common::config::LayoutMode;
use crate::layout_engine::LayoutSystemKind;
use crate::layout_engine::workspaces::WorkspaceLayoutState;
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
    #[serde(default)]
    layouts: Vec<CellLayout>,
}

/// A cell's layouts: a `Layout` as JSON text, so that comparing what is
/// saved with what is there costs a string comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CellLayout {
    space: u64,
    index: usize,
    layout: String,
}

/// A workspace's layouts: its layout system and which of its layouts is
/// active, in the mode they were made for.
#[derive(Serialize)]
struct LayoutRef<'a> {
    mode: LayoutMode,
    system: &'a LayoutSystemKind,
    state: &'a WorkspaceLayoutState,
}

#[derive(Deserialize)]
struct Layout {
    mode: LayoutMode,
    system: LayoutSystemKind,
    state: WorkspaceLayoutState,
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
    /// Cells' layouts not put back yet, by Space and place.
    layouts: HashMap<(SpaceId, usize), String>,
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
        cells.layouts = saved
            .layouts
            .iter()
            .map(|cell| ((SpaceId::new(cell.space), cell.index), cell.layout.clone()))
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
        // The layouts of cells holding windows, and those not put back yet.
        let mut occupied: Vec<(SpaceId, usize)> = assigned.values().copied().collect();
        occupied.sort_unstable();
        occupied.dedup();
        let mut layouts: Vec<CellLayout> = occupied
            .iter()
            .filter_map(|&(space, index)| {
                let workspace = &workspaces[*workspaces.workspace_ids(space).get(index)?];
                if matches!(workspace.layout_system, LayoutSystemKind::Floating(_)) {
                    return None;
                }
                let layout = serde_json::to_string(&LayoutRef {
                    mode: workspace.layout_mode,
                    system: &workspace.layout_system,
                    state: &workspace.layout_state,
                })
                .ok()?;
                Some(CellLayout { space: space.get(), index, layout })
            })
            .collect();
        layouts.extend(
            cells
                .layouts
                .iter()
                .filter(|(cell, _)| occupied.binary_search(cell).is_err())
                .map(|(&(space, index), layout)| CellLayout {
                    space: space.get(),
                    index,
                    layout: layout.clone(),
                }),
        );
        layouts.sort_unstable_by_key(|cell| (cell.space, cell.index));
        let saved = Saved { windows: entries, shown, layouts };
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
            self.put_back_layouts(space);
        }
    }

    /// Put back the kept layouts of `space`'s cells, with their remembered
    /// windows departed (`install_departed_layouts`), once its workspaces are
    /// made: when the Space is exposed, before any window comes, or when a
    /// window remembered there comes first. A layout made for another mode
    /// than its cell's, or that does not read, is dropped.
    pub(super) fn put_back_layouts(&mut self, space: SpaceId) {
        let cells: Vec<usize> = self
            .departures
            .cells
            .layouts
            .keys()
            .filter(|(was, _)| *was == space)
            .map(|&(_, index)| index)
            .collect();
        for index in cells {
            let Some(layout) = self.departures.cells.layouts.remove(&(space, index)) else {
                continue;
            };
            let Some(&workspace) = self.workspaces.workspace_ids(space).get(index) else {
                continue;
            };
            let layout = match serde_json::from_str::<Layout>(&layout) {
                Ok(layout) => layout,
                Err(error) => {
                    tracing::warn!(%error, ?space, index, "Unreadable cell layout; tiling anew");
                    continue;
                }
            };
            if self.workspaces[workspace].layout_mode != layout.mode {
                continue;
            }
            let away: Vec<WindowId> = self
                .departures
                .cells
                .remembered
                .iter()
                .filter(|&(_, &cell)| cell == (space, index))
                .map(|(&wid, _)| wid)
                .collect();
            self.install_departed_layouts(workspace, layout.system, layout.state, &away);
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
    use crate::layout_engine::LayoutSystem;
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

    const SCREEN: objc2_core_foundation::CGRect = objc2_core_foundation::CGRect {
        origin: objc2_core_foundation::CGPoint { x: 0.0, y: 0.0 },
        size: CGSize { width: 1200.0, height: 800.0 },
    };

    fn bsp_engine(space: SpaceId) -> (LayoutEngine, WindowStore) {
        let settings = VirtualWorkspaceSettings { grid_columns: 3, ..Default::default() };
        let layout = LayoutSettings { mode: crate::common::config::LayoutMode::Bsp, ..Default::default() };
        let mut engine = LayoutEngine::new(&settings, &layout, None);
        let mut store = WindowStore::default();
        let _ = engine.handle_event(&mut store, LayoutEvent::SpaceExposed(space, SCREEN.size));
        (engine, store)
    }

    fn frames(engine: &mut LayoutEngine, space: SpaceId) -> Vec<(WindowId, objc2_core_foundation::CGRect)> {
        let gaps = LayoutSettings::default().gaps;
        let mut frames = engine.calculate_layout(space, SCREEN, &gaps, Default::default());
        frames.sort_by_key(|(window, _)| (window.pid, window.idx));
        frames
    }

    /// A rift restarted by an install tiled each cell's windows again in the
    /// order it found them, at even splits, though they came back to the
    /// right cells.
    #[test]
    fn a_restarted_rift_arranges_a_cell_as_it_was() {
        let space = SpaceId::new(7);
        let (a, b, c) = (WindowId::new(10, 1), WindowId::new(10, 2), WindowId::new(11, 3));
        let path = std::env::temp_dir().join(format!("rift-grid-cells-l-{}.json", std::process::id()));

        let (mut before, mut store) = bsp_engine(space);
        observe(&mut before, &mut store, space, &[a, b, c]);
        let _ = before.handle_event(&mut store, LayoutEvent::WindowFocused(space, a));
        let _ = before.handle_command(
            &mut store,
            Some(space),
            &[space],
            &HashMap::default(),
            crate::layout_engine::LayoutCommand::ResizeWindowBy { amount: 0.2 },
        );
        let arranged = frames(&mut before, space);
        write_atomically(&path, &before.cells_changed(&store).unwrap()).unwrap();

        // The apps are found in another order, each app's windows together.
        let (mut after, mut store) = bsp_engine(space);
        after.load_cells_from(&path);
        observe(&mut after, &mut store, space, &[c]);
        observe(&mut after, &mut store, space, &[b, a]);
        assert_eq!(frames(&mut after, space), arranged);
        let _ = std::fs::remove_file(&path);
    }

    /// A window rift does not remember (its app was not attached when the
    /// file was written) comes into a cell first. The kept layouts no longer
    /// match, so the remembered windows go in as departures do when their
    /// workspace changed: after the nearest old neighbor already back, which
    /// keeps the last two in their order here, though not the first.
    #[test]
    fn a_window_it_does_not_remember_coming_first_falls_back_to_neighbors() {
        let space = SpaceId::new(7);
        let (a, b, c) = (WindowId::new(10, 1), WindowId::new(10, 2), WindowId::new(11, 3));
        let stranger = WindowId::new(12, 4);
        let path = std::env::temp_dir().join(format!("rift-grid-cells-s-{}.json", std::process::id()));

        let (mut before, mut store) = bsp_engine(space);
        observe(&mut before, &mut store, space, &[a, b, c]);
        let order = |engine: &LayoutEngine| {
            let (workspace, layout) = engine.workspaces.active_layout_for_space(space).unwrap();
            engine.workspaces[workspace].layout_system.all_windows_in_layout(layout)
        };
        let arranged = order(&before);
        write_atomically(&path, &before.cells_changed(&store).unwrap()).unwrap();

        let (mut after, mut store) = bsp_engine(space);
        after.load_cells_from(&path);
        let _ = after.handle_event(&mut store, LayoutEvent::SpaceExposed(space, SCREEN.size));
        observe(&mut after, &mut store, space, &[stranger]);
        observe(&mut after, &mut store, space, &[c]);
        observe(&mut after, &mut store, space, &[b, a]);
        assert_eq!(arranged, vec![a, c, b]);
        let kept: Vec<_> = order(&after).into_iter().filter(|&w| w != stranger).collect();
        let position = |w| kept.iter().position(|&k| k == w).unwrap();
        assert!(position(a) < position(b), "a, back after c, stays before b");
        assert!(order(&after).contains(&stranger) && kept.len() == 3, "all tiled");
        let _ = std::fs::remove_file(&path);
    }

    /// The layouts are put back only into the mode they were made in.
    #[test]
    fn a_layout_from_another_mode_is_not_put_back() {
        let space = SpaceId::new(7);
        let (a, b) = (WindowId::new(10, 1), WindowId::new(10, 2));
        let path = std::env::temp_dir().join(format!("rift-grid-cells-m-{}.json", std::process::id()));
        let (mut before, mut store) = bsp_engine(space);
        observe(&mut before, &mut store, space, &[a, b]);
        write_atomically(&path, &before.cells_changed(&store).unwrap()).unwrap();

        // Restarted with another default mode.
        let (mut after, mut store) = engine(space);
        after.load_cells_from(&path);
        let _ = after.handle_event(&mut store, LayoutEvent::SpaceExposed(space, SCREEN.size));
        observe(&mut after, &mut store, space, &[b, a]);
        assert_eq!(frames(&mut after, space).len(), 2, "both tiled anew");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_window_seen_before_its_space_is_shown_goes_back_too() {
        let path = std::env::temp_dir().join(format!("rift-grid-cells-u-{}.json", std::process::id()));
        let a = WindowId::new(10, 1);
        let saved = Saved { windows: vec![Entry { pid: 10, window: 1, space: 9, index: 12 }], shown: Vec::new(), layouts: Vec::new() };
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
            layouts: Vec::new(),
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
