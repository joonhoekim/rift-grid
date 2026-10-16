//! Window borders: what the reactor shows them and when. Drawing is in
//! `crate::ui::borders`.

use objc2_core_foundation::CGRect;

use super::{Event, Reactor};
use crate::actor::app::WindowId;
use crate::common::collections::HashMap;
use crate::common::config::BorderSettings;
use crate::ui::borders::BorderStyle;
use crate::sys::screen::SpaceId;

/// Window borders and what they were last drawn from.
#[derive(Default)]
pub(super) struct BorderManager {
    pub(super) borders: Option<crate::ui::borders::Borders>,
    /// The visible windows, frames and focus the borders show.
    pub(super) signature: Option<u64>,
    pub(super) focus: Option<WindowId>,
    /// The layout's focus as last seen, to notice when rift moves focus.
    pub(super) layout_focus: Option<WindowId>,
    /// A window rift just moved focus to, shown until the system reports it as
    /// the main window or this expires.
    pub(super) pending: Option<(WindowId, std::time::Instant)>,
    /// Windows were reordered since the borders last learned what covers them.
    pub(super) reordered: bool,
    /// When the borders were last reordered. Reorders that soon after are that
    /// ordering's own echo.
    pub(super) last_reorder: Option<std::time::Instant>,
    /// The parsed style, with the settings it was parsed from.
    style: Option<(BorderSettings, BorderStyle)>,
}


/// Events whose visibility changes the borders cannot see in the reactor's own
/// state, so they redraw even when nothing they track has changed.
pub(super) fn changes_visibility(event: &Event) -> bool {
    matches!(
        event,
        Event::WindowServerHidden(_)
            | Event::WindowServerUnhidden(_)
            | Event::ApplicationActivated(..)
            | Event::ApplicationGloballyActivated(_)
            | Event::ApplicationTerminated(_)
    )
}

impl BorderManager {
    fn style(&mut self, settings: &BorderSettings) -> BorderStyle {
        match &self.style {
            Some((read, style)) if read == settings => *style,
            _ => {
                let style = BorderStyle::new(settings);
                self.style = Some((settings.clone(), style));
                style
            }
        }
    }
}

impl Reactor {
    /// Handle the borders' own events; true when `event` was one.
    pub(super) fn handle_border_event(&mut self, event: &Event) -> bool {
        match event {
            Event::BorderSettle(generation) => {
                if let Some(borders) = &mut self.border_manager.borders {
                    borders.settle(*generation);
                }
                true
            }
            Event::WindowsReordered => {
                crate::actor::window_notify::reorder_seen();
                let echo = self
                    .border_manager
                    .last_reorder
                    .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(150));
                if !echo {
                    self.border_manager.reordered = true;
                    self.update_borders(false);
                }
                true
            }
            _ => false,
        }
    }

    /// Fill in what lies above each border's window: every normal-level window in
    /// front of it on the active spaces, from any app but rift.
    fn cover_border_targets(&self, targets: &mut [crate::ui::borders::BorderTarget]) {
        use crate::sys::window_server::{WindowServerId, get_windows, space_window_list_for_connection};
        let spaces: Vec<u64> = self.iter_active_spaces().map(|space| space.get()).collect();
        let front_to_back: Vec<WindowServerId> = space_window_list_for_connection(&spaces, 0, false)
            .into_iter()
            .map(WindowServerId::new)
            .collect();
        let own = std::process::id() as i32;
        // The query's own order is not the one asked for; keep front to back.
        let infos: HashMap<WindowServerId, _> =
            get_windows(&front_to_back).into_iter().map(|w| (w.id, w)).collect();
        let stack: Vec<(WindowServerId, CGRect)> = front_to_back
            .iter()
            .filter_map(|id| infos.get(id))
            .filter(|w| w.layer == 0 && w.pid != own)
            .map(|w| (w.id, w.frame))
            .collect();
        for target in targets {
            let Some(at) = stack.iter().position(|(id, _)| *id == target.window) else {
                continue;
            };
            target.covered_by = stack[..at].iter().map(|(_, frame)| *frame).collect();
        }
    }

    /// Keep a border on every visible window of the active workspaces, the focused
    /// one in the focused colour. Cheap when nothing they show has changed.
    pub(super) fn update_borders(&mut self, visibility_changed: bool) {
        use std::hash::{Hash, Hasher};

        use crate::ui::borders::{BorderTarget, Borders};
        let settings = &self.config.settings.ui.borders;
        if !settings.enabled {
            self.border_manager = BorderManager::default();
            return;
        }
        let previous = self.border_manager.style.as_ref().map(|(_, style)| *style);
        let style = self.border_manager.style(settings);
        // The system's main window is the focus: clicks, app switches and moves
        // within an app all reach it. But when rift moves focus to another app's
        // window, the main window first passes through that app's previous one;
        // for that short while the window rift is focusing stands in.
        let main = self.main_window();
        let layout_focus = self.layout_manager.layout_engine.focused_window();
        let manager = &mut self.border_manager;
        if layout_focus != manager.layout_focus {
            manager.layout_focus = layout_focus;
            manager.pending = layout_focus
                .filter(|&window| Some(window) != main)
                .map(|window| (window, std::time::Instant::now()));
        }
        let focused = match manager.pending {
            Some((window, since))
                if Some(window) != main
                    && since.elapsed() < std::time::Duration::from_millis(600) =>
            {
                Some(window)
            }
            _ => {
                manager.pending = None;
                main
            }
        };

        let spaces: Vec<SpaceId> = self.iter_active_spaces().collect();
        let mut candidates = Vec::new();
        for space in spaces {
            if self.is_fullscreen_space(space) {
                continue;
            }
            for wid in self
                .layout_manager
                .layout_engine
                .workspaces()
                .windows_in_active_workspace(&self.state.windows, space)
            {
                let Some(window) = self.state.windows.window(wid).filter(|w| w.is_admitted())
                else {
                    continue;
                };
                let Some(wsid) =
                    self.state.windows.record(wid).and_then(|record| record.window_server_id())
                else {
                    continue;
                };
                candidates.push(BorderTarget {
                    window: wsid,
                    frame: window.frame_monotonic,
                    focused: Some(wid) == focused,
                    covered_by: Vec::new(),
                });
            }
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for target in &candidates {
            target.window.as_u32().hash(&mut hasher);
            target.focused.hash(&mut hasher);
            for value in [
                target.frame.origin.x,
                target.frame.origin.y,
                target.frame.size.width,
                target.frame.size.height,
            ] {
                value.to_bits().hash(&mut hasher);
            }
        }
        let signature = hasher.finish();
        let reordered = std::mem::take(&mut self.border_manager.reordered);
        if !visibility_changed
            && !reordered
            && previous == Some(style)
            && self.border_manager.signature == Some(signature)
        {
            return;
        }
        self.border_manager.signature = Some(signature);
        // A newly focused window was raised, possibly over the others' borders.
        let reorder = reordered || self.border_manager.focus != focused;
        self.border_manager.focus = focused;
        if reorder {
            self.border_manager.last_reorder = Some(std::time::Instant::now());
        }
        candidates.retain(|t| crate::sys::window_server::window_is_ordered_in(t.window));
        self.cover_border_targets(&mut candidates);
        let settle = self
            .border_manager
            .borders
            .get_or_insert_with(Borders::default)
            .update(style, &candidates, reorder);
        if let Some((generation, after)) = settle
            && let Some(tx) = self.communication_manager.events_tx.clone()
        {
            std::thread::spawn(move || {
                std::thread::sleep(after);
                tx.send(Event::BorderSettle(generation));
            });
        }
    }
}
