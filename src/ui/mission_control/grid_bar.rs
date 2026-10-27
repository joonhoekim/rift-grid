//! The grid buttons of a grid Overview, under the menu bar: a row per display,
//! its name and then the grids on it. Dragging a button onto another display's
//! row hands the grid to that display.

use cgs::with_disabled_actions;

use super::*;

/// Whether a grid is on screen, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// On the display this bar is drawn on.
    Here,
    /// On another display.
    Elsewhere,
    No,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BarButton {
    /// 0-based.
    pub(crate) grid: usize,
    pub(crate) shown: Shown,
    /// "+": a grid on no display yet, to come up on this display.
    pub(crate) new: bool,
}

/// A display's grids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BarGroup {
    pub(crate) display: String,
    pub(crate) name: String,
    pub(crate) buttons: Vec<BarButton>,
}

const BUTTON: CGSize = CGSize { width: 34.0, height: 26.0 };
const BUTTON_GAP: f64 = 6.0;
const ROW_GAP: f64 = 4.0;
const PAD: f64 = 5.0;
const NAME_WIDTH: f64 = 150.0;
/// Below the menu bar.
const TOP_GAP: f64 = 10.0;
/// How far beside the bar a dropped button still counts as on a row.
const DROP_SLACK: f64 = 24.0;

/// The bar's frame in a parent `width` wide with `top` free at the top, and
/// within it each row's frame and its buttons' frames, for rows of `counts`
/// buttons.
fn layout(width: f64, top: f64, counts: &[usize]) -> (CGRect, Vec<CGRect>, Vec<Vec<CGRect>>) {
    let widest = counts.iter().copied().max().unwrap_or(0) as f64;
    let inner = NAME_WIDTH + widest * (BUTTON.width + BUTTON_GAP);
    let mut rows = Vec::new();
    let mut buttons = Vec::new();
    for (i, &count) in counts.iter().enumerate() {
        let y = PAD + i as f64 * (BUTTON.height + ROW_GAP);
        rows.push(rect(PAD / 2.0, y - ROW_GAP / 2.0, inner + PAD, BUTTON.height + ROW_GAP));
        buttons.push(
            (0..count)
                .map(|j| {
                    rect(PAD + NAME_WIDTH + j as f64 * (BUTTON.width + BUTTON_GAP), y, BUTTON.width, BUTTON.height)
                })
                .collect(),
        );
    }
    let height = 2.0 * PAD + counts.len() as f64 * (BUTTON.height + ROW_GAP) - ROW_GAP;
    let bar_width = inner - BUTTON_GAP + 2.0 * PAD;
    let bar = rect(((width - bar_width) / 2.0).round(), top + TOP_GAP, bar_width, height);
    (bar, rows, buttons)
}

/// How much of the top of a parent the bar takes, menu bar included, for
/// `rows` displays.
pub(crate) fn bar_bottom(top: f64, rows: usize) -> f64 {
    top + TOP_GAP + 2.0 * PAD + rows as f64 * (BUTTON.height + ROW_GAP) - ROW_GAP
}

pub(crate) struct GridBar {
    groups: Vec<BarGroup>,
    frame: CGRect,
    rows: Vec<CGRect>,
    frames: Vec<Vec<CGRect>>,
    layer: Retained<CALayer>,
    cells: Vec<Vec<Retained<CALayer>>>,
    target: Retained<CALayer>,
    ghost: Option<Retained<CALayer>>,
    /// A button pressed: its row and place, where the pointer went down, and
    /// whether it has moved far enough to be a drag.
    pressed: Option<((usize, usize), CGPoint, bool)>,
    scale: f64,
}

/// How far the pointer goes before pressing a button becomes dragging it.
const DRAG_START: f64 = 4.0;

impl GridBar {
    /// Draw `groups` in `parent`, a layer with its origin at the top left,
    /// below the first `top` points.
    pub(crate) fn new(parent: &CALayer, top: f64, groups: Vec<BarGroup>, scale: f64) -> Self {
        let counts: Vec<_> = groups.iter().map(|g| g.buttons.len()).collect();
        let (frame, rows, frames) = layout(parent.bounds().size.width, top, &counts);
        let layer = CALayer::layer();
        let target = CALayer::layer();
        let mut cells = Vec::new();
        with_disabled_actions(|| {
            layer.setFrame(frame);
            layer.setContentsScale(scale);
            layer.setZPosition(20.0);
            layer.setCornerRadius(10.0);
            layer.setBackgroundColor(Some(&color(0.06, 0.06, 0.08, 0.78)));
            target.setCornerRadius(8.0);
            target.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.16)));
            target.setOpacity(0.0);
            layer.addSublayer(&target);
            for (group, (row, frames)) in groups.iter().zip(rows.iter().zip(&frames)) {
                let name = label(
                    &layer,
                    &group.name,
                    rect(PAD + 4.0, row.origin.y + (row.size.height - 16.0) / 2.0, NAME_WIDTH - 12.0, 16.0),
                    scale,
                );
                unsafe {
                    name.setAlignmentMode(objc2_quartz_core::kCAAlignmentLeft);
                    name.setTruncationMode(objc2_quartz_core::kCATruncationEnd);
                }
                name.setFontSize(12.0);
                name.setForegroundColor(Some(&color(1.0, 1.0, 1.0, 0.7)));
                let row: Vec<_> = group
                    .buttons
                    .iter()
                    .zip(frames)
                    .map(|(button, &frame)| {
                        let cell = button_layer(button, frame, scale);
                        layer.addSublayer(&cell);
                        cell
                    })
                    .collect();
                cells.push(row);
            }
            parent.addSublayer(&layer);
        });
        Self {
            groups,
            frame,
            rows,
            frames,
            layer,
            cells,
            target,
            ghost: None,
            pressed: None,
            scale,
        }
    }

    pub(crate) fn groups(&self) -> &[BarGroup] { &self.groups }

    pub(crate) fn remove(&self) { with_disabled_actions(|| self.layer.removeFromSuperlayer()); }

    /// Whether `local`, a point in the parent, is on the bar.
    pub(crate) fn contains(&self, local: CGPoint) -> bool { contains(self.frame, local) }

    fn within(&self, local: CGPoint) -> CGPoint {
        CGPoint::new(local.x - self.frame.origin.x, local.y - self.frame.origin.y)
    }

    /// The button at `local`: its row and place.
    pub(crate) fn button_at(&self, local: CGPoint) -> Option<(usize, usize)> {
        let p = self.within(local);
        self.frames.iter().enumerate().find_map(|(row, frames)| {
            frames.iter().position(|f| contains(*f, p)).map(|index| (row, index))
        })
    }

    pub(crate) fn button(&self, (row, index): (usize, usize)) -> Option<&BarButton> {
        self.groups.get(row)?.buttons.get(index)
    }

    /// The row under `local`, give or take a little beside the bar.
    pub(crate) fn row_at(&self, local: CGPoint) -> Option<usize> {
        let p = self.within(local);
        if p.x < -DROP_SLACK || p.x > self.frame.size.width + DROP_SLACK {
            return None;
        }
        self.rows.iter().position(|r| p.y >= r.origin.y && p.y <= r.origin.y + r.size.height)
    }

    /// Outline the button of grid `grid`, where a dragged window would go.
    pub(crate) fn outline(&self, grid: Option<usize>) {
        with_disabled_actions(|| {
            for (cell, button) in
                self.cells.iter().flatten().zip(self.groups.iter().flat_map(|g| &g.buttons))
            {
                let on = Some(button.grid) == grid;
                cell.setBorderWidth(if on || button.shown == Shown::Elsewhere { 1.5 } else { 0.0 });
                if on {
                    cell.setBorderColor(Some(&color(1.0, 1.0, 1.0, 0.9)));
                } else {
                    cell.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
                }
            }
        });
    }

    /// Note a button pressed at `local`; false when there is none.
    pub(crate) fn press(&mut self, local: CGPoint) -> bool {
        let Some(button) = self.button_at(local) else { return false };
        self.pressed = Some((button, local, false));
        true
    }

    pub(crate) fn is_pressed(&self) -> bool { self.pressed.is_some() }

    /// Carry the pressed button to `local`, showing the row it would go to.
    pub(crate) fn drag(&mut self, local: CGPoint) {
        let Some((from, start, moved)) = self.pressed.as_mut() else { return };
        *moved |= (local.x - start.x).hypot(local.y - start.y) > DRAG_START;
        if !*moved {
            return;
        }
        let from = *from;
        let Some(button) = self.button(from).cloned() else { return };
        let p = self.within(local);
        let target = self.row_at(local).filter(|&r| r != from.0 && !button.new);
        with_disabled_actions(|| {
            let ghost = self.ghost.get_or_insert_with(|| {
                let ghost =
                    button_layer(&button, rect(0.0, 0.0, BUTTON.width, BUTTON.height), self.scale);
                ghost.setZPosition(10.0);
                ghost.setOpacity(0.9);
                self.layer.addSublayer(&ghost);
                if let Some(cell) = self.cells.get(from.0).and_then(|row| row.get(from.1)) {
                    cell.setOpacity(0.35);
                }
                ghost
            });
            ghost.setFrame(rect(
                p.x - BUTTON.width / 2.0,
                p.y - BUTTON.height / 2.0,
                BUTTON.width,
                BUTTON.height,
            ));
            match target {
                Some(r) => {
                    self.target.setFrame(self.rows[r]);
                    self.target.setOpacity(1.0);
                }
                None => self.target.setOpacity(0.0),
            }
        });
    }

    /// Let go of the pressed button at `local`: the button, and the display it
    /// was dropped on when it was dragged to another row.
    pub(crate) fn release(&mut self, local: CGPoint) -> Option<(BarButton, Option<String>)> {
        let (from, _, moved) = self.pressed.take()?;
        with_disabled_actions(|| {
            if let Some(ghost) = self.ghost.take() {
                ghost.removeFromSuperlayer();
            }
            self.target.setOpacity(0.0);
            for cell in self.cells.iter().flatten() {
                cell.setOpacity(1.0);
            }
        });
        let button = self.button(from)?.clone();
        let to = moved
            .then(|| self.row_at(local))
            .flatten()
            .filter(|&r| r != from.0 && !button.new)
            .map(|r| self.groups[r].display.clone());
        // A drag that comes back to no other row is no click either.
        if moved && to.is_none() {
            return None;
        }
        Some((button, to))
    }
}

fn button_layer(button: &BarButton, frame: CGRect, scale: f64) -> Retained<CALayer> {
    let cell = layer(frame, scale);
    cell.setCornerRadius(6.0);
    match button.shown {
        Shown::Here => {
            cell.setBackgroundColor(Some(&NSColor::controlAccentColor().CGColor()));
        }
        Shown::Elsewhere => {
            cell.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.1)));
            cell.setBorderWidth(1.5);
            cell.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
        }
        Shown::No => cell.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.1))),
    }
    let text = if button.new { "+".to_owned() } else { (button.grid + 1).to_string() };
    let number = label(
        &cell,
        &text,
        rect(0.0, (frame.size.height - 16.0) / 2.0, frame.size.width, 16.0),
        scale,
    );
    unsafe { number.setAlignmentMode(objc2_quartz_core::kCAAlignmentCenter) };
    number.setFontSize(13.0);
    let alpha = if button.shown == Shown::Here { 1.0 } else { 0.85 };
    number.setForegroundColor(Some(&color(1.0, 1.0, 1.0, alpha)));
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_stack_in_rows_under_the_menu_bar() {
        let (bar, rows, buttons) = layout(1000.0, 30.0, &[3, 2]);
        assert!((bar.origin.x + bar.size.width / 2.0 - 500.0).abs() <= 0.5, "centered");
        assert_eq!(bar.origin.y, 30.0 + TOP_GAP);
        assert!((bar.origin.y + bar.size.height - bar_bottom(30.0, 2)).abs() < 1e-9);
        assert_eq!((buttons[0].len(), buttons[1].len()), (3, 2));
        assert!(rows[0].origin.y + rows[0].size.height <= rows[1].origin.y + 1e-9);
        assert_eq!(buttons[0][0].origin.x, buttons[1][0].origin.x, "buttons line up");
        let last = buttons[0][2];
        assert!((bar.size.width - (last.origin.x + last.size.width) - PAD).abs() < 1e-9);
    }
}
