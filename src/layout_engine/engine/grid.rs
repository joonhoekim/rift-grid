//! The workspace grid's commands: moving focus and windows between tiles, and
//! past a cell's edge into the next cell. Where a direction leads comes from
//! `common::grid_nav`.

use objc2_core_foundation::{CGPoint, CGRect};

use super::{EventResponse, LayoutCommand, LayoutEngine};
use crate::actor::app::WindowId;
use crate::common::grid_nav;
use crate::layout_engine::{Direction, LayoutSystem};
use crate::model::VirtualWorkspaceId;
use crate::model::window_store::WindowStore;
use crate::sys::screen::SpaceId;

/// Windows by when they last had the focus, the latest first.
#[derive(Default)]
pub(super) struct RecentWindows(std::collections::VecDeque<WindowId>);

impl RecentWindows {
    const KEEP: usize = 64;

    pub(super) fn focused(&mut self, wid: WindowId) {
        if self.0.front() == Some(&wid) {
            return;
        }
        self.0.retain(|&w| w != wid);
        self.0.push_front(wid);
        self.0.truncate(Self::KEEP);
    }
}

impl LayoutEngine {
    /// The grid (0-based) shown on `space`.
    pub(crate) fn active_grid(&self, space: SpaceId) -> Option<usize> {
        let active = self.workspaces.active_workspace(space)?;
        let index = self.workspaces.workspace_ids(space).iter().position(|&id| id == active)?;
        self.workspaces.grid_of(index)
    }

    pub(super) fn handle_grid_command(
        &mut self,
        window_store: &mut WindowStore,
        space: SpaceId,
        command: &LayoutCommand,
    ) -> EventResponse {
        match *command {
            LayoutCommand::GridFocus(direction) => self.grid_focus(window_store, space, direction),
            LayoutCommand::GridMove(direction) => self.grid_move(window_store, space, direction),
            LayoutCommand::GridCell(direction) => {
                let Some(current) = self.workspaces.active_workspace(space) else {
                    return EventResponse::default();
                };
                match self.workspaces.grid_neighbor(space, current, direction) {
                    Some(target) => self.activate_workspace(window_store, space, target, None),
                    None => EventResponse::default(),
                }
            }
            LayoutCommand::GridCenter => {
                let Some(grid) = self.active_grid(space) else {
                    return EventResponse::default();
                };
                match self.workspaces.grid_center_cell(space, grid) {
                    Some(target) if Some(target) != self.workspaces.active_workspace(space) => {
                        self.activate_workspace(window_store, space, target, None)
                    }
                    _ => EventResponse::default(),
                }
            }
            LayoutCommand::SwitchToGrid(grid) => {
                let target = grid
                    .checked_sub(1)
                    .and_then(|grid| self.workspaces.grid_current_cell(space, grid));
                match target {
                    Some(target) if Some(target) != self.workspaces.active_workspace(space) => {
                        self.activate_workspace(window_store, space, target, None)
                    }
                    _ => EventResponse::default(),
                }
            }
            LayoutCommand::MoveWindowToGrid(grid) => {
                self.move_window_to_grid(window_store, space, grid, None)
            }
            LayoutCommand::SendWindowToGrid { window_id, grid } => {
                self.move_window_to_grid(window_store, space, grid, Some(window_id))
            }
            LayoutCommand::FocusRecentWindow => self.focus_recent_window(window_store),
            _ => EventResponse::default(),
        }
    }

    /// Send a window, the focused one when no index is given, to grid N's
    /// (1-based) current cell; focus stays.
    fn move_window_to_grid(
        &mut self,
        window_store: &mut WindowStore,
        space: SpaceId,
        grid: usize,
        window_id: Option<u32>,
    ) -> EventResponse {
        let Some(target) =
            grid.checked_sub(1).and_then(|grid| self.workspaces.grid_current_cell(space, grid))
        else {
            return EventResponse::default();
        };
        let Some(index) = self.workspaces.workspace_ids(space).iter().position(|&id| id == target)
        else {
            return EventResponse::default();
        };
        self.handle_virtual_workspace_command(
            window_store,
            space,
            &LayoutCommand::MoveWindowToWorkspace {
                workspace: crate::common::config::WorkspaceSelector::Index(index),
                follow: false,
                window_id,
            },
        )
    }

    /// Hand grid `grid` (0-based) from `source` to `target`, a display of
    /// `target_size`, which then shows it at its current cell. The focus stays
    /// where it is. Returns the windows moved, or `None` when the grid can't go.
    pub(crate) fn move_grid_to_space(
        &mut self,
        window_store: &mut WindowStore,
        source: SpaceId,
        target: SpaceId,
        grid: usize,
        target_size: objc2_core_foundation::CGSize,
    ) -> Option<(EventResponse, Vec<WindowId>)> {
        let (moved, cells) = self.workspaces.move_grid(window_store, source, target, grid)?;
        for &wid in &moved {
            self.floating.remove_active_for_window(wid);
        }
        self.move_departures(target, &cells);
        self.ensure_workspace_layouts(target, target_size);
        let cell = self.workspaces.grid_current_cell(target, grid)?;
        let mut response = self.activate_workspace(window_store, target, cell, None);
        response.focus_window = None;
        self.update_active_floating_windows(window_store, source);
        self.broadcast_windows_changed(window_store, source);
        Some((response, moved))
    }

    /// Focus the window that had the focus before the focused one, showing its
    /// cell first when it is in another one.
    fn focus_recent_window(&mut self, window_store: &WindowStore) -> EventResponse {
        let current = self.focused_window;
        let target = self.recent.0.iter().copied().filter(|&wid| Some(wid) != current).find_map(
            |wid| {
                let assignment = window_store.workspace_info_for_window(wid)?;
                Some((wid, assignment.space, assignment.workspace_id))
            },
        );
        let Some((wid, target_space, workspace)) = target else {
            return EventResponse::default();
        };
        if self.workspaces.active_workspace(target_space) == Some(workspace) {
            // `changed`, or the reactor drops a grid command's response unused.
            return EventResponse {
                focus_window: Some(wid),
                changed: true,
                ..EventResponse::default()
            };
        }
        self.activate_workspace(window_store, target_space, workspace, Some(wid))
    }

    fn focused_tile(&self, space: SpaceId) -> Option<WindowId> {
        self.focused_window.filter(|&wid| {
            !self.floating.is_floating(wid) && self.space_with_window(wid) == Some(space)
        })
    }

    /// A cell's tiles at the cell's own size, from its top left corner. There are no
    /// gaps, so tiles beside each other share their sides.
    fn cell_tiles(
        &self,
        space: SpaceId,
        workspace: VirtualWorkspaceId,
    ) -> Option<(grid_nav::Rect, Vec<(WindowId, grid_nav::Rect)>)> {
        let size = self.workspaces[workspace].layout_state.active_size()?;
        let layout = self.workspaces.active_layout(space, workspace)?;
        let tiles = self.workspaces[workspace].layout_system.calculate_layout(
            layout,
            CGRect::new(CGPoint::new(0.0, 0.0), size),
            self.layout_settings.stack.stack_offset,
            &self.window_layout_constraints,
            &crate::common::config::GapSettings::default(),
            crate::common::config::StackLineReserve::default(),
        );
        let rect = |f: CGRect| {
            grid_nav::Rect::new(f.origin.x, f.origin.y, f.size.width, f.size.height)
        };
        Some((
            grid_nav::Rect::new(0.0, 0.0, size.width, size.height),
            tiles.into_iter().map(|(wid, frame)| (wid, rect(frame))).collect(),
        ))
    }

    /// Focus the tile beside the focused one in the active cell, or past the cell's
    /// edge, the tile on the near side of the next cell level with the one left.
    fn grid_focus(
        &mut self,
        window_store: &mut WindowStore,
        space: SpaceId,
        direction: Direction,
    ) -> EventResponse {
        let Some((ws_id, layout)) = self.workspaces.active_layout_for_space(space) else {
            return EventResponse::default();
        };
        let dir = crate::model::virtual_workspace::grid_dir(direction);
        let mut along = None;
        if let Some(focused) = self.focused_tile(space)
            && let Some((_, tiles)) = self.cell_tiles(space, ws_id)
            && let Some(from) = tiles.iter().position(|(wid, _)| *wid == focused)
        {
            let rects: Vec<_> = tiles.iter().map(|(_, r)| *r).collect();
            if let Some(next) = grid_nav::neighbor(&rects, from, dir) {
                let next = tiles[next].0;
                let response = EventResponse {
                    changed: true,
                    focus_window: Some(next),
                    raise_windows: vec![next],
                    boundary_hit: None,
                };
                self.apply_focus_response(window_store, space, ws_id, layout, &response);
                return response;
            }
            let r = rects[from];
            along = Some(if dir.horizontal() {
                r.y + r.h / 2.0
            } else {
                r.x + r.w / 2.0
            });
        }
        let Some(target) = self.workspaces.grid_neighbor(space, ws_id, direction) else {
            return EventResponse::default();
        };
        let entry = self.cell_tiles(space, target).and_then(|(area, tiles)| {
            let rects: Vec<_> = tiles.iter().map(|(_, r)| *r).collect();
            let along = along.unwrap_or(if dir.horizontal() {
                area.h / 2.0
            } else {
                area.w / 2.0
            });
            let wid = tiles[grid_nav::entered(&rects, area, dir, along)?].0;
            (window_store.workspace_for_window(space, wid) == Some(target)).then_some(wid)
        });
        let response = self.activate_workspace(window_store, space, target, entry);
        self.record_grid_focus(space, target, &response);
        response
    }

    /// Note the focus a cell switch asks for right away, so the overview and the
    /// next grid command see it before the system reports the focus change.
    fn record_grid_focus(
        &mut self,
        space: SpaceId,
        workspace: VirtualWorkspaceId,
        response: &EventResponse,
    ) {
        if let Some(wid) = response.focus_window {
            self.focused_window = Some(wid);
            self.workspaces.set_last_focused_window(space, workspace, Some(wid));
        }
    }

    /// Swap with the neighboring tile in the active cell, or past the cell's edge,
    /// carry the window into the neighboring cell and follow it.
    fn grid_move(
        &mut self,
        window_store: &mut WindowStore,
        space: SpaceId,
        direction: Direction,
    ) -> EventResponse {
        let Some((ws_id, layout)) = self.workspaces.active_layout_for_space(space) else {
            return EventResponse::default();
        };
        let Some(wid) = self.focused_tile(space) else {
            return EventResponse::default();
        };
        self.workspaces[ws_id].layout_state.last_saved = Some(layout);
        if self.workspaces[ws_id].layout_system.move_selection(layout, direction) {
            return EventResponse {
                changed: true,
                focus_window: Some(wid),
                ..EventResponse::default()
            };
        }
        let Some(target) = self.workspaces.grid_neighbor(space, ws_id, direction) else {
            return EventResponse::default();
        };
        let Some(target_layout) = self.workspaces.active_layout(space, target) else {
            return EventResponse::default();
        };
        self.workspaces[ws_id].layout_system.remove_window(wid);
        if !self.workspaces.assign_window_to_workspace(window_store, space, wid, target) {
            self.workspaces[ws_id].layout_system.add_window_after_selection(layout, wid);
            return EventResponse::default();
        }
        self.workspaces[target].layout_system.add_window_from_direction(
            target_layout,
            wid,
            direction,
        );
        self.workspaces.set_last_focused_window(space, ws_id, None);
        let response = self.activate_workspace(window_store, space, target, Some(wid));
        self.record_grid_focus(space, target, &response);
        response
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    use super::*;
    use crate::common::config::{LayoutMode, LayoutSettings, VirtualWorkspaceSettings};
    use crate::layout_engine::{LayoutEvent, WindowLayoutInfo};

    fn window_layout_info(window_id: WindowId, current_size: CGSize) -> WindowLayoutInfo {
        WindowLayoutInfo {
            window_id,
            title: None,
            ax_role: None,
            ax_subrole: None,
            is_resizable: true,
            current_size,
            min_size: None,
            max_size: None,
        }
    }

    #[test]
    fn grid_commands_cross_cell_edges() {
        let mut settings = VirtualWorkspaceSettings::default();
        settings.default_workspace_count = 9;
        settings.grid_columns = 3;
        settings.default_workspace = 4;
        let mut layout_settings = LayoutSettings::default();
        layout_settings.mode = LayoutMode::Bsp;
        let mut engine = LayoutEngine::new(&settings, &layout_settings, None);
        let mut window_store = WindowStore::default();
        let space = SpaceId::new(90);
        let screen = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1920.0, 1080.0));
        let _ =
            engine.handle_event(&mut window_store, LayoutEvent::SpaceExposed(space, screen.size));
        let cell = |engine: &mut LayoutEngine, index: usize| {
            engine.workspaces_mut().workspace_id_at(space, Some(index)).unwrap()
        };
        let center = cell(&mut engine, 4);
        assert_eq!(engine.workspaces().active_workspace(space), Some(center));

        let left = WindowId::new(90, 1);
        let right = WindowId::new(90, 2);
        for wid in [left, right] {
            let _ = engine.handle_event(
                &mut window_store,
                LayoutEvent::windows_observed(
                    space,
                    wid.pid,
                    vec![window_layout_info(wid, CGSize::new(0.0, 0.0))],
                    None,
                ),
            );
        }
        let _ = engine.handle_event(&mut window_store, LayoutEvent::WindowFocused(space, left));
        let grid = |engine: &mut LayoutEngine, store: &mut WindowStore, command| {
            engine.handle_virtual_workspace_command(store, space, &command)
        };

        // Inside the cell, focus moves between tiles.
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridFocus(Direction::Right),
        );
        assert_eq!(response.focus_window, Some(right));
        assert_eq!(engine.workspaces().active_workspace(space), Some(center));

        // Past the cell's edge, it moves to the neighboring cell.
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridFocus(Direction::Right),
        );
        assert!(response.changed);
        assert_eq!(
            engine.workspaces().active_workspace(space),
            Some(cell(&mut engine, 5))
        );
        assert_eq!(response.focus_window, None, "the neighboring cell is empty");

        // Coming back enters on the near edge: the right tile.
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridFocus(Direction::Left),
        );
        assert_eq!(engine.workspaces().active_workspace(space), Some(center));
        assert_eq!(response.focus_window, Some(right));
        let _ = engine.handle_event(&mut window_store, LayoutEvent::WindowFocused(space, right));

        // Carrying the window: first it swaps within the cell, then it leaves.
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridMove(Direction::Left),
        );
        assert_eq!(response.focus_window, Some(right));
        assert_eq!(engine.workspaces().active_workspace(space), Some(center));
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridMove(Direction::Left),
        );
        let west = cell(&mut engine, 3);
        assert_eq!(engine.workspaces().active_workspace(space), Some(west));
        assert_eq!(response.focus_window, Some(right));
        assert_eq!(window_store.workspace_for_window(space, right), Some(west));
        assert_eq!(window_store.workspace_for_window(space, left), Some(center));

        // The grid does not wrap.
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridCell(Direction::Left),
        );
        assert!(!response.changed);
        assert_eq!(engine.workspaces().active_workspace(space), Some(west));
        let _ = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridCell(Direction::Up),
        );
        let response = grid(
            &mut engine,
            &mut window_store,
            LayoutCommand::GridCell(Direction::Up),
        );
        assert!(!response.changed);
        assert_eq!(
            engine.workspaces().active_workspace(space),
            Some(cell(&mut engine, 0))
        );
    }

    #[test]
    fn grid_focus_enters_the_next_cell_level_with_the_tile_left() {
        let mut settings = VirtualWorkspaceSettings::default();
        settings.default_workspace_count = 9;
        settings.grid_columns = 3;
        settings.default_workspace = 4;
        let mut layout_settings = LayoutSettings::default();
        layout_settings.mode = LayoutMode::Bsp;
        let mut engine = LayoutEngine::new(&settings, &layout_settings, None);
        let mut store = WindowStore::default();
        let space = SpaceId::new(91);
        let screen = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1920.0, 1080.0));
        let _ = engine.handle_event(&mut store, LayoutEvent::SpaceExposed(space, screen.size));
        let open = |engine: &mut LayoutEngine, store: &mut WindowStore, idx: u32| {
            let wid = WindowId::new(91, idx);
            let _ = engine.handle_event(
                store,
                LayoutEvent::windows_observed(
                    space,
                    wid.pid,
                    vec![window_layout_info(wid, CGSize::new(0.0, 0.0))],
                    None,
                ),
            );
            let _ = engine.handle_event(store, LayoutEvent::WindowFocused(space, wid));
            wid
        };
        let grid = |engine: &mut LayoutEngine, store: &mut WindowStore, command| {
            engine.handle_virtual_workspace_command(store, space, &command)
        };
        let cell = |engine: &mut LayoutEngine, index: usize| {
            engine.workspaces_mut().workspace_id_at(space, Some(index)).unwrap()
        };

        // Above: a | b.
        let _ = grid(&mut engine, &mut store, LayoutCommand::GridCell(Direction::Up));
        let a = open(&mut engine, &mut store, 1);
        let b = open(&mut engine, &mut store, 2);
        // The center: d | (e / f), each new window halving the focused tile.
        let _ = grid(&mut engine, &mut store, LayoutCommand::GridCell(Direction::Down));
        let d = open(&mut engine, &mut store, 3);
        let e = open(&mut engine, &mut store, 4);
        let f = open(&mut engine, &mut store, 5);
        let center = cell(&mut engine, 4);
        let focus = |engine: &mut LayoutEngine, store: &mut WindowStore, wid| {
            let _ = engine.handle_event(store, LayoutEvent::WindowFocused(space, wid));
        };

        // Inside the cell, by the tiles' sides.
        focus(&mut engine, &mut store, e);
        let response = grid(
            &mut engine,
            &mut store,
            LayoutCommand::GridFocus(Direction::Down),
        );
        assert_eq!(response.focus_window, Some(f));
        let response = grid(
            &mut engine,
            &mut store,
            LayoutCommand::GridFocus(Direction::Left),
        );
        assert_eq!(response.focus_window, Some(d));
        assert_eq!(engine.workspaces().active_workspace(space), Some(center));

        // Up from the left tile comes into the left one above; from the right, the right.
        let response = grid(&mut engine, &mut store, LayoutCommand::GridFocus(Direction::Up));
        assert_eq!(
            engine.workspaces().active_workspace(space),
            Some(cell(&mut engine, 1))
        );
        assert_eq!(response.focus_window, Some(a));
        let _ = grid(&mut engine, &mut store, LayoutCommand::GridCell(Direction::Down));
        focus(&mut engine, &mut store, e);
        let response = grid(&mut engine, &mut store, LayoutCommand::GridFocus(Direction::Up));
        assert_eq!(response.focus_window, Some(b));
    }

    #[test]
    fn grids_hold_cells_and_remember_where_they_were() {
        let mut settings = VirtualWorkspaceSettings::default();
        settings.grid_columns = 3;
        settings.default_workspace = 4;
        settings.workspace_names = Vec::new();
        let mut layout_settings = LayoutSettings::default();
        layout_settings.mode = LayoutMode::Bsp;
        let mut engine = LayoutEngine::new(&settings, &layout_settings, None);
        let mut store = WindowStore::default();
        let space = SpaceId::new(92);
        let screen = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1920.0, 1080.0));
        let _ = engine.handle_event(&mut store, LayoutEvent::SpaceExposed(space, screen.size));
        let cell = |engine: &mut LayoutEngine, index: usize| {
            engine.workspaces_mut().workspace_id_at(space, Some(index)).unwrap()
        };
        let run = |engine: &mut LayoutEngine, store: &mut WindowStore, command| {
            engine.handle_virtual_workspace_command(store, space, &command)
        };
        let active = |engine: &LayoutEngine| engine.workspaces().active_workspace(space);

        assert_eq!(engine.workspaces_mut().list_workspaces(space).len(), 81, "9 grids of 9 cells");
        assert_eq!(engine.workspaces_mut().list_workspaces(space)[13].1, "2·5");
        assert_eq!(active(&engine), Some(cell(&mut engine, 4)), "grid 1, center");

        // Cells never lead into the next grid.
        let _ = run(&mut engine, &mut store, LayoutCommand::GridCell(Direction::Right));
        assert_eq!(active(&engine), Some(cell(&mut engine, 5)));
        let edge = run(&mut engine, &mut store, LayoutCommand::GridCell(Direction::Right));
        assert!(!edge.changed, "grid 1's right edge, not grid 2");

        // A grid opens at its center, and comes back to the cell it was left on.
        let _ = run(&mut engine, &mut store, LayoutCommand::SwitchToGrid(2));
        assert_eq!(active(&engine), Some(cell(&mut engine, 13)));
        let _ = run(&mut engine, &mut store, LayoutCommand::GridCell(Direction::Up));
        assert_eq!(active(&engine), Some(cell(&mut engine, 10)));
        let _ = run(&mut engine, &mut store, LayoutCommand::SwitchToGrid(1));
        assert_eq!(active(&engine), Some(cell(&mut engine, 5)), "grid 1 where it was left");
        let _ = run(&mut engine, &mut store, LayoutCommand::SwitchToGrid(2));
        assert_eq!(active(&engine), Some(cell(&mut engine, 10)));
        let _ = run(&mut engine, &mut store, LayoutCommand::GridCenter);
        assert_eq!(active(&engine), Some(cell(&mut engine, 13)), "this grid's center");
        let nothing = run(&mut engine, &mut store, LayoutCommand::SwitchToGrid(10));
        assert!(!nothing.changed, "there are nine grids");

        // A window sent to a grid lands on that grid's current cell; focus stays.
        let wid = WindowId::new(92, 1);
        let _ = engine.handle_event(
            &mut store,
            LayoutEvent::windows_observed(
                space,
                wid.pid,
                vec![window_layout_info(wid, CGSize::new(0.0, 0.0))],
                None,
            ),
        );
        let _ = engine.handle_event(&mut store, LayoutEvent::WindowFocused(space, wid));
        let _ = run(&mut engine, &mut store, LayoutCommand::MoveWindowToGrid(1));
        assert_eq!(store.workspace_for_window(space, wid), Some(cell(&mut engine, 5)));
        assert_eq!(active(&engine), Some(cell(&mut engine, 13)), "focus stays in grid 2");

        // The window focused before is reached wherever it is, and back again.
        let other = WindowId::new(92, 2);
        let _ = engine.handle_event(
            &mut store,
            LayoutEvent::windows_observed(
                space,
                other.pid,
                vec![window_layout_info(other, CGSize::new(0.0, 0.0))],
                None,
            ),
        );
        let _ = engine.handle_event(&mut store, LayoutEvent::WindowFocused(space, other));
        let back = run(&mut engine, &mut store, LayoutCommand::FocusRecentWindow);
        assert_eq!(active(&engine), Some(cell(&mut engine, 5)), "the sent window's cell");
        assert_eq!(back.focus_window, Some(wid));
        let _ = engine.handle_event(&mut store, LayoutEvent::WindowFocused(space, wid));
        let again = run(&mut engine, &mut store, LayoutCommand::FocusRecentWindow);
        assert_eq!(active(&engine), Some(cell(&mut engine, 13)));
        assert_eq!(again.focus_window, Some(other));

        // In the same cell too.
        let third = WindowId::new(93, 3);
        let _ = engine.handle_event(
            &mut store,
            LayoutEvent::windows_observed(
                space,
                third.pid,
                vec![window_layout_info(third, CGSize::new(0.0, 0.0))],
                None,
            ),
        );
        let _ = engine.handle_event(&mut store, LayoutEvent::WindowFocused(space, other));
        let _ = engine.handle_event(&mut store, LayoutEvent::WindowFocused(space, third));
        let same = run(&mut engine, &mut store, LayoutCommand::FocusRecentWindow);
        assert_eq!(active(&engine), Some(cell(&mut engine, 13)));
        assert_eq!((same.focus_window, same.changed), (Some(other), true));
    }
}
