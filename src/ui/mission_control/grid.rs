//! The grid overview: every workspace a cell at its row and column, in two stages
//! (the current cell and its neighbours, then the whole grid), following the real
//! focus. The cells are laid out in the overview's world; the stages are cameras.

use super::*;
use crate::common::config::{GRID_COUNT, GridFollow, GridOverviewSettings};

/// `[settings.grid.overview]` as last noted by the reactor, which holds the
/// config; upstream's Overview actor is given its own section only.
static SETTINGS: std::sync::Mutex<Option<GridOverviewSettings>> = std::sync::Mutex::new(None);
/// Whether the Overview being opened starts on the whole grid.
static OPEN_WHOLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Note the grid Overview's settings, when they are new.
pub fn set_settings(settings: &GridOverviewSettings) {
    if let Ok(mut current) = SETTINGS.lock()
        && current.as_ref() != Some(settings)
    {
        *current = Some(settings.clone());
    }
}

fn settings() -> GridOverviewSettings {
    SETTINGS.lock().ok().and_then(|settings| settings.clone()).unwrap_or_default()
}

/// Open the next Overview on the whole grid (`overview_whole`), or as set.
pub(crate) fn open_whole(whole: bool) {
    OPEN_WHOLE.store(whole, std::sync::atomic::Ordering::Relaxed);
}

#[path = "grid_bar.rs"]
mod grid_bar;
pub(super) use grid_bar::*;

// The row of app, title and close button across a grid tile's top.
pub(super) const LABEL_ROW: f64 = 24.0;
/// The close button at the end of a tile's label.
pub(super) const LABEL_CLOSE: f64 = 16.0;
const CORNER: f64 = 10.0;

/// How long after rift moves the focus the overview shows rift's target rather
/// than the system's focus, which still reports the window before it.
const RIFT_FOCUS_BRIDGE: Duration = Duration::from_millis(600);

pub(super) fn label(
    parent: &CALayer,
    value: &str,
    frame: CGRect,
    scale: f64,
) -> Retained<CATextLayer> {
    text(parent, value, frame, scale, 12.0)
}

/// The selection a grid overview shows: the current cell, and the window the
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

/// The workspaces a display's overview shows: a grid's cells, or the
/// workspaces, the empty ones only when `show_empty`.
pub(super) fn filter_workspaces(workspaces: &mut Vec<RuntimeWorkspaceData>, show_empty: bool) {
    if grid_shape(workspaces).is_some() {
        keep_grid_on_screen(workspaces);
    } else if !show_empty {
        workspaces.retain(|ws| !ws.windows.is_empty());
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
            BarGroup {
                display: display.clone(),
                name: name.clone(),
                buttons,
            }
        })
        .collect();
    if let Some(free) = used.iter().position(|&used| !used)
        && let Some(row) = groups.iter_mut().find(|g| g.display == here)
    {
        row.buttons.push(BarButton {
            grid: free,
            shown: Shown::No,
            new: true,
        });
    }
    groups
}

/// The grid buttons for display `here`, from an overview snapshot.
pub(super) fn bar_groups_for(
    topology: &[RuntimeDisplayData],
    catalog: &[RuntimeWorkspaceData],
    here: &str,
) -> Vec<BarGroup> {
    let displays: Vec<_> = topology
        .iter()
        .filter(|d| d.is_visible_space)
        .filter_map(|d| {
            let space = d.info.space?;
            let name = d.info.name.clone().unwrap_or_else(|| "Display".to_owned());
            let workspaces = catalog.iter().filter(|w| w.space == space).cloned().collect();
            Some((d.info.display_uuid.clone(), name, workspaces))
        })
        .collect();
    bar_groups(&displays, here)
}

/// The grid buttons for display `here`, as the reactor has the grids now.
pub(super) fn grid_bar_groups(reactor: &ReactorHandle, here: &str) -> Vec<BarGroup> {
    let displays: Vec<_> = reactor
        .query_displays()
        .into_iter()
        .filter_map(|d| {
            let space = d.info.space?;
            let name = d.info.name.clone().unwrap_or_else(|| "Display".to_owned());
            Some((
                d.info.display_uuid.clone(),
                name,
                reactor.query_workspaces(Some(space)),
            ))
        })
        .collect();
    bar_groups(&displays, here)
}

/// The close button ending a grid tile's label, in `caption`.
fn label_close_button(caption: &CALayer, scale: f64) -> Retained<CALayer> {
    let button = layer(rect(0.0, 0.0, LABEL_CLOSE, LABEL_CLOSE), scale);
    button.setCornerRadius(LABEL_CLOSE / 2.0);
    button.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.18)));
    let cross = label(
        &button,
        "✕",
        rect(0.0, (LABEL_CLOSE - 12.0) / 2.0, LABEL_CLOSE, 12.0),
        scale,
    );
    unsafe { cross.setAlignmentMode(objc2_quartz_core::kCAAlignmentCenter) };
    cross.setFontSize(9.0);
    cross.setForegroundColor(Some(&color(1.0, 1.0, 1.0, 0.9)));
    caption.addSublayer(&button);
    button
}

/// cgs's capture limits for the Overview. Full-resolution previews need far
/// more than it allows by default: a window half a 5K display wide is past its
/// 256 MB for one stream.
pub(super) fn set_capture_limits() {
    const MB: usize = 1024 * 1024;
    if settings().full_resolution {
        cgs::compositor::preview::set_capture_limits(4096 * MB, 2048 * MB, 768 * MB);
    } else {
        cgs::compositor::preview::set_capture_limits(512 * MB, 256 * MB, 256 * MB);
    }
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
    /// The current cell at the grid zoom, its neighbours around it.
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
    /// Previews at the size they show at, full resolution.
    pub(super) full: bool,
}

impl GridLook {
    pub(super) fn new() -> Self {
        Self::from(&settings(), OPEN_WHOLE.load(std::sync::atomic::Ordering::Relaxed))
    }

    fn from(settings: &GridOverviewSettings, whole: bool) -> Self {
        let fit = settings.fit || whole;
        Self {
            gap: settings.gap.clamp(0.0, 0.1),
            labels: settings.labels,
            zoom: settings.zoom.clamp(0.2, 0.99),
            stage: if fit { GridStage::Whole } else { GridStage::Cell },
            fit,
            follow: settings.follow,
            full: settings.full_resolution,
        }
    }

    /// The gap between cells on screen in the first stage.
    pub(super) fn gap_px(self, display: CGSize) -> f64 {
        self.gap * display.width.min(display.height)
    }

    /// The gap between cells in the world, where a cell is the display's size.
    fn gap_world(self, display: CGSize) -> f64 { self.gap_px(display) / self.zoom }
}

/// Where the first stage's camera goes when the current cell is `active`: on it when
/// it centers or has no place yet, otherwise it stays unless the current cell would
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
    // Along one axis: the camera, the current cell, a cell's size and the view's.
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

/// A cell's place in the world: cells are the display's size, `gap` apart.
fn cell_origin(display: CGSize, gap: f64, (row, column): (usize, usize)) -> CGPoint {
    CGPoint::new(
        column as f64 * (display.width + gap),
        row as f64 * (display.height + gap),
    )
}

/// The camera that shows `area` of a `viewport`-sized overview the world point
/// `center` in the middle at `zoom`.
fn camera_on(center: CGPoint, area: CGRect, zoom: f64) -> Camera {
    Camera {
        zoom,
        position: CGPoint::new(center.x - area.mid().x / zoom, center.y - area.mid().y / zoom),
    }
}

/// The camera for a grid stage. In the first stage a cell is `zoom` of the
/// display, the camera (a point in cells) at the middle of `area`; the whole
/// grid is fitted and centered, never larger than `zoom`.
fn stage_camera(
    display: CGSize,
    area: CGRect,
    (rows, columns): (usize, usize),
    look: GridLook,
    (camera_row, camera_column): (f64, f64),
) -> Camera {
    let gap = look.gap_world(display);
    if look.stage == GridStage::Cell {
        let center = CGPoint::new(
            camera_column * (display.width + gap) + display.width / 2.0,
            camera_row * (display.height + gap) + display.height / 2.0,
        );
        return camera_on(center, area, look.zoom);
    }
    let width = columns as f64 * display.width + (columns as f64 - 1.0) * gap;
    let height = rows as f64 * display.height + (rows as f64 - 1.0) * gap;
    let margin = 2.0 * look.gap_px(display);
    let zoom = ((area.size.width - margin).max(1.0) / width)
        .min((area.size.height - margin).max(1.0) / height)
        .min(look.zoom);
    camera_on(CGPoint::new(width / 2.0, height / 2.0), area, zoom)
}

/// Move `layer`, already at its new frame, smoothly from `from`, where it was.
pub(super) fn glide(layer: &CALayer, from: CGRect) {
    let to = layer.frame();
    if from == to || from.size.width < 1.0 || from.size.height < 1.0 {
        return;
    }
    let start = CATransform3D::new_affine_transform(CGAffineTransform {
        a: from.size.width / to.size.width.max(1.0),
        b: 0.0,
        c: 0.0,
        d: from.size.height / to.size.height.max(1.0),
        tx: from.mid().x - to.mid().x,
        ty: from.mid().y - to.mid().y,
    });
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
    unsafe {
        animation.setFromValue(Some(&NSValue::valueWithCATransform3D(start)));
        animation.setToValue(Some(&NSValue::valueWithCATransform3D(CATransform3D::new_scale(
            1.0, 1.0, 1.0,
        ))));
    }
    animation.setDuration(0.2);
    layer.addAnimation_forKey(&animation, Some(&NSString::from_str("glide")));
}

/// A grid tile's label: app icon, title and close button across its top.
struct TileLabel {
    id: WindowId,
    layer: Retained<CALayer>,
    title: Retained<CATextLayer>,
    icon: Option<Retained<CALayer>>,
    close: Retained<CALayer>,
}

/// A display's grid overview, when its workspaces form a grid.
pub(super) struct GridState {
    pub(super) look: GridLook,
    pub(super) shape: Option<(usize, usize)>,
    bar: Option<GridBar>,
    /// Where the first stage looks, in cells; none until it opens.
    camera: std::cell::Cell<Option<(f64, f64)>>,
    /// Each cell's backdrop, in the world.
    cells: Vec<(crate::model::VirtualWorkspaceId, Retained<CALayer>)>,
    labels: Vec<TileLabel>,
}

impl GridState {
    pub(super) fn new(look: GridLook) -> Self {
        Self {
            look,
            shape: None,
            bar: None,
            camera: Default::default(),
            cells: Vec::new(),
            labels: Vec::new(),
        }
    }
}

/// The current cell, or the middle one when no cell is current.
fn current_cell(workspaces: &[RuntimeWorkspaceData], shape: (usize, usize)) -> (usize, usize) {
    workspaces
        .iter()
        .find(|ws| ws.is_active)
        .and_then(|ws| ws.grid_cell)
        .unwrap_or((shape.0 / 2, shape.1 / 2))
}

impl DisplayOverview {
    /// The menu bar's height on this display.
    fn menu_bar(&self) -> f64 { (self.info.frame.origin.y - self.bounds.origin.y).max(0.0) }

    /// Redraw the grid buttons when they changed.
    pub(super) fn set_grid_bar(&mut self, groups: Vec<BarGroup>) {
        if self.bar_matches(&groups) {
            return;
        }
        if let Some(bar) = self.grid.bar.take() {
            bar.remove();
        }
        if grid_shape(&self.workspaces).is_none() || groups.iter().all(|g| g.buttons.is_empty()) {
            return;
        }
        self.grid.bar = Some(GridBar::new(&self.root, self.menu_bar(), groups, self.scale));
    }

    fn bar_matches(&self, groups: &[BarGroup]) -> bool {
        match &self.grid.bar {
            Some(bar) => bar.groups() == groups,
            None => {
                grid_shape(&self.workspaces).is_none()
                    || groups.iter().all(|g| g.buttons.is_empty())
            }
        }
    }

    /// Outline the button a dragged window would go to, or none.
    fn outline_grid_button(&self, grid: Option<usize>) {
        if let Some(bar) = &self.grid.bar {
            bar.outline(grid);
        }
    }

    /// The grid (0-based) whose button is at `local`, a point in the overview.
    fn grid_button_at(&self, local: CGPoint) -> Option<usize> {
        let bar = self.grid.bar.as_ref()?;
        bar.button(bar.button_at(local)?).map(|b| b.grid)
    }

    /// Whether `local` is on the grid buttons.
    fn on_grid_row(&self, local: CGPoint) -> bool {
        self.grid.bar.as_ref().is_some_and(|bar| bar.contains(local))
    }

    /// Where the grid's cells go in the overview: below the grid buttons.
    fn grid_area(&self) -> CGRect {
        let size = self.bounds.size;
        let top = self
            .grid
            .bar
            .as_ref()
            .map_or(0.0, |bar| bar_bottom(self.menu_bar(), bar.groups().len()));
        rect(0.0, top, size.width, (size.height - top).max(1.0))
    }

    /// Put each workspace's region at its cell.
    pub(super) fn place_grid_cells(&mut self) {
        self.grid.shape = grid_shape(&self.workspaces);
        if self.grid.shape.is_none() {
            return;
        }
        let gap = self.grid.look.gap_world(self.info.frame.size);
        for region in &mut self.regions {
            if let Some(cell) = self
                .workspaces
                .iter()
                .find(|ws| ws.workspace_id == region.id)
                .and_then(|ws| ws.grid_cell)
            {
                region.origin = cell_origin(self.info.frame.size, gap, cell);
            }
        }
    }

    /// Whether a grid cell shows window `data` of `ws`: windows parked off the
    /// display would lie in a neighbouring cell.
    pub(super) fn grid_shows(&self, ws: &RuntimeWorkspaceData, data: &RuntimeWindowData) -> bool {
        self.grid.shape.is_none()
            || intersects(data.layout_frame.unwrap_or(data.info.frame), ws.display_frame)
    }

    /// The cell whose region starts at `origin`, in the world.
    pub(super) fn grid_cell_rect(&self, origin: CGPoint) -> Option<CGRect> {
        self.grid.shape.map(|_| CGRect::new(origin, self.info.frame.size))
    }

    /// The cell at `world`.
    pub(super) fn grid_workspace_at(&self, world: CGPoint) -> Option<&RuntimeWorkspaceData> {
        self.workspaces.iter().find(|ws| {
            self.grid_cell_rect(self.workspace_origin(ws.index))
                .is_some_and(|r| contains(r, world))
        })
    }

    /// The camera for the grid's stage; none without a grid.
    pub(super) fn grid_camera(&self) -> Option<Camera> {
        let shape = grid_shape(&self.workspaces)?;
        let area = self.grid_area();
        let look = self.grid.look;
        let camera = follow_camera(
            self.grid.camera.get(),
            current_cell(&self.workspaces, shape),
            area.size,
            self.info.frame.size,
            look,
        );
        if look.stage == GridStage::Cell {
            self.grid.camera.set(Some(camera));
        }
        Some(stage_camera(self.info.frame.size, area, shape, look, camera))
    }

    /// Open on the real focus.
    pub(super) fn open_grid(&mut self) { self.grid_select(false); }

    /// In a grid the selection is the real focus.
    fn grid_select(&mut self, rift_moved: bool) {
        if self.grid.shape.is_some()
            && let Some(selection) = grid_focus_selection(&self.workspaces, rift_moved)
        {
            self.selection = selection;
        }
    }

    /// Follow the focus: select it, and move the camera when the stage's camera
    /// moved (another current cell, or the grid buttons took more or less room),
    /// over `duration` once the overview is in.
    pub(super) fn follow_grid(&mut self, rift_moved: bool, duration: Option<f64>) {
        self.grid_select(rift_moved);
        let Some(camera) = self.grid_camera() else { return };
        if camera != self.home {
            self.home = camera;
            if let Some(duration) = duration {
                self.animate(camera, duration);
            }
        }
    }

    /// Whether a snapshot leaves the grid as it is shown: the focus, and the
    /// grid buttons.
    pub(super) fn grid_unchanged(
        &self,
        incoming: &[RuntimeWorkspaceData],
        topology: &[RuntimeDisplayData],
        catalog: &[RuntimeWorkspaceData],
    ) -> bool {
        if self.grid.shape.is_none() && grid_shape(incoming).is_none() {
            return true;
        }
        let focus = |workspaces: &[RuntimeWorkspaceData]| {
            workspaces
                .iter()
                .map(|ws| {
                    (
                        ws.is_active,
                        ws.layout_focus,
                        ws.windows.iter().map(|w| w.is_focused).collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        focus(&self.workspaces) == focus(incoming)
            && self.bar_matches(&bar_groups_for(topology, catalog, &self.info.display_uuid))
    }

    /// The cells' backdrops and the tiles' labels, after the windows changed.
    pub(super) fn sync_grid_layers(&mut self) {
        let grid = self.grid.shape.is_some();
        let gap = self.grid.look.gap_world(self.info.frame.size);
        self.grid.cells.retain(|(id, l)| {
            let keep = grid && self.workspaces.iter().any(|ws| ws.workspace_id == *id);
            if !keep {
                l.removeFromSuperlayer();
            }
            keep
        });
        let labels = grid && self.grid.look.labels;
        self.grid.labels.retain(|label| {
            let keep = labels && self.windows.iter().any(|w| w.data.id == label.id);
            if !keep {
                label.layer.removeFromSuperlayer();
            }
            keep
        });
        if !grid {
            return;
        }
        for ws in &self.workspaces {
            let Some(cell) = ws.grid_cell else { continue };
            let frame = CGRect::new(
                cell_origin(self.info.frame.size, gap, cell),
                self.info.frame.size,
            );
            let backdrop = match self.grid.cells.iter().find(|(id, _)| *id == ws.workspace_id) {
                Some((_, l)) => l.clone(),
                None => {
                    let l = layer(frame, self.scale);
                    l.setZPosition(0.);
                    l.setCornerRadius(CORNER);
                    l.setBorderColor(Some(&color(1.0, 1.0, 1.0, 0.55)));
                    self.pictures.insertSublayer_atIndex(&l, 0);
                    self.grid.cells.push((ws.workspace_id, l.clone()));
                    l
                }
            };
            backdrop.setFrame(frame);
            backdrop.setBackgroundColor(Some(&color(
                1.0,
                1.0,
                1.0,
                if ws.is_active { 0.065 } else { 0.035 },
            )));
        }
        if !labels {
            return;
        }
        for w in &self.windows {
            let index = match self.grid.labels.iter().position(|l| l.id == w.data.id) {
                Some(index) => index,
                None => {
                    self.grid.labels.push(TileLabel::new(w, self.scale));
                    self.grid.labels.len() - 1
                }
            };
            // A window's shadow layer can be replaced; keep the label on the current one.
            w.shadow.addSublayer(&self.grid.labels[index].layer);
        }
    }

    /// Keep the grid's outlines and labels their size on screen at `zoom`, and
    /// fade them with the overview's decorations.
    pub(super) fn update_grid_decorations(&self, zoom: f64, relayout: bool, fade: Option<f32>) {
        if self.grid.shape.is_none() {
            return;
        }
        if relayout {
            for (id, l) in &self.grid.cells {
                let active =
                    self.workspaces.iter().any(|ws| ws.workspace_id == *id && ws.is_active);
                l.setBorderWidth(if active { 1.0 / zoom } else { 0.0 });
            }
            for label in &self.grid.labels {
                if let Some(w) = self.windows.iter().find(|w| w.data.id == label.id) {
                    label.layout(
                        CGSize::new(w.world.size.width * zoom, w.world.size.height * zoom),
                        zoom,
                    );
                }
            }
        }
        if let Some(opacity) = fade {
            for (_, l) in &self.grid.cells {
                decoration_fade(l, false, opacity, self.decoration_duration);
            }
            for label in &self.grid.labels {
                decoration_fade(&label.layer, false, opacity, self.decoration_duration);
            }
        }
    }

    /// The window whose label's close button is at `local`, a little beyond it
    /// included.
    fn close_button_at(&self, local: CGPoint) -> Option<WindowServerId> {
        let zoom = self.applied.map_or(self.camera.zoom, |(camera, _)| camera.zoom);
        self.grid.labels.iter().find_map(|label| {
            if label.layer.isHidden() {
                return None;
            }
            let w = self.windows.iter().find(|w| w.data.id == label.id && !w.shadow.isHidden())?;
            let tile = self.camera.frame(w.world);
            let button = label.close.frame();
            let scale = self.camera.zoom / zoom;
            let hit = rect(
                tile.origin.x + button.origin.x * scale - 3.0,
                tile.origin.y + button.origin.y * scale - 3.0,
                button.size.width * scale + 6.0,
                button.size.height * scale + 6.0,
            );
            contains(hit, local).then_some(w.data.info.sys_id).flatten()
        })
    }
}

impl TileLabel {
    fn new(w: &SceneWindow, scale: f64) -> Self {
        let caption = layer(CGRect::ZERO, scale);
        caption.setAnchorPoint(CGPoint::ZERO);
        caption.setZPosition(2.0);
        caption.setBackgroundColor(Some(&color(0.06, 0.06, 0.08, 0.78)));
        caption.setMaskedCorners(
            objc2_quartz_core::CACornerMask::LayerMinXMinYCorner
                | objc2_quartz_core::CACornerMask::LayerMaxXMinYCorner,
        );
        let icon = w.icon_image.as_deref().map(|image| {
            let icon = layer(CGRect::ZERO, scale);
            image_contents(&icon, image);
            caption.addSublayer(&icon);
            icon
        });
        let title = label(
            &caption,
            if w.data.info.title.is_empty() {
                w.data.app_name.as_deref().unwrap_or("Window")
            } else {
                &w.data.info.title
            },
            CGRect::ZERO,
            scale,
        );
        unsafe { title.setAlignmentMode(objc2_quartz_core::kCAAlignmentLeft) };
        let close = label_close_button(&caption, scale);
        Self {
            id: w.data.id,
            layer: caption,
            title,
            icon,
            close,
        }
    }

    /// Lay the label out across the top of a tile `tile` big on screen; the
    /// label sits in the tile's world frame, scaled back by `zoom`.
    fn layout(&self, tile: CGSize, zoom: f64) {
        let (icon, pad) = (16.0, 8.0);
        let close = LABEL_CLOSE + 6.0;
        let text_x = pad + if self.icon.is_some() { icon + 6.0 } else { 0.0 };
        let width = tile.width;
        self.layer.setBounds(rect(0.0, 0.0, width, LABEL_ROW));
        self.layer.setPosition(CGPoint::ZERO);
        self.layer.setAffineTransform(CGAffineTransform {
            a: 1.0 / zoom,
            b: 0.0,
            c: 0.0,
            d: 1.0 / zoom,
            tx: 0.0,
            ty: 0.0,
        });
        self.layer.setCornerRadius(CORNER * zoom);
        self.layer.setHidden(tile.height < 2.0 * LABEL_ROW || width < 4.0 * LABEL_ROW);
        if let Some(layer) = &self.icon {
            layer.setFrame(rect(pad, (LABEL_ROW - icon) / 2.0, icon, icon));
        }
        self.title.setFrame(rect(
            text_x,
            (LABEL_ROW - 16.0) / 2.0,
            (width - text_x - pad - close).max(1.0),
            16.0,
        ));
        let at = (LABEL_ROW - LABEL_CLOSE) / 2.0;
        self.close
            .setFrame(rect(width - at - LABEL_CLOSE, at, LABEL_CLOSE, LABEL_CLOSE));
    }
}

impl OverviewSession {
    /// Note that rift ran a command, so the system's focus may lag behind it.
    pub(crate) fn note_command(&mut self) { self.rift_focus_at = Some(Instant::now()); }

    /// Whether rift moved the focus a moment ago, so the system's focus may lag.
    pub(super) fn rift_moved_focus(&self) -> bool {
        self.rift_focus_at.is_some_and(|at| at.elapsed() < RIFT_FOCUS_BRIDGE)
    }

    /// The display under `point` and `point` in it.
    fn display_at(&self, point: CGPoint) -> Option<(&DisplayOverview, CGPoint)> {
        let d = self.displays.iter().find(|d| contains(d.bounds, point))?;
        Some((d, d.local(point)))
    }

    /// The grid (0-based) whose button in the grid row is under `point`.
    fn grid_button_at(&self, point: CGPoint) -> Option<usize> {
        let (d, local) = self.display_at(point)?;
        d.grid_button_at(local)
    }

    /// Whether `point` is on a display's grid row, which covers the cells under it.
    pub(super) fn on_grid_row(&self, point: CGPoint) -> bool {
        self.display_at(point).is_some_and(|(d, local)| d.on_grid_row(local))
    }

    /// Outline the grid button a window dragged to `point` would go to.
    pub(super) fn outline_grid_buttons(&self, point: CGPoint) {
        let target = self.grid_button_at(point);
        for d in &self.displays {
            d.outline_grid_button(target.filter(|_| contains(d.bounds, point)));
        }
    }

    /// The fork's input before the overview's own: a label's close button, a
    /// window dropped on a grid button, the grid buttons, and moving the focus
    /// in a grid. None when `input` is the overview's.
    pub(super) fn grid_input(&mut self, input: &Input) -> Option<Option<OverviewAction>> {
        let close = |s: &Self, p: CGPoint| s.display_at(p).and_then(|(d, l)| d.close_button_at(l));
        match *input {
            // Pressed on the button, not a drag of the tile under it.
            Input::PointerDown(p) if close(self, p).is_some() => return Some(None),
            Input::PointerUp(p) | Input::Click(p) if self.drag.is_none() => {
                if let Some(id) = close(self, p) {
                    return Some(Some(OverviewAction::CloseWindow(id)));
                }
                if let Some((display, selection)) = self.cell_clicked(p) {
                    self.pressed = None;
                    NSCursor::arrowCursor().set();
                    self.activate_display(display);
                    self.change_selection(selection);
                    return Some(self.activate(false));
                }
            }
            // Over the grid row a window goes to the grid of the button it is dropped on.
            Input::PointerUp(p) | Input::Click(p) if self.drag.is_some() && self.on_grid_row(p) => {
                let drag = self.drag.take()?;
                for d in &self.displays {
                    d.outline_grid_button(None);
                }
                let grid = self.grid_button_at(p).filter(|_| drag.moved || drag.pending);
                let window = drag.window;
                self.end_drag(drag, true);
                return Some(grid.map(|grid| {
                    OverviewAction::Command(crate::layout_engine::LayoutCommand::SendWindowToGrid {
                        window_id: window.idx.get(),
                        grid: grid + 1,
                    })
                }));
            }
            _ => {}
        }
        if let Some(action) = self.grid_bar_input(input) {
            return Some(action);
        }
        self.grid_keys(input)
    }

    /// A click, not a pan, on a cell away from its windows, labels and
    /// controls: the display and the selection that goes into that cell.
    fn cell_clicked(&self, point: CGPoint) -> Option<(usize, Selection)> {
        if let Some((start, _)) = self.pressed
            && (point.x - start.x).hypot(point.y - start.y) >= 4.0
        {
            return None;
        }
        let display = self.displays.iter().position(|d| contains(d.bounds, point))?;
        let d = &self.displays[display];
        d.grid.shape?;
        let local = d.local(point);
        if self.on_grid_row(point)
            || d.toolbar_item_at(local).is_some()
            || d.workspace_label_at(local).is_some()
            || d.hit(point).is_some()
        {
            return None;
        }
        let ws = d.grid_workspace_at(d.camera.unproject(local))?;
        Some((display, Selection {
            workspace: ws.index,
            window: ws.focused_window,
        }))
    }

    /// Arrow keys and Tab move the real focus in a grid; the overview follows.
    fn grid_keys(&self, input: &Input) -> Option<Option<OverviewAction>> {
        use crate::layout_engine::{Direction, LayoutCommand};
        self.displays.get(self.active)?.grid.shape?;
        let command = match *input {
            Input::Up => LayoutCommand::Focus(Direction::Up),
            Input::Down => LayoutCommand::Focus(Direction::Down),
            Input::WorkspaceSteps(steps) if steps < 0 => LayoutCommand::Focus(Direction::Up),
            Input::WorkspaceSteps(steps) if steps > 0 => LayoutCommand::Focus(Direction::Down),
            Input::Left => LayoutCommand::Focus(Direction::Left),
            Input::Right => LayoutCommand::Focus(Direction::Right),
            Input::WindowSteps(steps) if steps < 0 => LayoutCommand::Focus(Direction::Left),
            Input::WindowSteps(steps) if steps > 0 => LayoutCommand::Focus(Direction::Right),
            Input::Cycle(true) => LayoutCommand::NextWindow,
            Input::Cycle(false) => LayoutCommand::PrevWindow,
            _ => return None,
        };
        Some(Some(OverviewAction::Command(command)))
    }

    /// Pressing, dragging and letting go of a grid button: let go on another
    /// display's row, the grid goes to that display; let go where it was
    /// pressed, the grid shows. None when `input` is not the buttons'.
    fn grid_bar_input(&mut self, input: &Input) -> Option<Option<OverviewAction>> {
        use crate::layout_engine::LayoutCommand;
        let pressed = self
            .displays
            .iter()
            .position(|d| d.grid.bar.as_ref().is_some_and(GridBar::is_pressed));
        match (input, pressed) {
            (&Input::PointerDown(p), _) => {
                let d = self.displays.iter_mut().find(|d| contains(d.bounds, p))?;
                let at = d.local(p);
                let bar = d.grid.bar.as_mut().filter(|bar| bar.contains(at))?;
                bar.press(at);
                Some(None)
            }
            (&(Input::PointerDrag(p) | Input::Pan(p) | Input::Move(p)), Some(i)) => {
                let d = &mut self.displays[i];
                let at = d.local(p);
                d.grid.bar.as_mut()?.drag(at);
                Some(None)
            }
            (&(Input::PointerUp(p) | Input::Click(p)), Some(i)) => {
                let d = &mut self.displays[i];
                let at = d.local(p);
                let released = d.grid.bar.as_mut()?.release(at);
                Some(released.map(|(button, to)| {
                    OverviewAction::Command(match to {
                        Some(display) => LayoutCommand::MoveGridToDisplay {
                            grid: button.grid + 1,
                            display: crate::model::reactor::DisplaySelector::Uuid(display),
                        },
                        None => LayoutCommand::Grid(button.grid + 1),
                    })
                }))
            }
            (&(Input::PointerUp(p) | Input::Click(p)), None) if self.on_grid_row(p) => Some(None),
            _ => None,
        }
    }

    /// Step a grid overview back from the current cell to the whole grid. False when
    /// there is no further stage, and opening Overview again closes it.
    pub(crate) fn step_back(&mut self) -> bool {
        self.set_stage(GridStage::Cell, GridStage::Whole, |_| true)
    }

    /// Esc: step a grid overview in from the whole grid to the current cell. False
    /// when it is already there, or the whole grid is its only stage, and Esc
    /// closes it.
    pub(crate) fn step_in(&mut self) -> bool {
        self.set_stage(GridStage::Whole, GridStage::Cell, |look| !look.fit)
    }

    /// Move each grid display at stage `from` (and `allowed`) to stage `to`.
    fn set_stage(
        &mut self,
        from: GridStage,
        to: GridStage,
        allowed: impl Fn(GridLook) -> bool,
    ) -> bool {
        if matches!(self.phase, Phase::Exiting { .. }) {
            return false;
        }
        let stepping = |d: &DisplayOverview| {
            d.grid.shape.is_some() && d.grid.look.stage == from && allowed(d.grid.look)
        };
        if !self.displays.iter().any(stepping) {
            return false;
        }
        let _tx = LayerTransaction::disabled().flush_on_commit();
        // Before the overview has come in, its entry goes to the new stage.
        let duration = (!matches!(self.phase, Phase::Waiting(_))).then_some(self.duration);
        for d in self.displays.iter_mut().filter(|d| stepping(d)) {
            d.grid.look.stage = to;
            d.grid.camera.set(None);
            let Some(camera) = d.grid_camera() else { continue };
            d.home = camera;
            if let Some(duration) = duration {
                d.animate(camera, duration);
                d.policy_dirty = true;
                d.apply_camera(d.applied.map_or(1., |(_, opacity)| opacity));
            }
        }
        true
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
            focused_window: None,
            display_frame: rect(0.0, 0.0, 1600.0, 1000.0),
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

    fn look(stage: GridStage, zoom: f64, follow: GridFollow) -> GridLook {
        GridLook {
            gap: 0.04,
            labels: true,
            zoom,
            stage,
            fit: false,
            follow,
            full: false,
        }
    }

    #[test]
    fn a_grid_keeps_every_cell_of_the_grid_on_screen() {
        let workspaces: Vec<_> = (0..9)
            .map(|index| RuntimeWorkspaceData {
                grid_cell: Some((index / 3, index % 3)),
                ..workspace(index, Vec::new())
            })
            .collect();
        assert_eq!(grid_shape(&workspaces), Some((3, 3)));
        assert_eq!(grid_shape(&workspaces[..1]), Some((1, 1)));
        assert_eq!(grid_shape(&[workspace(0, Vec::new())]), None);

        let mut filtered = workspaces.clone();
        filter_workspaces(&mut filtered, false);
        assert_eq!(filtered.len(), 9, "empty cells keep their place");
        let mut plain = vec![
            workspace(0, Vec::new()),
            workspace(1, vec![window(1, rect(0.0, 0.0, 10.0, 10.0), false)]),
        ];
        filter_workspaces(&mut plain, false);
        assert_eq!(plain.len(), 1, "outside a grid empty workspaces go");

        let empty: GridOverviewSettings = toml::from_str("").unwrap();
        assert_eq!(
            GridLook::from(&GridOverviewSettings::default(), false),
            GridLook::from(&empty, false)
        );
    }

    #[test]
    fn the_first_stage_shows_the_current_cell_at_the_grid_zoom() {
        let display = CGSize::new(1600.0, 1000.0);
        let area = rect(0.0, 0.0, 1600.0, 1000.0);
        let cell = look(GridStage::Cell, 0.5, GridFollow::Center);
        let gap = cell.gap_world(display);
        assert_eq!(cell.gap_px(display), 40.0, "a fraction of the shorter side");
        let camera = stage_camera(display, area, (3, 3), cell, (1.0, 1.0));
        assert_eq!(camera.zoom, 0.5);
        let center = cell_origin(display, gap, (1, 1));
        let frame = camera.frame(CGRect::new(center, display));
        assert_eq!(
            frame,
            rect(400.0, 250.0, 800.0, 500.0),
            "zoom of the display, centered"
        );
        let right = camera.frame(CGRect::new(cell_origin(display, gap, (1, 2)), display));
        assert!(
            (right.origin.x - (frame.max().x + 40.0)).abs() < 1e-9,
            "the neighbour a gap away"
        );

        // Below the grid buttons the cell centers in what is left.
        let below = rect(0.0, 100.0, 1600.0, 900.0);
        let camera = stage_camera(display, below, (3, 3), cell, (1.0, 1.0));
        let frame = camera.frame(CGRect::new(center, display));
        assert!((frame.mid().y - below.mid().y).abs() < 1e-9);
    }

    #[test]
    fn the_whole_grid_is_fitted_and_centered_never_larger_than_the_zoom() {
        let display = CGSize::new(1600.0, 1000.0);
        let area = rect(0.0, 0.0, 1600.0, 1000.0);
        let whole = look(GridStage::Whole, 0.5, GridFollow::Center);
        let gap = whole.gap_world(display);
        let camera = stage_camera(display, area, (3, 3), whole, (0.0, 0.0));
        let first = camera.frame(CGRect::new(cell_origin(display, gap, (0, 0)), display));
        let last = camera.frame(CGRect::new(cell_origin(display, gap, (2, 2)), display));
        assert!(first.size.width < 800.0, "smaller than the first stage");
        assert!(first.origin.x >= 0.0 && first.origin.y >= 0.0);
        assert!(last.max().x <= area.size.width && last.max().y <= area.size.height);
        assert!(((first.origin.x + last.max().x) / 2.0 - area.mid().x).abs() < 1e-9);
        assert!(((first.origin.y + last.max().y) / 2.0 - area.mid().y).abs() < 1e-9);

        let small = look(GridStage::Whole, 0.3, GridFollow::Center);
        let camera = stage_camera(display, area, (1, 1), small, (0.0, 0.0));
        assert_eq!(camera.zoom, 0.3);
    }

    #[test]
    fn the_bar_lists_each_displays_grids_and_one_free_grid() {
        let ws =
            |index: usize, windows: Vec<RuntimeWindowData>, active: bool| RuntimeWorkspaceData {
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
        assert_eq!(
            bar_groups(&displays, "b")[1].buttons.last(),
            Some(&button(4, Shown::No, true))
        );

        let mut workspaces = here;
        keep_grid_on_screen(&mut workspaces);
        assert_eq!(workspaces.len(), 9, "only the grid on screen");
        assert!(workspaces.iter().all(|ws| ws.grid == Some(2)));
    }

    #[test]
    fn reveal_moves_the_camera_only_to_show_the_current_cell() {
        let display = CGSize::new(1600.0, 1000.0);
        let look = look(GridStage::Cell, 0.5, GridFollow::Reveal);
        let camera = |camera, active| follow_camera(camera, active, display, display, look);
        // Opening: on the current cell.
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
        // A current cell already on show leaves the camera where it is.
        assert_eq!(camera(Some((1.2, 1.3)), (1, 1)), (1.2, 1.3));
        // Centering puts the camera on the current cell every time.
        let center = GridLook {
            follow: GridFollow::Center,
            ..look
        };
        assert_eq!(
            follow_camera(Some((1.0, 1.0)), (1, 2), display, display, center),
            (1.0, 2.0)
        );
        // A cell larger than the view with its margins is centered.
        let big = GridLook { zoom: 0.99, ..look };
        assert_eq!(
            follow_camera(Some((1.0, 1.0)), (1, 2), display, display, big),
            (1.0, 2.0)
        );
    }

    #[test]
    fn grid_selection_follows_the_system_focus_in_the_current_cell() {
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

        // The system's focus wins over the layout's within the current cell.
        let ws = cells(1, [vec![], vec![tile(1, false), tile(2, true)]], id(1));
        assert_eq!(select(&ws), Selection { workspace: 1, window: id(2) });
        // Until the system catches up with a move into another cell, the layout's
        // focus stands in.
        let ws = cells(1, [vec![tile(3, true)], vec![tile(1, false)]], id(1));
        assert_eq!(select(&ws), Selection { workspace: 1, window: id(1) });
        // Right after rift moved the focus, its target wins until the system catches up.
        let ws = cells(1, [vec![], vec![tile(1, false), tile(2, true)]], id(1));
        assert_eq!(grid_focus_selection(&ws, true).unwrap(), Selection {
            workspace: 1,
            window: id(1)
        });
        // An empty current cell has no focused window.
        let ws = cells(1, [vec![tile(3, true)], vec![]], None);
        assert_eq!(select(&ws), Selection { workspace: 1, window: None });
    }
}
