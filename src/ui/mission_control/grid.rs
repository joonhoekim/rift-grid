//! The grid overview: every workspace a cell at its row and column, in two stages
//! (the active cell and its neighbours, then the whole grid), following the real
//! focus.

use super::*;
use crate::common::config::GridFollow;

// The app and title row above a grid tile, drawn in the gap between cells.
pub(super) const LABEL_ROW: f64 = 22.0;
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
/// empty or not. Returns the grids for the button row: those with windows and
/// the one on screen, in order, then "+" for the first grid neither is.
pub(super) fn keep_grid_on_screen(workspaces: &mut Vec<RuntimeWorkspaceData>) -> Vec<GridButton> {
    let on_screen = workspaces.iter().find(|ws| ws.is_active).and_then(|ws| ws.grid);
    let mut grids: Vec<_> = workspaces
        .iter()
        .filter(|ws| !ws.windows.is_empty() || ws.grid == on_screen)
        .filter_map(|ws| ws.grid)
        .collect();
    grids.dedup();
    let unused = workspaces
        .iter()
        .filter_map(|ws| ws.grid)
        .find(|grid| !grids.contains(grid));
    if on_screen.is_some() {
        workspaces.retain(|ws| ws.grid == on_screen);
    }
    grids
        .into_iter()
        .map(|grid| GridButton { grid, on_screen: Some(grid) == on_screen, new: false })
        .chain(unused.map(|grid| GridButton { grid, on_screen: false, new: true }))
        .collect()
}

/// A button in the overview's grid row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GridButton {
    /// 0-based.
    pub(super) grid: usize,
    pub(super) on_screen: bool,
    /// "+": a grid not in use yet.
    pub(super) new: bool,
}

/// The overview's grid row as drawn.
pub(super) struct GridRow {
    layer: Retained<CALayer>,
    cells: Vec<Retained<CALayer>>,
    buttons: Vec<GridButton>,
}

const BUTTON: CGSize = CGSize { width: 34.0, height: 26.0 };
const BUTTON_GAP: f64 = 6.0;
const BUTTON_PAD: f64 = 5.0;

/// The grid row, centered at the top of the overview: the row's frame and each
/// button's frame within it.
pub(super) fn grid_button_frames(overview: CGSize, count: usize) -> (CGRect, Vec<CGRect>) {
    let n = count as f64;
    let width = n * BUTTON.width + (n - 1.0).max(0.0) * BUTTON_GAP + 2.0 * BUTTON_PAD;
    let height = BUTTON.height + 2.0 * BUTTON_PAD;
    let row = rect((overview.width - width) / 2.0, 12.0, width, height);
    let buttons = (0..count)
        .map(|i| {
            rect(
                BUTTON_PAD + i as f64 * (BUTTON.width + BUTTON_GAP),
                BUTTON_PAD,
                BUTTON.width,
                BUTTON.height,
            )
        })
        .collect();
    (row, buttons)
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

    /// Labels sit in the gap above each tile, so they need a gap at least their height.
    pub(super) fn labels_fit(self, display: CGSize) -> bool {
        self.labels && self.gap_px(display) >= LABEL_ROW
    }
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
    project_grid_at(display, overview, workspaces, shape, look, (row as f64, column as f64))
}

/// Every cell of the grid at once, the first stage's camera at `camera`.
pub(super) fn project_grid_at(
    display: CGRect,
    overview: CGSize,
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
            let frame =
                grid_cell_frame(overview, display.size, shape, ws.grid_cell?, gap, look, camera);
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

    /// Redraw the grid row when the grids in use or the one on screen changed.
    pub(super) fn set_grid_buttons(&mut self, buttons: Vec<GridButton>) {
        if self.grid_row.as_ref().is_some_and(|drawn| drawn.buttons == buttons) {
            return;
        }
        if let Some(drawn) = self.grid_row.take() {
            drawn.layer.removeFromSuperlayer();
        }
        if buttons.is_empty() || self.grid().is_none() {
            return;
        }
        let (frame, frames) = grid_button_frames(self.bounds.size, buttons.len());
        let row = layer(pixel_aligned(frame, self.scale), self.scale);
        row.setZPosition(10.0);
        row.setCornerRadius(10.0);
        row.setBackgroundColor(Some(&color(0.06, 0.06, 0.08, 0.72)));
        let mut cells = Vec::with_capacity(buttons.len());
        with_disabled_actions(|| {
            for (button, frame) in buttons.iter().zip(frames) {
                let cell = layer(frame, self.scale);
                cell.setCornerRadius(6.0);
                if button.on_screen {
                    cell.setBackgroundColor(Some(&NSColor::controlAccentColor().CGColor()));
                } else {
                    cell.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.1)));
                }
                let text = if button.new { "+".to_owned() } else { (button.grid + 1).to_string() };
                let number = label(
                    &cell,
                    &text,
                    rect(0.0, (frame.size.height - 16.0) / 2.0, frame.size.width, 16.0),
                    self.scale,
                );
                unsafe {
                    number.setAlignmentMode(objc2_quartz_core::kCAAlignmentCenter);
                }
                number.setFontSize(13.0);
                if button.on_screen {
                    number.setForegroundColor(Some(&color(1.0, 1.0, 1.0, 1.0)));
                }
                row.addSublayer(&cell);
                cells.push(cell);
            }
        });
        self.root.addSublayer(&row);
        self.grid_row = Some(GridRow { layer: row, cells, buttons });
    }

    /// Outline the button a dragged window would go to, or none.
    pub(super) fn outline_grid_button(&self, grid: Option<usize>) {
        let Some(drawn) = &self.grid_row else { return };
        with_disabled_actions(|| {
            for (cell, button) in drawn.cells.iter().zip(&drawn.buttons) {
                if Some(button.grid) == grid {
                    cell.setBorderWidth(2.0);
                    cell.setBorderColor(Some(&color(1.0, 1.0, 1.0, 0.9)));
                } else {
                    cell.setBorderWidth(0.0);
                }
            }
        });
    }

    /// The grid (0-based) whose button is at `local`, a point in the overview.
    pub(super) fn grid_button_at(&self, local: CGPoint) -> Option<usize> {
        let drawn = self.grid_row.as_ref()?;
        let (row, frames) = grid_button_frames(self.bounds.size, drawn.buttons.len());
        frames.iter().zip(&drawn.buttons).find_map(|(frame, button)| {
            let frame = rect(
                row.origin.x + frame.origin.x,
                row.origin.y + frame.origin.y,
                frame.size.width,
                frame.size.height,
            );
            contains(frame, local).then_some(button.grid)
        })
    }

    /// Whether `local` is on the grid row, buttons or the space between them.
    pub(super) fn on_grid_row(&self, local: CGPoint) -> bool {
        self.grid_row.as_ref().is_some_and(|drawn| {
            contains(grid_button_frames(self.bounds.size, drawn.buttons.len()).0, local)
        })
    }

    pub(super) fn project(&self) -> Vec<WorkspaceProjection> {
        match self.grid() {
            Some(shape) => {
                let camera = follow_camera(
                    self.camera.get(),
                    active_cell(&self.workspaces, shape),
                    self.bounds.size,
                    self.info.frame.size,
                    self.look,
                );
                self.camera.set(Some(camera));
                project_grid_at(
                    self.info.frame,
                    self.bounds.size,
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

    /// Follow focus changes without rebuilding the cards: re-read which cell is
    /// active and which window the system has focused, and move the outlines.
    /// Anything more than that waits for a full refresh.
    pub(crate) fn sync_focus(&mut self, reactor: &ReactorHandle, cache: Option<&PreviewCache>) {
        if self.drag.is_some() {
            return;
        }
        let rift_moved = self.rift_moved_focus();
        let Some(d) = self.displays.get_mut(self.active).filter(|d| d.grid().is_some()) else {
            return;
        };
        let mut workspaces = reactor.query_workspaces(d.info.space);
        let buttons = filter_workspaces(&mut workspaces, self.show_empty_workspaces);
        let same_cards = workspaces.len() == d.workspaces.len()
            && workspaces.iter().zip(&d.workspaces).all(|(a, b)| {
                a.index == b.index
                    && a.windows.iter().map(|w| w.id).eq(b.windows.iter().map(|w| w.id))
            });
        if !same_cards {
            // Windows came or went, or the grid changed: rebuild.
            self.reload(reactor, cache, None);
            return;
        }
        d.set_grid_buttons(buttons);
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
            d.rebuild(cache);
        }
        let Some(selection) = grid_focus_selection(&d.workspaces, rift_moved) else {
            return;
        };
        if selection != self.selection || follow {
            let old = (self.active, self.selection);
            self.selection = selection;
            self.highlight(Some(old));
        }
        if follow {
            self.sync_images(cache);
            self.request_previews(cache);
        }
    }

    /// Step a grid overview back from the active cell to the whole grid. False when
    /// there is no further stage, and opening Overview again closes it.
    pub(crate) fn step_back(&mut self, cache: Option<&PreviewCache>) -> bool {
        let grids = self.displays.iter().filter(|d| d.grid().is_some());
        if grids.clone().count() == 0 || grids.into_iter().all(|d| d.look.stage == GridStage::Whole) {
            return false;
        }
        let _transaction = LayerTransaction::disabled().flush_on_commit();
        for d in self.displays.iter_mut().filter(|d| d.grid().is_some()) {
            d.look.stage = GridStage::Whole;
            d.animate = true;
            d.rebuild(cache);
        }
        self.highlight(None);
        self.sync_images(cache);
        self.request_previews(cache);
        true
    }

    /// Esc: step a grid overview in from the whole grid to the active cell. False
    /// when it is already there, or the whole grid is its only stage, and Esc
    /// closes it.
    pub(crate) fn step_in(&mut self, cache: Option<&PreviewCache>) -> bool {
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
            d.rebuild(cache);
        }
        self.highlight(None);
        self.sync_images(cache);
        self.request_previews(cache);
        true
    }

    /// Capture the visible windows again. Cards keep their current image until
    /// the new one arrives.
    pub(crate) fn recapture(&mut self, cache: Option<&PreviewCache>) {
        let Some(previews) = &mut self.previews else {
            return;
        };
        if previews.failed {
            return;
        }
        previews.schedule.expire();
        self.request_previews(cache);
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
            layout_focus: None,
        }
    }

    fn request(id: u32) -> PreviewRequest {
        PreviewRequest {
            id: WindowId::new(123, id),
            sys_id: WindowServerId::new(id),
            bundle: Some("org.example".into()),
            width: 200,
            height: 100,
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

        // Labels go in the gap, so a gap narrower than a label row has none.
        assert!(look.labels_fit(display.size));
        assert!(!GridLook { gap: 0.01, ..look }.labels_fit(display.size));
        assert!(!GridLook { labels: false, ..look }.labels_fit(display.size));
        let empty: MissionControlSettings = toml::from_str("").unwrap();
        assert_eq!(GridLook::new(&MissionControlSettings::default()), GridLook::new(&empty));
    }

    #[test]
    fn grid_row_lists_grids_in_use_and_the_one_on_screen() {
        let ws = |index: usize, windows: Vec<RuntimeWindowData>, active: bool| RuntimeWorkspaceData {
            grid_cell: Some(((index % 9) / 3, index % 3)),
            grid: Some(index / 9),
            is_active: active,
            ..workspace(index, windows)
        };
        let tile = |id| window(id, rect(0.0, 0.0, 100.0, 100.0), false);
        // Windows in grids 1 and 4, two of them in grid 4; grid 3 is on screen, empty.
        let mut workspaces: Vec<_> = (0..45)
            .map(|index| match index {
                4 => ws(index, vec![tile(1)], false),
                27 | 30 => ws(index, vec![tile(index as u32)], false),
                22 => ws(index, Vec::new(), true),
                _ => ws(index, Vec::new(), false),
            })
            .collect();
        let buttons = keep_grid_on_screen(&mut workspaces);
        assert_eq!(
            buttons,
            [
                GridButton { grid: 0, on_screen: false, new: false },
                GridButton { grid: 2, on_screen: true, new: false },
                GridButton { grid: 3, on_screen: false, new: false },
                GridButton { grid: 1, on_screen: false, new: true },
            ],
            "grid 2 is the first not in use"
        );
        assert_eq!(workspaces.len(), 9, "only the grid on screen");
        assert!(workspaces.iter().all(|ws| ws.grid == Some(2)));

        let overview = CGSize::new(1600.0, 1000.0);
        let (row, frames) = grid_button_frames(overview, 3);
        assert!((row.mid().x - 800.0).abs() < 1e-9, "centered");
        assert_eq!(frames.len(), 3);
        assert!(frames[0].max().x < frames[1].origin.x);
        assert!(frames.iter().all(|f| f.max().x <= row.size.width && f.max().y <= row.size.height));
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
    fn expiring_the_schedule_recaptures_only_settled_windows() {
        let mut schedule = PreviewSchedule::default();
        for id in 1..=3 {
            schedule.enqueue(request(id));
        }
        let first = schedule.next().unwrap().id;
        schedule.complete(first);
        let second = schedule.next().unwrap().id;
        // First is captured, second is in flight, third is still queued.
        schedule.expire();
        assert!(schedule.needs(first));
        assert!(!schedule.needs(second));
        assert!(!schedule.needs(WindowId::new(123, 3)));
        schedule.enqueue(request(1));
        schedule.enqueue(request(3));
        assert_eq!(schedule.pending.iter().map(|p| p.id.idx.get()).collect::<Vec<_>>(), [3, 1]);
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
