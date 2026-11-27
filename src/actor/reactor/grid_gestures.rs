//! The grid's trackpad swipes, on the focused display as the keys are, whichever
//! display the pointer is on: each does the action `[settings.grid.gestures]`
//! gives it. The content follows the fingers: swiping left shows the cell to the
//! right, or the next grid.

use super::{Command, DisplaySelector, Event, Reactor};
use crate::actor::gesture::{Context, GridAction, GridSwipe};
use crate::common::config::{GRID_COUNT, GestureAction};
use crate::layout_engine::{Direction, LayoutCommand};

impl Reactor {
    pub(super) fn grid_swipe(&mut self, context: &Context, swipe: GridSwipe, fingers: Direction) {
        // Home finds its display itself: the focused display may show a
        // fullscreen app, which has no Space to resolve here.
        if swipe.action(fingers) == Some(GridAction::Do(GestureAction::Home)) {
            if !self.is_mission_control_active() {
                self.home_command();
            }
            return;
        }
        let Some(space) = self.command_context_space() else {
            return;
        };
        if !self.gesture_space_active(space)
            || !self.layout_manager.layout_engine.workspaces().grid_enabled()
        {
            return;
        }
        let action = match swipe.action(fingers) {
            Some(GridAction::Do(action)) => action,
            Some(GridAction::OverviewBack) => return self.grid_overview_step(false),
            None => return,
        };
        let command = match action {
            GestureAction::None | GestureAction::Home => return,
            GestureAction::Overview => return self.grid_overview_step(true),
            GestureAction::GoToCellLeft => LayoutCommand::GoToCell(Direction::Left),
            GestureAction::GoToCellRight => LayoutCommand::GoToCell(Direction::Right),
            GestureAction::GoToCellUp => LayoutCommand::GoToCell(Direction::Up),
            GestureAction::GoToCellDown => LayoutCommand::GoToCell(Direction::Down),
            GestureAction::FocusLeft => LayoutCommand::Focus(Direction::Left),
            GestureAction::FocusRight => LayoutCommand::Focus(Direction::Right),
            GestureAction::FocusUp => LayoutCommand::Focus(Direction::Up),
            GestureAction::FocusDown => LayoutCommand::Focus(Direction::Down),
            GestureAction::NextGrid | GestureAction::PreviousGrid => {
                let forward = action == GestureAction::NextGrid;
                let Some(grid) = self.next_grid_on(space, forward) else {
                    return;
                };
                LayoutCommand::Grid(grid + 1)
            }
            GestureAction::FocusDefaultCell => LayoutCommand::FocusDefaultCell,
            GestureAction::FocusRecentWindow => LayoutCommand::FocusRecentWindow,
            GestureAction::FocusDisplayLeft
            | GestureAction::FocusDisplayRight
            | GestureAction::FocusDisplayUp
            | GestureAction::FocusDisplayDown => {
                let way = match action {
                    GestureAction::FocusDisplayLeft => Direction::Left,
                    GestureAction::FocusDisplayRight => Direction::Right,
                    GestureAction::FocusDisplayUp => Direction::Up,
                    _ => Direction::Down,
                };
                return self.handle_event_inner(Event::Command(Command::Reactor(
                    super::ReactorCommand::FocusDisplay(DisplaySelector::Direction(way)),
                )));
            }
        };
        let before = self.layout_manager.layout_engine.workspaces().active_workspace(space);
        self.handle_event_inner(Event::Command(Command::Layout(command)));
        let after = self.layout_manager.layout_engine.workspaces().active_workspace(space);
        let ticks =
            before != after || (self.grid_hit_edge() && self.config.settings.grid.moves.edge_glow);
        if ticks && let Some(pattern) = context.haptic {
            let _ = crate::sys::haptics::perform_haptic(pattern);
        }
    }

    /// Step the Overview out a stage, opening it, or back in, leaving it.
    fn grid_overview_step(&mut self, out: bool) {
        if let Some(wm) = &self.communication_manager.wm_sender {
            wm.send(crate::actor::wm_controller::WmEvent::OverviewStep { out });
        }
    }

    /// The grid after (or before) the one `space` shows, among the grids on its
    /// display, round the end; `None` with only one.
    fn next_grid_on(&self, space: crate::sys::screen::SpaceId, forward: bool) -> Option<usize> {
        let current = self.layout_manager.layout_engine.active_grid(space)?;
        let on: Vec<usize> = (0..GRID_COUNT).filter(|&g| self.grid_is_on(space, g)).collect();
        let at = on.iter().position(|&g| g == current)?;
        let next = if forward {
            (at + 1) % on.len()
        } else {
            (at + on.len() - 1) % on.len()
        };
        Some(on[next]).filter(|&g| g != current)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    use super::super::testing::{
        Apps, make_windows, space_state_event, test_reactor_with_workspace_settings,
    };
    use super::*;
    use crate::actor::app::WindowId;
    use crate::common::config::{Config, LayoutMode};
    use crate::sys::screen::SpaceId;

    fn one_display() -> (Reactor, SpaceId, Context) {
        crate::sys::screen::TEST_ACTIVE_DISPLAY.with(|display| display.take());
        let settings = crate::common::config::VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut reactor = test_reactor_with_workspace_settings(&settings);
        let space = SpaceId::new(1);
        let frame = CGRect::new(CGPoint::new(0., 0.), CGSize::new(1000., 1000.));
        reactor.handle_event(space_state_event(vec![frame], vec![Some(space)]));
        let mut config = Config::default();
        config.virtual_workspaces.grid_layout = Some(crate::common::config::GridLayout::new(3, 3));
        config.settings.gestures.haptics_enabled = false;
        let context = Context::new(
            1,
            0,
            space,
            (LayoutMode::Traditional, 3),
            crate::actor::gesture::Settings::new(&config),
            Duration::ZERO,
        )
        .unwrap();
        (reactor, space, context)
    }

    const THREE: GridSwipe = GridSwipe {
        actions: crate::common::config::SwipeActions {
            left: GestureAction::GoToCellRight,
            right: GestureAction::GoToCellLeft,
            up: GestureAction::GoToCellDown,
            down: GestureAction::GoToCellUp,
        },
        open: false,
    };
    const GRIDS: GridSwipe = GridSwipe {
        actions: crate::common::config::SwipeActions {
            left: GestureAction::NextGrid,
            right: GestureAction::PreviousGrid,
            up: GestureAction::Overview,
            down: GestureAction::Home,
        },
        open: false,
    };

    fn cell(reactor: &Reactor, space: SpaceId) -> Option<usize> {
        let workspaces = reactor.layout_manager.layout_engine.workspaces();
        let active = workspaces.active_workspace(space)?;
        workspaces.workspace_ids(space).iter().position(|&id| id == active)
    }

    #[test]
    fn the_cell_comes_in_from_where_the_fingers_go() {
        let (mut reactor, space, context) = one_display();
        let center = cell(&reactor, space).unwrap();
        reactor.grid_swipe(&context, THREE, Direction::Left);
        assert_eq!(
            cell(&reactor, space),
            Some(center + 1),
            "left shows the cell to the right"
        );
        reactor.grid_swipe(&context, THREE, Direction::Up);
        assert_eq!(
            cell(&reactor, space),
            Some(center + 4),
            "up shows the cell below"
        );
        reactor.grid_swipe(&context, THREE, Direction::Down);
        reactor.grid_swipe(&context, THREE, Direction::Right);
        assert_eq!(cell(&reactor, space), Some(center));
    }

    #[test]
    fn four_fingers_go_round_the_displays_grids() {
        let (mut reactor, space, context) = one_display();
        let grid = |reactor: &Reactor| reactor.layout_manager.layout_engine.active_grid(space);
        reactor.grid_swipe(&context, GRIDS, Direction::Left);
        assert_eq!(grid(&reactor), Some(0), "one grid: nowhere to go");

        let mut apps = Apps::new();
        reactor.handle_events(apps.make_app_with_opts(
            1,
            make_windows(2),
            Some(WindowId::new(1, 1)),
            true,
            true,
        ));
        reactor.handle_event(Event::ApplicationGloballyActivated(1));
        apps.simulate_until_quiet(&mut reactor);
        // Grids 1 and 3 hold a window each; grid 2 is on no display.
        reactor.handle_test_layout_command(LayoutCommand::SendToGrid(3));
        apps.simulate_until_quiet(&mut reactor);
        assert!(reactor.grid_is_on(space, 2));

        reactor.grid_swipe(&context, GRIDS, Direction::Left);
        assert_eq!(
            grid(&reactor),
            Some(2),
            "left goes to the next grid, past an empty one"
        );
        reactor.grid_swipe(&context, GRIDS, Direction::Left);
        assert_eq!(grid(&reactor), Some(0), "and round to the first");
        reactor.grid_swipe(&context, GRIDS, Direction::Right);
        assert_eq!(grid(&reactor), Some(2), "right goes back, round the start");
    }
}
