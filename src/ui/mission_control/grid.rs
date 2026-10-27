//! The grid overview: every workspace a cell at its row and column, in two stages
//! (the active cell and its neighbours, then the whole grid), following the real
//! focus.

use super::*;
use crate::common::config::{GRID_COUNT, GridFollow};

#[path = "grid_bar.rs"]
mod grid_bar;
pub(super) use grid_bar::*;

// The app and title over a grid tile's bottom left corner, and its distance from it.
pub(super) const LABEL_ROW: f64 = 22.0;
pub(super) const LABEL_INSET: f64 = 6.0;
// Grid cards are bare previews with a hairline gap between tiles.
pub(super) const BARE_INSET: f64 = 1.5;

/// Where a card shows its preview: the whole card for a bare card, otherwise
/// fitted above the caption.
pub(super) fn card_image_frame(size: CGSize, bare: bool) -> CGRect {
    if bare {
        return rect(
            BARE_INSET,
            BARE_INSET,
            (size.width - 2.0 * BARE_INSET).max(1.0),
            (size.height - 2.0 * BARE_INSET).max(1.0),
        );
    }
    preview_frame(size)
}

/// How long after rift moves the focus the overview shows rift's target rather
/// than the system's focus, which still reports the window before it.
const RIFT_FOCUS_BRIDGE: std::time::Duration = std::time::Duration::from_millis(600);

/// The selection a grid overview shows: the active cell, and the window the
/// system reports focused when it is in that cell. Right after rift moves the
/// focus (`rift_moved`) the system has not caught up, so the layout's focus
/// stands in for it.
pub(super) fn grid_focus_selection(
    workspaces: &[RuntimeWorkspaceData],
    rift_moved: bool,
) -> Option<Selection> {
    let ws = workspaces.iter().find(|ws| ws.is_active)?;
    let system = ws.windows.iter().find(|w| w.is_focused).map(|w| w.id);
    Some(Selection {
        workspace: ws.index,
        window: if rift_moved {
            ws.layout_focus.or(system)
        } else {
            system.or(ws.layout_focus)
        },
    })
}

/// The overview shows one grid, the one on screen, every cell of it in place,
/// empty or not.
pub(super) fn keep_grid_on_screen(workspaces: &mut Vec<RuntimeWorkspaceData>) {
    let on_screen = workspaces.iter().find(|ws| ws.is_active).and_then(|ws| ws.grid);
    if on_screen.is_some() {
        workspaces.retain(|ws| ws.grid == on_screen);
    }
}

/// The grid buttons drawn on display `here`: a row per display, `displays`
/// as (UUID, name, workspaces), with the grids on it (shown, or holding
/// windows), and "+" ending `here`'s row for the first grid on none.
pub(super) fn bar_groups(
    displays: &[(String, String, Vec<RuntimeWorkspaceData>)],
    here: &str,
) -> Vec<BarGroup> {
    let mut used = [false; GRID_COUNT];
    let mut groups: Vec<BarGroup> = displays
        .iter()
        .map(|(display, name, workspaces)| {
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
                        _ if display == here => Shown::Here,
                        _ => Shown::Elsewhere,
                    };
                    BarButton { grid, shown, new: false }
                })
                .collect();
            BarGroup { display: display.clone(), name: name.clone(), buttons }
        })
        .collect();
    if let Some(free) = used.iter().position(|&used| !used)
        && let Some(row) = groups.iter_mut().find(|g| g.display == here)
    {
        row.buttons.push(BarButton { grid: free, shown: Shown::No, new: true });
    }
    groups
}

/// The grid buttons for display `here`, as the reactor has the grids now.
pub(super) fn grid_bar_groups(reactor: &ReactorHandle, here: &str) -> Vec<BarGroup> {
    let displays: Vec<_> = reactor
        .query_displays()
        .into_iter()
        .filter_map(|d| {
            let space = d.info.space?;
            let name = d.info.name.clone().unwrap_or_else(|| "Display".to_owned());
            Some((d.info.display_uuid.clone(), name, reactor.query_workspaces(Some(space))))
        })
        .collect();
    bar_groups(&displays, here)
}

/// Rows and columns when the workspaces form a grid.
pub(super) fn grid_shape(workspaces: &[RuntimeWorkspaceData]) -> Option<(usize, usize)> {
    workspaces
        .iter()
        .map(|ws| ws.grid_cell)
        .try_fold((0, 0), |(rows, columns), cell| {
            let (row, column) = cell?;
            Some((rows.max(row + 1), columns.max(column + 1)))
        })
        .filter(|&(rows, columns)| rows > 0 && columns > 0)
}

/// How far a grid overview has stepped back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GridStage {
    /// The active cell at the grid zoom, its neighbours around it.
    Cell,
    /// The whole grid.
    Whole,
}

/// How a grid overview looks, from the overview settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GridLook {
    /// Gap between cells as a fraction of the display's shorter side.
    gap: f64,
    labels: bool,
    /// A cell's size in the first stage, as a fraction of the display.
    zoom: f64,
    stage: GridStage,
    /// The whole grid is the only stage.
    fit: bool,
    follow: GridFollow,
}

impl GridLook {
    pub(super) fn new(settings: &MissionControlSettings) -> Self {
        Self {
            gap: settings.grid_gap.clamp(0.0, 0.1),
            labels: settings.grid_labels,
            zoom: settings.grid_zoom.clamp(0.2, 0.99),
            stage: if settings.grid_fit {
                GridStage::Whole
            } else {
                GridStage::Cell
            },
            fit: settings.grid_fit,
            follow: settings.grid_follow,
        }
    }

    pub(super) fn gap_px(self, display: CGSize) -> f64 { self.gap * display.width.min(display.height) }

    pub(super) fn labels(self) -> bool { self.labels }
}

/// The frame of one grid cell. Cells keep the display's aspect ratio and lie `gap`
/// apart. In the first stage a cell is `zoom` of the display and the camera, a
/// point in cells, is at the center; the whole grid is fitted and centered, never
/// larger than `zoom`.
pub(super) fn grid_cell_frame(
    overview: CGSize,
    display: CGSize,
    (rows, columns): (usize, usize),
    (row, column): (usize, usize),
    gap: f64,
    look: GridLook,
    (camera_row, camera_column): (f64, f64),
) -> CGRect {
    let (rows_f, columns_f) = (rows as f64, columns as f64);
    let aspect = display.width / display.height;
    let zoomed = display.width * look.zoom;
    if look.stage == GridStage::Cell {
        let (width, height) = (zoomed, zoomed / aspect);
        return rect(
            (overview.width - width) / 2.0 + (column as f64 - camera_column) * (width + gap),
            (overview.height - height) / 2.0 + (row as f64 - camera_row) * (height + gap),
            width,
            height,
        );
    }
    let available_width = (overview.width - (columns_f + 1.0) * gap).max(1.0);
    let available_height = (overview.height - (rows_f + 1.0) * gap).max(1.0);
    let width = (available_width / columns_f).min(available_height / rows_f * aspect).min(zoomed);
    let height = width / aspect;
    let total_width = columns_f * width + (columns_f - 1.0) * gap;
    let total_height = rows_f * height + (rows_f - 1.0) * gap;
    rect(
        (overview.width - total_width) / 2.0 + column as f64 * (width + gap),
        (overview.height - total_height) / 2.0 + row as f64 * (height + gap),
        width,
        height,
    )
}

/// Where the first stage's camera goes when the active cell is `active`: on it when
/// it centers or has no place yet, otherwise it stays unless the active cell would
/// not show whole, a margin from the edges, and then moves only that far. A cell
/// too large to show whole with its margins is centered.
pub(super) fn follow_camera(
    camera: Option<(f64, f64)>,
    (row, column): (usize, usize),
    overview: CGSize,
    display: CGSize,
    look: GridLook,
) -> (f64, f64) {
    let active = (row as f64, column as f64);
    let Some((camera_row, camera_column)) = camera.filter(|_| look.follow == GridFollow::Reveal)
    else {
        return active;
    };
    let gap = look.gap_px(display);
    let margin = gap.max(8.0);
    let width = display.width * look.zoom;
    let height = width * display.height / display.width;
    // Along one axis: the camera, the active cell, a cell's size and the view's.
    let axis = |camera: f64, cell: f64, size: f64, view: f64| {
        let stride = size + gap;
        let room = view / 2.0 - size / 2.0 - margin;
        if room < 0.0 {
            return cell;
        }
        let offset = (cell - camera) * stride;
        if offset.abs() <= room {
            camera
        } else {
            camera + (offset - room * offset.signum()) / stride
        }
    };
    (
        axis(camera_row, active.0, height, overview.height),
        axis(camera_column, active.1, width, overview.width),
    )
}

/// A workspace's windows inside its cell, scaled from the display.
fn project_windows(
    ws: &RuntimeWorkspaceData,
    display: CGRect,
    frame: CGRect,
    scale: f64,
    x: f64,
) -> (Vec<WindowProjection>, crate::actor::drag::DragScene) {
    let mut windows: Vec<_> = ws
        .windows
        .iter()
        .enumerate()
        .filter_map(|(source, window)| {
            let w = window.layout_frame.unwrap_or(window.info.frame);
            let projected = rect(
                x + (w.origin.x - display.origin.x) * scale,
                (w.origin.y - display.origin.y) * scale,
                w.size.width * scale,
                w.size.height * scale,
            );
            intersects(projected, rect(0.0, 0.0, frame.size.width, frame.size.height)).then_some(
                WindowProjection {
                    id: window.id,
                    floating: window.is_floating,
                    source,
                    frame: projected,
                },
            )
        })
        .collect();
    windows.sort_by_key(|w| w.floating);
    let scene = crate::actor::drag::DragScene {
        action_override: (ws.layout_mode == "stack")
            .then_some(crate::layout_engine::WindowDropAction::Swap),
        source_neighbors: [false; 4],
        targets: windows
            .iter()
            .rev()
            .filter_map(|w| {
                (!w.floating).then_some(crate::actor::drag::DragSceneTarget {
                    window: w.id,
                    space: ws.space,
                    frame: w.frame,
                })
            })
            .collect(),
    };
    (windows, scene)
}

/// The active cell, or the middle one when no cell is active.
fn active_cell(workspaces: &[RuntimeWorkspaceData], shape: (usize, usize)) -> (usize, usize) {
    workspaces
        .iter()
        .find(|ws| ws.is_active)
        .and_then(|ws| ws.grid_cell)
        .unwrap_or((shape.0 / 2, shape.1 / 2))
}

/// Every cell of the grid at once, each a scaled copy of the display, with the
/// camera on the active cell.
#[cfg(test)]
pub(super) fn project_grid(
    display: CGRect,
    overview: CGSize,
    workspaces: &[RuntimeWorkspaceData],
    shape: (usize, usize),
    look: GridLook,
) -> Vec<WorkspaceProjection> {
    let (row, column) = active_cell(workspaces, shape);
    let area = rect(0.0, 0.0, overview.width, overview.height);
    project_grid_at(display, area, workspaces, shape, look, (row as f64, column as f64))
}

/// Every cell of the grid at once in `area`, a part of the overview, the first
/// stage's camera at `camera`.
pub(super) fn project_grid_at(
    display: CGRect,
    area: CGRect,
    workspaces: &[RuntimeWorkspaceData],
    shape: (usize, usize),
    look: GridLook,
    camera: (f64, f64),
) -> Vec<WorkspaceProjection> {
    if display.size.height <= 0.0 || display.size.width <= 0.0 {
        return Vec::new();
    }
    let gap = look.gap_px(display.size);
    workspaces
        .iter()
        .enumerate()
        .filter_map(|(source, ws)| {
            let mut frame =
                grid_cell_frame(area.size, display.size, shape, ws.grid_cell?, gap, look, camera);
            frame.origin.x += area.origin.x;
            frame.origin.y += area.origin.y;
            let scale = frame.size.width / display.size.width;
            let (windows, scene) = project_windows(ws, display, frame, scale, 0.0);
            Some(WorkspaceProjection {
                source,
                cell: true,
                frame,
                scene,
                windows,
            })
        })
        .collect()
}

/// Where `layer` is on screen, partway through a glide or at rest.
pub(super) fn shown_frame(layer: &CALayer) -> CGRect {
    unsafe { layer.presentationLayer() }.map_or_else(|| layer.frame(), |shown| shown.frame())
}

/// Move `layer`, already at its new frame, smoothly from `from`, usually its `shown_frame`.
pub(super) fn glide(layer: &CALayer, from: CGRect) {
    let to = layer.frame();
    if from == to {
        return;
    }
    let position = CABasicAnimation::animationWithKeyPath(Some(
        &objc2_foundation::NSString::from_str("position"),
    ));
    let bounds = CABasicAnimation::animationWithKeyPath(Some(
        &objc2_foundation::NSString::from_str("bounds"),
    ));
    unsafe {
        position.setFromValue(Some(&*objc2_foundation::NSValue::valueWithPoint(CGPoint::new(
            from.origin.x + from.size.width / 2.0,
            from.origin.y + from.size.height / 2.0,
        ))));
        bounds.setFromValue(Some(&*objc2_foundation::NSValue::valueWithRect(rect(
            0.0,
            0.0,
            from.size.width,
            from.size.height,
        ))));
    }
    for (key, animation) in [("position", &position), ("bounds", &bounds)] {
        animation.setPreferredFrameRateRange(CAFrameRateRange::new(60.0, 120.0, 120.0));
        animation.setDuration(0.2);
        layer.addAnimation_forKey(animation, Some(&objc2_foundation::NSString::from_str(key)));
    }
}

impl DisplayOverview {
    pub(super) fn grid(&self) -> Option<(usize, usize)> { grid_shape(&self.workspaces) }

    /// The menu bar's height on this display.
    fn menu_bar(&self) -> f64 { (self.info.frame.origin.y - self.bounds.origin.y).max(0.0) }

    /// Redraw the grid buttons when they changed.
    pub(super) fn set_grid_bar(&mut self, groups: Vec<BarGroup>) {
        if self.grid_row.as_ref().is_some_and(|bar| bar.groups() == groups) {
            return;
        }
        if let Some(bar) = self.grid_row.take() {
            bar.remove();
        }
        if self.grid().is_none() || groups.iter().all(|g| g.buttons.is_empty()) {
            return;
        }
        self.grid_row = Some(GridBar::new(&self.root, self.menu_bar(), groups, self.scale));
    }

    /// Outline the button a dragged window would go to, or none.
    pub(super) fn outline_grid_button(&self, grid: Option<usize>) {
        if let Some(bar) = &self.grid_row {
            bar.outline(grid);
        }
    }

    /// The grid (0-based) whose button is at `local`, a point in the overview.
    pub(super) fn grid_button_at(&self, local: CGPoint) -> Option<usize> {
        let bar = self.grid_row.as_ref()?;
        bar.button(bar.button_at(local)?).map(|b| b.grid)
    }

    /// Whether `local` is on the grid buttons.
    pub(super) fn on_grid_row(&self, local: CGPoint) -> bool {
        self.grid_row.as_ref().is_some_and(|bar| bar.contains(local))
    }

    /// Where the grid's cells go in the overview: below the grid buttons.
    fn grid_area(&self) -> CGRect {
        let size = self.bounds.size;
        let top = self
            .grid_row
            .as_ref()
            .map_or(0.0, |bar| bar_bottom(self.menu_bar(), bar.groups().len()));
        rect(0.0, top, size.width, (size.height - top).max(1.0))
    }

    pub(super) fn project(&self) -> Vec<WorkspaceProjection> {
        match self.grid() {
            Some(shape) => {
                let area = self.grid_area();
                let camera = follow_camera(
                    self.camera.get(),
                    active_cell(&self.workspaces, shape),
                    area.size,
                    self.info.frame.size,
                    self.look,
                );
                self.camera.set(Some(camera));
                project_grid_at(
                    self.info.frame,
                    area,
                    &self.workspaces,
                    shape,
                    self.look,
                    camera,
                )
            }
            None => project(
                self.info.frame,
                self.bounds.size,
                &self.workspaces,
                self.centered,
                self.workspace_offset,
                &self.offsets,
            ),
        }
    }
}

impl OverviewSession {
    /// Whether rift moved the focus a moment ago, so the system's focus may lag.
    pub(super) fn rift_moved_focus(&self) -> bool {
        self.rift_focus_at.is_some_and(|at| at.elapsed() < RIFT_FOCUS_BRIDGE)
    }

    /// The grid (0-based) whose button in the grid row is under `point`.
    pub(super) fn grid_button_at(&self, point: CGPoint) -> Option<usize> {
        let d = self.displays.iter().find(|d| contains(d.bounds, point))?;
        d.grid_button_at(CGPoint::new(point.x - d.bounds.origin.x, point.y - d.bounds.origin.y))
    }

    /// Whether `point` is on a display's grid row, which covers the cells under it.
    pub(super) fn on_grid_row(&self, point: CGPoint) -> bool {
        self.displays.iter().find(|d| contains(d.bounds, point)).is_some_and(|d| {
            d.on_grid_row(CGPoint::new(point.x - d.bounds.origin.x, point.y - d.bounds.origin.y))
        })
    }

    /// Pressing, dragging and letting go of a grid button: let go on another
    /// display's row, the grid goes to that display; let go where it was
    /// pressed, the grid shows. None when `input` is not the buttons'.
    pub(super) fn grid_bar_input(&mut self, input: &Input) -> Option<Option<OverviewAction>> {
        use crate::layout_engine::LayoutCommand;
        let local = |d: &DisplayOverview, p: CGPoint| {
            CGPoint::new(p.x - d.bounds.origin.x, p.y - d.bounds.origin.y)
        };
        let pressed = self
            .displays
            .iter()
            .position(|d| d.grid_row.as_ref().is_some_and(GridBar::is_pressed));
        match (input, pressed) {
            (&Input::PointerDown(p), _) => {
                let d = self.displays.iter_mut().find(|d| contains(d.bounds, p))?;
                let at = local(d, p);
                let bar = d.grid_row.as_mut().filter(|bar| bar.contains(at))?;
                bar.press(at);
                Some(None)
            }
            (&(Input::PointerDrag(p) | Input::Move(p)), Some(i)) => {
                let d = &mut self.displays[i];
                let at = local(d, p);
                d.grid_row.as_mut()?.drag(at);
                Some(None)
            }
            (&(Input::PointerUp(p) | Input::Click(p)), Some(i)) => {
                let d = &mut self.displays[i];
                let at = local(d, p);
                let released = d.grid_row.as_mut()?.release(at);
                Some(released.map(|(button, to)| {
                    OverviewAction::Command(match to {
                        Some(display) => LayoutCommand::MoveGridToDisplay {
                            grid: button.grid + 1,
                            display: crate::model::reactor::DisplaySelector::Uuid(display),
                        },
                        None => LayoutCommand::SwitchToGrid(button.grid + 1),
                    })
                }))
            }
            // A dragged window dropped on a button is the drag's.
            (&(Input::PointerUp(p) | Input::Click(p)), None)
                if self.on_grid_row(p) && !self.drag.as_ref().is_some_and(|d| d.started) =>
            {
                Some(None)
            }
            _ => None,
        }
    }

    /// Follow focus changes without rebuilding the cards: re-read which cell is
    /// active and which window the system has focused, and move the outlines.
    /// Anything more than that waits for a full refresh.
    pub(crate) fn sync_focus(&mut self, reactor: &ReactorHandle) {
        if self.drag.is_some() {
            return;
        }
        let rift_moved = self.rift_moved_focus();
        let Some(d) = self.displays.get_mut(self.active).filter(|d| d.grid().is_some()) else {
            return;
        };
        let mut workspaces = reactor.query_workspaces(d.info.space);
        filter_workspaces(&mut workspaces, self.show_empty_workspaces);
        let same_cards = workspaces.len() == d.workspaces.len()
            && workspaces.iter().zip(&d.workspaces).all(|(a, b)| {
                a.index == b.index
                    && a.windows.iter().map(|w| w.id).eq(b.windows.iter().map(|w| w.id))
            });
        if !same_cards {
            // Windows came or went, or the grid changed: rebuild.
            self.reload(reactor, None);
            return;
        }
        let groups = grid_bar_groups(reactor, &d.info.display_uuid);
        let bar_rows = d.grid_row.as_ref().map(|bar| bar.groups().len());
        if bar_rows.is_some_and(|rows| rows != groups.len()) {
            // Another display came or went: the cells move under the buttons.
            self.reload(reactor, None);
            return;
        }
        d.set_grid_bar(groups);
        let active_was = d.workspaces.iter().position(|ws| ws.is_active);
        for (ws, fresh) in d.workspaces.iter_mut().zip(&workspaces) {
            ws.is_active = fresh.is_active;
            ws.layout_focus = fresh.layout_focus;
            for (w, fresh) in ws.windows.iter_mut().zip(&fresh.windows) {
                w.is_focused = fresh.is_focused;
            }
        }
        let _transaction = LayerTransaction::disabled().flush_on_commit();
        // In the first stage the camera stays on the active cell.
        let follow = d.look.stage == GridStage::Cell
            && d.workspaces.iter().position(|ws| ws.is_active) != active_was;
        if follow {
            d.animate = true;
            d.rebuild();
        }
        let Some(selection) = grid_focus_selection(&d.workspaces, rift_moved) else {
            return;
        };
        if selection != self.selection || follow {
            let old = (self.active, self.selection);
            self.selection = selection;
            self.highlight(Some(old));
        }
        self.sync_metal();
    }

    /// Step a grid overview back from the active cell to the whole grid. False when
    /// there is no further stage, and opening Overview again closes it.
    pub(crate) fn step_back(&mut self) -> bool {
        let grids = self.displays.iter().filter(|d| d.grid().is_some());
        if grids.clone().count() == 0 || grids.into_iter().all(|d| d.look.stage == GridStage::Whole) {
            return false;
        }
        let _transaction = LayerTransaction::disabled().flush_on_commit();
        for d in self.displays.iter_mut().filter(|d| d.grid().is_some()) {
            d.look.stage = GridStage::Whole;
            d.animate = true;
            d.rebuild();
        }
        self.highlight(None);
        self.sync_metal();
        true
    }

    /// Esc: step a grid overview in from the whole grid to the active cell. False
    /// when it is already there, or the whole grid is its only stage, and Esc
    /// closes it.
    pub(crate) fn step_in(&mut self) -> bool {
        let whole = |d: &DisplayOverview| {
            d.grid().is_some() && d.look.stage == GridStage::Whole && !d.look.fit
        };
        if !self.displays.iter().any(whole) {
            return false;
        }
        let _transaction = LayerTransaction::disabled().flush_on_commit();
        for d in self.displays.iter_mut().filter(|d| whole(d)) {
            d.look.stage = GridStage::Cell;
            d.camera.set(None);
            d.animate = true;
            d.rebuild();
        }
        self.highlight(None);
        self.sync_metal();
        true
    }

    /// The window a grid overview outlines as focused on display `display`.
    pub(super) fn grid_focused(&self, display: usize) -> Option<WindowId> {
        (display == self.active && self.displays.get(display)?.grid().is_some())
            .then_some(self.selection.window)
            .flatten()
    }
}

/// A preview's border: thick for the focused window of a grid, thin while
/// hovered.
pub(super) fn preview_border(
    id: WindowId,
    focused: Option<WindowId>,
    hovered: Option<WindowId>,
) -> f64 {
    if Some(id) == focused {
        2.5
    } else if Some(id) == hovered {
        1.5
    } else {
        0.0
    }
}

/// Keep a workspace canvas the size of its workspace: a grid cell grows and
/// shrinks between the overview's stages.
pub(super) fn fit_canvas(
    canvas: &mut Option<cgs::PreviewCompositor>,
    sizes: &mut HashMap<usize, CGSize>,
    index: usize,
    size: CGSize,
    scale: f64,
) {
    let resized = sizes.insert(index, size).is_some_and(|was| was != size);
    if resized
        && let Some(metal) = canvas
        && let Err(error) = metal.resize(rect(0.0, 0.0, size.width, size.height), scale)
    {
        tracing::warn!(?error, "Workspace preview canvas resize failed");
        *canvas = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, frame: CGRect, floating: bool) -> RuntimeWindowData {
        serde_json::from_value(serde_json::json!({
            "id": {"pid": 123,"idx": id}, "title": "Example", "frame": {
                "origin": {"x": frame.origin.x,"y": frame.origin.y},
                "size": {"width": frame.size.width,"height": frame.size.height}
            }, "is_floating": floating,"is_focused": false,"window_server_id": id,
            "bundle_id": "org.example"
        }))
        .unwrap()
    }

    fn workspace(index: usize, windows: Vec<RuntimeWindowData>) -> RuntimeWorkspaceData {
        RuntimeWorkspaceData {
            workspace_id: slotmap::KeyData::from_ffi((1u64 << 32) | (index as u64 + 1)).into(),
            space: crate::sys::screen::SpaceId::new(1),
            id: index.to_string(),
            index,
            name: index.to_string(),
            layout_mode: "scrolling".into(),
            is_active: index == 1,
            window_count: windows.len(),
            windows,
            grid_cell: None,
            grid: None,
            grid_elsewhere: false,
            layout_focus: None,
            display_uuid: None,
            is_focused: false,
        }
    }

    #[test]
    fn grid_overview_lays_cells_out_in_rows_and_columns() {
        let display = rect(0.0, 0.0, 1600.0, 1000.0);
        let overview = CGSize::new(1600.0, 1000.0);
        let workspaces: Vec<_> = (0..9)
            .map(|index| {
                let windows = if index == 4 {
                    vec![
                        window(1, rect(0.0, 0.0, 800.0, 1000.0), false),
                        window(2, rect(800.0, 0.0, 800.0, 1000.0), false),
                    ]
                } else {
                    Vec::new()
                };
                RuntimeWorkspaceData {
                    grid_cell: Some((index / 3, index % 3)),
                    ..workspace(index, windows)
                }
            })
            .collect();
        assert_eq!(grid_shape(&workspaces), Some((3, 3)));
        assert_eq!(grid_shape(&workspaces[..1]), Some((1, 1)));
        assert_eq!(grid_shape(&[workspace(0, Vec::new())]), None);

        let mut filtered = workspaces.clone();
        filter_workspaces(&mut filtered, false);
        assert_eq!(filtered.len(), 9, "empty cells keep their place");

        let look = GridLook {
            gap: 0.04,
            labels: true,
            zoom: 0.99,
            stage: GridStage::Whole,
            fit: false,
            follow: GridFollow::Center,
        };
        let gap = look.gap_px(display.size);
        assert_eq!(gap, 40.0, "a fraction of the shorter side");
        let projection = project_grid(display, overview, &workspaces, (3, 3), look);
        assert_eq!(projection.len(), 9);
        let frame = |i: usize| projection[i].frame;
        // Same size, the display's aspect ratio, inside the overview, centered.
        for ws in &projection {
            assert!(ws.cell);
            assert_eq!(ws.frame.size, frame(0).size);
            assert!((ws.frame.size.width / ws.frame.size.height - 1.6).abs() < 1e-9);
            assert!(ws.frame.origin.x >= 0.0 && ws.frame.max().x <= overview.width);
            assert!(ws.frame.origin.y >= 0.0 && ws.frame.max().y <= overview.height);
        }
        assert!((frame(4).mid().x - overview.width / 2.0).abs() < 1e-9);
        assert!((frame(4).mid().y - overview.height / 2.0).abs() < 1e-9);
        // Row by row from the top left.
        assert!(frame(0).max().x < frame(1).origin.x);
        assert_eq!(frame(0).origin.y, frame(2).origin.y);
        assert_eq!(frame(0).max().y + gap, frame(3).origin.y);
        assert_eq!(frame(0).max().x + gap, frame(1).origin.x);
        assert_eq!(frame(0).origin.x, frame(6).origin.x);

        // The center cell's two tiles split it in half, and the whole cell is the hit target.
        let center = &projection[4];
        let backdrop = workspace_backdrop(center);
        assert_eq!(backdrop.origin, CGPoint::new(0.0, 0.0));
        assert!((backdrop.size.width - frame(4).size.width).abs() < 1e-9);
        let half = frame(4).size.width / 2.0;
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let left = center.windows[0].frame;
        assert!(close(left.size.width, half) && close(left.size.height, frame(4).size.height));
        assert!(close(center.windows[1].frame.origin.x, half));
        // Cards are bare previews: the bottom of a tile, where a caption would sit,
        // still selects its window.
        let bottom_left = CGPoint::new(frame(4).origin.x + 4.0, frame(4).max().y - 4.0);
        assert_eq!(
            hit(&workspaces, &projection, bottom_left).and_then(|s| s.window),
            Some(center.windows[0].id)
        );
        assert_eq!(
            card_image_frame(center.windows[0].frame.size, true).size.height,
            frame(4).size.height - 2.0 * BARE_INSET
        );
        let corner = CGPoint::new(frame(8).max().x - 2.0, frame(8).max().y - 2.0);
        assert_eq!(hit(&workspaces, &projection, corner).map(|s| s.workspace), Some(8));
        let between = CGPoint::new(frame(0).max().x + gap / 2.0, frame(0).mid().y);
        assert_eq!(hit(&workspaces, &projection, between), None);

        let empty: MissionControlSettings = toml::from_str("").unwrap();
        assert_eq!(GridLook::new(&MissionControlSettings::default()), GridLook::new(&empty));
    }

    #[test]
    fn the_bar_lists_each_displays_grids_and_one_free_grid() {
        let ws = |index: usize, windows: Vec<RuntimeWindowData>, active: bool| RuntimeWorkspaceData {
            grid_cell: Some(((index % 9) / 3, index % 3)),
            grid: Some(index / 9),
            is_active: active,
            window_count: windows.len(),
            ..workspace(index, windows)
        };
        let tile = |id| window(id, rect(0.0, 0.0, 100.0, 100.0), false);
        // Here: windows in grids 1 and 4; grid 3 is on screen, empty.
        let here: Vec<_> = (0..45)
            .map(|index| match index {
                4 => ws(index, vec![tile(1)], false),
                27 | 30 => ws(index, vec![tile(index as u32)], false),
                22 => ws(index, Vec::new(), true),
                _ => ws(index, Vec::new(), false),
            })
            .collect();
        // There: grid 2 on screen.
        let there: Vec<_> = (0..45).map(|index| ws(index, Vec::new(), index == 13)).collect();
        let displays = [
            ("a".to_owned(), "Studio".to_owned(), here.clone()),
            ("b".to_owned(), "Built-in".to_owned(), there),
        ];
        let groups = bar_groups(&displays, "a");
        let button = |grid, shown, new| BarButton { grid, shown, new };
        assert_eq!(groups[0].name, "Studio");
        assert_eq!(
            groups[0].buttons,
            [
                button(0, Shown::No, false),
                button(2, Shown::Here, false),
                button(3, Shown::No, false),
                button(4, Shown::No, true),
            ],
            "+ for grid 5, the first on no display, on this display's row"
        );
        assert_eq!(groups[1].buttons, [button(1, Shown::Elsewhere, false)]);
        assert_eq!(bar_groups(&displays, "b")[1].buttons.last(), Some(&button(4, Shown::No, true)));

        let mut workspaces = here;
        keep_grid_on_screen(&mut workspaces);
        assert_eq!(workspaces.len(), 9, "only the grid on screen");
        assert!(workspaces.iter().all(|ws| ws.grid == Some(2)));
    }

    #[test]
    fn reveal_moves_the_camera_only_to_show_the_active_cell() {
        let display = CGSize::new(1600.0, 1000.0);
        let look = GridLook {
            gap: 0.04,
            labels: true,
            zoom: 0.5,
            stage: GridStage::Cell,
            fit: false,
            follow: GridFollow::Reveal,
        };
        let camera = |camera, active| follow_camera(camera, active, display, display, look);
        // Opening: on the active cell.
        assert_eq!(camera(None, (1, 1)), (1.0, 1.0));
        // A cell is 800x500 with a 40 gap; the view has room for a neighbour's
        // whole cell only if it fits within 1600/2 - 400 - 40 = 360 of the center.
        // The next cell is 840 away: the camera moves 480 of 840.
        let (row, column) = camera(Some((1.0, 1.0)), (1, 2));
        assert_eq!(row, 1.0);
        assert!((column - (1.0 + 480.0 / 840.0)).abs() < 1e-9);
        // Back to the cell it came from: that cell is now 480 off center, past the
        // 360 of room, so the camera moves 120 back, not all the way.
        let (_, back) = camera(Some((row, column)), (1, 1));
        assert!((back - (column - 120.0 / 840.0)).abs() < 1e-9);
        // An active cell already on show leaves the camera where it is.
        assert_eq!(camera(Some((1.2, 1.3)), (1, 1)), (1.2, 1.3));
        // Centering puts the camera on the active cell every time.
        let center = GridLook { follow: GridFollow::Center, ..look };
        assert_eq!(follow_camera(Some((1.0, 1.0)), (1, 2), display, display, center), (1.0, 2.0));
        // A cell larger than the view with its margins is centered.
        let big = GridLook { zoom: 0.99, ..look };
        assert_eq!(follow_camera(Some((1.0, 1.0)), (1, 2), display, display, big), (1.0, 2.0));
    }

    #[test]
    fn grid_selection_follows_the_system_focus_in_the_active_cell() {
        let tile = |id, focused| {
            let mut w = window(id, rect(0.0, 0.0, 100.0, 100.0), false);
            w.is_focused = focused;
            w
        };
        let cells = |active: usize, windows: [Vec<RuntimeWindowData>; 2], layout_focus| {
            windows
                .into_iter()
                .enumerate()
                .map(|(index, windows)| RuntimeWorkspaceData {
                    is_active: index == active,
                    grid_cell: Some((0, index)),
                    layout_focus: (index == active).then_some(layout_focus).flatten(),
                    ..workspace(index, windows)
                })
                .collect::<Vec<_>>()
        };
        let id = |n| Some(WindowId::new(123, n));
        let select = |ws: &[RuntimeWorkspaceData]| grid_focus_selection(ws, false).unwrap();

        // The system's focus wins over the layout's within the active cell.
        let ws = cells(1, [vec![], vec![tile(1, false), tile(2, true)]], id(1));
        assert_eq!(select(&ws), Selection { workspace: 1, window: id(2) });
        // Until the system catches up with a move into another cell, the layout's
        // focus stands in.
        let ws = cells(1, [vec![tile(3, true)], vec![tile(1, false)]], id(1));
        assert_eq!(select(&ws), Selection { workspace: 1, window: id(1) });
        // Right after rift moved the focus, its target wins until the system catches up.
        let ws = cells(1, [vec![], vec![tile(1, false), tile(2, true)]], id(1));
        assert_eq!(grid_focus_selection(&ws, true).unwrap(), Selection { workspace: 1, window: id(1) });
        // An empty active cell has no focused window.
        let ws = cells(1, [vec![tile(3, true)], vec![]], None);
        assert_eq!(select(&ws), Selection { workspace: 1, window: None });
    }

    #[test]
    fn grid_overview_first_stage_centers_the_active_cell() {
        let display = rect(0.0, 0.0, 1600.0, 1000.0);
        let overview = CGSize::new(1600.0, 1000.0);
        let mut workspaces: Vec<_> = (0..9)
            .map(|index| RuntimeWorkspaceData {
                grid_cell: Some((index / 3, index % 3)),
                is_active: index == 4,
                ..workspace(index, Vec::new())
            })
            .collect();
        let look = GridLook {
            gap: 0.04,
            labels: true,
            zoom: 0.5,
            stage: GridStage::Cell,
            fit: false,
            follow: GridFollow::Center,
        };
        let frames = |workspaces: &[RuntimeWorkspaceData], look| {
            project_grid(display, overview, workspaces, grid_shape(workspaces).unwrap(), look)
                .iter()
                .map(|ws| ws.frame)
                .collect::<Vec<_>>()
        };

        let cell = frames(&workspaces, look);
        assert_eq!(cell[4], rect(400.0, 250.0, 800.0, 500.0), "zoom of the display, centered");
        assert_eq!(cell[5].origin.x, cell[4].max().x + 40.0, "the neighbour a gap away");
        assert_eq!(cell[1].max().y + 40.0, cell[4].origin.y);
        assert!(cell[3].origin.x < 0.0 && cell[3].max().x > 0.0, "neighbours show at the edges");

        // The camera is on the active cell, wherever it is.
        workspaces[4].is_active = false;
        workspaces[0].is_active = true;
        assert_eq!(frames(&workspaces, look)[0], rect(400.0, 250.0, 800.0, 500.0));

        // The whole grid is fitted, and the first stage is the larger one.
        let whole = frames(&workspaces, GridLook { stage: GridStage::Whole, ..look });
        assert!(whole[0].size.width < 800.0);
        assert!(whole[0].origin.x >= 0.0 && whole[8].max().x <= overview.width);
        // Never larger than the zoom, even when the grid would fit bigger.
        let one = [RuntimeWorkspaceData { grid_cell: Some((0, 0)), ..workspace(0, Vec::new()) }];
        let small = GridLook { stage: GridStage::Whole, zoom: 0.3, ..look };
        assert_eq!(frames(&one, small)[0].size.width, 480.0);
    }
}
