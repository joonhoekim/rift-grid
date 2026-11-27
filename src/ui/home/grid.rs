//! The grid's layout and picking with the arrow keys, apart from drawing
//! (docs/home.md, "The grid" and "Search").

/// A slot, its icon and the room around the grid, as subline's (points here).
pub const SLOT_W: f64 = 108.0;
pub const SLOT_H: f64 = 116.0;
pub const ICON: f64 = 60.0;
pub const PAD: f64 = 16.0;
/// Above the first row; the room the line under the frequent row takes; the
/// room a category's title takes above its rows.
pub const GRID_TOP: f64 = 8.0;
pub const LINE_GAP: f64 = 17.0;
pub const HEAD_H: f64 = 40.0;
/// The panel is never narrower than this many slots.
const PANEL_SLOTS: f64 = 3.0;

/// Slots that fit across `width`, at least one.
pub fn columns(width: f64) -> usize { (((width - 2.0 * PAD) / SLOT_W).floor() as usize).max(1) }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The `window` style's panel: `share` of the usable area, in its middle, at
/// least three slots wide (but never wider than the area).
pub fn panel(usable: Rect, share: (f64, f64)) -> Rect {
    let least = PANEL_SLOTS * SLOT_W + 2.0 * PAD;
    let w = (usable.w * share.0).max(least).min(usable.w);
    let h = usable.h * share.1;
    Rect {
        x: usable.x + (usable.w - w) / 2.0,
        y: usable.y + (usable.h - h) / 2.0,
        w,
        h,
    }
}

/// What the grid shows, row by row: indices into the apps. With no search, an
/// optional frequent row and then every app, in categories or not; with a
/// search, the results.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    pub rows: Vec<Vec<usize>>,
    /// Whether the first row is the frequent row (a line follows it).
    pub frequent_row: bool,
    /// Each category's first row and its title; its rows run to the next one's.
    pub heads: Vec<(usize, String)>,
}

impl Shown {
    pub fn new(frequent: &[usize], all: &[usize], columns: usize) -> Self {
        Self::grouped(frequent, &[(String::new(), all.to_vec())], columns)
    }

    /// The frequent row, then each group on rows of its own under its title.
    /// One group alone has no title, as subline's grid with one category.
    pub fn grouped(frequent: &[usize], groups: &[(String, Vec<usize>)], columns: usize) -> Self {
        let columns = columns.max(1);
        let frequent = &frequent[..frequent.len().min(columns)];
        let mut rows = Vec::new();
        if !frequent.is_empty() {
            rows.push(frequent.to_vec());
        }
        let groups: Vec<_> = groups.iter().filter(|(_, apps)| !apps.is_empty()).collect();
        let titled = groups.len() > 1;
        let mut heads = Vec::new();
        for (title, apps) in groups {
            if titled {
                heads.push((rows.len(), title.clone()));
            }
            rows.extend(apps.chunks(columns).map(<[usize]>::to_vec));
        }
        Self {
            rows,
            frequent_row: !frequent.is_empty(),
            heads,
        }
    }

    pub fn get(&self, pick: Pick) -> Option<usize> {
        self.rows.get(pick.row).and_then(|row| row.get(pick.col)).copied()
    }

    pub fn first(&self) -> Option<Pick> {
        self.rows.iter().position(|row| !row.is_empty()).map(|row| Pick { row, col: 0 })
    }

    /// The top of row `row` in the grid view (`rows.len()` gives the bottom),
    /// with the frequent row's line and the titles above it.
    pub fn row_top(&self, row: usize) -> f64 {
        let line = if self.frequent_row && row >= 1 {
            LINE_GAP
        } else {
            0.0
        };
        let titles = self.heads.iter().filter(|(start, _)| *start <= row).count();
        GRID_TOP + row as f64 * SLOT_H + line + titles as f64 * HEAD_H
    }

    /// The rows of each category: its title, its first row and the row after
    /// its last.
    pub fn sections(&self) -> Vec<(&str, usize, usize)> {
        self.heads
            .iter()
            .enumerate()
            .map(|(i, (start, title))| {
                let end = self.heads.get(i + 1).map_or(self.rows.len(), |(next, _)| *next);
                (title.as_str(), *start, end)
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
    Left,
    Right,
}

/// The slot picked after `step` from `pick`. With nothing picked any step
/// picks the first slot; at the grid's ends the pick stays.
pub fn step(shown: &Shown, pick: Option<Pick>, step: Step) -> Option<Pick> {
    let Some(pick) = pick.filter(|pick| shown.get(*pick).is_some()) else {
        return shown.first();
    };
    let row_len = |row: usize| shown.rows.get(row).map_or(0, Vec::len);
    let moved = match step {
        Step::Up => pick.row.checked_sub(1).map(|row| (row, pick.col)),
        Step::Down => Some((pick.row + 1, pick.col)).filter(|(row, _)| row_len(*row) > 0),
        Step::Left => match pick.col.checked_sub(1) {
            Some(col) => Some((pick.row, col)),
            None => pick.row.checked_sub(1).map(|row| (row, usize::MAX)),
        },
        Step::Right if pick.col + 1 < row_len(pick.row) => Some((pick.row, pick.col + 1)),
        Step::Right => Some((pick.row + 1, 0)).filter(|(row, _)| row_len(*row) > 0),
    };
    Some(match moved {
        Some((row, col)) => Pick {
            row,
            col: col.min(row_len(row).saturating_sub(1)),
        },
        None => pick,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(row: usize, col: usize) -> Option<Pick> { Some(Pick { row, col }) }

    #[test]
    fn columns_fit_the_width() {
        assert_eq!(columns(0.0), 1);
        assert_eq!(columns(3.0 * SLOT_W + 2.0 * PAD), 3);
        assert_eq!(columns(3.0 * SLOT_W + 2.0 * PAD - 1.0), 2);
    }

    #[test]
    fn the_panel_is_centred_and_never_narrower_than_three_slots() {
        let usable = Rect {
            x: 0.0,
            y: 25.0,
            w: 1000.0,
            h: 800.0,
        };
        assert_eq!(panel(usable, (0.6, 0.5)), Rect {
            x: 200.0,
            y: 225.0,
            w: 600.0,
            h: 400.0
        });
        let narrow = panel(usable, (0.3, 0.5));
        assert_eq!(narrow.w, 3.0 * SLOT_W + 2.0 * PAD);
        let tiny = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        assert_eq!(panel(tiny, (0.3, 1.0)).w, 200.0);
    }

    #[test]
    fn the_frequent_row_comes_first_and_is_one_row() {
        let shown = Shown::new(&[4, 5, 6, 7], &[0, 1, 2, 3, 4, 5, 6], 3);
        assert_eq!(shown.rows, vec![
            vec![4, 5, 6],
            vec![0, 1, 2],
            vec![3, 4, 5],
            vec![6]
        ]);
        assert!(shown.frequent_row);
        let plain = Shown::new(&[], &[0, 1], 3);
        assert_eq!(plain.rows, vec![vec![0, 1]]);
        assert!(!plain.frequent_row);
    }

    #[test]
    fn categories_start_rows_of_their_own_under_a_title() {
        let groups = [
            ("Internet".to_owned(), vec![0, 1, 2, 3]),
            ("Empty".to_owned(), vec![]),
            ("Games".to_owned(), vec![4]),
        ];
        let shown = Shown::grouped(&[4], &groups, 3);
        assert_eq!(shown.rows, vec![vec![4], vec![0, 1, 2], vec![3], vec![4]]);
        assert_eq!(shown.heads, vec![
            (1, "Internet".to_owned()),
            (3, "Games".to_owned())
        ]);
        assert_eq!(shown.sections(), vec![("Internet", 1, 3), ("Games", 3, 4)]);
        assert_eq!(shown.row_top(0), GRID_TOP);
        assert_eq!(shown.row_top(1), GRID_TOP + SLOT_H + LINE_GAP + HEAD_H);
        assert_eq!(
            shown.row_top(3),
            GRID_TOP + 3.0 * SLOT_H + LINE_GAP + 2.0 * HEAD_H
        );
        assert_eq!(
            shown.row_top(4),
            GRID_TOP + 4.0 * SLOT_H + LINE_GAP + 2.0 * HEAD_H
        );
        // Arrows go from a category's short last row into the next one.
        assert_eq!(step(&shown, p(2, 0), Step::Down), p(3, 0));
        // One category alone has no title.
        let one = Shown::grouped(&[], &groups[..1], 3);
        assert!(one.heads.is_empty());
    }

    #[test]
    fn arrows_move_by_rows_and_across_them() {
        // Rows of 3, 3 and 1.
        let shown = Shown::new(&[], &[0, 1, 2, 3, 4, 5, 6], 3);
        assert_eq!(step(&shown, None, Step::Down), p(0, 0));
        assert_eq!(step(&shown, p(0, 2), Step::Down), p(1, 2));
        assert_eq!(step(&shown, p(1, 2), Step::Down), p(2, 0));
        assert_eq!(step(&shown, p(2, 0), Step::Down), p(2, 0));
        assert_eq!(step(&shown, p(0, 1), Step::Up), p(0, 1));
        assert_eq!(step(&shown, p(0, 2), Step::Right), p(1, 0));
        assert_eq!(step(&shown, p(1, 0), Step::Left), p(0, 2));
        assert_eq!(step(&shown, p(0, 0), Step::Left), p(0, 0));
        assert_eq!(step(&shown, p(2, 0), Step::Right), p(2, 0));
    }

    #[test]
    fn picking_starts_on_the_frequent_row() {
        let shown = Shown::new(&[2], &[0, 1, 2], 3);
        let first = step(&shown, None, Step::Right);
        assert_eq!(first.and_then(|pick| shown.get(pick)), Some(2));
        assert_eq!(step(&shown, first, Step::Down), p(1, 0));
    }

    #[test]
    fn nothing_to_pick_in_an_empty_grid() {
        assert_eq!(step(&Shown::default(), None, Step::Down), None);
    }
}
