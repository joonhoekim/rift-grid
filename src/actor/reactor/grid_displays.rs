//! Grids belong to displays, as workspaces do in Hyprland: a grid is on one
//! display at a time, the one showing it or holding its windows, and its number
//! leads there. `switch_to_grid` shows the grid on its display and moves the
//! focus there (not while the Overview is open); `move_window_to_grid` sends the window to that display. A grid
//! on no display yet comes up on the focused one. A display that comes up showing
//! an empty grid another display shows already is given the first free grid.
//! `move_grid_to_display` hands a grid to another display, from the overview.

use super::events::EventOutcome;
use super::{Command, DisplaySelector, Event, Reactor, ReactorCommand, WorkspaceSwitchOrigin};
use crate::common::config::GRID_COUNT;
use crate::layout_engine::LayoutCommand;
use crate::sys::screen::SpaceId;

impl Reactor {
    /// The displays in use, the main one first, by their spaces and UUIDs.
    fn grid_displays(&self) -> Vec<(SpaceId, String)> {
        self.space_state
            .screens
            .iter()
            .filter_map(|screen| {
                let space = screen.space.filter(|&space| self.is_space_active(space))?;
                Some((space, screen.display_uuid.clone()))
            })
            .collect()
    }

    fn grid_has_windows(&self, space: SpaceId, grid: usize) -> bool {
        self.layout_manager.layout_engine.workspaces().grid_has_windows(
            &self.state.windows,
            space,
            grid,
        )
    }

    /// Whether grid `grid` (0-based) is on the display showing `space`.
    pub(super) fn grid_is_on(&self, space: SpaceId, grid: usize) -> bool {
        self.layout_manager.layout_engine.active_grid(space) == Some(grid)
            || self.grid_has_windows(space, grid)
    }

    /// The other display grid `grid` is on, when it is not on `here`: the one
    /// showing it, else one holding its windows.
    fn grid_elsewhere(&self, here: SpaceId, grid: usize) -> Option<(SpaceId, String)> {
        if self.grid_is_on(here, grid) {
            return None;
        }
        let others: Vec<_> =
            self.grid_displays().into_iter().filter(|&(space, _)| space != here).collect();
        let shows = |(space, _): &&(SpaceId, String)| {
            self.layout_manager.layout_engine.active_grid(*space) == Some(grid)
        };
        others
            .iter()
            .find(shows)
            .or_else(|| others.iter().find(|(space, _)| self.grid_has_windows(*space, grid)))
            .cloned()
    }

    /// Whether the grid of the workspace at `index` on `space` is on another
    /// display, for the overview's "+".
    pub(super) fn grid_elsewhere_at(&self, space: Option<SpaceId>, index: usize) -> bool {
        let grid = self.layout_manager.layout_engine.workspaces().grid_of(index);
        space
            .zip(grid)
            .is_some_and(|(space, grid)| self.grid_elsewhere(space, grid).is_some())
    }

    fn focus_display(&mut self, display: String) {
        self.handle_event_inner(Event::Command(Command::Reactor(ReactorCommand::FocusDisplay(
            DisplaySelector::Uuid(display),
        ))));
    }

    /// Hand grid `grid` (0-based) to the display `display` selects, which shows
    /// it. A display that showed it shows its next grid with windows instead, or
    /// else a free one, and the focus stays there.
    fn hand_grid_over(&mut self, grid: usize, display: &DisplaySelector) {
        let Some(here) = self.command_context_space() else {
            return;
        };
        let source = if self.grid_is_on(here, grid) {
            Some(here)
        } else {
            self.grid_elsewhere(here, grid).map(|(space, _)| space)
        };
        let Some(source) = source else { return };
        let origin = self.space_state.screen_by_space(source).map(|screen| screen.frame.mid());
        let Some(screen) = self.screen_for_selector(display, origin).cloned() else {
            return;
        };
        let Some(target) = screen.space.filter(|&space| self.is_space_active(space)) else {
            return;
        };
        if target == source {
            return;
        }
        let shown = self.layout_manager.layout_engine.active_grid(source) == Some(grid);
        let Some((response, moved)) = self.layout_manager.layout_engine.move_grid_to_space(
            &mut self.state.windows,
            source,
            target,
            grid,
            screen.frame.size,
        ) else {
            return;
        };
        let mut outcome =
            EventOutcome::layout_changed(false).with_layout_response(response, Some(target));
        for wid in moved {
            let Some(window) = self.state.windows.window_mut(wid) else {
                continue;
            };
            let frame = Self::center_frame_on_screen(window.frame_monotonic, screen.frame);
            window.frame_monotonic = frame;
            let sys_id = window.info.sys_id;
            if let Some(sys_id) = sys_id {
                self.state.windows.observe_native_space(sys_id, target, true);
            }
            outcome = outcome.with_pre_layout_window_frame_write(wid, frame, true);
        }
        self.workspace_switch_manager
            .start_workspace_switch(WorkspaceSwitchOrigin::Manual);
        self.apply_event_outcome(outcome);
        if !shown {
            return;
        }
        let displays = self.grid_displays();
        let next = (0..GRID_COUNT)
            .find(|&g| g != grid && self.grid_has_windows(source, g))
            .or_else(|| {
                (0..GRID_COUNT).find(|&g| {
                    g != grid && displays.iter().all(|&(space, _)| !self.grid_is_on(space, g))
                })
            });
        if let Some(next) = next {
            let command_space = self.space_state.command_space;
            self.space_state.command_space = Some(source);
            self.handle_event_inner(Event::Command(Command::Layout(LayoutCommand::SwitchToGrid(
                next + 1,
            ))));
            self.space_state.command_space = command_space;
        }
    }

    /// Run a grid command on the display its grid is on. Other events come back.
    pub(super) fn handle_grid_display_event(&mut self, event: Event) -> Result<(), Event> {
        let Event::Command(Command::Layout(command)) = &event else {
            return Err(event);
        };
        if let LayoutCommand::MoveGridToDisplay { grid, display } = command
            && self.layout_manager.layout_engine.workspaces().grid_enabled()
        {
            if let Some(grid) = grid.checked_sub(1).filter(|&grid| grid < GRID_COUNT) {
                let display = display.clone();
                self.hand_grid_over(grid, &display);
            }
            return Ok(());
        }
        let (LayoutCommand::SwitchToGrid(grid) | LayoutCommand::MoveWindowToGrid(grid)) = *command
        else {
            return Err(event);
        };
        let switch = matches!(command, LayoutCommand::SwitchToGrid(_));
        if !self.layout_manager.layout_engine.workspaces().grid_enabled() {
            return Err(event);
        }
        let Some(here) = self.command_context_space() else {
            return Err(event);
        };
        let Some((there, display)) =
            grid.checked_sub(1).and_then(|grid| self.grid_elsewhere(here, grid))
        else {
            return Err(event);
        };
        if switch {
            self.space_state.command_space = Some(there);
            self.handle_event_inner(event);
            // From the Overview, the grid shows there and the Overview stays.
            if crate::ui::grid_motion::overview_open() {
                self.space_state.command_space = Some(here);
            } else {
                self.focus_display(display);
            }
            return Ok(());
        }
        // The window lands in the cell on screen there, and the focus goes with
        // it. When the grid is out of sight, the window goes on into it and the
        // focus comes back.
        let shown = self.layout_manager.layout_engine.active_grid(there) == Some(grid - 1);
        self.handle_event_inner(Event::Command(Command::Reactor(
            ReactorCommand::MoveWindowToDisplay {
                selector: DisplaySelector::Uuid(display),
                window_id: None,
            },
        )));
        self.space_state.command_space = Some(there);
        if !shown {
            self.handle_event_inner(event);
            self.space_state.command_space = Some(here);
            if let Some((_, display)) =
                self.grid_displays().into_iter().find(|&(space, _)| space == here)
            {
                self.focus_display(display);
            }
        }
        Ok(())
    }

    /// After the displays change: a display showing an empty grid that an
    /// earlier display shows too is given the first grid on no display.
    pub(super) fn settle_display_grids(&mut self) {
        if !self.layout_manager.layout_engine.workspaces().grid_enabled() {
            return;
        }
        let displays = self.grid_displays();
        let mut shown = Vec::new();
        for &(space, _) in &displays {
            let Some(grid) = self.layout_manager.layout_engine.active_grid(space) else {
                continue;
            };
            if !shown.contains(&grid) || self.grid_has_windows(space, grid) {
                shown.push(grid);
                continue;
            }
            let free = (0..GRID_COUNT).find(|&grid| {
                !shown.contains(&grid) && displays.iter().all(|&(s, _)| !self.grid_is_on(s, grid))
            });
            let Some(free) = free else { continue };
            let command_space = self.space_state.command_space;
            self.space_state.command_space = Some(space);
            self.handle_event_inner(Event::Command(Command::Layout(LayoutCommand::SwitchToGrid(
                free + 1,
            ))));
            self.space_state.command_space = command_space;
            shown.push(free);
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    use super::super::testing::{
        Apps, make_windows, space_state_event, test_reactor_with_workspace_settings,
    };
    use super::*;
    use crate::actor::app::WindowId;

    fn two_displays() -> (Reactor, SpaceId, SpaceId) {
        crate::sys::screen::TEST_ACTIVE_DISPLAY.with(|display| display.take());
        let settings = crate::common::config::VirtualWorkspaceSettings {
            grid_columns: 3,
            ..Default::default()
        };
        let mut reactor = test_reactor_with_workspace_settings(&settings);
        let (left, right) = (SpaceId::new(1), SpaceId::new(2));
        let frames =
            [0., 1000.].map(|x| CGRect::new(CGPoint::new(x, 0.), CGSize::new(1000., 1000.)));
        reactor.handle_event(space_state_event(frames.to_vec(), vec![Some(left), Some(right)]));
        (reactor, left, right)
    }

    fn grid(reactor: &Reactor, space: SpaceId) -> Option<usize> {
        reactor.layout_manager.layout_engine.active_grid(space)
    }

    #[test]
    fn each_display_comes_up_on_a_grid_of_its_own() {
        let (reactor, left, right) = two_displays();
        assert_eq!((grid(&reactor, left), grid(&reactor, right)), (Some(0), Some(1)));
    }

    #[test]
    fn a_grids_number_leads_to_its_display() {
        let (mut reactor, left, right) = two_displays();
        assert_eq!(reactor.raw_command_space(), Some(left));

        reactor.handle_test_layout_command(LayoutCommand::SwitchToGrid(2));
        assert_eq!(
            reactor.raw_command_space(),
            Some(right),
            "the focus goes to grid 2"
        );
        assert_eq!((grid(&reactor, left), grid(&reactor, right)), (Some(0), Some(1)));

        reactor.handle_test_layout_command(LayoutCommand::SwitchToGrid(3));
        assert_eq!(
            grid(&reactor, right),
            Some(2),
            "a grid on no display comes up here"
        );
        assert_eq!(reactor.raw_command_space(), Some(right));

        reactor.handle_test_layout_command(LayoutCommand::SwitchToGrid(1));
        assert_eq!(reactor.raw_command_space(), Some(left));
        assert_eq!(grid(&reactor, left), Some(0));

        // Grid 2 left the screen empty, so it is on no display now.
        reactor.handle_test_layout_command(LayoutCommand::SwitchToGrid(2));
        assert_eq!(reactor.raw_command_space(), Some(left));
        assert_eq!((grid(&reactor, left), grid(&reactor, right)), (Some(1), Some(2)));
    }

    #[test]
    fn a_window_sent_to_a_grid_goes_to_its_display() {
        for shown in [true, false] {
            let (mut reactor, left, right) = two_displays();
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
            let window = WindowId::new(1, 1);
            assert!(reactor.grid_has_windows(left, 0));
            if !shown {
                // Grid 2 holds a window on the right, out of sight.
                reactor.handle_event(Event::Command(Command::Reactor(
                    ReactorCommand::MoveWindowToDisplay {
                        selector: DisplaySelector::Uuid("test-display-1".into()),
                        window_id: Some(1),
                    },
                )));
                apps.simulate_until_quiet(&mut reactor);
                assert!(reactor.grid_has_windows(right, 1));
                reactor.space_state.command_space = Some(right);
                reactor.handle_test_layout_command(LayoutCommand::SwitchToGrid(3));
                reactor.handle_event(Event::WindowServerFocusChanged(window, left));
                assert_eq!(grid(&reactor, right), Some(2));
                assert_eq!(reactor.raw_command_space(), Some(left));
            }

            reactor.handle_test_layout_command(LayoutCommand::MoveWindowToGrid(2));
            apps.simulate_until_quiet(&mut reactor);
            let current =
                reactor.layout_manager.layout_engine.workspaces().grid_current_cell(right, 1);
            assert_eq!(
                reactor.test_workspace_for_window(right, window),
                current,
                "shown: {shown}"
            );
            let focus = if shown { right } else { left };
            assert_eq!(reactor.raw_command_space(), Some(focus), "shown: {shown}");
        }
    }

    #[test]
    fn a_grid_handed_to_another_display_takes_its_windows_and_layout() {
        let (mut reactor, left, right) = two_displays();
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
        let windows = [WindowId::new(1, 1), WindowId::new(1, 2)];
        let tiles = |reactor: &mut Reactor, space| {
            let workspace =
                reactor.layout_manager.layout_engine.workspaces().active_workspace(space);
            workspace.map(|workspace| reactor.test_workspace_windows(space, workspace))
        };
        assert_eq!(tiles(&mut reactor, left), Some(windows.to_vec()));

        reactor.handle_test_layout_command(LayoutCommand::MoveGridToDisplay {
            grid: 1,
            display: DisplaySelector::Uuid("test-display-1".into()),
        });
        apps.simulate_until_quiet(&mut reactor);
        assert_eq!(grid(&reactor, right), Some(0), "the right display shows grid 1");
        assert_eq!(tiles(&mut reactor, right), Some(windows.to_vec()));
        assert_eq!(
            grid(&reactor, left),
            Some(1),
            "the left display takes grid 2, free now"
        );
        assert!(!reactor.grid_has_windows(left, 0));
        assert_eq!(reactor.raw_command_space(), Some(left), "the focus stays");
        let engine = &reactor.layout_manager.layout_engine;
        let cell = engine.workspaces().active_workspace(right).unwrap();
        let layout = engine.workspaces().active_layout(right, cell);
        assert!(
            layout.is_some(),
            "the cell has a layout for the right display's size"
        );

        // Back again, from the display that has it.
        reactor.handle_test_layout_command(LayoutCommand::MoveGridToDisplay {
            grid: 1,
            display: DisplaySelector::Uuid("test-display-0".into()),
        });
        apps.simulate_until_quiet(&mut reactor);
        assert_eq!(tiles(&mut reactor, left), Some(windows.to_vec()));
        assert_eq!(grid(&reactor, left), Some(0));
    }
}
