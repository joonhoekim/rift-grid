//! Where a direction leads: from a cell of a grid of workspaces to the one
//! beside it, from a tile to the tile beside it, and into the next cell at the
//! tile level with the one left. Cells are numbers and tiles plain rectangles
//! in points, so this knows nothing of windows or workspaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    /// Columns and rows one step goes.
    pub fn delta(self) -> (isize, isize) {
        match self {
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
            Self::Up => (0, -1),
            Self::Down => (0, 1),
        }
    }

    pub fn horizontal(self) -> bool { matches!(self, Self::Left | Self::Right) }
}

/// From its top left corner, `w` wide and `h` high, in points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self { Self { x, y, w, h } }

    pub fn right(&self) -> f64 { self.x + self.w }

    pub fn bottom(&self) -> f64 { self.y + self.h }

    fn center(&self) -> (f64, f64) { (self.x + self.w / 2.0, self.y + self.h / 2.0) }
}

/// How far apart two edges may be and still meet: a point, for rounding.
const SLACK: f64 = 1.0;

/// `rows` × `columns` cells, numbered row by row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub rows: usize,
    pub columns: usize,
}

impl Grid {
    pub const fn new(rows: usize, columns: usize) -> Self { Self { rows, columns } }

    /// Row and column of cell `index`.
    pub fn cell_of(self, index: usize) -> (usize, usize) {
        let columns = self.columns.max(1);
        (index / columns, index % columns)
    }

    pub fn index(self, row: usize, column: usize) -> Option<usize> {
        (row < self.rows && column < self.columns).then_some(row * self.columns + column)
    }

    /// The cell `dx` columns and `dy` rows from `index`, if there is one: the
    /// edges do not wrap.
    pub fn beside(self, index: usize, dx: isize, dy: isize) -> Option<usize> {
        let (r, c) = self.cell_of(index);
        self.index(r.checked_add_signed(dy)?, c.checked_add_signed(dx)?)
    }

    pub fn towards(self, index: usize, dir: Dir) -> Option<usize> {
        let (dx, dy) = dir.delta();
        self.beside(index, dx, dy)
    }
}

/// The tile beside `rects[from]` towards `dir`: the one sharing the most of
/// that side, then the nearest, then the first.
pub fn neighbor(rects: &[Rect], from: usize, dir: Dir) -> Option<usize> {
    let r = rects.get(from)?;
    let meet = |a: f64, b: f64| (a - b).abs() <= SLACK;
    let touching = |c: &Rect| match dir {
        Dir::Left => meet(c.right(), r.x),
        Dir::Right => meet(r.right(), c.x),
        Dir::Up => meet(c.bottom(), r.y),
        Dir::Down => meet(r.bottom(), c.y),
    };
    let overlap = |c: &Rect| match dir.horizontal() {
        true => r.bottom().min(c.bottom()) - r.y.max(c.y),
        false => r.right().min(c.right()) - r.x.max(c.x),
    };
    let distance = |c: &Rect| {
        let (a, b) = (r.center(), c.center());
        (a.0 - b.0).hypot(a.1 - b.1)
    };
    rects
        .iter()
        .enumerate()
        .filter(|&(i, c)| i != from && touching(c) && overlap(c) > 0.0)
        .min_by(|(_, a), (_, b)| {
            overlap(b).total_cmp(&overlap(a)).then(distance(a).total_cmp(&distance(b)))
        })
        .map(|(i, _)| i)
}

/// Coming into `area` moving `dir`, the tile on the side entered that spans
/// `along` (the other axis' position), or the nearest to it on that side.
pub fn entered(rects: &[Rect], area: Rect, dir: Dir, along: f64) -> Option<usize> {
    let on = |a: f64, b: f64| (a - b).abs() < SLACK;
    let on_side = |c: &Rect| match dir {
        Dir::Right => on(c.x, area.x),
        Dir::Left => on(c.right(), area.right()),
        Dir::Down => on(c.y, area.y),
        Dir::Up => on(c.bottom(), area.bottom()),
    };
    let gap = |c: &Rect| {
        let (from, to) = if dir.horizontal() { (c.y, c.bottom()) } else { (c.x, c.right()) };
        if along < from {
            from - along
        } else if along > to {
            along - to
        } else {
            0.0
        }
    };
    rects
        .iter()
        .enumerate()
        .filter(|(_, c)| on_side(c))
        .min_by(|(_, a), (_, b)| gap(a).total_cmp(&gap(b)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_beside_stop_at_the_edges() {
        let g = Grid::new(2, 3);
        assert_eq!(g.cell_of(4), (1, 1));
        assert_eq!(g.index(1, 2), Some(5));
        assert_eq!(g.index(2, 0), None);
        assert_eq!(g.beside(4, 1, 0), Some(5));
        assert_eq!(g.beside(5, 1, 0), None, "the end of a row does not wrap to the next");
        assert_eq!(g.beside(3, -1, 0), None);
        assert_eq!(g.towards(1, Dir::Down), Some(4));
        assert_eq!(g.towards(4, Dir::Down), None);
        assert_eq!(g.towards(1, Dir::Up), None);
        assert_eq!(g.beside(0, 2, 1), Some(5));
    }

    const AREA: Rect = Rect::new(0.0, 0.0, 1200.0, 800.0);

    /// One tile on the left, two stacked on the right.
    fn three() -> [Rect; 3] {
        [
            Rect::new(0.0, 0.0, 600.0, 800.0),
            Rect::new(600.0, 0.0, 600.0, 400.0),
            Rect::new(600.0, 400.0, 600.0, 400.0),
        ]
    }

    #[test]
    fn neighbors_are_found_by_their_shared_side() {
        let rects = three();
        assert_eq!(
            neighbor(&rects, 0, Dir::Right),
            Some(1),
            "the upper of the two shares as much, nearer first"
        );
        assert_eq!(neighbor(&rects, 2, Dir::Left), Some(0));
        assert_eq!(neighbor(&rects, 1, Dir::Down), Some(2));
        assert_eq!(neighbor(&rects, 2, Dir::Up), Some(1));
        assert_eq!(neighbor(&rects, 0, Dir::Left), None, "past the cell's edge");
        assert_eq!(neighbor(&rects, 1, Dir::Up), None);
        assert_eq!(neighbor(&rects, 3, Dir::Up), None);
        assert_eq!(
            entered(&rects, AREA, Dir::Left, 200.0),
            Some(1),
            "coming in from the right, at the height of the upper one"
        );
        assert_eq!(entered(&rects, AREA, Dir::Left, 600.0), Some(2));
        assert_eq!(entered(&rects, AREA, Dir::Right, 700.0), Some(0));
        assert_eq!(entered(&rects, AREA, Dir::Up, 100.0), Some(0));
        assert_eq!(entered(&rects, AREA, Dir::Up, 1000.0), Some(2));
    }

    #[test]
    fn the_most_shared_side_wins() {
        let rects = [
            Rect::new(0.0, 0.0, 600.0, 800.0),
            Rect::new(600.0, 0.0, 600.0, 300.0),
            Rect::new(600.0, 300.0, 600.0, 500.0),
        ];
        assert_eq!(neighbor(&rects, 0, Dir::Right), Some(2));
        let apart = [Rect::new(0.0, 0.0, 600.0, 800.0), Rect::new(602.0, 0.0, 600.0, 800.0)];
        assert_eq!(neighbor(&apart, 0, Dir::Right), None, "a gap is not a shared side");
    }

    #[test]
    fn fractions_of_points() {
        let third = 1000.0 / 3.0;
        let rects = [
            Rect::new(0.0, 0.0, third, 600.0),
            Rect::new(third, 0.0, third, 600.0),
            Rect::new(third + third, 0.0, 1000.0 - (third + third), 300.0),
            Rect::new(third + third, 300.0, 1000.0 - (third + third), 300.0),
        ];
        let area = Rect::new(0.0, 0.0, 1000.0, 600.0);
        assert_eq!(neighbor(&rects, 0, Dir::Right), Some(1));
        assert_eq!(neighbor(&rects, 1, Dir::Right), Some(2));
        assert_eq!(neighbor(&rects, 3, Dir::Left), Some(1));
        assert_eq!(neighbor(&rects, 2, Dir::Down), Some(3));
        assert_eq!(neighbor(&rects, 1, Dir::Up), None);
        assert_eq!(
            entered(&rects, area, Dir::Left, 450.0),
            Some(3),
            "the right edge, rounded or not"
        );
        assert_eq!(entered(&rects, area, Dir::Down, 500.0), Some(1));
        assert_eq!(entered(&rects, area, Dir::Up, 999.5), Some(3));
        assert_eq!(entered(&rects, area, Dir::Down, 999.5), Some(2));
        let offset = [
            Rect::new(-1440.5, 25.25, 720.25, 875.0),
            Rect::new(-720.25, 25.25, 720.25, 875.0),
        ];
        assert_eq!(neighbor(&offset, 1, Dir::Left), Some(0), "on a display left of the main one");
        assert_eq!(
            entered(&offset, Rect::new(-1440.5, 25.25, 1440.5, 875.0), Dir::Right, 0.0),
            Some(0)
        );
    }
}
