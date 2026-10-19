//! The grid Overview as the screen drawing back (`grid_style = "zoom"`): opened,
//! steered and closed here, in place of the card overview.

use std::time::Duration;

use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGDisplayBounds, CGPreflightScreenCaptureAccess};

use super::{Event, Input, MissionControlActor};
use crate::actor::reactor;
use crate::common::config::GridStyle;
use crate::layout_engine::{Direction, LayoutCommand};
use crate::model::VirtualWorkspaceId;
use crate::sys::dispatch::DispatchExt;
use crate::sys::screen::SpaceId;
use crate::common::config::GRID_COUNT;
use crate::ui::grid_motion::{
    BarButton, BarGroup, GridBar, GridOverview, Note, OverviewCell, OverviewStages,
    OverviewWindow, Shareable, Shown,
};

/// How far the pointer goes before pressing a grid button becomes dragging it.
const DRAG_START: f64 = 4.0;

/// How long the first opening may wait for the list of capturable windows, and
/// each opening for the capture of the screen.
const CONTENT_WAIT: Duration = Duration::from_millis(250);
const SCREEN_WAIT: Duration = Duration::from_millis(200);
/// How old that list may be before an opening fetches it again, for the next.
const CONTENT_AGE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct GridZoom {
    shareable: Shareable,
    generation: u64,
    open: Option<Open>,
}

struct Open {
    overview: GridOverview<VirtualWorkspaceId>,
    display: String,
    /// The display's frame, which window frames are taken relative to.
    frame: CGRect,
    /// Windows newer than the list of capturable ones, asked for when it comes.
    unknown: Vec<(u32, CGSize)>,
    /// Coming in to a cell; the overlay goes when that ends.
    entering: bool,
    /// The grid buttons, a group per display.
    bar: Option<GridBar>,
    drag: Option<BarDrag>,
}

/// A grid button pressed: its group and place, and where the pointer went down.
struct BarDrag {
    button: (usize, usize),
    from: CGPoint,
    moved: bool,
}

/// The grid on a display as the reactor has it.
struct GridState {
    active: (usize, usize),
    columns: usize,
    cells: Vec<OverviewCell<VirtualWorkspaceId>>,
    focused: Option<u32>,
}

impl MissionControlActor {
    /// Handle `event` when the zoomed grid Overview takes it; hand it back
    /// otherwise.
    pub(super) fn grid_zoom(&mut self, event: Event) -> Option<Event> {
        let open = self.grid_zoom.open.is_some();
        match event {
            Event::GridZoom(note) => self.grid_zoom_note(note),
            Event::ShowAll if open => self.grid_zoom_step_back(),
            Event::ShowAll
                if self.session.is_none()
                    && self.settings.enabled
                    && self.settings.grid_style == GridStyle::Zoom =>
            {
                if !self.grid_zoom_open() {
                    // Not a grid, or nothing to capture with: the cards instead.
                    return Some(Event::ShowAll);
                }
            }
            Event::Input(input) if open => self.grid_zoom_input(input),
            Event::Dismiss if open => self.grid_zoom_close(),
            Event::Configure(_) => {
                self.grid_zoom_close();
                return Some(event);
            }
            Event::Changed | Event::RefreshCurrentWorkspace if open => self.grid_zoom_sync(),
            Event::PumpPreviews | Event::StartPreviews(_) if open => {}
            event => return Some(event),
        }
        None
    }

    fn grid_zoom_input(&mut self, input: Input) {
        if self.grid_zoom.open.as_ref().is_none_or(|open| open.entering) {
            return;
        }
        if self.grid_zoom_bar_input(&input) {
            return;
        }
        let Some(open) = self.grid_zoom.open.as_mut() else { return };
        let overview = &mut open.overview;
        let command = match input {
            Input::Left => LayoutCommand::GridFocus(Direction::Left),
            Input::Right => LayoutCommand::GridFocus(Direction::Right),
            Input::Up => LayoutCommand::GridFocus(Direction::Up),
            Input::Down => LayoutCommand::GridFocus(Direction::Down),
            Input::Cycle(true) => LayoutCommand::NextWindow,
            Input::Cycle(false) => LayoutCommand::PrevWindow,
            Input::Move(point) => {
                overview.select_at(point);
                return;
            }
            Input::Activate => return self.grid_zoom_enter(),
            // The input actor reports a click as the button going up.
            Input::Click(point) | Input::PointerUp(point) => {
                if overview.select_at(point) {
                    self.grid_zoom_enter();
                }
                return;
            }
            Input::Dismiss if overview.shows_whole() && !self.settings.grid_fit => {
                return overview.show_cell();
            }
            Input::Dismiss => {
                overview.select_active();
                return self.grid_zoom_enter();
            }
            _ => return,
        };
        // Queries share the reactor's queue, so the sync sees the result.
        self.reactor.send(reactor::Event::Command(reactor::Command::Layout(command)));
        self.grid_zoom_sync();
    }

    /// Handle `input` when it is for the grid buttons: pressing, dragging and
    /// letting go of one. Dropped on another display's group, the grid goes to
    /// that display; let go where it was pressed, it is a click.
    fn grid_zoom_bar_input(&mut self, input: &Input) -> bool {
        let Some(open) = self.grid_zoom.open.as_mut() else { return false };
        let Some(bar) = open.bar.as_mut() else { return false };
        let local = |p: CGPoint| CGPoint::new(p.x - open.frame.origin.x, p.y - open.frame.origin.y);
        match *input {
            Input::PointerDown(p) => {
                let Some(button) = bar.button_at(local(p)) else { return false };
                open.drag = Some(BarDrag { button, from: p, moved: false });
            }
            Input::PointerDrag(p) => {
                let Some(drag) = open.drag.as_mut() else { return false };
                drag.moved |= (p.x - drag.from.x).hypot(p.y - drag.from.y) > DRAG_START;
                if drag.moved {
                    bar.drag(drag.button, local(p));
                    open.overview.flush();
                }
            }
            Input::PointerUp(p) | Input::Click(p) => {
                let drag = open.drag.take();
                let Some((button, moved)) = drag
                    .map(|d| (d.button, d.moved))
                    .or_else(|| bar.button_at(local(p)).map(|b| (b, false)))
                else {
                    return false;
                };
                bar.end_drag();
                open.overview.flush();
                let Some(pressed) = bar.button(button).cloned() else { return true };
                if moved {
                    let Some(group) = bar.group_at(local(p)).filter(|&g| g != button.0) else {
                        return true;
                    };
                    if pressed.new {
                        return true;
                    }
                    let display = bar.groups()[group].display.clone();
                    self.reactor.send(reactor::Event::Command(reactor::Command::Layout(
                        LayoutCommand::MoveGridToDisplay {
                            grid: pressed.grid + 1,
                            display: reactor::DisplaySelector::Uuid(display),
                        },
                    )));
                    self.grid_zoom_sync();
                } else {
                    self.grid_zoom_bar_click(&pressed);
                }
            }
            Input::Move(_) | Input::Scroll { .. } if open.drag.is_some() => {}
            _ => return open.drag.is_some(),
        }
        true
    }

    /// A grid button clicked: the grid shows on its display, and the Overview
    /// stays, following it when it is this display's.
    fn grid_zoom_bar_click(&mut self, button: &BarButton) {
        if button.shown != Shown::No {
            return;
        }
        let command = LayoutCommand::SwitchToGrid(button.grid + 1);
        self.reactor.send(reactor::Event::Command(reactor::Command::Layout(command)));
        self.grid_zoom_sync();
    }

    /// The grid buttons: for each display, the Overview's own first, the grids
    /// on it (shown, or holding windows); then "+" for the first grid on none.
    fn grid_zoom_bar_groups(&self, display: &str) -> Vec<BarGroup> {
        let mut displays = self.reactor.query_displays();
        displays.sort_by_key(|d| d.info.display_uuid != display);
        let mut used = [false; GRID_COUNT];
        let mut groups: Vec<BarGroup> = displays
            .iter()
            .filter_map(|d| {
                let space = d.info.space?;
                let here = d.info.display_uuid == display;
                let workspaces = self.reactor.query_workspaces(Some(space));
                let shown = workspaces.iter().find(|w| w.is_active).and_then(|w| w.grid);
                let mut grids: Vec<usize> = workspaces
                    .iter()
                    .filter(|w| w.window_count > 0 || w.grid == shown)
                    .filter_map(|w| w.grid)
                    .filter(|&g| g < GRID_COUNT)
                    .collect();
                grids.dedup();
                let buttons = grids
                    .into_iter()
                    .map(|grid| {
                        used[grid] = true;
                        let shown = match () {
                            _ if Some(grid) != shown => Shown::No,
                            _ if here => Shown::Here,
                            _ => Shown::Elsewhere,
                        };
                        BarButton { grid, shown, new: false }
                    })
                    .collect();
                Some(BarGroup { display: d.info.display_uuid.clone(), buttons })
            })
            .collect();
        if let Some(free) = used.iter().position(|&used| !used)
            && let Some(first) = groups.first_mut().filter(|g| g.display == display)
        {
            first.buttons.push(BarButton { grid: free, shown: Shown::No, new: true });
        }
        groups
    }

    /// Draw the grid buttons again when they changed.
    fn grid_zoom_bar_refresh(&mut self) {
        let Some(display) = self.grid_zoom.open.as_ref().map(|open| open.display.clone()) else {
            return;
        };
        let groups = self.grid_zoom_bar_groups(&display);
        let Some(open) = self.grid_zoom.open.as_mut() else { return };
        if open.drag.is_some() || open.bar.as_ref().is_some_and(|bar| bar.groups() == groups) {
            return;
        }
        if let Some(bar) = open.bar.take() {
            bar.remove();
        }
        if groups.iter().any(|g| !g.buttons.is_empty()) {
            let scale = open.overview.screen_layer().contentsScale();
            open.bar = Some(GridBar::new(open.overview.screen_layer(), groups, scale));
        }
        open.overview.flush();
    }

    /// The grid on `space`, whose display is at `frame`, as it is now.
    fn grid_zoom_state(&self, space: SpaceId, frame: CGRect) -> Option<GridState> {
        let workspaces = self.reactor.query_workspaces(Some(space));
        let active = workspaces.iter().find(|w| w.is_active)?;
        let grid = active.grid?;
        let in_grid: Vec<_> = workspaces.iter().filter(|w| w.grid == Some(grid)).collect();
        let columns = in_grid.iter().filter_map(|w| w.grid_cell).map(|c| c.1 + 1).max()?;
        let focused = active.layout_focus.and_then(|id| {
            let window = active.windows.iter().find(|w| w.id == id)?;
            Some(window.info.sys_id?.as_u32())
        });
        let labels = self.settings.grid_labels;
        let cells = in_grid
            .iter()
            .filter_map(|w| {
                let windows = w
                    .windows
                    .iter()
                    .filter_map(|window| {
                        let id = window.info.sys_id?.as_u32();
                        let at = window.layout_frame.unwrap_or(window.info.frame);
                        let title = &window.info.title;
                        let label = match window.app_name.as_deref() {
                            Some(app) if !title.is_empty() && title != app => {
                                Some(format!("{app} · {title}"))
                            }
                            Some(app) => Some(app.to_owned()),
                            None => Some(title.clone()),
                        };
                        Some(OverviewWindow {
                            id,
                            frame: CGRect::new(
                                CGPoint::new(at.origin.x - frame.origin.x, at.origin.y - frame.origin.y),
                                at.size,
                            ),
                            floating: window.is_floating,
                            label: label.filter(|l| labels && !l.is_empty()),
                        })
                    })
                    .collect();
                Some(OverviewCell { cell: w.grid_cell?, windows, key: w.workspace_id })
            })
            .collect();
        Some(GridState { active: active.grid_cell?, columns, cells, focused })
    }

    /// Open on the display in use, drawing back from its active cell. False
    /// when that is not a grid cell or the screen cannot be captured.
    fn grid_zoom_open(&mut self) -> bool {
        if !CGPreflightScreenCaptureAccess() {
            return false;
        }
        let displays = self.reactor.query_displays();
        let Some(display) = displays.iter().find(|d| d.is_active_context) else { return false };
        let Some(space) = display.info.space else { return false };
        let frame = display.info.frame;
        let Some(state) = self.grid_zoom_state(space, frame) else { return false };

        let zoom = &mut self.grid_zoom;
        if !zoom.shareable.has_content() {
            zoom.shareable.fetch_now(CONTENT_WAIT);
        }
        let scale = display.info.backing_scale.max(1.0);
        let display_id = display.info.id.as_u32();
        let bounds = CGDisplayBounds(display_id);
        let local = CGRect::new(
            CGPoint::new(frame.origin.x - bounds.origin.x, frame.origin.y - bounds.origin.y),
            frame.size,
        );
        let Some(screen) =
            zoom.shareable.capture_display_now(display_id, local, scale, &[], SCREEN_WAIT)
        else {
            return false;
        };

        zoom.generation += 1;
        let gap = self.settings.grid_gap * frame.size.width.min(frame.size.height);
        let stages = OverviewStages {
            zoom: self.settings.grid_zoom.clamp(0.2, 0.99),
            fit: self.settings.grid_fit,
            follow: self.settings.grid_follow,
        };
        let mut overview = match GridOverview::open(
            zoom.generation,
            frame,
            scale,
            &screen,
            state.columns,
            state.active,
            state.cells,
            gap,
            stages,
            crate::ui::grid_motion::motion(),
        ) {
            Ok(overview) => overview,
            Err(error) => {
                tracing::debug!(?error, "Zoomed grid overview unavailable");
                return false;
            }
        };
        overview.set_active(state.active, state.focused);

        match zoom.shareable.wallpaper(display_id, local) {
            Some(image) => overview.set_wallpaper(image.clone()),
            None => {
                let tx = self.tx.clone();
                zoom.shareable.capture_wallpaper(display_id, local, scale, move |note| {
                    tx.send(Event::GridZoom(note))
                });
            }
        }
        let windows = overview.windows();
        zoom.open = Some(Open {
            overview,
            display: display.info.display_uuid.clone(),
            frame,
            unknown: Vec::new(),
            entering: false,
            bar: None,
            drag: None,
        });
        self.grid_zoom_bar_refresh();
        self.grid_zoom_capture(windows);
        self.input_tx.send(super::super::input::Request::SetMissionControlActive(true));
        crate::ui::grid_motion::set_overview_open(true);
        true
    }

    /// Ask for captures of `windows`; those newer than the list of capturable
    /// windows are captured once it is fetched again.
    fn grid_zoom_capture(&mut self, windows: Vec<(u32, CGSize)>) {
        let zoom = &mut self.grid_zoom;
        let Some(open) = zoom.open.as_mut() else { return };
        let tx = self.tx.clone();
        let send = move |note| tx.send(Event::GridZoom(note));
        let generation = open.overview.generation();
        for (id, size) in windows {
            if !zoom.shareable.capture_window(id, size, 1.0, generation, send.clone()) {
                open.unknown.push((id, size));
            }
        }
        if !open.unknown.is_empty() || zoom.shareable.wants(CONTENT_AGE) {
            zoom.shareable.refresh(send);
        }
    }

    /// Show the grid as it is now: the cell and window focused, windows that
    /// came or went, and another grid switched to.
    fn grid_zoom_sync(&mut self) {
        let Some(open) = self.grid_zoom.open.as_ref().filter(|open| !open.entering) else {
            return;
        };
        let (display, frame) = (open.display.clone(), open.frame);
        let displays = self.reactor.query_displays();
        let Some(space) =
            displays.iter().find(|d| d.info.display_uuid == display).and_then(|d| d.info.space)
        else {
            return;
        };
        let Some(state) = self.grid_zoom_state(space, frame) else { return };
        let Some(open) = self.grid_zoom.open.as_mut() else { return };
        let wanted = open.overview.update(state.cells);
        open.overview.set_active(state.active, state.focused);
        self.grid_zoom_bar_refresh();
        self.grid_zoom_capture(wanted);
    }

    /// Step back to the whole grid, or come in from it.
    fn grid_zoom_step_back(&mut self) {
        match self.grid_zoom.open.as_mut().filter(|open| !open.entering) {
            Some(open) if !open.overview.shows_whole() => open.overview.show_whole(),
            _ => self.grid_zoom_enter(),
        }
    }

    /// Come in to the cell selected, switching to it, and close when there.
    fn grid_zoom_enter(&mut self) {
        let Some(open) = self.grid_zoom.open.as_mut().filter(|open| !open.entering) else {
            return;
        };
        open.entering = true;
        open.drag = None;
        if let Some(bar) = &open.bar {
            bar.hide();
        }
        let length = open.overview.enter();
        if !open.overview.selected_is_active()
            && let Some(&workspace) = open.overview.selected()
        {
            self.reactor.send(reactor::Event::OverviewSelectWorkspace {
                display: open.display.clone(),
                workspace,
            });
        }
        // A little longer: windows shown again redraw a moment after they arrive.
        let after = Duration::from_secs_f64(length) + Duration::from_millis(120);
        let when = dispatchr::time::Time::new_after(
            dispatchr::time::Time::NOW,
            i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
        );
        dispatchr::queue::main().after_f_s(
            when,
            (self.tx.clone(), open.overview.generation()),
            |(tx, generation)| tx.send(Event::GridZoom(Note::End(generation))),
        );
    }

    fn grid_zoom_note(&mut self, note: Note) {
        let zoom = &mut self.grid_zoom;
        match note {
            Note::Content(content) => {
                zoom.shareable.set_content(content.0);
                if let Some(open) = zoom.open.as_mut() {
                    let tx = self.tx.clone();
                    let send = move |note| tx.send(Event::GridZoom(note));
                    let generation = open.overview.generation();
                    for (id, size) in std::mem::take(&mut open.unknown) {
                        zoom.shareable.capture_window(id, size, 1.0, generation, send.clone());
                    }
                }
            }
            Note::Wallpaper { display, rect, image } => {
                let image = image.map(|i| i.0);
                if let (Some(open), Some(image)) = (zoom.open.as_mut(), &image) {
                    open.overview.set_wallpaper(image.clone());
                }
                zoom.shareable.set_wallpaper(display, rect, image);
            }
            Note::Window { generation, window, image } => {
                if let Some(open) =
                    zoom.open.as_ref().filter(|open| open.overview.generation() == generation)
                    && let Some(image) = image
                {
                    open.overview.set_window(window, &image.0);
                }
            }
            Note::End(generation) => {
                if zoom.open.as_ref().is_some_and(|open| open.overview.generation() == generation) {
                    self.grid_zoom_close();
                }
            }
            _ => {}
        }
    }

    fn grid_zoom_close(&mut self) {
        if self.grid_zoom.open.take().is_some() {
            self.input_tx.send(super::super::input::Request::SetMissionControlActive(false));
            crate::ui::grid_motion::set_overview_open(false);
        }
    }
}
