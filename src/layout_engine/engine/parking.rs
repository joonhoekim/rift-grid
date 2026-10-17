//! Windows away for a while: minimized, hidden with their app, or in native
//! fullscreen. Each keeps the workspace it left, and the workspace keeps its
//! layouts as they were before the first of them left, so they come back to
//! the same places at the same sizes rather than next to whatever is selected
//! by then.

use rift_protocol::ContainerTreeNode;

use super::LayoutEngine;
use crate::actor::app::{WindowId, pid_t};
use crate::common::collections::{HashMap, HashSet};
use crate::layout_engine::{LayoutId, LayoutSystem, LayoutSystemKind};
use crate::model::WindowStore;
use crate::model::virtual_workspace::{VirtualWorkspace, VirtualWorkspaceId};
use crate::sys::screen::SpaceId;

#[derive(Debug, Default)]
pub(super) struct Parking {
    windows: HashMap<WindowId, ParkedWindow>,
    /// A workspace's layouts from before its windows away left.
    layouts: HashMap<VirtualWorkspaceId, LayoutSystemKind>,
    departures: u64,
}

#[derive(Debug)]
struct ParkedWindow {
    space: SpaceId,
    workspace: VirtualWorkspaceId,
    /// The windows of the layout it left, in order, for when that layout has
    /// changed by the time it comes back.
    order: Vec<WindowId>,
    /// When it left, and whether its split was rebalanced then, as for a
    /// window the store no longer places: to take it out of the layouts from
    /// before the same way.
    departure: u64,
    rebalanced: bool,
}

impl LayoutEngine {
    /// Note where `wid` is and take it out of its workspace's layouts, for a
    /// while.
    pub(super) fn park_window(&mut self, window_store: &WindowStore, wid: WindowId) {
        // The workspace whose layouts hold it, or else the one it is assigned
        // to: a floating window has only that.
        let tiled = self.workspaces.workspaces.iter().find_map(|(id, workspace)| {
            let holds = |layout| workspace.layout_system.contains_window(layout, wid);
            workspace.layout_state.all_layouts().any(holds).then_some((workspace.space, id))
        });
        // Removed again on its way, it keeps the place noted first.
        if tiled.is_none() && self.parking.windows.contains_key(&wid) {
            return;
        }
        let Some((space, workspace_id)) = tiled.or_else(|| {
            let assigned = window_store.workspace_info_for_window(wid)?;
            Some((assigned.space, assigned.workspace_id))
        }) else {
            return;
        };
        let workspace = &mut self.workspaces[workspace_id];
        let order = workspace
            .layout_state
            .active()
            .filter(|&layout| workspace.layout_system.contains_window(layout, wid))
            .map(|layout| workspace.layout_system.all_windows_in_layout(layout))
            .unwrap_or_default();
        let rebalanced = window_store.workspaces_for_window(wid).is_empty();
        if tiled.is_some() {
            // Keep the layouts from before the first window away left, while
            // the workspace has changed only by windows leaving.
            let present = windows(workspace, &workspace.layout_system);
            let kept = self.parking.layouts.get(&workspace_id).is_some_and(|before| {
                trimmed(workspace, before, &present, None, &self.parking.windows)
                    .is_some_and(|before| same_layouts(workspace, &before))
            });
            if !kept && let Some(before) = workspace.layout_system.preview_clone() {
                self.parking.layouts.insert(workspace_id, before);
            }
            remove(&mut workspace.layout_system, wid, rebalanced);
        }
        self.parking.departures += 1;
        self.parking.windows.insert(wid, ParkedWindow {
            space,
            workspace: workspace_id,
            order,
            departure: self.parking.departures,
            rebalanced,
        });
    }

    /// The workspace `wid` left on `space`, when it is coming back to it.
    pub(super) fn parked_workspace(
        &self,
        wid: WindowId,
        space: SpaceId,
    ) -> Option<VirtualWorkspaceId> {
        let parked = self.parking.windows.get(&wid)?;
        (parked.space == space).then_some(parked.workspace)
    }

    /// Forget `wid` once it is back on `space`, the space it left: in
    /// fullscreen it is placed on a space of its own meanwhile.
    pub(super) fn forget_parked_window_on(&mut self, wid: WindowId, space: SpaceId) {
        if self.parking.windows.get(&wid).is_some_and(|parked| parked.space == space) {
            self.forget_parked_window(wid);
        }
    }

    pub(super) fn forget_parked_window(&mut self, wid: WindowId) {
        if let Some(parked) = self.parking.windows.remove(&wid) {
            self.forget_parked_layouts(parked.workspace);
        }
    }

    pub(super) fn forget_parked_app(&mut self, pid: pid_t) {
        let gone: Vec<_> =
            self.parking.windows.keys().filter(|wid| wid.pid == pid).copied().collect();
        for wid in gone {
            self.forget_parked_window(wid);
        }
    }

    /// Layouts from before carry the settings of then: windows away come back
    /// after their neighbors instead.
    pub(super) fn forget_parked_layouts_all(&mut self) { self.parking.layouts.clear(); }

    fn forget_parked_layouts(&mut self, workspace: VirtualWorkspaceId) {
        if !self.parking.windows.values().any(|parked| parked.workspace == workspace) {
            self.parking.layouts.remove(&workspace);
        }
    }

    /// Put `wid` back in `workspace`'s `layout`: in the layouts from before it
    /// left, when the workspace has changed only by windows leaving, or else
    /// after the window it followed. False when it has no place there to go
    /// back to.
    pub(super) fn restore_parked_window(
        &mut self,
        workspace_id: VirtualWorkspaceId,
        layout: LayoutId,
        wid: WindowId,
    ) -> bool {
        if self
            .parking
            .windows
            .get(&wid)
            .is_none_or(|parked| parked.workspace != workspace_id)
        {
            return false;
        }
        let Some(parked) = self.parking.windows.remove(&wid) else {
            return false;
        };
        let restored = self.restore_parked_layouts(workspace_id, wid)
            || self.restore_after_neighbor(workspace_id, layout, wid, &parked.order);
        self.forget_parked_layouts(parked.workspace);
        restored
    }

    fn restore_parked_layouts(&mut self, workspace_id: VirtualWorkspaceId, wid: WindowId) -> bool {
        let Some(before) = self.parking.layouts.get(&workspace_id) else {
            return false;
        };
        let workspace = &mut self.workspaces[workspace_id];
        let mut present = windows(workspace, &workspace.layout_system);
        let parked = &self.parking.windows;
        let Some(without) = trimmed(workspace, before, &present, None, parked) else {
            return false;
        };
        if !same_layouts(workspace, &without) {
            return false;
        }
        present.insert(wid);
        let Some(with) = trimmed(workspace, before, &present, Some(wid), parked) else {
            return false;
        };
        workspace.layout_system = with;
        true
    }

    fn restore_after_neighbor(
        &mut self,
        workspace_id: VirtualWorkspaceId,
        layout: LayoutId,
        wid: WindowId,
        order: &[WindowId],
    ) -> bool {
        let system = &mut self.workspaces[workspace_id].layout_system;
        let Some(at) = order.iter().position(|&window| window == wid) else {
            return false;
        };
        let Some(&neighbor) = order[..at]
            .iter()
            .rev()
            .chain(&order[at + 1..])
            .find(|&&window| system.contains_window(layout, window))
        else {
            return false;
        };
        system.select_window(layout, neighbor);
        system.add_window_after_selection(layout, wid);
        true
    }
}

/// The windows in `system`'s layouts for `workspace`.
fn windows(workspace: &VirtualWorkspace, system: &LayoutSystemKind) -> HashSet<WindowId> {
    workspace
        .layout_state
        .all_layouts()
        .filter(|&layout| system.contains_layout(layout))
        .flat_map(|layout| system.all_windows_in_layout(layout))
        .collect()
}

/// `system`, layouts for `workspace`, with only the windows in `keep`, the
/// others taken out as they left; none when it has no `needed`.
fn trimmed(
    workspace: &VirtualWorkspace,
    system: &LayoutSystemKind,
    keep: &HashSet<WindowId>,
    needed: Option<WindowId>,
    parked: &HashMap<WindowId, ParkedWindow>,
) -> Option<LayoutSystemKind> {
    let all = windows(workspace, system);
    if needed.is_some_and(|wid| !all.contains(&wid)) {
        return None;
    }
    let mut gone: Vec<_> = all.difference(keep).copied().collect();
    gone.sort_by_key(|wid| parked.get(wid).map_or(u64::MAX, |parked| parked.departure));
    let mut system = system.preview_clone()?;
    for wid in gone {
        remove(
            &mut system,
            wid,
            parked.get(&wid).is_some_and(|parked| parked.rebalanced),
        );
    }
    Some(system)
}

fn remove(system: &mut LayoutSystemKind, wid: WindowId, rebalance: bool) {
    if rebalance {
        system.remove_window_and_rebalance_parent(wid);
    } else {
        system.remove_window(wid);
    }
}

/// Whether `other` places the windows of every layout of `workspace` as its
/// own layouts do.
fn same_layouts(workspace: &VirtualWorkspace, other: &LayoutSystemKind) -> bool {
    workspace.layout_state.all_layouts().all(|layout| {
        other.contains_layout(layout)
            && same_shape(
                &other.container_tree(layout),
                &workspace.layout_system.container_tree(layout),
            )
    })
}

/// Whether two layouts place the same windows the same way, whatever their
/// selection and node identities.
fn same_shape(a: &ContainerTreeNode, b: &ContainerTreeNode) -> bool {
    let weights = match (a.weight, b.weight) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-6,
        (a, b) => a.is_none() && b.is_none(),
    };
    weights
        && a.node_type == b.node_type
        && a.layout_kind == b.layout_kind
        && a.window_id == b.window_id
        && a.is_fullscreen == b.is_fullscreen
        && a.is_fullscreen_within_gaps == b.is_fullscreen_within_gaps
        && a.role == b.role
        && a.children.len() == b.children.len()
        && a.children.iter().zip(&b.children).all(|(a, b)| same_shape(a, b))
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    use super::*;
    use crate::common::config::{LayoutMode, LayoutSettings, VirtualWorkspaceSettings};
    use crate::layout_engine::LayoutEvent;
    use crate::layout_engine::engine::WindowLayoutInfo;

    const SCREEN: CGRect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize { width: 1200.0, height: 800.0 },
    };

    struct Setup {
        engine: LayoutEngine,
        store: WindowStore,
        space: SpaceId,
    }

    impl Setup {
        fn new(mode: LayoutMode, windows: &[WindowId]) -> Self {
            let settings = LayoutSettings {
                mode,
                ..LayoutSettings::default()
            };
            let mut setup = Self {
                engine: LayoutEngine::new(&VirtualWorkspaceSettings::default(), &settings, None),
                store: WindowStore::default(),
                space: SpaceId::new(7),
            };
            setup.event(LayoutEvent::SpaceExposed(setup.space, SCREEN.size));
            setup.observe(windows);
            setup
        }

        fn event(&mut self, event: LayoutEvent) {
            let _ = self.engine.handle_event(&mut self.store, event);
        }

        fn observe(&mut self, windows: &[WindowId]) {
            let info = windows
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
            self.event(LayoutEvent::windows_observed(self.space, 1, info, None));
        }

        fn focus(&mut self, window: WindowId) {
            self.event(LayoutEvent::WindowFocused(self.space, window));
        }

        fn frames(&mut self) -> Vec<(WindowId, CGRect)> {
            let mut frames = self.engine.calculate_layout(
                self.space,
                SCREEN,
                &LayoutSettings::default().gaps,
                Default::default(),
            );
            frames.sort_by_key(|(window, _)| window.idx);
            frames
        }

        fn order(&self) -> Vec<WindowId> {
            let (workspace, layout) =
                self.engine.workspaces.active_layout_for_space(self.space).unwrap();
            self.engine.workspaces[workspace].layout_system.all_windows_in_layout(layout)
        }
    }

    fn windows(count: u32) -> Vec<WindowId> {
        (1..=count).map(|idx| WindowId::new(1, idx)).collect()
    }

    #[test]
    fn a_window_comes_back_to_its_place_in_every_layout() {
        for mode in [
            LayoutMode::Bsp,
            LayoutMode::Traditional,
            LayoutMode::MasterStack,
            LayoutMode::Scrolling,
            LayoutMode::Stack,
        ] {
            let w = windows(3);
            let mut setup = Setup::new(mode, &w);
            // Somewhere else selected, so a new window would go there.
            setup.focus(w[2]);
            let before = setup.frames();
            setup.event(LayoutEvent::WindowRemoved(w[0]));
            setup.event(LayoutEvent::WindowAdded(setup.space, w[0]));
            assert_eq!(setup.frames(), before, "{mode:?}");
        }
    }

    #[test]
    fn a_minimized_window_comes_back_at_its_size() {
        let w = windows(3);
        let mut setup = Setup::new(LayoutMode::Bsp, &w);
        setup.focus(w[0]);
        let space = setup.space;
        let _ = setup.engine.handle_command(
            &mut setup.store,
            Some(space),
            &[space],
            &HashMap::default(),
            crate::layout_engine::LayoutCommand::ResizeWindowBy { amount: 0.2 },
        );
        let before = setup.frames();
        setup.focus(w[1]);
        // Minimized, the window store lets go of its workspace first, and its
        // split is rebalanced.
        let _ = setup.store.remove_window_assignment(w[2]);
        setup.event(LayoutEvent::WindowRemoved(w[2]));
        assert_ne!(setup.frames().len(), before.len());
        setup.event(LayoutEvent::WindowAdded(space, w[2]));
        assert_eq!(setup.frames(), before);
    }

    #[test]
    fn windows_away_together_come_back_in_any_order() {
        let w = windows(4);
        let mut setup = Setup::new(LayoutMode::Bsp, &w);
        let before = setup.frames();
        setup.focus(w[3]);
        for &window in &w[..3] {
            setup.event(LayoutEvent::WindowRemoved(window));
        }
        for &window in [w[1], w[2], w[0]].iter() {
            setup.event(LayoutEvent::WindowAdded(setup.space, window));
        }
        assert_eq!(setup.frames(), before);
    }

    #[test]
    fn a_window_comes_back_after_its_neighbor_when_the_layout_changed() {
        let w = windows(4);
        let mut setup = Setup::new(LayoutMode::Traditional, &w[..3]);
        let before = setup.order();
        let after = before[before.iter().position(|&window| window == w[1]).unwrap() - 1];
        setup.event(LayoutEvent::WindowRemoved(w[1]));
        // A new window while it is away, the selection away from its place.
        setup.observe(&[w[0], w[2], w[3]]);
        setup.focus(w[3]);
        setup.event(LayoutEvent::WindowAdded(setup.space, w[1]));
        let order = setup.order();
        let at = |window| order.iter().position(|&w| w == window).unwrap();
        assert_eq!(at(w[1]), at(after) + 1, "{order:?}");
    }

    #[test]
    fn a_window_without_its_workspace_comes_back_to_the_one_it_left() {
        let w = windows(2);
        let mut setup = Setup::new(LayoutMode::Bsp, &w);
        let space = setup.space;
        let left = setup.store.workspace_for_window(space, w[0]).unwrap();
        // Minimized, the window store lets go of its workspace first.
        let _ = setup.store.remove_window_assignment(w[0]);
        setup.event(LayoutEvent::WindowRemoved(w[0]));
        let ids = setup.engine.workspaces.workspace_ids(space);
        let other = *ids.iter().find(|&&id| id != left).unwrap();
        setup.engine.workspaces.set_active_workspace(space, other);
        setup.event(LayoutEvent::WindowAdded(space, w[0]));
        assert_eq!(setup.store.workspace_for_window(space, w[0]), Some(left));
    }
}
