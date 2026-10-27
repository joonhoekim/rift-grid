//! Moves between grid cells and grids, shown: what the reactor notes before a
//! command and finds changed after it, and when the slide and the grid map it
//! asks `crate::ui::grid_motion` for start and end.

use std::time::Duration;

use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGDisplayBounds, CGPreflightScreenCaptureAccess};
use tracing::debug;

use super::{Command, Event, Reactor};
use crate::actor::app::WindowId;
use crate::common::collections::HashMap;
use crate::layout_engine::LayoutCommand;
use crate::model::VirtualWorkspaceId;
use crate::sys::screen::SpaceId;
use crate::ui::grid_motion::{
    Carried, Hud, HudView, Image, Motion, Note, Pan, SendBox, Shareable, Step, Tile,
};

/// How long after a move its slide waits for the captures of the cell it goes to.
const CAPTURE_WAIT: Duration = Duration::from_millis(130);
/// How much longer than the screen's capture the carried windows' may take.
const HOLD_WAIT: Duration = Duration::from_millis(40);
/// How long a move may wait for the capture of the screen it leaves. It takes
/// longer while the cell moved to is being captured too.
const SCREEN_WAIT: Duration = Duration::from_millis(150);
/// How old the list of capturable windows may get before it is fetched again.
const CONTENT_AGE: Duration = Duration::from_secs(20);
/// How long after a slide its overlay starts to fade out, and how long that takes.
const REDRAW_WAIT: Duration = Duration::from_millis(40);
const FADE_OUT: Duration = Duration::from_millis(150);
/// How far into a slide a carried window that changes size is captured again,
/// as a share of the slide: its app has redrawn it at its new size by then.
const RECAPTURE_AT: f64 = 0.3;
/// The shortest cross-fade from a carried window's old look to its new one.
const SHORTEST_FADE: Duration = Duration::from_millis(80);

#[derive(Default)]
pub(super) struct GridMotionManager {
    shareable: Shareable,
    generation: u64,
    pan: Option<Pan>,
    pending: Option<Pending>,
    recapture: Option<Recapture>,
    hud: Option<Hud>,
}

/// The carried windows of a slide that change size, to capture again once they
/// have it, and when the slide ends.
struct Recapture {
    generation: u64,
    windows: Vec<(u32, CGSize)>,
    end: std::time::Instant,
}

// Made empty with the reactor; its windows, layers and captures are only made and
// used on the reactor's thread.
unsafe impl Send for GridMotionManager {}

/// A slide waiting for the captures of the cell it goes to.
struct Pending {
    generation: u64,
    step: Step,
    /// The cell moved to, and how many columns (and rows) its grid has.
    cell: (usize, usize),
    columns: usize,
    workspace: VirtualWorkspaceId,
    /// The usable part of the display, which the overlay covers.
    screen: CGRect,
    display: u32,
    carried: Vec<(WindowId, CGRect)>,
    images: HashMap<u32, Option<Image>>,
    /// Each window's size when its capture was asked for.
    sizes: HashMap<u32, CGSize>,
    waiting: usize,
    wallpaper: bool,
}

/// The active cell of a display, and its windows, before a command.
pub(super) struct Before {
    space: SpaceId,
    workspace: VirtualWorkspaceId,
    windows: Vec<(WindowId, CGRect)>,
}

/// Commands that can move to another cell or grid.
fn moves_cells(command: &LayoutCommand) -> bool {
    matches!(
        command,
        LayoutCommand::GridFocus(_)
            | LayoutCommand::GridMove(_)
            | LayoutCommand::GridCell(_)
            | LayoutCommand::GridCenter
            | LayoutCommand::SwitchToGrid(_)
            | LayoutCommand::FocusRecentWindow
            | LayoutCommand::NextWorkspace(_)
            | LayoutCommand::PrevWorkspace(_)
            | LayoutCommand::SwitchToWorkspace(_)
            | LayoutCommand::SwitchToLastWorkspace
            | LayoutCommand::MoveWindowToWorkspace { follow: true, .. }
    )
}

impl Reactor {
    fn grid_motion_send(&self) -> impl Fn(Note) + Send + Clone + 'static {
        let tx = self.communication_manager.events_tx.clone();
        move |note| {
            if let Some(tx) = &tx {
                tx.send(Event::GridMotion(note));
            }
        }
    }

    fn grid_motion_later(&self, after: Duration, note: Note) {
        self.send_later(after, Event::GridMotion(note));
    }

    /// Note the active cell before a command that may move to another one.
    pub(super) fn grid_motion_before(&mut self, event: &Event) -> Option<Before> {
        crate::ui::grid_motion::set_motion(&self.config.settings.ui.grid_motion);
        self.grid_motion_prefetch();
        let Event::Command(Command::Layout(command)) = event else { return None };
        let settings = &self.config.settings.ui.grid_motion;
        // Captures and timers report back through the reactor's own queue.
        if !(settings.pan || settings.hud)
            || self.communication_manager.events_tx.is_none()
            || !moves_cells(command)
            || !self.layout_manager.layout_engine.workspaces().grid_enabled()
            || self.is_mission_control_active()
        {
            return None;
        }
        let space = self.command_context_space()?;
        let workspace = self.layout_manager.layout_engine.workspaces().active_workspace(space)?;
        let windows = self.state.windows.workspace_windows(workspace)
            .into_iter()
            .filter_map(|wid| self.state.windows.window(wid).map(|w| (wid, w.frame_monotonic)))
            .collect();
        Some(Before { space, workspace, windows })
    }

    /// Keep the list of capturable windows fresh enough for the next slide, so
    /// that the first move after a while slides as well.
    fn grid_motion_prefetch(&mut self) {
        if !self.config.settings.ui.grid_motion.pan
            || self.communication_manager.events_tx.is_none()
            || !self.grid_motion_manager.shareable.wants(CONTENT_AGE)
            || !self.layout_manager.layout_engine.workspaces().grid_enabled()
        {
            return;
        }
        if !CGPreflightScreenCaptureAccess() {
            self.grid_motion_manager.shareable.defer();
            return;
        }
        let send = self.grid_motion_send();
        self.grid_motion_manager.shareable.refresh(send);
    }

    /// Capture each display's wallpaper ahead of the first slide on it.
    fn grid_motion_warm(&mut self) {
        let send = self.grid_motion_send();
        for screen in &self.space_state.screens {
            let display = screen.id.as_u32();
            let bounds = CGDisplayBounds(display);
            let local = CGRect::new(
                CGPoint::new(
                    screen.frame.origin.x - bounds.origin.x,
                    screen.frame.origin.y - bounds.origin.y,
                ),
                screen.frame.size,
            );
            let shareable = &self.grid_motion_manager.shareable;
            if shareable.wallpaper(display, local).is_none() {
                shareable.capture_wallpaper(
                    display,
                    local,
                    screen.backing_scale.max(1.0),
                    send.clone(),
                );
            }
        }
    }

    /// The command ran: if it moved to another cell, show it. Runs before the
    /// windows are moved, so the screen still shows the cell left.
    pub(super) fn grid_motion_after(&mut self, before: Before) {
        let Before { space, workspace: from, windows } = before;
        let store = self.layout_manager.layout_engine.workspaces();
        let Some(to) = store.active_workspace(space).filter(|&to| to != from) else { return };
        let ids = store.workspace_ids(space);
        let (Some(from_index), Some(to_index)) = (
            ids.iter().position(|&id| id == from),
            ids.iter().position(|&id| id == to),
        ) else {
            return;
        };
        let (Some(from_grid), Some(to_grid), Some(from_cell), Some(to_cell)) = (
            store.grid_of(from_index),
            store.grid_of(to_index),
            store.grid_cell(from_index),
            store.grid_cell(to_index),
        ) else {
            return;
        };
        let Some(screen) = self.space_state.screen_by_space(space).cloned() else { return };
        let step = if from_grid == to_grid {
            Step::Cells {
                dx: to_cell.1 as isize - from_cell.1 as isize,
                dy: to_cell.0 as isize - from_cell.0 as isize,
            }
        } else {
            Step::Grid { forward: to_grid > from_grid }
        };
        self.grid_motion_manager.generation += 1;
        let generation = self.grid_motion_manager.generation;
        let settings = self.config.settings.ui.grid_motion.clone();
        let display = screen.id.as_u32();
        let scale = screen.backing_scale.max(1.0);

        let view = (settings.hud && !crate::ui::grid_motion::overview_open()).then(|| {
            let per_grid = ids
                .iter()
                .enumerate()
                .filter(|(i, _)| store.grid_of(*i) == Some(to_grid))
                .collect::<Vec<_>>();
            let rows = per_grid.iter().filter_map(|(i, _)| store.grid_cell(*i)).map(|c| c.0 + 1).max();
            let cols = per_grid.iter().filter_map(|(i, _)| store.grid_cell(*i)).map(|c| c.1 + 1).max();
            if let (Some(rows), Some(columns)) = (rows, cols) {
                let occupied = per_grid
                    .iter()
                    .map(|&(_, &id)| {
                        !self.state.windows.workspace_windows(id).is_empty()
                    })
                    .collect();
                Some(HudView {
                    grid: to_grid,
                    rows,
                    columns,
                    cell: to_cell,
                    occupied,
                    from: (from_grid == to_grid).then_some(from_cell),
                })
            } else {
                None
            }
        });

        // The slide first: its capture of the screen must not show the map.
        if settings.pan
            && settings.duration_ms > 0.0
            && !self.is_fullscreen_space(space)
            && !crate::ui::grid_motion::overview_open()
        {
            let carried: Vec<_> = windows
                .into_iter()
                .filter(|(wid, _)| self.state.windows.workspace_for_window(*wid) == Some(to))
                .collect();
            self.begin_pan(generation, step, to, to_cell, &screen, carried);
        }

        if let Some(view) = view.flatten() {
            self.show_grid_hud(generation, display, screen.frame, scale, &view);
            self.grid_motion_later(
                Duration::from_secs_f64(settings.hud_ms.clamp(100.0, 5000.0) / 1000.0),
                Note::HudFade(generation),
            );
        }
    }

    fn show_grid_hud(
        &mut self,
        generation: u64,
        display: u32,
        frame: CGRect,
        scale: f64,
        view: &HudView,
    ) {
        let manager = &mut self.grid_motion_manager;
        if let Some(hud) = manager.hud.as_mut().filter(|hud| hud.fits(display, view)) {
            hud.update(generation, view);
            return;
        }
        manager.hud = None;
        match Hud::show(generation, display, frame, scale, view) {
            Ok(hud) => manager.hud = Some(hud),
            Err(error) => debug!(?error, "Grid map unavailable"),
        }
    }

    fn begin_pan(
        &mut self,
        generation: u64,
        step: Step,
        to: VirtualWorkspaceId,
        cell: (usize, usize),
        screen: &crate::sys::screen::ScreenInfo,
        carried: Vec<(WindowId, CGRect)>,
    ) {
        if !self.grid_motion_manager.shareable.has_content() || !CGPreflightScreenCaptureAccess()
        {
            self.grid_motion_manager.pan = None;
            self.grid_motion_manager.pending = None;
            return;
        }
        // A move during a slide carries it on; a slide still waiting starts now.
        let going =
            self.grid_motion_manager.pan.as_ref().is_some_and(|pan| pan.covers(screen.frame));
        if going {
            if let Some(earlier) = self.grid_motion_manager.pending.as_ref().map(|p| p.generation) {
                self.run_pan(earlier);
            }
        } else {
            self.grid_motion_manager.pan = None;
        }
        self.grid_motion_manager.pending = None;
        let started = std::time::Instant::now();
        let display = screen.id.as_u32();
        let scale = screen.backing_scale.max(1.0);
        let bounds = CGDisplayBounds(display);
        let visible = screen.frame;
        let local = CGRect::new(
            CGPoint::new(visible.origin.x - bounds.origin.x, visible.origin.y - bounds.origin.y),
            visible.size,
        );
        let sys_id = |reactor: &Self, wid: WindowId| {
            reactor.state.windows.window(wid).and_then(|w| w.info.sys_id).map(|id| id.as_u32())
        };
        let leave_out: Vec<u32> =
            carried.iter().filter_map(|(wid, _)| sys_id(self, *wid)).collect();
        // The cell moved to is captured meanwhile; its windows are where they are.
        let send = self.grid_motion_send();
        let mut images = HashMap::default();
        let mut sizes = HashMap::default();
        let mut waiting = 0;
        let mut unknown = false;
        let windows = self.state.windows.workspace_windows(to);
        let manager = &self.grid_motion_manager;
        for wid in windows {
            let Some(window) = self.state.windows.window(wid) else { continue };
            let Some(id) = window.info.sys_id.map(|id| id.as_u32()) else { continue };
            let size = window.frame_monotonic.size;
            sizes.insert(id, size);
            // At one point a pixel: they are seen only in motion, and it is faster.
            if manager.shareable.capture_window(id, size, 1.0, generation, send.clone()) {
                waiting += 1;
            } else {
                images.insert(id, None);
                unknown = true;
            }
        }
        if unknown {
            // A window newer than the list: there next time.
            self.grid_motion_manager.shareable.expire();
        }
        let manager = &mut self.grid_motion_manager;
        let wallpaper = manager.shareable.wallpaper(display, local).is_none()
            && manager.shareable.capture_wallpaper(display, local, scale, send.clone());
        if let Some(pan) = manager.pan.as_mut().filter(|_| going) {
            pan.continue_as(generation);
        } else {
            if let Some(hud) = &manager.hud {
                hud.set_visible(false);
            }
            // The carried windows are captured alongside, to hold their place
            // on the overlay until they move: the screen is captured without them.
            let held: Vec<_> = carried
                .iter()
                .filter_map(|&(wid, before)| {
                    let id = self.state.windows.window(wid)?.info.sys_id?.as_u32();
                    let rx = manager.shareable.capture_window_soon(id, before.size, 1.0)?;
                    Some((id, before, rx))
                })
                .collect();
            let from = manager.shareable.capture_display_now(
                display,
                local,
                scale,
                &leave_out,
                SCREEN_WAIT,
            );
            let deadline = std::time::Instant::now() + HOLD_WAIT;
            let held: Vec<_> = held
                .into_iter()
                .map(|(id, before, rx)| {
                    let wait = deadline.saturating_duration_since(std::time::Instant::now());
                    let image = rx.recv_timeout(wait).ok().flatten().map(|image| image.0);
                    if image.is_some() {
                        images.insert(id, image.clone());
                        sizes.insert(id, before.size);
                    }
                    let from = CGRect::new(
                        CGPoint::new(
                            before.origin.x - visible.origin.x,
                            before.origin.y - visible.origin.y,
                        ),
                        before.size,
                    );
                    Carried {
                        window: Some(id),
                        from,
                        to: from,
                        image,
                        captured: before.size,
                    }
                })
                .collect();
            if let Some(hud) = &manager.hud {
                hud.set_visible(true);
            }
            let Some(from) = from else {
                debug!("Grid slide skipped: the screen was not captured in time");
                return;
            };
            match Pan::show(generation, visible, scale, &from, &held) {
                Ok(pan) => manager.pan = Some(pan),
                Err(error) => {
                    debug!(?error, "Grid slide unavailable");
                    return;
                }
            }
        }
        self.grid_motion_manager.pending = Some(Pending {
            generation,
            step,
            cell,
            columns: self.config.virtual_workspaces.grid_columns,
            workspace: to,
            screen: visible,
            display,
            carried,
            images,
            sizes,
            waiting,
            wallpaper,
        });
        // Never before the windows are given their new frames, which happens
        // after this returns: the slide places the cell's windows by them.
        if waiting == 0 && !wallpaper {
            self.grid_motion_send()(Note::Start(generation));
        } else {
            let waited = started.elapsed();
            self.grid_motion_later(CAPTURE_WAIT.saturating_sub(waited), Note::Start(generation));
        }
    }

    /// Slide with the captures that have arrived.
    fn run_pan(&mut self, generation: u64) {
        let manager = &mut self.grid_motion_manager;
        if manager.pending.as_ref().is_none_or(|p| p.generation != generation) {
            return;
        }
        let pending = manager.pending.take().unwrap();
        if manager.pan.as_ref().is_none_or(|pan| pan.generation() != generation) {
            return;
        }
        let origin = pending.screen.origin;
        let local = |frame: CGRect| {
            CGRect::new(
                CGPoint::new(frame.origin.x - origin.x, frame.origin.y - origin.y),
                frame.size,
            )
        };
        let engine = &self.layout_manager.layout_engine;
        let mut images = pending.images;
        let sizes = pending.sizes;
        let captured = |id: Option<u32>, frame: CGRect| {
            id.and_then(|id| sizes.get(&id).copied()).unwrap_or(frame.size)
        };
        let mut windows: Vec<(bool, CGRect, Option<Image>, CGSize)> = self.state.windows.workspace_windows(pending.workspace)
            .into_iter()
            .filter(|wid| !pending.carried.iter().any(|(c, _)| c == wid))
            .filter_map(|wid| {
                let window = self.state.windows.window(wid)?;
                let id = window.info.sys_id.map(|id| id.as_u32());
                let image = id.and_then(|id| images.remove(&id)).flatten();
                let frame = local(window.frame_monotonic);
                Some((engine.is_window_floating(wid), frame, image, captured(id, frame)))
            })
            .collect();
        // Tiles do not overlap; floating windows go above them.
        windows.sort_by_key(|(floating, ..)| *floating);
        let tiles: Vec<_> = windows
            .into_iter()
            .map(|(_, frame, image, captured)| Tile { frame, image, captured })
            .collect();
        let carried: Vec<_> = pending
            .carried
            .iter()
            .filter_map(|(wid, before)| {
                let window = self.state.windows.window(*wid)?;
                let id = window.info.sys_id.map(|id| id.as_u32());
                let image = id.and_then(|id| images.remove(&id)).flatten();
                let to = local(window.frame_monotonic);
                Some(Carried {
                    window: id,
                    from: local(*before),
                    to,
                    image,
                    captured: captured(id, *before),
                })
            })
            .collect();
        let bounds = CGDisplayBounds(pending.display);
        let wallpaper_rect = CGRect::new(
            CGPoint::new(origin.x - bounds.origin.x, origin.y - bounds.origin.y),
            pending.screen.size,
        );
        let wallpaper = manager.shareable.wallpaper(pending.display, wallpaper_rect);
        let motion = Motion::new(&self.config.settings.ui.grid_motion);
        let Some(pan) = manager.pan.as_mut() else { return };
        let shape = (pending.columns, pending.cell);
        let duration =
            pan.run(pending.step, shape, wallpaper.map(|w| &**w), &tiles, &carried, motion);
        // The overlay fades out a moment after the slide, as windows shown again
        // redraw at their new size a moment after they arrive.
        let settled = Duration::from_secs_f64(duration) + REDRAW_WAIT;
        pan.fade_out(settled.as_secs_f64(), FADE_OUT.as_secs_f64());
        self.grid_motion_later(settled + FADE_OUT, Note::End(generation));
        // A carried window that changes size shows its old look as it goes; it
        // turns into how its app draws it at the new size, captured partway.
        let windows: Vec<_> = carried
            .iter()
            .filter(|c| {
                (c.from.size.width - c.to.size.width).abs() >= 2.0
                    || (c.from.size.height - c.to.size.height).abs() >= 2.0
            })
            .filter_map(|c| Some((c.window?, c.to.size)))
            .collect();
        self.grid_motion_manager.recapture = (!windows.is_empty()).then(|| Recapture {
            generation,
            windows,
            end: std::time::Instant::now() + Duration::from_secs_f64(duration),
        });
        if self.grid_motion_manager.recapture.is_some() {
            self.grid_motion_later(
                Duration::from_secs_f64(duration * RECAPTURE_AT),
                Note::Recapture(generation),
            );
        }
    }

    /// Handle the slide's and the map's own events, or hand `event` back.
    pub(super) fn handle_grid_motion_event(&mut self, event: Event) -> Result<(), Event> {
        let Event::GridMotion(note) = event else { return Err(event) };
        let manager = &mut self.grid_motion_manager;
        match note {
            Note::Content(content) => {
                manager.shareable.set_content(content.0);
                self.grid_motion_warm();
            }
            Note::Wallpaper { display, rect, image } => {
                manager.shareable.set_wallpaper(display, rect, image.map(|i| i.0));
                let ready = manager.pending.as_mut().and_then(|p| {
                    if p.display != display || !p.wallpaper {
                        return None;
                    }
                    p.wallpaper = false;
                    (p.waiting == 0).then_some(p.generation)
                });
                if let Some(generation) = ready {
                    self.run_pan(generation);
                }
            }
            Note::Window { generation, window, image } => {
                let ready =
                    manager.pending.as_mut().filter(|p| p.generation == generation).and_then(|p| {
                        // A failed capture keeps one already held.
                        match image {
                            Some(image) => _ = p.images.insert(window, Some(image.0)),
                            None => _ = p.images.entry(window).or_insert(None),
                        }
                        p.waiting = p.waiting.saturating_sub(1);
                        (p.waiting == 0 && !p.wallpaper).then_some(generation)
                    });
                if let Some(generation) = ready {
                    self.run_pan(generation);
                }
            }
            Note::Start(generation) => self.run_pan(generation),
            Note::End(generation) => {
                if manager.pan.as_ref().is_some_and(|pan| pan.generation() == generation) {
                    manager.pan = None;
                }
            }
            Note::HudFade(generation) => {
                if let Some(hud) = manager.hud.as_ref().filter(|h| h.generation() == generation) {
                    hud.fade();
                    self.grid_motion_later(Duration::from_millis(200), Note::HudGone(generation));
                }
            }
            Note::HudGone(generation) => {
                if manager.hud.as_ref().is_some_and(|h| h.generation() == generation) {
                    manager.hud = None;
                }
            }
            // The Overview's own; it never comes here.
            Note::OverviewGone(_) => {}
            Note::Recapture(generation) => {
                let send = self.grid_motion_send();
                let manager = &self.grid_motion_manager;
                let Some(recapture) =
                    manager.recapture.as_ref().filter(|r| r.generation == generation)
                else {
                    return Ok(());
                };
                for &(window, size) in &recapture.windows {
                    let send = send.clone();
                    manager.shareable.window_capture(window, size, 1.0, move |image| {
                        send(Note::Carried {
                            generation,
                            window,
                            image: image.map(SendBox),
                        })
                    });
                }
            }
            Note::Carried { generation, window, image } => {
                let Some(image) = image else { return Ok(()) };
                let Some(end) = manager
                    .recapture
                    .as_ref()
                    .filter(|r| r.generation == generation)
                    .map(|r| r.end)
                else {
                    return Ok(());
                };
                if let Some(pan) = manager.pan.as_ref().filter(|p| p.generation() == generation) {
                    let left = end.saturating_duration_since(std::time::Instant::now());
                    pan.show_carried_as(window, &image.0, left.max(SHORTEST_FADE).as_secs_f64());
                }
            }
        }
        Ok(())
    }
}
