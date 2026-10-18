//! The grid Overview drawn as the slide between cells is: the screen draws back
//! until the grid shows, each cell its wallpaper with captures of its windows
//! where the layout puts them, and comes in again to the cell chosen.

use objc2_core_foundation::CGAffineTransform;

use super::*;
use crate::common::collections::HashSet;
use crate::common::config::GridFollow;

/// A window of a cell, in the overlay's coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct OverviewWindow {
    pub id: u32,
    pub frame: CGRect,
    pub floating: bool,
    /// Its app's name and title; none for no label.
    pub label: Option<String>,
}

/// A cell of the grid shown, at its row and column.
pub struct OverviewCell<T> {
    pub cell: (usize, usize),
    pub windows: Vec<OverviewWindow>,
    /// What the caller knows the cell by.
    pub key: T,
}

/// How the Overview steps back, from the overview settings.
#[derive(Clone, Copy, Debug)]
pub struct OverviewStages {
    /// A cell's size in the first stage, as a fraction of the display; the
    /// whole grid is never shown larger.
    pub zoom: f64,
    /// The whole grid is the only stage.
    pub fit: bool,
    /// How the first stage follows the active cell.
    pub follow: GridFollow,
}

/// Where the camera looks: the scale it shows the world at and where the
/// world's origin is, in the overlay's points.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    scale: f64,
    position: CGPoint,
}

struct Cell<T> {
    at: (usize, usize),
    key: T,
    layer: Retained<CALayer>,
    back: Retained<CALayer>,
    /// The screen as it was when the Overview opened, over the cell it showed
    /// until that cell changes.
    screen: Option<Retained<CALayer>>,
    windows: Vec<OverviewWindow>,
}

struct Tile {
    layer: Retained<CALayer>,
    /// The size its capture was last asked for at.
    size: CGSize,
    /// Over the cell's screen as well as the tile: the focus outline and label.
    outline: Retained<CALayer>,
    label: Option<Retained<CALayer>>,
}

pub struct GridOverview<T> {
    generation: u64,
    frame: CGRect,
    columns: usize,
    /// The cell at the world's origin: the one the screen showed at first.
    home: (usize, usize),
    /// The cell on screen under the Overview.
    active: (usize, usize),
    selected: (usize, usize),
    cells: Vec<Cell<T>>,
    tiles: HashMap<u32, Tile>,
    focused: Option<u32>,
    wallpaper: Option<Image>,
    ring: Retained<CALayer>,
    gap: f64,
    stages: OverviewStages,
    whole: View,
    /// Where the camera is going.
    view: View,
    motion: Motion,
    root: Retained<CALayer>,
    camera: Retained<CALayer>,
    world: Retained<CALayer>,
    scale: f64,
    _surface: WindowSurface,
    _window: CgsWindow,
}

impl<T> Drop for GridOverview<T> {
    fn drop(&mut self) {
        release_layer_tree(&self.root);
        CATransaction::flush();
    }
}

/// How round a cell's corners look once the grid shows whole, in points.
const CORNER: f64 = 10.0;
/// The selection ring's width and its distance from the cell, in points.
const RING: f64 = 3.0;
const RING_GAP: f64 = 4.0;
/// The focused window's outline, in points.
const FOCUS: f64 = 2.0;
/// A label's height and its distance from its tile's bottom left corner, in points.
const LABEL_ROW: f64 = 22.0;
const LABEL_INSET: f64 = 6.0;

impl<T: PartialEq> GridOverview<T> {
    /// Cover `frame` with `screen`, the display as it is, showing cell `home`
    /// of a grid `columns` wide and high, then draw back to the first of
    /// `stages`, with `gap` points between cells once the whole grid shows.
    pub fn open(
        generation: u64,
        frame: CGRect,
        scale: f64,
        screen: &CGImage,
        columns: usize,
        home: (usize, usize),
        cells: Vec<OverviewCell<T>>,
        gap: f64,
        stages: OverviewStages,
        motion: Motion,
    ) -> Result<Self, CgsWindowError> {
        let size = frame.size;
        let columns = columns.max(1);
        let n = columns as f64;
        // The grid fills nine tenths of the screen, `gap` apart as it shows.
        let fit = |side: f64| (0.9 * side - (n - 1.0) * gap) / (n * side);
        let whole_scale = fit(size.width).min(fit(size.height)).min(stages.zoom).max(0.05);
        let world_gap = gap / whole_scale;
        let step = CGSize::new(size.width + world_gap, size.height + world_gap);
        let middle = (n - 1.0) / 2.0;
        let whole = View {
            scale: whole_scale,
            position: CGPoint::new(
                -(middle - home.1 as f64) * step.width,
                -(middle - home.0 as f64) * step.height,
            ),
        };
        let root = CALayer::layer();
        let camera = CALayer::layer();
        let world = CALayer::layer();
        let ring = CALayer::layer();
        with_disabled_actions(|| {
            root.setFrame(rect(0.0, 0.0, size.width, size.height));
            root.setContentsScale(scale);
            root.setGeometryFlipped(true);
            root.setMasksToBounds(true);
            root.setBackgroundColor(Some(&color(0.03, 0.03, 0.04, 1.0)));
            camera.setFrame(rect(0.0, 0.0, size.width, size.height));
            world.setAnchorPoint(CGPoint::new(0.0, 0.0));
            world.setPosition(CGPoint::new(0.0, 0.0));
            world.setBounds(rect(0.0, 0.0, size.width, size.height));
            ring.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
            ring.setOpacity(0.0);
            ring.setZPosition(5.0);
            world.addSublayer(&ring);
            camera.addSublayer(&world);
            root.addSublayer(&camera);
        });
        let window = CgsWindow::new_compositor(frame, 0.0)?;
        window.set_resolution(scale)?;
        window.set_level(NSPopUpMenuWindowLevel as i32)?;
        let surface = WindowSurface::new_scaled(window.id(), root.bounds(), &root, scale)?;
        let mut overview = Self {
            generation,
            frame,
            columns,
            home,
            active: home,
            selected: home,
            cells: Vec::new(),
            tiles: HashMap::default(),
            focused: None,
            wallpaper: None,
            ring,
            gap: world_gap,
            stages,
            whole,
            view: View { scale: 1.0, position: CGPoint::new(0.0, 0.0) },
            motion,
            root,
            camera,
            world,
            scale,
            _surface: surface,
            _window: window,
        };
        with_disabled_actions(|| {
            overview.set_cells(cells);
            if let Some(cell) = overview.cells.iter_mut().find(|c| c.at == home) {
                let layer = CALayer::layer();
                layer.setFrame(rect(0.0, 0.0, size.width, size.height));
                layer.setContentsScale(scale);
                layer.setZPosition(4.0);
                set_contents(&layer, screen);
                cell.layer.addSublayer(&layer);
                cell.screen = Some(layer);
            }
        });
        overview._surface.flush();
        overview._window.order_above(None)?;
        CATransaction::flush();
        let first = if stages.fit { whole } else { overview.cell_view(home) };
        overview.go(first);
        Ok(overview)
    }

    pub fn generation(&self) -> u64 { self.generation }

    /// The windows to capture: all of them, for the cells as first shown.
    pub fn windows(&self) -> Vec<(u32, CGSize)> {
        self.tiles.iter().map(|(id, tile)| (*id, tile.size)).collect()
    }

    /// The cell selected, and what the caller knows it by.
    pub fn selected(&self) -> Option<&T> {
        self.cells.iter().find(|c| c.at == self.selected).map(|c| &c.key)
    }

    pub fn selected_is_active(&self) -> bool { self.selected == self.active }

    /// Select the cell on screen under the Overview.
    pub fn select_active(&mut self) { self.select(self.active); }

    /// Whether the whole grid shows, the last stage.
    pub fn shows_whole(&self) -> bool { self.view == self.whole }

    /// Step back to the whole grid.
    pub fn show_whole(&mut self) { self.go(self.whole); }

    /// Come forward to the first stage, around the active cell.
    pub fn show_cell(&mut self) {
        self.select(self.active);
        let view = self.cell_view(self.active);
        self.go(view);
    }

    /// Show `image` for the cells' wallpaper.
    pub fn set_wallpaper(&mut self, image: Image) {
        with_disabled_actions(|| {
            for cell in &self.cells {
                set_contents(&cell.back, &image);
            }
        });
        self.wallpaper = Some(image);
        CATransaction::flush();
    }

    /// Show the grid's cells as they are now, keeping the captures of windows
    /// still in them. Returns the windows to capture: those new to the
    /// Overview, and those whose size changed.
    pub fn update(&mut self, cells: Vec<OverviewCell<T>>) -> Vec<(u32, CGSize)> {
        let wanted = with_disabled_actions(|| self.set_cells(cells));
        CATransaction::flush();
        wanted
    }

    fn set_cells(&mut self, cells: Vec<OverviewCell<T>>) -> Vec<(u32, CGSize)> {
        let size = self.frame.size;
        let step = CGSize::new(size.width + self.gap, size.height + self.gap);
        let mut old = std::mem::take(&mut self.cells);
        let mut wanted = Vec::new();
        let mut kept = HashSet::default();
        for new in cells {
            let mut cell = match old.iter().position(|c| c.at == new.cell) {
                Some(i) => {
                    let mut cell = old.swap_remove(i);
                    if (cell.key != new.key || cell.windows != new.windows)
                        && let Some(screen) = cell.screen.take()
                    {
                        screen.removeFromSuperlayer();
                    }
                    cell.key = new.key;
                    cell
                }
                None => {
                    let at = cell_origin(new.cell, self.home, step);
                    let layer = CALayer::layer();
                    layer.setFrame(rect(at.x, at.y, size.width, size.height));
                    layer.setContentsScale(self.scale);
                    layer.setMasksToBounds(true);
                    layer.setCornerRadius(self.corner());
                    layer.setBackgroundColor(Some(&color(0.08, 0.08, 0.1, 1.0)));
                    let back = CALayer::layer();
                    back.setFrame(rect(0.0, 0.0, size.width, size.height));
                    back.setContentsScale(self.scale);
                    if let Some(image) = &self.wallpaper {
                        set_contents(&back, image);
                    }
                    layer.addSublayer(&back);
                    self.world.addSublayer(&layer);
                    Cell {
                        at: new.cell,
                        key: new.key,
                        layer,
                        back,
                        screen: None,
                        windows: Vec::new(),
                    }
                }
            };
            let mut windows = new.windows;
            windows.sort_by_key(|w| w.floating);
            for (z, window) in windows.iter().enumerate() {
                kept.insert(window.id);
                let mut tile = self.tiles.remove(&window.id).unwrap_or_else(|| {
                    let layer = CALayer::layer();
                    layer.setContentsScale(self.scale);
                    // Captures fill the tile, of whatever size they were taken at.
                    unsafe { layer.setContentsGravity(objc2_quartz_core::kCAGravityResize) };
                    placeholder(&layer);
                    let outline = CALayer::layer();
                    outline.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
                    outline.setZPosition(5.0);
                    Tile { layer, size: CGSize::new(0.0, 0.0), outline, label: None }
                });
                if !same_size(tile.size, window.frame.size) {
                    tile.size = window.frame.size;
                    wanted.push((window.id, tile.size));
                }
                tile.layer.setFrame(window.frame);
                tile.layer.setZPosition(z as f64 * 0.01);
                cell.layer.addSublayer(&tile.layer);
                tile.outline.setFrame(window.frame);
                cell.layer.addSublayer(&tile.outline);
                if let Some(label) = tile.label.take() {
                    label.removeFromSuperlayer();
                }
                tile.label = window.label.as_deref().and_then(|text| self.label(text, window.frame));
                if let Some(label) = &tile.label {
                    cell.layer.addSublayer(label);
                }
                self.tiles.insert(window.id, tile);
            }
            cell.windows = windows;
            self.cells.push(cell);
        }
        for cell in old {
            cell.layer.removeFromSuperlayer();
        }
        self.tiles.retain(|id, tile| {
            let keep = kept.contains(id);
            if !keep {
                tile.layer.removeFromSuperlayer();
                tile.outline.removeFromSuperlayer();
                if let Some(label) = &tile.label {
                    label.removeFromSuperlayer();
                }
            }
            keep
        });
        self.outline_focused();
        wanted
    }

    /// Show `image` for window `id`.
    pub fn set_window(&self, id: u32, image: &CGImage) {
        let Some(tile) = self.tiles.get(&id) else { return };
        with_disabled_actions(|| {
            tile.layer.setBackgroundColor(None);
            tile.layer.setCornerRadius(0.0);
            set_contents(&tile.layer, image);
        });
        CATransaction::flush();
    }

    /// Note the cell on screen under the Overview and the window focused, and
    /// select that cell.
    pub fn set_active(&mut self, cell: (usize, usize), focused: Option<u32>) {
        self.active = cell;
        if self.focused != focused {
            self.focused = focused;
            with_disabled_actions(|| self.outline_focused());
        }
        self.select(cell);
        CATransaction::flush();
    }

    fn outline_focused(&self) {
        let width = if self.view.scale < 1.0 { FOCUS / self.view.scale } else { 0.0 };
        for (id, tile) in &self.tiles {
            tile.outline.setBorderWidth(if Some(*id) == self.focused { width } else { 0.0 });
        }
    }

    /// Select the cell under `point`, in global coordinates; false when there
    /// is none.
    pub fn select_at(&mut self, point: CGPoint) -> bool {
        let View { scale, position } = self.view;
        let (w, h) = (self.frame.size.width, self.frame.size.height);
        let local = CGPoint::new(point.x - self.frame.origin.x, point.y - self.frame.origin.y);
        let world = CGPoint::new(
            (local.x - w / 2.0) / scale + w / 2.0 - position.x,
            (local.y - h / 2.0) / scale + h / 2.0 - position.y,
        );
        let step = CGSize::new(w + self.gap, h + self.gap);
        let column = (world.x / step.width).floor() + self.home.1 as f64;
        let row = (world.y / step.height).floor() + self.home.0 as f64;
        let within = world.x.rem_euclid(step.width) < w && world.y.rem_euclid(step.height) < h;
        let n = self.columns as f64;
        if !within || column < 0.0 || row < 0.0 || column >= n || row >= n {
            return false;
        }
        self.select((row as usize, column as usize));
        true
    }

    fn select(&mut self, cell: (usize, usize)) {
        if cell == self.selected {
            return;
        }
        self.selected = cell;
        self.place_ring(true);
        if !self.shows_whole() {
            let view = self.cell_view(cell);
            if view != self.view {
                self.go(view);
            }
        }
        CATransaction::flush();
    }

    /// The first stage's view with `cell` in it: centered, or as `follow`
    /// says, moved from where the camera is only as far as it takes to show
    /// the cell whole, a gap from the edges.
    fn cell_view(&self, cell: (usize, usize)) -> View {
        let size = self.frame.size;
        let scale = self.stages.zoom;
        let step = CGSize::new(size.width + self.gap, size.height + self.gap);
        let at = cell_origin(cell, self.home, step);
        let centered = CGPoint::new(-at.x, -at.y);
        if self.view.scale != scale || self.stages.follow == GridFollow::Center {
            return View { scale, position: centered };
        }
        // On screen, a world point p is at the center plus `scale` times
        // (p + position - center); keep the cell `gap` points inside.
        let margin = self.gap * scale;
        let reach = |from: f64, cell: f64, side: f64| {
            let least = (margin - side / 2.0) / scale + side / 2.0 - cell;
            let most = (side / 2.0 - margin) / scale + side / 2.0 - cell - side;
            if least > most { -cell } else { from.clamp(least, most) }
        };
        let position = CGPoint::new(
            reach(self.view.position.x, at.x, size.width),
            reach(self.view.position.y, at.y, size.height),
        );
        View { scale, position }
    }

    fn place_ring(&self, glide_there: bool) {
        let size = self.frame.size;
        let step = CGSize::new(size.width + self.gap, size.height + self.gap);
        let at = cell_origin(self.selected, self.home, step);
        let scale = self.view.scale;
        let out = (RING_GAP + RING) / scale;
        let to = rect(at.x - out, at.y - out, size.width + 2.0 * out, size.height + 2.0 * out);
        let from = unsafe { self.ring.presentationLayer() }.map_or(self.ring.frame(), |l| l.frame());
        with_disabled_actions(|| {
            self.ring.setBorderWidth(RING / scale);
            self.ring.setCornerRadius((CORNER + RING_GAP) / scale);
            self.ring.setFrame(to);
        });
        if glide_there && from != to {
            glide_with(&self.ring, from, 0.18, &timing());
        }
    }

    /// Come in to the cell selected; returns how long it takes, in seconds.
    pub fn enter(&mut self) -> f64 {
        let size = self.frame.size;
        let step = CGSize::new(size.width + self.gap, size.height + self.gap);
        let at = cell_origin(self.selected, self.home, step);
        self.go(View { scale: 1.0, position: CGPoint::new(-at.x, -at.y) })
    }

    fn corner(&self) -> f64 { if self.view.scale < 1.0 { CORNER / self.view.scale } else { 0.0 } }

    /// Move the camera to `view`, from wherever it is; returns how long it takes.
    fn go(&mut self, view: View) -> f64 {
        let motion = self.motion;
        self.view = view;
        let timing = curve_timing(motion.curve.unwrap_or(DEFAULT_CURVE));
        let make = |key: &str, from: &AnyObject| match motion.curve {
            Some(_) => basic(key, from, motion.duration, &timing),
            None => spring_animation(key, from, motion.duration),
        };
        let animate_from = |layer: &CALayer, key: &str, from: f64| {
            let animation = make(key, &NSNumber::new_f64(from));
            layer.addAnimation_forKey(&animation, Some(&NSString::from_str(key)));
            animation.duration()
        };
        // `transform.scale` averages in the depth scale, which stays 1, so
        // both ends are given. Mid-animation it scales the depth too, and the
        // transform is no longer affine: `affineTransform` reads 1 then, and a
        // move during a move would start from the screen at full size.
        let scale_key = NSString::from_str("transform.scale.x");
        let scale_to = |layer: &CALayer, to: f64| {
            let shown = unsafe { layer.presentationLayer() }
                .map_or_else(|| objc2::Message::retain(layer), |shown| shown)
                .valueForKeyPath(&scale_key)
                .and_then(|value| value.downcast::<NSNumber>().ok())
                .map_or(to, |value| value.as_f64());
            let animation = make("transform.scale", &NSNumber::new_f64(shown));
            unsafe { animation.setToValue(Some(&NSNumber::new_f64(to))) };
            layer.setAffineTransform(CGAffineTransform { a: to, b: 0.0, c: 0.0, d: to, tx: 0.0, ty: 0.0 });
            layer.addAnimation_forKey(&animation, Some(&NSString::from_str("transform.scale")));
            animation.duration()
        };
        let shown_position = unsafe { self.world.presentationLayer() }
            .map_or_else(|| self.world.position(), |shown| shown.position());
        let corner = self.corner();
        let entering = view.scale >= 1.0;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let mut length = scale_to(&self.camera, view.scale);
        let position = make("position", &point_value(shown_position));
        self.world.setPosition(view.position);
        self.world.addAnimation_forKey(&position, Some(&NSString::from_str("position")));
        length = length.max(position.duration());
        for cell in &self.cells {
            let shown = unsafe { cell.layer.presentationLayer() }
                .map_or(cell.layer.cornerRadius(), |shown| shown.cornerRadius());
            cell.layer.setCornerRadius(corner);
            animate_from(&cell.layer, "cornerRadius", shown);
        }
        // Labels and the focus outline keep their size on screen.
        for (id, tile) in &self.tiles {
            if let Some(label) = &tile.label {
                scale_to(label, 1.0 / view.scale);
                if entering {
                    label.setOpacity(0.0);
                    animate_from(label, "opacity", 1.0);
                }
            }
            if Some(*id) == self.focused {
                let shown = tile.outline.borderWidth();
                tile.outline.setBorderWidth(if entering { 0.0 } else { FOCUS / view.scale });
                animate_from(&tile.outline, "borderWidth", shown);
            }
        }
        let ring_shown = self.ring.opacity() as f64;
        if entering {
            self.ring.setOpacity(0.0);
            animate_with(&self.ring, "opacity", &NSNumber::new_f64(ring_shown), motion.duration, &timing);
        } else {
            let (width, round, frame) =
                (self.ring.borderWidth(), self.ring.cornerRadius(), self.ring.frame());
            self.ring.setOpacity(1.0);
            self.place_ring(false);
            if ring_shown < 1.0 {
                animate_with(&self.ring, "opacity", &NSNumber::new_f64(0.0), motion.duration, &timing);
            } else {
                // The ring keeps its width on screen as the scale changes.
                animate_with(&self.ring, "borderWidth", &NSNumber::new_f64(width), motion.duration, &timing);
                animate_with(&self.ring, "cornerRadius", &NSNumber::new_f64(round), motion.duration, &timing);
                glide_with(&self.ring, frame, motion.duration, &timing);
            }
        }
        CATransaction::commit();
        CATransaction::flush();
        length
    }

    /// A label reading `text` for a tile at `frame`, kept at its size on
    /// screen at the tile's bottom left corner; none when the tile is too small for it
    /// once the whole grid shows.
    fn label(&self, text: &str, frame: CGRect) -> Option<Retained<CALayer>> {
        let whole = self.whole.scale;
        let room = frame.size.width * whole - 2.0 * LABEL_INSET;
        if frame.size.height * whole < LABEL_ROW + 2.0 * LABEL_INSET || room < 40.0 {
            return None;
        }
        let pad = 8.0;
        let title = CATextLayer::layer();
        title.setContentsScale(self.scale);
        let font = NSFont::systemFontOfSize(12.0);
        unsafe {
            title.setFont(Some(&*(Retained::as_ptr(&font) as *const CFType)));
            title.setTruncationMode(objc2_quartz_core::kCATruncationEnd);
            let text = CFString::from_str(text);
            title.setString(Some(&*(text.as_ref() as *const AnyObject)));
        }
        title.setFontSize(12.0);
        title.setForegroundColor(Some(&color(0.94, 0.94, 0.97, 0.92)));
        let width = (title.preferredFrameSize().width + 2.0 * pad).min(room);
        title.setFrame(rect(pad, (LABEL_ROW - 16.0) / 2.0, (width - 2.0 * pad).max(1.0), 16.0));
        let pill = CALayer::layer();
        pill.setFrame(rect(LABEL_INSET, 0.0, width, LABEL_ROW));
        pill.setBackgroundColor(Some(&color(0.06, 0.06, 0.08, 0.78)));
        pill.setCornerRadius(LABEL_ROW / 2.0);
        pill.addSublayer(&title);
        let label = CALayer::layer();
        label.setAnchorPoint(CGPoint::new(0.0, 1.0));
        label.setBounds(rect(0.0, 0.0, width + LABEL_INSET, LABEL_ROW + LABEL_INSET));
        label.setPosition(CGPoint::new(frame.origin.x, frame.origin.y + frame.size.height));
        label.setZPosition(6.0);
        let k = 1.0 / self.view.scale;
        label.setAffineTransform(CGAffineTransform { a: k, b: 0.0, c: 0.0, d: k, tx: 0.0, ty: 0.0 });
        label.addSublayer(&pill);
        Some(label)
    }
}

fn same_size(a: CGSize, b: CGSize) -> bool {
    (a.width - b.width).abs() < 2.0 && (a.height - b.height).abs() < 2.0
}

/// Where cell `cell` lies in the world, whose origin is cell `home`'s.
fn cell_origin(cell: (usize, usize), home: (usize, usize), step: CGSize) -> CGPoint {
    CGPoint::new(
        (cell.1 as f64 - home.1 as f64) * step.width,
        (cell.0 as f64 - home.0 as f64) * step.height,
    )
}

/// A plain card where a window is, until its capture arrives.
fn placeholder(layer: &CALayer) {
    layer.setBackgroundColor(Some(&color(0.16, 0.16, 0.19, 0.95)));
    layer.setCornerRadius(12.0);
    layer.setMasksToBounds(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_lie_a_step_apart_from_home() {
        let step = CGSize::new(100.0, 50.0);
        assert_eq!(cell_origin((1, 1), (1, 1), step), CGPoint::new(0.0, 0.0));
        assert_eq!(cell_origin((0, 2), (1, 1), step), CGPoint::new(100.0, -50.0));
        assert_eq!(cell_origin((2, 0), (1, 1), step), CGPoint::new(-100.0, 50.0));
    }
}
