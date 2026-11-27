//! The reactor's part of home (docs/home.md): which display the `home`
//! command opens it on, and closing it when the view moves.

use super::Reactor;
use crate::actor::home;
use crate::actor::wm_controller::WmEvent;
use crate::model::VirtualWorkspaceId;
use crate::sys::screen::SpaceId;

/// Each display's Space and its current cell, while home is open.
pub(super) type HomeView = Option<Vec<(SpaceId, Option<VirtualWorkspaceId>)>>;

impl Reactor {
    /// `home`: on the focused display, the menu bar's, asked now. Not
    /// `command_context_space`, which falls back to another display's Space
    /// when the focused one shows a fullscreen app; then home is told no
    /// display, which closes it if it is open and opens nothing.
    pub(super) fn home_command(&mut self) {
        if self.is_mission_control_active() {
            return;
        }
        let display = self.home_display();
        match &self.communication_manager.wm_sender {
            Some(wm) => wm.send(WmEvent::Home { display }),
            None => home::send(home::Event::Toggle(display)),
        }
    }

    /// The focused display's id, `None` while it shows a fullscreen app.
    fn home_display(&self) -> Option<u32> {
        let focused = crate::sys::screen::active_menu_bar_display_uuid();
        let screens = &self.space_state.screens;
        let screen = focused
            .and_then(|uuid| screens.iter().find(|screen| screen.display_uuid == uuid))
            // No answer, or one that matches none: with one display, that one.
            .or_else(|| (screens.len() == 1).then(|| &screens[0]))?;
        screen.space.is_some().then(|| screen.id.as_u32())
    }

    /// What home's closing compares against, taken before an event.
    pub(super) fn home_before(&self) -> HomeView {
        crate::ui::home::is_open().then(|| self.home_view())
    }

    /// Close home if the event moved any display to another cell.
    pub(super) fn home_after(&self, before: HomeView) {
        if let Some(before) = before
            && before != self.home_view()
        {
            home::send(home::Event::Close);
        }
    }

    fn home_view(&self) -> Vec<(SpaceId, Option<VirtualWorkspaceId>)> {
        let workspaces = self.layout_manager.layout_engine.workspaces();
        self.space_state
            .screens
            .iter()
            .filter_map(|screen| screen.space)
            .map(|space| (space, workspaces.active_workspace(space)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    use super::super::testing::{space_state_event, test_reactor_with_workspace_settings};
    use super::*;
    use crate::actor::reactor::{Command, Event};
    use crate::layout_engine::{Direction, LayoutCommand};

    fn grid_reactor(spaces: Vec<Option<SpaceId>>) -> Reactor {
        let settings = crate::common::config::VirtualWorkspaceSettings {
            grid_layout: Some(crate::common::config::GridLayout::new(3, 3)),
            ..Default::default()
        };
        let mut reactor = test_reactor_with_workspace_settings(&settings);
        let frame = CGRect::new(CGPoint::new(0., 0.), CGSize::new(1000., 1000.));
        reactor.handle_event(space_state_event(vec![frame], spaces));
        reactor
    }

    #[test]
    fn a_move_to_another_cell_changes_what_home_compares() {
        let mut reactor = grid_reactor(vec![Some(SpaceId::new(1))]);
        let before = reactor.home_view();
        reactor.handle_event(Event::Command(Command::Layout(LayoutCommand::GoToCell(
            Direction::Right,
        ))));
        assert_ne!(reactor.home_view(), before);
        let before = reactor.home_view();
        reactor.handle_event(Event::Command(Command::Layout(LayoutCommand::GoToCell(
            Direction::Right,
        ))));
        reactor.handle_event(Event::Command(Command::Layout(LayoutCommand::GoToCell(
            Direction::Left,
        ))));
        reactor.handle_event(Event::Command(Command::Layout(LayoutCommand::GoToCell(
            Direction::Right,
        ))));
        assert_eq!(reactor.home_view(), before, "back in the same cell");
    }

    #[test]
    fn a_display_showing_a_fullscreen_app_gets_no_home() {
        let reactor = grid_reactor(vec![Some(SpaceId::new(1))]);
        assert!(reactor.home_display().is_some());
        let reactor = grid_reactor(vec![None]);
        assert_eq!(reactor.home_display(), None);
    }
}
