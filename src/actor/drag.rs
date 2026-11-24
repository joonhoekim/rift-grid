//! Single-owner drag interaction primitives.
//!
//! The event tap only publishes the latest pointer value. Geometry and drag
//! semantics are consumed by the drag actor/reactor bridge, never in the tap.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use objc2_core_foundation::{CGPoint, CGRect};

use crate::actor::app::WindowId;
use crate::common::config::{DragDropSettings, MouseAction, MouseDropAction};
use crate::layout_engine::{Direction, WindowDropAction};
pub use crate::model::drag::{
    DragCancel, DragCommit, DragKind, DragScene, DragSceneTarget, DragSource, DropIntent,
    DropTarget, DropZone,
};
use crate::sys::screen::SpaceId;

mod dock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct DragMotion {
    pub point: CGPoint,
}

/// Latest-value transport with at most one outstanding actor wake.
#[derive(Debug, Default)]
struct DragMotionState {
    latest: Mutex<Option<DragMotion>>,
    wake_queued: AtomicBool,
}

#[derive(Clone, Debug, Default)]
pub struct DragMotionPublisher(Arc<DragMotionState>);

impl DragMotionPublisher {
    /// Publish a sample. Returns true exactly when the caller must enqueue a wake.
    pub fn publish(&self, motion: DragMotion) -> bool {
        let mut latest = self.0.latest.lock().expect("drag motion mutex poisoned");
        *latest = Some(motion);
        !self.0.wake_queued.swap(true, Ordering::AcqRel)
    }

    /// Consume the newest sample and re-arm publication atomically with respect
    /// to the value mutex, so a concurrent publication cannot be stranded.
    pub fn take_latest(&self) -> Option<DragMotion> {
        let mut latest = self.0.latest.lock().expect("drag motion mutex poisoned");
        let motion = latest.take();
        self.0.wake_queued.store(false, Ordering::Release);
        motion
    }

    pub fn wake_queued(&self) -> bool { self.0.wake_queued.load(Ordering::Acquire) }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub source: DragSource,
    button: MouseButton,
    pub pointer: CGPoint,
    pub anchor_point: CGPoint,
    pub scene: DragScene,
    target: TargetState,
    pub kind: DragKind,
}

#[derive(Debug, Clone, Copy, Default)]
enum TargetState {
    #[default]
    None,
    Candidate(DropIntent),
    Validated(DropTarget),
    /// The layout cannot take this drop; kept so the pointer staying in the
    /// same zone does not ask the layout again.
    Rejected(DropIntent),
}

impl TargetState {
    fn intent(self) -> Option<DropIntent> {
        match self {
            Self::Candidate(intent) => Some(intent),
            Self::Validated(target) => Some(target.intent),
            Self::None | Self::Rejected(_) => None,
        }
    }

    /// The zone under the pointer, whether or not the layout accepted it.
    fn hit(self) -> Option<DropIntent> {
        match self {
            Self::Rejected(intent) => Some(intent),
            other => other.intent(),
        }
    }

    fn validated(self) -> Option<DropTarget> {
        match self {
            Self::Validated(target) => Some(target),
            Self::None | Self::Candidate(_) | Self::Rejected(_) => None,
        }
    }
}

impl Session {
    fn new(
        source: DragSource,
        button: MouseButton,
        pointer: CGPoint,
        scene: DragScene,
        kind: DragKind,
    ) -> Self {
        Self {
            source,
            button,
            pointer,
            anchor_point: pointer,
            scene,
            target: TargetState::None,
            kind,
        }
    }

    fn invalidate_drop(&mut self) { self.target = TargetState::None; }

    /// The drop target under `point`, for a move of a tiled window. Over the
    /// dragged window's own tile there is none, whatever else is drawn there
    /// (stack-mates share the tile's frame).
    fn target_at(&self, point: CGPoint, settings: &DragDropSettings) -> Option<DropIntent> {
        if !settings.enabled
            || !self.source.tiled
            || !self.kind.is_move()
            || contains(self.source.origin_frame, point, 0.0)
        {
            return None;
        }
        dock::target(&self.scene, point, settings.drop_zone_fraction, None)
    }
}

#[derive(Debug, Clone)]
pub struct DragActor {
    session: Option<Session>,
    settings: DragDropSettings,
}

impl DragActor {
    pub fn new(settings: DragDropSettings) -> Self { Self { session: None, settings } }

    pub fn is_active(&self) -> bool { self.session.is_some() }

    pub fn drop_enabled(&self) -> bool { self.settings.enabled }

    fn session(&self) -> Option<&Session> { self.session.as_ref() }

    pub fn source(&self) -> Option<DragSource> { self.session().map(|session| session.source) }

    pub fn target(&self) -> Option<DropTarget> { self.session()?.target.validated() }

    pub fn preview_target(&self) -> Option<DropTarget> {
        let session = self.session()?;
        if let Some(target) = session.target.validated() {
            return (session.source.tiled || target.intent.action == WindowDropAction::Stack)
                .then_some(target);
        }
        if !session.source.tiled
            || !matches!(session.target, TargetState::None)
            || !contains(session.source.origin_frame, session.pointer, 0.0)
        {
            return None;
        }
        Some(DropTarget {
            intent: DropIntent {
                window: session.source.window,
                space: session.source.current_space?,
                frame: session.source.origin_frame,
                zone: DropZone::Center,
                action: WindowDropAction::Swap,
            },
            preview_area: session.source.origin_frame,
        })
    }

    pub fn intent(&self) -> Option<DropIntent> { self.session()?.target.intent() }

    pub fn kind(&self) -> Option<DragKind> { self.session().map(|session| session.kind) }

    /// Update reactor-resolved space membership, with that space's scene, and invalidate
    /// destination state.
    pub fn update_current_space(&mut self, current_space: Option<SpaceId>, scene: DragScene) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.source.current_space = current_space;
        session.scene = scene;
        session.invalidate_drop();
    }

    pub fn replace_scene(&mut self, scene: DragScene) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.scene = scene;
        session.invalidate_drop();
        session.target = session
            .target_at(session.pointer, &self.settings)
            .map_or(TargetState::None, TargetState::Candidate);
    }

    /// Record the layout's answer for the current candidate: the frame the
    /// window would land in, or `None` when the layout cannot take the drop.
    pub fn set_preview(&mut self, intent: DropIntent, preview_area: Option<CGRect>) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !session
            .target
            .intent()
            .is_some_and(|current| current.window == intent.window && current.zone == intent.zone)
        {
            return;
        }
        session.target = match preview_area {
            Some(preview_area) => TargetState::Validated(DropTarget { intent, preview_area }),
            None => TargetState::Rejected(intent),
        };
    }

    pub fn update_config(&mut self, settings: DragDropSettings) {
        let semantics_changed = self.settings.drop_zone_fraction != settings.drop_zone_fraction;
        self.settings = settings;
        if (semantics_changed || !settings.enabled)
            && let Some(session) = &mut self.session
        {
            session.invalidate_drop();
        }
    }

    pub fn begin_native(&mut self, source: DragSource, scene: DragScene) {
        if self.is_active() {
            return;
        }
        let kind = DragKind::native(source.origin_frame, source.last_frame);
        let pointer = CGPoint::new(
            source.last_frame.origin.x + source.last_frame.size.width / 2.0,
            source.last_frame.origin.y + source.last_frame.size.height / 2.0,
        );
        self.session = Some(Session::new(source, MouseButton::Left, pointer, scene, kind));
    }

    /// Updates an existing native drag's frame. The pointer, not the frame, decides which
    /// display the drag is on (`update_current_space`).
    ///
    /// Returns `true` when `window` owns the active native drag session.
    pub fn update_native(&mut self, window: WindowId, frame: CGRect) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        if session.source.window != window
            || !matches!(session.kind, DragKind::NativeMove | DragKind::NativeResize)
        {
            return false;
        }
        session.source.last_frame = frame;
        session.kind = DragKind::native(session.source.origin_frame, frame);
        // A move that became a resize must not commit the drop it had found.
        if session.kind == DragKind::NativeResize {
            session.invalidate_drop();
        }
        true
    }

    pub fn begin_modifier(
        &mut self,
        source: DragSource,
        button: MouseButton,
        point: CGPoint,
        action: MouseAction,
        scene: DragScene,
    ) {
        if self.is_active() {
            return;
        }
        let kind = match action {
            MouseAction::Move => DragKind::ModifierMove,
            MouseAction::None => return,
        };
        self.session = Some(Session::new(source, button, point, scene, kind));
    }

    pub fn motion(&mut self, motion: DragMotion) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        let home_preview_changed = contains(session.source.origin_frame, session.pointer, 0.0)
            != contains(session.source.origin_frame, motion.point, 0.0);
        session.pointer = motion.point;
        if session.kind == DragKind::ModifierMove {
            let dx = motion.point.x - session.anchor_point.x;
            let dy = motion.point.y - session.anchor_point.y;
            session.source.last_frame = CGRect::new(
                CGPoint::new(
                    session.source.origin_frame.origin.x + dx,
                    session.source.origin_frame.origin.y + dy,
                ),
                session.source.origin_frame.size,
            );
        }
        let next = session.target_at(motion.point, &self.settings);
        let changed = next != session.target.hit();
        if changed {
            session.target = next.map_or(TargetState::None, TargetState::Candidate);
        }
        changed || home_preview_changed
    }

    pub fn interactive_update(&mut self) -> Option<(WindowId, CGRect)> {
        let Some(session) = &mut self.session else {
            return None;
        };
        (session.kind == DragKind::ModifierMove)
            .then_some((session.source.window, session.source.last_frame))
    }

    pub fn owns_button(&self, button: MouseButton) -> bool {
        self.session().is_some_and(|session| session.button == button)
    }

    pub fn finish(&mut self, button: MouseButton) -> Option<DragCommit> {
        if !self.owns_button(button) {
            return None;
        }
        let session = self.session.take()?;
        Some(DragCommit {
            source: session.source,
            target: session.target.validated(),
        })
    }

    pub fn cancel(&mut self) -> Option<DragCancel> {
        let session = self.session.take()?;
        Some(DragCancel {
            source: session.source,
            kind: session.kind,
        })
    }

    pub fn window_removed(&mut self, window: WindowId) {
        let Some(session) = &mut self.session else {
            return;
        };
        if session.source.window == window {
            self.cancel();
            return;
        }
        session.scene.targets.retain(|target| target.window != window);
        if session.target.intent().is_some_and(|intent| intent.window == window) {
            session.target = TargetState::None;
        }
    }
}

impl PartialEq for DropTarget {
    fn eq(&self, other: &Self) -> bool {
        self.intent.window == other.intent.window
            && self.intent.space == other.intent.space
            && self.intent.zone == other.intent.zone
    }
}

impl PartialEq for DropIntent {
    fn eq(&self, other: &Self) -> bool {
        self.window == other.window && self.space == other.space && self.action == other.action
    }
}

pub(super) fn contains(rect: CGRect, point: CGPoint, margin: f64) -> bool {
    point.x >= rect.origin.x - margin
        && point.x <= rect.origin.x + rect.size.width + margin
        && point.y >= rect.origin.y - margin
        && point.y <= rect.origin.y + rect.size.height + margin
}

pub fn classify_zone(frame: CGRect, point: CGPoint, fraction: f64) -> Option<DropZone> {
    if frame.size.width <= 0.0 || frame.size.height <= 0.0 || !contains(frame, point, 0.0) {
        return None;
    }
    let left = (point.x - frame.origin.x) / frame.size.width;
    let right = 1.0 - left;
    let top = (point.y - frame.origin.y) / frame.size.height;
    let bottom = 1.0 - top;
    if left >= fraction && right >= fraction && top >= fraction && bottom >= fraction {
        return Some(DropZone::Center);
    }
    let mut nearest = (left, DropZone::West);
    for candidate in [
        (right, DropZone::East),
        (top, DropZone::North),
        (bottom, DropZone::South),
    ] {
        if candidate.0 < nearest.0 {
            nearest = candidate;
        }
    }
    Some(nearest.1)
}

pub fn resolve_action(zone: DropZone, center: MouseDropAction) -> WindowDropAction {
    match zone {
        DropZone::Center => center.into(),
        DropZone::West => WindowDropAction::Insert(Direction::Left),
        DropZone::East => WindowDropAction::Insert(Direction::Right),
        DropZone::North => WindowDropAction::Insert(Direction::Up),
        DropZone::South => WindowDropAction::Insert(Direction::Down),
    }
}

/// Hit-test an Overview scene, which includes the dragged window for presentation.
pub fn hit_test_projected(
    scene: &DragScene,
    source: WindowId,
    point: CGPoint,
) -> Option<DropIntent> {
    dock::target(scene, point, 0.25, Some(source))
}

pub fn preview_frame(target: DropTarget) -> CGRect {
    let mut frame = target.preview_area;
    frame.origin.x += 4.0;
    frame.origin.y += 4.0;
    frame.size.width = (frame.size.width - 8.0).max(0.0);
    frame.size.height = (frame.size.height - 8.0).max(0.0);
    frame
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::CGSize;

    use super::*;

    fn frame(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
    }
    fn point(x: f64, y: f64) -> CGPoint { CGPoint::new(x, y) }
    fn rect() -> CGRect { frame(0.0, 0.0, 200.0, 100.0) }
    fn w(idx: u32) -> WindowId { WindowId::new(1, idx) }
    fn space() -> SpaceId { SpaceId::new(1) }
    fn target(idx: u32, frame: CGRect) -> DragSceneTarget {
        DragSceneTarget {
            window: w(idx),
            space: space(),
            frame,
        }
    }
    fn source(tiled: bool) -> DragSource {
        DragSource {
            window: w(1),
            origin_frame: rect(),
            last_frame: rect(),
            origin_space: Some(space()),
            current_space: Some(space()),
            tiled,
        }
    }
    fn scene(frame: CGRect) -> DragScene {
        DragScene {
            action_override: None,
            targets: vec![target(2, frame)],
        }
    }
    fn scene_with(targets: Vec<DragSceneTarget>) -> DragScene {
        DragScene {
            action_override: None,
            targets,
        }
    }
    fn motion(actor: &mut DragActor, x: f64, y: f64) -> bool {
        actor.motion(DragMotion { point: point(x, y) })
    }
    fn native(tiled: bool, last_frame: CGRect, scene: DragScene) -> DragActor {
        let mut source = source(tiled);
        source.last_frame = last_frame;
        let mut actor = DragActor::new(DragDropSettings::default());
        actor.begin_native(source, scene);
        actor
    }

    #[test]
    fn classifies_target_local_center_and_edges() {
        for (coords, zone) in [
            ((100., 50.), DropZone::Center),
            ((1., 50.), DropZone::West),
            ((199., 50.), DropZone::East),
            ((100., 1.), DropZone::North),
            ((100., 99.), DropZone::South),
            ((0., 0.), DropZone::West),
            ((200., 0.), DropZone::East),
            ((0., 100.), DropZone::West),
            ((200., 100.), DropZone::East),
        ] {
            assert_eq!(
                classify_zone(rect(), point(coords.0, coords.1), 0.25),
                Some(zone)
            );
        }
        assert_eq!(
            resolve_action(DropZone::South, MouseDropAction::Swap),
            WindowDropAction::Insert(Direction::Down),
        );
        assert_eq!(classify_zone(rect(), point(-1.0, 50.0), 0.25), None);
    }

    #[test]
    fn preview_inset_clamps_small_tiles() {
        let intent = dock::target(&scene(rect()), point(100.0, 50.0), 0.25, None).unwrap();
        for (area, expected) in [
            (rect(), frame(4.0, 4.0, 192.0, 92.0)),
            (frame(10.0, 20.0, 6.0, 6.0), frame(14.0, 24.0, 0.0, 0.0)),
        ] {
            assert_eq!(
                preview_frame(DropTarget { intent, preview_area: area }),
                expected
            );
        }
    }

    #[test]
    fn publisher_coalesces_to_newest_motion() {
        let publisher = DragMotionPublisher::default();
        for x in 0..1_000 {
            assert_eq!(
                publisher.publish(DragMotion {
                    point: CGPoint::new(x as f64, 0.0),
                }),
                x == 0
            );
        }
        assert!(publisher.wake_queued());
        assert_eq!(publisher.take_latest().unwrap().point.x, 999.0);
        assert!(!publisher.wake_queued());
        assert!(publisher.publish(DragMotion {
            point: CGPoint::new(1_000.0, 0.0),
        }));
    }

    #[test]
    fn native_tiled_move_targets_pointer_zone_and_commits_once() {
        let target_frame = frame(200.0, 0.0, 100.0, 100.0);
        let moved = frame(10.0, 0.0, 200.0, 100.0);
        let mut actor = native(true, moved, scene(target_frame));
        assert!(motion(&mut actor, 201.0, 50.0));
        let intent = actor.intent().unwrap();
        assert_eq!(intent.action, WindowDropAction::Insert(Direction::Left));
        actor.set_preview(intent, Some(target_frame));
        let preview = preview_frame(actor.target().unwrap());
        assert_eq!(preview.origin, CGPoint::new(204.0, 4.0));
        assert_eq!(preview.size, CGSize::new(92.0, 92.0));
        let commit = actor.finish(MouseButton::Left).unwrap();
        assert_eq!(commit.source.window, w(1));
        assert_eq!(commit.target.unwrap().intent.window, w(2));
        assert!(actor.finish(MouseButton::Left).is_none());
    }

    #[test]
    fn floating_moves_do_not_preview_the_starting_position() {
        let mut native_drag = native(false, rect(), DragScene::default());
        motion(&mut native_drag, 100.0, 50.0);
        assert!(native_drag.preview_target().is_none());

        let mut modifier_drag = DragActor::new(DragDropSettings::default());
        modifier_drag.begin_modifier(
            source(false),
            MouseButton::Left,
            point(100.0, 50.0),
            MouseAction::Move,
            DragScene::default(),
        );
        assert!(modifier_drag.preview_target().is_none());
        motion(&mut modifier_drag, 110.0, 50.0);
        assert!(modifier_drag.preview_target().is_none());
    }

    #[test]
    fn a_drop_the_layout_rejects_shows_nothing_and_is_not_asked_again() {
        let mut actor = native(true, rect(), scene(frame(200.0, 0.0, 200.0, 100.0)));
        motion(&mut actor, 201.0, 50.0);
        let intent = actor.intent().unwrap();
        actor.set_preview(intent, None);
        assert!(actor.intent().is_none());
        assert!(actor.preview_target().is_none());
        // Still in the same zone: nothing changed, so the reactor does not ask again.
        assert!(!motion(&mut actor, 205.0, 50.0));
        // A new zone is a new question.
        assert!(motion(&mut actor, 300.0, 50.0));
        assert_eq!(actor.intent().unwrap().zone, DropZone::Center);
        actor.set_preview(actor.intent().unwrap(), None);
        assert!(actor.finish(MouseButton::Left).unwrap().target.is_none());
    }

    #[test]
    fn overlapping_targets_prefer_scene_order() {
        let visible = w(2);
        let scene = scene_with(vec![target(2, rect()), target(3, rect())]);
        let intent = dock::target(&scene, point(100.0, 50.0), 0.25, None).unwrap();
        assert_eq!(intent.window, visible);
    }

    #[test]
    fn the_dragged_tile_itself_only_shows_home() {
        // Every zone of the source's own tile, with a neighbor right past it.
        let mut actor = native(true, rect(), scene(frame(200.0, 0.0, 200.0, 100.0)));
        for (x, y) in [(1.0, 50.0), (199.0, 50.0), (100.0, 1.0), (100.0, 99.0), (100.0, 50.0)] {
            motion(&mut actor, x, y);
            assert!(actor.intent().is_none());
            assert_eq!(actor.preview_target().unwrap().preview_area, rect());
        }
        assert!(actor.finish(MouseButton::Left).unwrap().target.is_none());
    }

    #[test]
    fn a_stack_mate_drawn_over_the_dragged_tile_is_not_a_target() {
        // Stack members share the tile's frame; over the tile only home is shown.
        let mut actor = native(true, rect(), scene(rect()));
        for (x, y) in [(100.0, 50.0), (1.0, 50.0), (199.0, 99.0)] {
            motion(&mut actor, x, y);
            assert!(actor.intent().is_none());
            assert_eq!(actor.preview_target().unwrap().preview_area, rect());
        }
        assert!(actor.finish(MouseButton::Left).unwrap().target.is_none());
    }

    #[test]
    fn tiles_own_their_left_and_top_edges_only() {
        let scene = scene_with(vec![
            target(2, frame(-200.0, 0.0, 200.0, 100.0)),
            target(3, frame(0.0, 0.0, 200.0, 100.0)),
        ]);
        // x = 0 is tile 3's left edge, not tile 2's right edge (a parked column).
        assert_eq!(dock::target(&scene, point(0.0, 50.0), 0.25, None).unwrap().window, w(3));
        assert!(dock::target(&scene, point(200.0, 50.0), 0.25, None).is_none());
        assert!(dock::target(&scene, point(100.0, 100.0), 0.25, None).is_none());
    }

    #[test]
    fn a_move_that_turns_into_a_resize_drops_its_target() {
        let target_frame = frame(200.0, 0.0, 200.0, 100.0);
        let mut actor = native(true, rect(), scene(target_frame));
        motion(&mut actor, 201.0, 50.0);
        actor.set_preview(actor.intent().unwrap(), Some(target_frame));
        assert!(actor.target().is_some());
        assert!(actor.update_native(w(1), frame(0.0, 0.0, 180.0, 100.0)));
        assert!(actor.target().is_none());
        assert!(actor.finish(MouseButton::Left).unwrap().target.is_none());
    }

    #[test]
    fn zones_follow_the_pointer_without_stickiness() {
        let mut actor = native(true, rect(), scene(frame(200.0, 0.0, 200.0, 100.0)));
        assert!(motion(&mut actor, 201.0, 50.0));
        assert_eq!(actor.intent().unwrap().zone, DropZone::West);
        // Just past the West zone is the Center: no margin holds on to West.
        assert!(motion(&mut actor, 251.0, 50.0));
        assert_eq!(actor.intent().unwrap().zone, DropZone::Center);
    }

    #[test]
    fn gaps_and_empty_space_are_not_targets() {
        let scene = scene_with(vec![
            target(2, frame(220.0, 0.0, 100.0, 100.0)),
            target(3, frame(330.0, 0.0, 100.0, 100.0)),
        ]);
        assert!(dock::target(&scene, point(325.0, 50.0), 0.25, None).is_none());
        assert!(dock::target(&scene, point(270.0, 150.0), 0.25, None).is_none());
        let mut actor = native(true, rect(), scene);
        motion(&mut actor, 325.0, 50.0);
        assert!(actor.intent().is_none());
        assert!(actor.preview_target().is_none());
    }

    #[test]
    fn edges_insert_and_the_center_swaps() {
        let scene = scene(rect());
        for ((x, y), action) in [
            ((100.0, 50.0), WindowDropAction::Swap),
            ((1.0, 50.0), WindowDropAction::Insert(Direction::Left)),
            ((199.0, 50.0), WindowDropAction::Insert(Direction::Right)),
            ((100.0, 1.0), WindowDropAction::Insert(Direction::Up)),
            ((100.0, 99.0), WindowDropAction::Insert(Direction::Down)),
        ] {
            assert_eq!(dock::target(&scene, point(x, y), 0.25, None).unwrap().action, action);
        }
    }

    #[test]
    fn overview_never_targets_the_dragged_card() {
        let scene =
            scene_with(vec![target(1, rect()), target(2, frame(200.0, 0.0, 200.0, 100.0))]);
        assert!(hit_test_projected(&scene, w(1), point(199.0, 50.0)).is_none());
        assert_eq!(hit_test_projected(&scene, w(1), point(201.0, 50.0)).unwrap().window, w(2));
    }

    #[test]
    fn replacing_scene_invalidates_a_validated_preview() {
        let mut actor = native(true, rect(), scene(frame(200.0, 0.0, 200.0, 100.0)));
        motion(&mut actor, 201.0, 50.0);
        let intent = actor.intent().unwrap();
        actor.set_preview(intent, Some(rect()));
        assert!(actor.target().is_some());

        // Still under the pointer: only a tile there can be a target.
        let moved = frame(180.0, 0.0, 200.0, 100.0);
        actor.replace_scene(scene(moved));
        assert!(actor.target().is_none());
        let moved_intent = actor.intent().unwrap();
        assert_eq!(moved_intent.frame, moved);
        actor.set_preview(moved_intent, Some(moved));
        let mut settings = DragDropSettings::default();
        settings.drop_zone_fraction = 0.3;
        actor.update_config(settings);
        assert!(actor.intent().is_none());
        assert!(actor.target().is_none());
    }

    #[test]
    fn floating_move_never_builds_a_drop_target() {
        let moved = frame(10.0, 0.0, 200.0, 100.0);
        let mut actor = native(false, moved, scene(rect()));
        motion(&mut actor, 100.0, 50.0);
        assert!(actor.target().is_none());
    }

    #[test]
    fn entering_another_space_targets_that_spaces_scene() {
        let destination = SpaceId::new(2);
        let mut actor = DragActor::new(DragDropSettings::default());
        actor.begin_modifier(
            source(true),
            MouseButton::Left,
            point(100.0, 50.0),
            MouseAction::Move,
            scene(frame(200.0, 0.0, 200.0, 100.0)),
        );
        motion(&mut actor, 300.0, 50.0);
        let intent = actor.intent().unwrap();
        actor.set_preview(intent, Some(rect()));
        assert!(actor.target().is_some());

        let there = frame(1000.0, 0.0, 200.0, 100.0);
        let scene = scene_with(vec![DragSceneTarget {
            window: w(3),
            space: destination,
            frame: there,
        }]);
        actor.update_current_space(Some(destination), scene);
        assert_eq!(actor.source().unwrap().current_space, Some(destination));
        assert!(actor.intent().is_none());
        assert!(actor.target().is_none());
        motion(&mut actor, 300.0, 50.0);
        assert!(actor.intent().is_none());
        motion(&mut actor, 1100.0, 50.0);
        let intent = actor.intent().unwrap();
        assert_eq!((intent.window, intent.space), (w(3), destination));
        actor.set_preview(intent, Some(there));
        let target = actor.finish(MouseButton::Left).unwrap().target.unwrap();
        assert_eq!((target.intent.window, target.intent.space), (w(3), destination));
    }

    #[test]
    fn stack_scene_resolves_every_zone_to_swap() {
        let mut scene = scene(rect());
        scene.action_override = Some(WindowDropAction::Swap);
        for point in [point(1.0, 50.0), point(100.0, 50.0)] {
            let intent = dock::target(&scene, point, 0.25, None).unwrap();
            assert_eq!(intent.action, WindowDropAction::Swap);
        }
    }

    #[test]
    fn cancellation_identifies_native_and_modifier_sessions() {
        let mut moved = source(false);
        moved.last_frame = frame(20.0, 20.0, 200.0, 100.0);
        let mut actor = native(false, moved.last_frame, DragScene::default());
        assert_eq!(actor.cancel().unwrap().kind, DragKind::NativeMove);

        actor.begin_modifier(
            moved,
            MouseButton::Left,
            point(10.0, 10.0),
            MouseAction::Move,
            DragScene::default(),
        );
        assert_eq!(actor.cancel().unwrap().kind, DragKind::ModifierMove);
    }

    #[test]
    fn only_the_owning_button_finishes_a_modifier_drag() {
        let mut actor = DragActor::new(DragDropSettings::default());
        actor.begin_modifier(
            source(true),
            MouseButton::Left,
            point(10.0, 10.0),
            MouseAction::Move,
            DragScene::default(),
        );

        assert!(actor.finish(MouseButton::Right).is_none());
        assert!(actor.is_active());
        assert!(actor.finish(MouseButton::Left).is_some());
        assert!(!actor.is_active());
    }
}
