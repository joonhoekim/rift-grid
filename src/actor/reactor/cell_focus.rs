//! Focus stays in a cell that closing its last window emptied, as in subline
//! (docs/grids.md).
//!
//! When a window goes, macOS gives the keyboard to another window of the app,
//! or activates another app, and upstream follows that window to its cell. In
//! grid mode the fork keeps the display on the emptied cell instead, and its
//! holder (`ui/home/holder.rs`) takes the keyboard there, so that typed keys
//! do not go to a window in another cell.
//!
//! - The guard: before upstream handles the departure of rift's focused
//!   window from a cell it is alone in, upstream's
//!   `suppress_auto_workspace_switch_until_input` is set, which keeps
//!   `auto_switch_to_window_workspace` from switching until real input (a key
//!   down, a mouse up, a command). macOS gives the keyboard away within a few
//!   milliseconds, before the window leaves the layout (a closing window is
//!   only suspended until CGS reports it closed, a quarter second later).
//! - The hold: once the cell is empty, rift's fallback takes the holder in
//!   place of the desktop (`take_focus`). The desktop is no place for the
//!   focus: making it key activates Finder, which may key one of its windows.

use std::time::{Duration, Instant};

use super::{Event, Reactor};
use crate::actor::app::{WindowId, pid_t};
use crate::actor::home;
use crate::layout_engine::LayoutEngine;
use crate::model::VirtualWorkspaceId;
use crate::sys::screen::{ScreenInfo, SpaceId};
use crate::sys::window_server;

/// How long after a departure macOS's reassignments of the keyboard are
/// answered by the holder; they came within about 310 ms when measured.
const GUARD: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct CellFocus {
    guard: Option<Guard>,
    hold: Option<Hold>,
}

struct Guard {
    space: SpaceId,
    since: Instant,
    /// The guard set upstream's flag; it was not already set (after a wake).
    set_flag: bool,
    /// The holder took the keyboard during the guard.
    keyed: bool,
}

#[derive(Clone, Copy)]
struct Hold {
    space: SpaceId,
    workspace: VirtualWorkspaceId,
}

/// What `cell_focus_after` needs from before the event: a native focus change
/// to this window.
pub(super) type Before = Option<WindowId>;

enum Departing {
    Window(WindowId),
    App(pid_t),
}

impl CellFocus {
    /// Rift's fallback for an empty cell: in grid mode the holder takes the
    /// keyboard, elsewhere the desktop, as upstream.
    pub(super) fn take_focus(
        &mut self,
        engine: &LayoutEngine,
        space: SpaceId,
        screen: &ScreenInfo,
    ) -> bool {
        let workspaces = engine.workspaces();
        let workspace = workspaces.active_workspace(space);
        let Some(workspace) = workspace.filter(|_| workspaces.grid_enabled()) else {
            return window_server::focus_desktop_window(screen);
        };
        self.hold = Some(Hold { space, workspace });
        tracing::debug!(?space, ?workspace, "cell focus: the holder takes the empty cell");
        home::send(home::Event::Hold(screen.id.as_u32()));
        true
    }
}

impl Reactor {
    pub(super) fn cell_focus_before(&mut self, event: &Event) -> Before {
        if !self.layout_manager.layout_engine.workspaces().grid_enabled() {
            return None;
        }
        // The guard ends before the event, so that an activation without
        // input a while after the departure is followed as upstream does.
        self.expire_guard();
        if matches!(event, Event::SystemWoke | Event::SessionDidBecomeActive) {
            // Upstream sets the same flag for these, to be cleared by input
            // only; the guard must not clear it when it ends.
            if let Some(guard) = self.cell_focus.guard.take() {
                self.release_unheld(&guard);
            }
        }
        self.start_guard(event);
        match *event {
            Event::WindowServerFocusChanged(window, _) => Some(window),
            _ => None,
        }
    }

    pub(super) fn cell_focus_after(&mut self, before: Before) {
        if !self.layout_manager.layout_engine.workspaces().grid_enabled() {
            self.end_guard();
            self.end_hold();
            return;
        }
        self.expire_guard();
        if let Some(hold) = self.cell_focus.hold {
            let workspaces = self.layout_manager.layout_engine.workspaces();
            // The display shows another cell, or a window came to this one.
            if workspaces.active_workspace(hold.space) != Some(hold.workspace)
                || !workspaces
                    .windows_in_active_workspace(&self.state.windows, hold.space)
                    .is_empty()
            {
                self.end_hold();
            }
        }

        let Some(window) = before else { return };
        if !self.is_off_screen(window) {
            if self.state.windows.contains_window(window) {
                self.end_hold();
            }
            return;
        }
        // macOS gave the keyboard to a window of another cell as the focused
        // one went. Later moves are the user's (a Dock click, Cmd+Tab):
        // upstream follows them, rift-grid being the app they leave.
        if let Some(guard) = &mut self.cell_focus.guard
            && self.suppress_auto_workspace_switch_until_input
        {
            guard.keyed = true;
            let space = guard.space;
            self.key_holder(space);
        }
    }

    fn expire_guard(&mut self) {
        if self
            .cell_focus
            .guard
            .as_ref()
            .is_some_and(|guard| guard.since.elapsed() > GUARD)
        {
            self.end_guard();
        }
    }

    fn end_guard(&mut self) {
        let Some(guard) = self.cell_focus.guard.take() else {
            return;
        };
        if guard.set_flag {
            self.suppress_auto_workspace_switch_until_input = false;
        }
        self.release_unheld(&guard);
    }

    /// The holder took the keyboard during a guard that no hold followed (the
    /// window stayed, or the user went to another cell): put it away.
    fn release_unheld(&self, guard: &Guard) {
        if guard.keyed && self.cell_focus.hold.is_none() {
            home::send(home::Event::Release);
        }
    }

    /// Start the guard if `event` is the departure of rift's focused window
    /// from a cell it is alone in, the current cell of its display.
    fn start_guard(&mut self, event: &Event) {
        let departing = match *event {
            Event::WindowInvalidated(window, _) | Event::WindowDestroyed(window) => {
                Departing::Window(window)
            }
            Event::WindowClosed(wsid) => match self.state.windows.tracked_window_id(wsid) {
                Some(window) => Departing::Window(window),
                None => return,
            },
            Event::ApplicationTerminated(pid) | Event::ApplicationThreadTerminated(pid) => {
                Departing::App(pid)
            }
            _ => return,
        };
        let engine = &self.layout_manager.layout_engine;
        let Some(focused) = engine.focused_window().or_else(|| {
            // AX suspension clears the focus before the window goes.
            let space = self.workspace_command_space()?;
            let workspaces = engine.workspaces();
            workspaces.last_focused_window(space, workspaces.active_workspace(space)?)
        }) else {
            return;
        };
        let stays = |window: WindowId| match departing {
            Departing::Window(departing) => window != departing,
            Departing::App(pid) => window.pid != pid,
        };
        if stays(focused) {
            return;
        }
        let Some(space) = engine.space_with_window(focused).filter(|s| self.is_space_active(*s))
        else {
            return;
        };
        let workspaces = engine.workspaces();
        let Some(active) = workspaces.active_workspace(space) else {
            return;
        };
        if workspaces.workspace_for_window(&self.state.windows, space, focused) != Some(active)
            || workspaces
                .windows_in_active_workspace(&self.state.windows, space)
                .into_iter()
                .any(stays)
        {
            return;
        }
        // A window's departure comes in several signals (AX, then CGS); a
        // guard already running keeps what it set.
        let previous = self.cell_focus.guard.take();
        let set_flag = !self.suppress_auto_workspace_switch_until_input
            || previous.as_ref().is_some_and(|guard| guard.set_flag);
        let keyed = previous.is_some_and(|guard| guard.keyed);
        self.suppress_auto_workspace_switch_until_input = true;
        self.cell_focus.guard = Some(Guard {
            space,
            since: Instant::now(),
            set_flag,
            keyed,
        });
        tracing::debug!(?focused, ?space, set_flag, "cell focus: guard for a departure");
    }

    /// A window rift knows, in a cell its display does not show.
    fn is_off_screen(&self, window: WindowId) -> bool {
        if !self.state.windows.contains_window(window) {
            return false;
        }
        // A cell's windows not on screen are moved to a corner, maybe of
        // another display: the space they were assigned to comes first.
        let Some(space) = self
            .assigned_space_for_window_id(window)
            .or_else(|| self.best_space_for_window_id(window))
        else {
            return false;
        };
        let workspaces = self.layout_manager.layout_engine.workspaces();
        match (
            workspaces.workspace_for_window(&self.state.windows, space, window),
            workspaces.active_workspace(space),
        ) {
            (Some(workspace), Some(active)) => workspace != active,
            _ => false,
        }
    }

    fn key_holder(&self, space: SpaceId) {
        tracing::debug!(?space, "cell focus: the holder takes the keyboard back");
        if let Some(screen) = self.space_state.screen_by_space(space) {
            home::send(home::Event::Hold(screen.id.as_u32()));
        }
    }

    fn end_hold(&mut self) {
        if let Some(hold) = self.cell_focus.hold.take() {
            tracing::debug!(space = ?hold.space, "cell focus: hold ends");
            home::send(home::Event::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    use super::super::testing::{Apps, make_windows, test_reactor_with_workspace_settings};
    use super::*;
    use crate::actor::app::Quiet;
    use crate::layout_engine::LayoutEvent;

    /// A 3x3 grid: app 1's first window alone in the current cell, the
    /// others of `windows` in the cell before it; app 2 (if `other_app`) with
    /// one window there too.
    fn setup(windows: usize, other_app: bool) -> (Apps, Reactor, VirtualWorkspaceId) {
        let settings = crate::common::config::VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut reactor = test_reactor_with_workspace_settings(&settings);
        let mut apps = Apps::new();
        let screen = CGRect::new(CGPoint::ZERO, CGSize::new(1000., 1000.));
        apps.make_app_and_settle_on_screen(&mut reactor, screen, space(), 1, make_windows(windows));
        if other_app {
            apps.make_app_and_settle(&mut reactor, 2, make_windows(1));
        }
        let active = current(&reactor);
        let other = reactor.test_workspace_ids(space()).into_iter().find(|w| *w != active).unwrap();
        let mut elsewhere: Vec<_> = (2..=windows).map(|idx| WindowId::new(1, idx as u32)).collect();
        if other_app {
            elsewhere.push(WindowId::new(2, 1));
        }
        for window in elsewhere {
            assert!(reactor.assign_test_window_to_workspace(space(), window, other));
        }
        apps.simulate_until_quiet(&mut reactor);
        reactor.handle_event(Event::ApplicationGloballyActivated(1));
        reactor.handle_event(Event::ApplicationMainWindowChanged(1, Some(first()), Quiet::No));
        reactor.handle_test_layout_event(LayoutEvent::WindowFocused(space(), first()));
        reactor.handle_event(Event::WindowServerFocusChanged(first(), space()));
        apps.simulate_until_quiet(&mut reactor);
        assert_eq!(current(&reactor), active);
        assert_eq!(
            reactor.layout_manager.layout_engine.focused_window(),
            Some(first())
        );
        (apps, reactor, active)
    }

    fn space() -> SpaceId { SpaceId::new(1) }

    fn first() -> WindowId { WindowId::new(1, 1) }

    fn current(reactor: &Reactor) -> VirtualWorkspaceId {
        reactor
            .layout_manager
            .layout_engine
            .workspaces()
            .active_workspace(space())
            .unwrap()
    }

    /// macOS keys `window`, a window of another cell, as it does when the
    /// focused one goes.
    fn macos_keys(apps: &mut Apps, reactor: &mut Reactor, window: WindowId) {
        reactor.handle_event(Event::ApplicationMainWindowChanged(
            window.pid,
            Some(window),
            Quiet::No,
        ));
        reactor.handle_event(Event::WindowServerFocusChanged(window, space()));
        apps.simulate_until_quiet(reactor);
    }

    /// Run A: W1 closes, macOS keys W2 of the same app in another cell before
    /// CGS reports W1 closed.
    fn close_first(apps: &mut Apps, reactor: &mut Reactor) {
        let wsid = reactor.test_window_server_id(first());
        reactor.handle_event(Event::WindowInvalidated(
            first(),
            super::super::WindowInvalidationSource::AxDestroyedNotification,
        ));
        macos_keys(apps, reactor, WindowId::new(1, 2));
        reactor.handle_event(Event::WindowClosed(wsid));
        apps.simulate_until_quiet(reactor);
    }

    #[test]
    fn without_a_departure_native_focus_follows_the_window_to_its_cell() {
        let (mut apps, mut reactor, active) = setup(2, false);
        macos_keys(&mut apps, &mut reactor, WindowId::new(1, 2));
        assert_ne!(
            current(&reactor),
            active,
            "upstream follows the same app's window"
        );
    }

    #[test]
    fn closing_the_last_window_keeps_the_cell_and_holds_it() {
        let (mut apps, mut reactor, active) = setup(2, false);
        close_first(&mut apps, &mut reactor);
        assert_eq!(current(&reactor), active, "the emptied cell stays");
        assert!(reactor.cell_focus.hold.is_some(), "the holder has the keyboard");
        assert_eq!(reactor.layout_manager.layout_engine.focused_window(), None);
    }

    #[test]
    fn quitting_the_app_of_the_last_window_keeps_the_cell() {
        let (mut apps, mut reactor, active) = setup(1, true);
        let wsid = reactor.test_window_server_id(first());
        reactor.handle_event(Event::WindowClosed(wsid));
        reactor.handle_event(Event::ApplicationGloballyActivated(2));
        macos_keys(&mut apps, &mut reactor, WindowId::new(2, 1));
        reactor.handle_event(Event::ApplicationActivated(2, Quiet::No));
        apps.simulate_until_quiet(&mut reactor);
        assert_eq!(current(&reactor), active);
    }

    #[test]
    fn a_window_left_in_the_cell_starts_no_guard() {
        let (mut apps, mut reactor, _) = setup(1, false);
        apps.make_app_and_settle(&mut reactor, 3, make_windows(1));
        assert_eq!(
            reactor.test_workspace_for_window(space(), WindowId::new(3, 1)),
            Some(current(&reactor))
        );
        reactor.handle_event(Event::ApplicationGloballyActivated(1));
        reactor.handle_test_layout_event(LayoutEvent::WindowFocused(space(), first()));
        reactor.handle_event(Event::WindowInvalidated(
            first(),
            super::super::WindowInvalidationSource::AxDestroyedNotification,
        ));
        assert!(reactor.cell_focus.guard.is_none());
    }

    #[test]
    fn after_input_an_activation_follows_the_app_to_its_cell() {
        let (mut apps, mut reactor, active) = setup(2, false);
        close_first(&mut apps, &mut reactor);
        // A Dock click: rift-grid was in front with the holder.
        reactor.handle_event(Event::MouseUp(crate::actor::drag::MouseButton::Left));
        reactor.handle_event(Event::ApplicationGloballyActivated(1));
        macos_keys(&mut apps, &mut reactor, WindowId::new(1, 2));
        reactor.handle_event(Event::ApplicationActivated(1, Quiet::No));
        apps.simulate_until_quiet(&mut reactor);
        assert_ne!(current(&reactor), active);
        assert!(reactor.cell_focus.hold.is_none());
    }

    #[test]
    fn a_window_opened_in_the_held_cell_ends_the_hold() {
        let (mut apps, mut reactor, active) = setup(2, false);
        close_first(&mut apps, &mut reactor);
        assert!(reactor.cell_focus.hold.is_some());
        apps.make_app_and_settle(&mut reactor, 3, make_windows(1));
        assert_eq!(current(&reactor), active);
        assert!(reactor.cell_focus.hold.is_none());
    }

    #[test]
    fn a_wake_keeps_upstreams_suppression_after_the_guard() {
        let (_apps, mut reactor, _) = setup(2, false);
        reactor.handle_event(Event::WindowInvalidated(
            first(),
            super::super::WindowInvalidationSource::AxDestroyedNotification,
        ));
        assert!(reactor.cell_focus.guard.as_ref().is_some_and(|guard| guard.set_flag));
        reactor.handle_event(Event::SystemWoke);
        assert!(reactor.cell_focus.guard.is_none());
        assert!(
            reactor.suppress_auto_workspace_switch_until_input,
            "upstream's, until input"
        );
    }

    #[test]
    fn the_guard_ends_its_suppression_across_the_departures_signals() {
        let (mut apps, mut reactor, _) = setup(2, false);
        // AX's invalidation, then CGS's close: two departure signals.
        close_first(&mut apps, &mut reactor);
        assert!(reactor.suppress_auto_workspace_switch_until_input);
        reactor.cell_focus.guard.as_mut().unwrap().since -= GUARD * 2;
        reactor.handle_event(Event::ApplicationDeactivated(2));
        assert!(reactor.cell_focus.guard.is_none());
        assert!(
            !reactor.suppress_auto_workspace_switch_until_input,
            "an activation without input is followed again"
        );
    }
}
