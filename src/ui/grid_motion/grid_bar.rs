//! The row of grid buttons at the top of the Overview, a group per display: the
//! Overview's own display first, then each other display, a line between
//! groups. Dragging a button onto another group hands the grid to that display.

use super::*;

/// Whether a grid is on screen, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// On the Overview's display.
    Here,
    /// On another display.
    Elsewhere,
    No,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarButton {
    /// 0-based.
    pub grid: usize,
    pub shown: Shown,
    /// "+": a grid on no display yet, to come up on the Overview's display.
    pub new: bool,
}

/// A display's grids, by the display's UUID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarGroup {
    pub display: String,
    pub buttons: Vec<BarButton>,
}

const BUTTON: CGSize = CGSize { width: 34.0, height: 26.0 };
const BUTTON_GAP: f64 = 6.0;
const PAD: f64 = 5.0;
/// Between groups, with the dividing line in the middle.
const GROUP_GAP: f64 = 17.0;
const TOP: f64 = 12.0;
/// How far above and below the row a dropped button still counts as on it.
const DROP_SLACK: f64 = 24.0;

/// Where the row goes in a parent `width` wide, groups of `counts` buttons: the
/// row's frame, each group's section and each button's frame, within the row.
fn layout(width: f64, counts: &[usize]) -> (CGRect, Vec<CGRect>, Vec<Vec<CGRect>>) {
    let mut x = PAD;
    let mut sections = Vec::new();
    let mut buttons = Vec::new();
    for (i, &count) in counts.iter().enumerate() {
        if i > 0 {
            x += GROUP_GAP - BUTTON_GAP;
        }
        let start = x;
        let mut frames = Vec::new();
        for _ in 0..count {
            frames.push(rect(x, PAD, BUTTON.width, BUTTON.height));
            x += BUTTON.width + BUTTON_GAP;
        }
        x -= BUTTON_GAP;
        sections.push(rect(
            start - PAD / 2.0,
            PAD / 2.0,
            x - start + PAD,
            BUTTON.height + PAD,
        ));
        x += BUTTON_GAP;
        buttons.push(frames);
    }
    let row_width = x - BUTTON_GAP + PAD;
    let row = rect(
        ((width - row_width) / 2.0).round(),
        TOP,
        row_width,
        BUTTON.height + 2.0 * PAD,
    );
    (row, sections, buttons)
}

pub struct GridBar {
    groups: Vec<BarGroup>,
    row: CGRect,
    sections: Vec<CGRect>,
    frames: Vec<Vec<CGRect>>,
    layer: Retained<CALayer>,
    cells: Vec<Vec<Retained<CALayer>>>,
    target: Retained<CALayer>,
    ghost: Option<Retained<CALayer>>,
    scale: f64,
}

impl GridBar {
    /// Draw `groups` at the top of `parent`, a layer with its origin at the top
    /// left.
    pub fn new(parent: &CALayer, groups: Vec<BarGroup>, scale: f64) -> Self {
        let counts: Vec<_> = groups.iter().map(|g| g.buttons.len()).collect();
        let (row, sections, frames) = layout(parent.bounds().size.width, &counts);
        let layer = CALayer::layer();
        let target = CALayer::layer();
        let mut cells = Vec::new();
        with_disabled_actions(|| {
            layer.setFrame(row);
            layer.setContentsScale(scale);
            layer.setZPosition(20.0);
            layer.setCornerRadius(10.0);
            layer.setBackgroundColor(Some(&color(0.06, 0.06, 0.08, 0.78)));
            target.setCornerRadius(8.0);
            target.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.16)));
            target.setOpacity(0.0);
            layer.addSublayer(&target);
            for section in sections.iter().skip(1) {
                let line = CALayer::layer();
                let x = section.origin.x - (GROUP_GAP - PAD) / 2.0;
                line.setFrame(rect(x, PAD + 3.0, 1.0, BUTTON.height - 6.0));
                line.setBackgroundColor(Some(&color(1.0, 1.0, 1.0, 0.35)));
                layer.addSublayer(&line);
            }
            for (group, frames) in groups.iter().zip(&frames) {
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
            row,
            sections,
            frames,
            layer,
            cells,
            target,
            ghost: None,
            scale,
        }
    }

    pub fn groups(&self) -> &[BarGroup] { &self.groups }

    pub fn remove(&self) { with_disabled_actions(|| self.layer.removeFromSuperlayer()); }

    pub fn hide(&self) { self.layer.setOpacity(0.0); }

    fn within(&self, local: CGPoint) -> CGPoint {
        CGPoint::new(local.x - self.row.origin.x, local.y - self.row.origin.y)
    }

    /// The button at `local`, a point in the parent: its group and place there.
    pub fn button_at(&self, local: CGPoint) -> Option<(usize, usize)> {
        let p = self.within(local);
        self.frames.iter().enumerate().find_map(|(group, frames)| {
            frames.iter().position(|f| contains(*f, p)).map(|index| (group, index))
        })
    }

    pub fn button(&self, (group, index): (usize, usize)) -> Option<&BarButton> {
        self.groups.get(group)?.buttons.get(index)
    }

    /// The group whose section is under `local`, give or take a little above
    /// and below the row.
    pub fn group_at(&self, local: CGPoint) -> Option<usize> {
        let p = self.within(local);
        if p.y < -DROP_SLACK || p.y > self.row.size.height + DROP_SLACK {
            return None;
        }
        self.sections
            .iter()
            .position(|s| p.x >= s.origin.x && p.x <= s.origin.x + s.size.width)
    }

    /// Show button `from` carried to `local`, and the group it would go to.
    pub fn drag(&mut self, from: (usize, usize), local: CGPoint) {
        let Some(button) = self.button(from).cloned() else {
            return;
        };
        let p = self.within(local);
        let target = self.group_at(local).filter(|&g| g != from.0);
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
                Some(g) => {
                    self.target.setFrame(self.sections[g]);
                    self.target.setOpacity(1.0);
                }
                None => self.target.setOpacity(0.0),
            }
        });
    }

    pub fn end_drag(&mut self) {
        with_disabled_actions(|| {
            if let Some(ghost) = self.ghost.take() {
                ghost.removeFromSuperlayer();
            }
            self.target.setOpacity(0.0);
            for cell in self.cells.iter().flatten() {
                cell.setOpacity(1.0);
            }
        });
    }
}

fn contains(frame: CGRect, p: CGPoint) -> bool {
    p.x >= frame.origin.x
        && p.x <= frame.origin.x + frame.size.width
        && p.y >= frame.origin.y
        && p.y <= frame.origin.y + frame.size.height
}

fn button_layer(button: &BarButton, frame: CGRect, scale: f64) -> Retained<CALayer> {
    let cell = CALayer::layer();
    cell.setFrame(frame);
    cell.setContentsScale(scale);
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
    let text = if button.new {
        "+".to_owned()
    } else {
        (button.grid + 1).to_string()
    };
    let number = CATextLayer::layer();
    number.setContentsScale(scale);
    number.setFrame(rect(
        0.0,
        (frame.size.height - 16.0) / 2.0,
        frame.size.width,
        16.0,
    ));
    let font = NSFont::systemFontOfSize(13.0);
    unsafe {
        number.setFont(Some(&*(Retained::as_ptr(&font) as *const CFType)));
        number.setAlignmentMode(objc2_quartz_core::kCAAlignmentCenter);
        let text = CFString::from_str(&text);
        number.setString(Some(&*(text.as_ref() as *const AnyObject)));
    }
    number.setFontSize(13.0);
    let alpha = if button.shown == Shown::Here {
        1.0
    } else {
        0.85
    };
    number.setForegroundColor(Some(&color(1.0, 1.0, 1.0, alpha)));
    cell.addSublayer(&number);
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_sit_side_by_side_in_one_centered_row() {
        let (row, sections, buttons) = layout(1000.0, &[2, 1]);
        assert!(
            (row.origin.x + row.size.width / 2.0 - 500.0).abs() <= 0.5,
            "centered"
        );
        assert_eq!(buttons[0].len(), 2);
        let first_end = buttons[0][1].origin.x + BUTTON.width;
        assert!((buttons[1][0].origin.x - first_end - GROUP_GAP).abs() < 1e-9);
        assert!(sections[0].origin.x + sections[0].size.width < sections[1].origin.x);
        let last = buttons[1][0];
        assert!((row.size.width - (last.origin.x + last.size.width) - PAD).abs() < 1e-9);
    }
}
