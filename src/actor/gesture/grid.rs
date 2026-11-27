//! The workspace grid's swipes, `[settings.grid.gestures]`: a table of what
//! each swipe of three and of four fingers does, as subline's. They take their
//! finger counts before upstream's swipes, and stay on while the Overview is
//! open, when upstream's are off. A swipe does its action once, when it has gone
//! the swipe distance; a swipe with nothing to do is left to macOS.

use super::*;
use crate::common::config::{GestureAction, SwipeActions};
use crate::layout_engine::Direction;

/// What a grid swipe of some finger count does each way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSwipe {
    pub actions: SwipeActions,
    /// The Overview is open: the way opposite a swipe that opens it steps back.
    pub open: bool,
}

impl GridSwipe {
    /// What the swipe does going `way`: its action; but with the Overview
    /// open, the way opposite the one that opens it steps back, whatever it is
    /// bound to (four fingers down is `home`, up `overview`).
    pub fn action(self, way: Direction) -> Option<GridAction> {
        if self.open && self.actions.get(way.opposite()) == GestureAction::Overview {
            return Some(GridAction::OverviewBack);
        }
        match self.actions.get(way) {
            GestureAction::None => None,
            action => Some(GridAction::Do(action)),
        }
    }

    fn allows(self, way: Direction) -> bool { self.action(way).is_some() }
}

/// What a grid swipe asks the reactor for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAction {
    Do(GestureAction),
    /// Step the Overview back in a stage, or leave it.
    OverviewBack,
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
    three: Option<ActionConfig>,
    four: Option<ActionConfig>,
    /// The Overview is open.
    open: bool,
}

impl Actions {
    /// `tolerance` is upstream's, for the left-and-right grid swipe.
    pub(super) fn new(config: &Config, tolerance: f64) -> Self {
        let settings = &config.settings.grid.gestures;
        let on = config.virtual_workspaces.grid_layout.is_some();
        let action = |fingers: usize, actions: SwipeActions| {
            (on && actions.any()).then_some(ActionConfig {
                fingers,
                invert: false,
                tolerance,
                threshold: config.settings.gestures.distance_pct.clamp(0.01, 1.0),
                scrolling: false,
                propagate: false,
                boundary_threshold: 0.0,
                animate: None,
                grid: Some(GridSwipe { actions, open: false }),
            })
        };
        Self {
            three: action(3, settings.three),
            four: action(4, settings.four),
            open: false,
        }
    }

    pub(super) fn any(self) -> bool { self.three.is_some() || self.four.is_some() }

    fn all(self) -> impl Iterator<Item = ActionConfig> { self.three.into_iter().chain(self.four) }

    pub(super) fn vertical(self) -> bool {
        self.all().filter_map(|a| a.grid).any(|s| {
            let open = GridSwipe { open: true, ..s };
            open.allows(Direction::Up) || open.allows(Direction::Down)
        })
    }

    pub(super) fn open(self) -> bool { self.open }

    pub(super) fn for_fingers(self, fingers: usize) -> Option<ActionConfig> {
        let mut action = self.all().find(|a| a.fingers == fingers)?;
        if let Some(swipe) = action.grid.as_mut() {
            swipe.open = self.open;
        }
        Some(action)
    }

    pub(super) fn max_fingers(self) -> usize { self.all().map(|a| a.fingers).max().unwrap_or(0) }
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

    fn config(side: usize) -> Config {
        let mut config = Config::default();
        config.virtual_workspaces.grid_layout =
            (side > 0).then(|| crate::common::config::GridLayout::new(side, side));
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
            assert_eq!(grid_event(&mut rx), Some((THREE, way)));
            assert_eq!(grid_event(&mut rx), None, "one step a swipe");
        }
    }

    const THREE: GridSwipe = GridSwipe {
        actions: SwipeActions {
            left: GestureAction::GoToCellRight,
            right: GestureAction::GoToCellLeft,
            up: GestureAction::GoToCellDown,
            down: GestureAction::GoToCellUp,
        },
        open: false,
    };
    const GRIDS: GridSwipe = GridSwipe {
        actions: SwipeActions {
            left: GestureAction::NextGrid,
            right: GestureAction::PreviousGrid,
            up: GestureAction::Overview,
            down: GestureAction::Home,
        },
        open: false,
    };

    #[test]
    fn the_defaults_are_subline_s_table() {
        let settings = crate::common::config::GridGestureSettings::default();
        assert_eq!((settings.three, settings.four), (THREE.actions, GRIDS.actions));
        let parsed = Config::parse(
            "[settings.grid.gestures.three]\nleft = \"focus_display_right\"\n[keys]\n",
        )
        .unwrap();
        let three = parsed.settings.grid.gestures.three;
        assert_eq!(three.left, GestureAction::FocusDisplayRight);
        assert_eq!(
            three.right,
            GestureAction::GoToCellLeft,
            "the ways not given keep theirs"
        );
        assert_eq!(parsed.settings.grid.gestures.four, GRIDS.actions);
    }

    #[test]
    fn four_fingers_go_every_way_and_a_way_with_nothing_is_left_to_macos() {
        for (x, y, way) in [
            (-0.12, 0.0, Direction::Left),
            (0.0, 0.12, Direction::Up),
            (0.0, -0.12, Direction::Down),
        ] {
            let (input, _, mut rx) = device(&config(3));
            swipe(&input, &[(4, 0.0, 0.0), (4, x / 2.0, y / 2.0), (4, x, y)]);
            assert_eq!(grid_event(&mut rx), Some((GRIDS, way)));
        }

        let mut without = config(3);
        without.settings.grid.gestures.four.up = GestureAction::None;
        without.settings.grid.gestures.four.down = GestureAction::None;
        let four = Settings::new(&without).grid.four.unwrap().grid.unwrap();
        assert!(!four.allows(Direction::Up));
        assert!(!four.allows(Direction::Down));
        for y in [-0.12, 0.12] {
            let (input, control, mut rx) = device(&without);
            swipe(&input, &[(4, 0.0, 0.0), (4, 0.0, y / 2.0), (4, 0.0, y)]);
            assert_eq!(grid_event(&mut rx), None);
            assert_eq!(control.ownership_guard().owner, Owner::System);
        }
    }

    #[test]
    fn with_the_overview_open_only_the_grid_swipes_stay_and_down_is_ours() {
        let open = GridSwipe { open: true, ..GRIDS };
        assert_eq!(
            open.action(Direction::Down),
            Some(GridAction::OverviewBack),
            "open, down steps back though it is bound to home"
        );
        assert_eq!(
            GRIDS.action(Direction::Down),
            Some(GridAction::Do(GestureAction::Home))
        );
        let unbound = GridSwipe {
            actions: SwipeActions {
                down: GestureAction::None,
                ..GRIDS.actions
            },
            open: true,
        };
        assert_eq!(unbound.action(Direction::Down), Some(GridAction::OverviewBack));
        for (fingers, y, expected) in [
            (4, -0.12, Some((open, Direction::Down))),
            (4, 0.12, Some((open, Direction::Up))),
            (
                3,
                -0.12,
                Some((GridSwipe { open: true, ..THREE }, Direction::Down)),
            ),
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
        let open = GridSwipe { open: true, ..GRIDS };
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

        config.virtual_workspaces.grid_layout = Some(crate::common::config::GridLayout::new(3, 3));
        config.settings.grid.gestures.three = SwipeActions {
            left: GestureAction::None,
            right: GestureAction::None,
            up: GestureAction::None,
            down: GestureAction::None,
        };
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
        assert_eq!(action.grid, Some(THREE));
    }
}
