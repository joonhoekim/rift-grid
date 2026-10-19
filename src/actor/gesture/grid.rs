//! The workspace grid's swipes, `[settings.grid_gestures]`: one finger count goes
//! to the neighboring cell any way, another goes through the display's grids
//! left and right, and up and down through the Overview's stages. They take
//! their finger counts before upstream's swipes, and stay on while the Overview
//! is open, when upstream's are off. A swipe moves one step, when it has gone
//! the swipe distance.

use super::*;
use crate::layout_engine::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridSwipe {
    /// To the neighboring cell, up, down, left or right.
    Cell,
    /// Through the grids on the display, left or right. With `overview`, up
    /// opens the Overview and steps out a stage, and down steps back in while it
    /// is `open`; with the Overview closed, down is left to macOS.
    Grids { overview: bool, open: bool },
}

impl GridSwipe {
    fn allows(self, way: Direction) -> bool {
        match self {
            Self::Cell => true,
            Self::Grids { overview, open } => match way {
                Direction::Left | Direction::Right => true,
                Direction::Up => overview,
                Direction::Down => overview && open,
            },
        }
    }
}

impl Settings {
    pub fn grid_on(self) -> bool { self.grid.any() }
}

impl Control {
    /// Whether the Overview is open, after each `configure`: the actions then
    /// come from `Actions::for_fingers` alone, however a swipe picks one (at
    /// its start, or when a finger lands late).
    pub fn set_overview(&self, open: bool) { self.routing.lock().settings.grid.open = open; }
}

/// How far the fingers go before Rift takes a grid swipe, so that a finger
/// landing late still makes a three-finger swipe a four-finger one.
const CLAIM_DISTANCE: f64 = 0.015;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Actions {
    cell: Option<ActionConfig>,
    grids: Option<ActionConfig>,
    /// The Overview is open.
    open: bool,
}

impl Actions {
    /// `tolerance` is upstream's, for the left-and-right grid swipe.
    pub(super) fn new(config: &Config, tolerance: f64) -> Self {
        let settings = &config.settings.grid_gestures;
        let on = settings.enabled && config.virtual_workspaces.grid_columns > 0;
        let action = |fingers: usize, swipe| {
            (on && fingers > 0).then_some(ActionConfig {
                fingers,
                invert: false,
                tolerance,
                threshold: config.settings.gestures.distance_pct.clamp(0.01, 1.0),
                scrolling: false,
                propagate: false,
                boundary_threshold: 0.0,
                animate: None,
                grid: Some(swipe),
            })
        };
        let cell = action(settings.cell_fingers, GridSwipe::Cell);
        let grids = action(settings.grid_fingers, GridSwipe::Grids {
            overview: settings.overview,
            open: false,
        })
        .filter(|grids| cell.is_none_or(|cell| cell.fingers != grids.fingers));
        Self { cell, grids, open: false }
    }

    pub(super) fn any(self) -> bool { self.cell.is_some() || self.grids.is_some() }

    pub(super) fn vertical(self) -> bool {
        self.cell.is_some()
            || self.grids.is_some_and(|a| a.grid.is_some_and(|s| s.allows(Direction::Up)))
    }

    pub(super) fn open(self) -> bool { self.open }

    pub(super) fn for_fingers(self, fingers: usize) -> Option<ActionConfig> {
        let mut action = self.cell.into_iter().chain(self.grids).find(|a| a.fingers == fingers)?;
        if let Some(GridSwipe::Grids { open, .. }) = action.grid.as_mut() {
            *open = self.open;
        }
        Some(action)
    }

    pub(super) fn max_fingers(self) -> usize {
        self.cell.into_iter().chain(self.grids).map(|a| a.fingers).max().unwrap_or(0)
    }
}

impl DeviceSession {
    /// A grid swipe's travel `x`, `y` (up is positive) since it began: take it
    /// once it has gone a little way a way it allows, else leave it to macOS;
    /// then send one step past the swipe distance.
    pub(super) fn grid_swipe(
        &mut self,
        swipe: GridSwipe,
        x: f64,
        y: f64,
        tx: &Sender,
        control: &Control,
    ) {
        let (across, along) = (x.abs(), y.abs());
        let vertical = along > across;
        let way = match (vertical, if vertical { y } else { x } > 0.0) {
            (true, true) => Direction::Up,
            (true, false) => Direction::Down,
            (false, true) => Direction::Right,
            (false, false) => Direction::Left,
        };
        if self.owner == Owner::Undecided {
            if across.max(along) < CLAIM_DISTANCE {
                return;
            }
            if !swipe.allows(way) || !control.claim(&self.context) {
                self.owner = Owner::System;
                return;
            }
            self.owner = Owner::Rift;
            tracing::debug!(
                session = self.context.session,
                ?swipe,
                fingers = self.context.action.fingers,
                "grid gesture claimed"
            );
        }
        if self.owner != Owner::Rift {
            return;
        }
        let travel = if vertical { along } else { across };
        if travel < self.context.action.threshold || !swipe.allows(way) {
            return;
        }
        self.blocked = true;
        tx.send(Event::Gesture(Lifecycle::Grid {
            context: self.context.clone(),
            swipe,
            fingers: way,
            control: control.clone(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use multitouch::{Contact, ContactState, Finger, Hand, Point, Vector};
    use objc2_core_foundation::CGSize;

    use super::*;

    fn contacts(count: usize, x: f32, y: f32) -> Vec<Contact> {
        (0..count)
            .map(|i| {
                Contact::new(
                    0,
                    0.0,
                    i as i32,
                    ContactState::Touching,
                    Some(Finger::Index),
                    Some(Hand::Right),
                    Vector::new(Point::new(x + i as f32 * 0.05, y), Point::ZERO),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    Vector::new(Point::ZERO, Point::ZERO),
                    0.0,
                )
            })
            .collect()
    }

    fn config(grid_columns: usize) -> Config {
        let mut config = Config::default();
        config.virtual_workspaces.grid_columns = grid_columns;
        config.settings.gestures.distance_pct = 0.08;
        config.settings.gestures.haptics_enabled = false;
        config
    }

    /// A device on one big display, and what it sends.
    fn device(
        config: &Config,
    ) -> (Arc<Mutex<DeviceInput>>, Control, crate::actor::Receiver<Event>) {
        let (tx, rx) = crate::actor::channel();
        let control = Control::new(config);
        control.configure(Settings::new(config), true, vec![(
            CGRect::new(CGPoint::new(-1e9, -1e9), CGSize::new(2e9, 2e9)),
            SpaceId::new(10),
            LayoutMode::Traditional,
        )]);
        let input = Arc::new(Mutex::new(DeviceInput {
            session: None,
            control: control.clone(),
            tx,
            origin: Instant::now(),
            last_count: 0,
        }));
        (input, control, rx)
    }

    /// Fingers going from (0.5, 0.5) by each of `steps`.
    fn swipe(input: &Arc<Mutex<DeviceInput>>, steps: &[(usize, f32, f32)]) {
        for &(count, x, y) in steps {
            let frame = contacts(count, 0.5 + x, 0.5 + y);
            DeviceInput::deliver(input, multitouch::ContactEvent::Frame(&frame));
        }
    }

    fn grid_event(rx: &mut crate::actor::Receiver<Event>) -> Option<(GridSwipe, Direction)> {
        while let Ok((_, event)) = rx.try_recv() {
            if let Event::Gesture(Lifecycle::Grid { swipe, fingers, .. }) = event {
                return Some((swipe, fingers));
            }
        }
        None
    }

    #[test]
    fn three_fingers_go_any_way_once_per_swipe() {
        for (x, y, way) in [
            (0.0, 0.12, Direction::Up),
            (0.0, -0.12, Direction::Down),
            (-0.12, 0.0, Direction::Left),
            (0.12, 0.01, Direction::Right),
        ] {
            let (input, control, mut rx) = device(&config(3));
            swipe(&input, &[
                (3, 0.0, 0.0),
                (3, x / 4.0, y / 4.0),
                (3, x / 2.0, y / 2.0),
            ]);
            assert_eq!(
                control.ownership_guard().owner,
                Owner::Rift,
                "taken before the distance"
            );
            assert_eq!(grid_event(&mut rx), None, "not yet past the distance");
            swipe(&input, &[(3, x, y), (3, x * 2.0, y * 2.0)]);
            assert_eq!(grid_event(&mut rx), Some((GridSwipe::Cell, way)));
            assert_eq!(grid_event(&mut rx), None, "one step a swipe");
        }
    }

    const GRIDS: GridSwipe = GridSwipe::Grids { overview: true, open: false };

    #[test]
    fn four_fingers_go_left_right_and_up_and_leave_down_to_macos() {
        for (x, y, way) in [(-0.12, 0.0, Direction::Left), (0.0, 0.12, Direction::Up)] {
            let (input, _, mut rx) = device(&config(3));
            swipe(&input, &[(4, 0.0, 0.0), (4, x / 2.0, y / 2.0), (4, x, y)]);
            assert_eq!(grid_event(&mut rx), Some((GRIDS, way)));
        }

        let mut without_overview = config(3);
        without_overview.settings.grid_gestures.overview = false;
        assert!(
            !Settings::new(&without_overview)
                .grid
                .grids
                .unwrap()
                .grid
                .unwrap()
                .allows(Direction::Up)
        );
        for (config, y) in [(config(3), -0.12), (without_overview, 0.12)] {
            let (input, control, mut rx) = device(&config);
            swipe(&input, &[(4, 0.0, 0.0), (4, 0.0, y / 2.0), (4, 0.0, y)]);
            assert_eq!(grid_event(&mut rx), None);
            assert_eq!(control.ownership_guard().owner, Owner::System);
        }
    }

    #[test]
    fn with_the_overview_open_only_the_grid_swipes_stay_and_down_is_ours() {
        let open = GridSwipe::Grids { overview: true, open: true };
        for (fingers, y, expected) in [
            (4, -0.12, Some((open, Direction::Down))),
            (4, 0.12, Some((open, Direction::Up))),
            (3, -0.12, Some((GridSwipe::Cell, Direction::Down))),
        ] {
            let (input, control, mut rx) = device(&config(3));
            control.set_overview(true);
            swipe(&input, &[
                (fingers, 0.0, 0.0),
                (fingers, 0.0, y / 2.0),
                (fingers, 0.0, y),
            ]);
            assert_eq!(grid_event(&mut rx), expected);
            assert_eq!(control.ownership_guard().owner, Owner::Rift);
        }

        let mut config = config(0);
        config.settings.gestures.enabled = true;
        let (input, control, mut rx) = device(&config);
        control.set_overview(true);
        swipe(&input, &[(3, 0.0, 0.0), (3, -0.06, 0.0), (3, -0.12, 0.0)]);
        assert!(input.lock().session.is_none(), "upstream's swipes stay off");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_late_fourth_finger_in_the_overview_still_steps_down() {
        let (input, control, mut rx) = device(&config(3));
        control.set_overview(true);
        swipe(&input, &[
            (3, 0.0, 0.0),
            (3, 0.0, -0.005),
            (4, 0.0, -0.005),
            (4, 0.0, -0.06),
            (4, 0.0, -0.12),
        ]);
        let open = GridSwipe::Grids { overview: true, open: true };
        assert_eq!(grid_event(&mut rx), Some((open, Direction::Down)));
    }

    #[test]
    fn a_late_fourth_finger_makes_it_a_grid_swipe() {
        let (input, _, mut rx) = device(&config(3));
        swipe(&input, &[
            (3, 0.0, 0.0),
            (3, -0.005, 0.0),
            (4, -0.005, 0.0),
            (4, -0.05, 0.0),
        ]);
        swipe(&input, &[(4, -0.12, 0.0)]);
        assert_eq!(grid_event(&mut rx), Some((GRIDS, Direction::Left)));
    }

    #[test]
    fn without_a_grid_the_swipes_stay_upstreams() {
        let mut config = config(0);
        config.settings.gestures.enabled = true;
        let settings = Settings::new(&config);
        assert!(!settings.vertical());
        assert!(
            settings
                .action_for(LayoutMode::Traditional, 3)
                .is_some_and(|a| a.grid.is_none())
        );
        assert!(settings.action_for(LayoutMode::Traditional, 4).is_none());

        config.virtual_workspaces.grid_columns = 3;
        config.settings.grid_gestures.enabled = false;
        assert!(
            Settings::new(&config)
                .action_for(LayoutMode::Traditional, 3)
                .is_some_and(|a| a.grid.is_none())
        );
    }

    #[test]
    fn the_grid_takes_upstreams_finger_count() {
        let mut config = config(3);
        config.settings.gestures.enabled = true;
        config.settings.gestures.consume_dock_swipe = false;
        let settings = Settings::new(&config);
        assert!(settings.vertical());
        assert!(settings.consume);
        let action = settings.action_for(LayoutMode::Traditional, 3).unwrap();
        assert_eq!(action.grid, Some(GridSwipe::Cell));
    }
}
