//! The state a mounted viewer's callbacks share: configuration, transform,
//! channel leaf and inertia, and the operations that apply gestures to them.

#[cfg(doc)]
use super::types::WheelScaleGate;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::{Axis, Matrix4, Offset, Point, Rect, Vec2};
use flui_interaction::focus::{FocusNode, KeyEventResult};
use flui_interaction::pointer::WheelPreferencesProvider;
use flui_objects::SubtreeAnchor;
use flui_platform::input::keyboard::{ImeComposition, Key, KeyPress, KeyState, NamedKey};
use flui_platform::input::pointer::{ScrollEvent, ScrollPhase};
use flui_rendering::pipeline::PipelineCell;
use flui_semantics::ScrollUnit as SemanticsScrollUnit;

use super::boundary::{align_to_axis, contain_transform, gesture_transform, uniform_scale};
use super::fling::FocalFling;
use super::leaf::{ContactlessInput, GestureTracking, ViewerCallbacks, ViewerLeaf};
use super::types::{PanAxis, PanIntent, ZoomIntent};
use super::zoom_gestures::toward;
use super::{KEY_STEP, Overlay, PAGE_FRACTION, Travel, ViewerConfig, ZOOM_STEP};
use crate::scroll::WheelScrollDistances;
use crate::scroll::nested::{ScrollCause, ScrollDevice, ScrollEndReason};
use crate::scroll::wheel_axis_delta;
use crate::support::command_modifiers;
use crate::{CommandHandle, TransformationController};

/// Everything a mounted viewer's callbacks share: the configuration, the
/// transform, the channel leaf and the inertia. It lives in the element's
/// state; every callback holds it, so a rebuild between two events of one
/// gesture continues that gesture under the new configuration.
pub(super) struct ViewerCore {
    pub(super) controller: RefCell<TransformationController>,
    pub(super) config: Cell<ViewerConfig>,
    pub(super) callbacks: RefCell<ViewerCallbacks>,
    /// Publishes the child's `RenderId` while mounted — see
    /// [`geometry`](Self::geometry).
    pub(super) anchor: SubtreeAnchor,
    /// Acquired in lifecycle hooks, read only from event callbacks.
    pub(super) pipeline: RefCell<Option<PipelineCell>>,
    pub(super) gesture: GestureTracking,
    pub(super) leaf: ViewerLeaf,
    pub(super) fling: FocalFling,
    /// Where a double tap happened, for the zoom it performs.
    pub(super) focal_hint: Cell<Option<Offset<f64>>>,
    /// Performs the first command of the alternatives, resolved below the
    /// viewer's own actions.
    pub(super) command: RefCell<Option<CommandHandle>>,
    /// The focus node of the alternatives' row, which a contact on the
    /// viewer focuses so its keys work next.
    pub(super) row_focus: RefCell<Option<Rc<FocusNode>>>,
    /// What the gesture pulled past the bounds, unresisted.
    pub(super) travel: Cell<Travel>,
    /// What the viewer shows past the bounds.
    pub(super) overlay: Cell<Overlay>,
    /// While an overlay shows: the in-bounds transform under it, and the
    /// value published with the overlay.
    pub(super) stretched: Cell<Option<(Matrix4, Matrix4)>>,
}

impl ViewerCore {
    pub(super) fn controller(&self) -> TransformationController {
        self.controller.borrow().clone()
    }

    pub(super) fn callbacks(&self) -> ViewerCallbacks {
        self.callbacks.borrow().clone()
    }

    /// The viewport rect and the boundary rect, in scene coordinates, or
    /// `None` before the child is mounted and laid out.
    ///
    /// The child is laid out under this widget's own constraints, and
    /// everything between them (`Listener`, `GestureDetector`, `ClipRect`,
    /// `Transform`) adopts the child's size, so the viewport and the child's
    /// own unmargined rect (inflated by `boundary_margin` for the boundary)
    /// are the same rectangle and one anchor serves both.
    pub(super) fn geometry(&self) -> Option<(Rect<f64>, Rect<f64>)> {
        let owner = self.pipeline.borrow().clone()?;
        let render_id = self.anchor.get()?;
        let size = owner.with(|owner| owner.box_size(render_id))?;
        let rect = Rect::from_origin_size(Point::new(0.0, 0.0), size);
        Some((rect, self.config.get().boundary_margin.inflate_rect(rect)))
    }

    pub(super) fn scale(&self) -> f64 {
        uniform_scale(&self.controller().value())
    }

    /// `delta` limited to the axes the viewer pans now.
    pub(super) fn lock(&self, delta: Offset<f64>) -> Offset<f64> {
        match self.config.get().pan_axis {
            PanAxis::Free => delta,
            PanAxis::Horizontal => align_to_axis(delta, Axis::Horizontal),
            PanAxis::Vertical => align_to_axis(delta, Axis::Vertical),
            PanAxis::Aligned => self
                .gesture
                .current_axis
                .get()
                .map_or(delta, |axis| align_to_axis(delta, axis)),
        }
    }

    /// The in-bounds transform: the published one without the overlay shown
    /// past the bounds. A value written from outside since replaces the
    /// overlay.
    pub(super) fn base(&self) -> Matrix4 {
        let value = self.controller().value();
        match self.stretched.get() {
            Some((base, published)) if published == value => base,
            Some(_) => {
                self.travel.set(Travel::default());
                self.overlay.set(Overlay::default());
                self.stretched.set(None);
                value
            }
            None => value,
        }
    }

    /// Publish the in-bounds transform `base` with the overlay over it. An
    /// overlay that would not publish a finite, invertible transform is
    /// dropped.
    pub(super) fn publish(&self, base: Matrix4) {
        let overlay = self.overlay.get();
        let shown = if overlay.is_rest() {
            None
        } else {
            overlay
                .matrix()
                .map(|overlay| overlay * base)
                .filter(|shown| shown.try_inverse().is_some())
        };
        let controller = self.controller();
        if let Some(shown) = shown {
            self.stretched.set(Some((base, shown)));
            controller.set_value(shown);
        } else {
            self.travel.set(Travel::default());
            self.overlay.set(Overlay::default());
            self.stretched.set(None);
            controller.set_value(base);
        }
    }

    /// Bring the scene back inside the bounds at once.
    pub(super) fn collapse(&self) {
        if self.stretched.get().is_some() {
            let base = self.base();
            self.overlay.set(Overlay::default());
            self.publish(base);
        }
    }

    /// A path that is no gesture moves the scene: the motion stops and the
    /// scene is inside its bounds.
    pub(super) fn interrupt(&self) {
        self.fling.stop(ScrollEndReason::Preempted);
        self.collapse();
    }

    /// A gesture starts over a snap-back: it continues from where the scene
    /// shows.
    pub(super) fn catch_stretch(&self) {
        let overlay = self.overlay.get();
        if overlay.is_rest() {
            self.travel.set(Travel::default());
            return;
        }
        let Some((viewport, _)) = self.geometry() else {
            self.collapse();
            return;
        };
        self.travel
            .set(overlay.travel(viewport.width(), viewport.height()));
    }

    /// Show the travel through the rubber band, the scale part about `focal`.
    pub(super) fn show_travel(&self, base: Matrix4, focal: Offset<f64>) {
        let Some((viewport, _)) = self.geometry() else {
            return;
        };
        self.overlay.set(Overlay::of(
            self.travel.get(),
            focal,
            viewport.width(),
            viewport.height(),
        ));
        self.publish(base);
    }

    /// Move the scene by `wanted` viewport pixels within the boundary and
    /// return how far it moved toward `wanted` on each axis. With `commit`
    /// false, only measure.
    pub(super) fn translate(&self, wanted: Offset<f64>, commit: bool) -> Offset<f64> {
        if wanted == Offset::ZERO || !wanted.is_finite() {
            return Offset::ZERO;
        }
        let Some((viewport, boundary)) = self.geometry() else {
            return Offset::ZERO;
        };
        if !viewport.is_finite() || viewport.width() <= 0.0 || viewport.height() <= 0.0 {
            return Offset::ZERO;
        }
        let before = self.base();
        let Some(next) = contain_transform(
            Matrix4::translation(wanted.dx, wanted.dy, 0.0) * before,
            viewport,
            boundary,
        ) else {
            return Offset::ZERO;
        };
        let (bx, by, _) = before.translation_component();
        let (nx, ny, _) = next.translation_component();
        let moved = Offset::new(toward(nx - bx, wanted.dx), toward(ny - by, wanted.dy));
        if commit && next != before {
            self.publish(next);
        }
        moved
    }

    /// The viewer's part of a channel step `offer` (physical channel axes:
    /// the scene moves against it). On the axes `stretch` names — those the
    /// viewer holds for a contact — what the boundary stops is pulled past
    /// it through the rubber band and consumed whole.
    pub(super) fn consume(&self, offer: Vec2, stretch: [bool; 2]) -> Vec2 {
        if !self.config.get().pan_enabled {
            return Vec2::ZERO;
        }
        let wanted = self.lock(Offset::new(-offer.x, -offer.y));
        if stretch == [false, false] {
            let moved = self.translate(wanted, true);
            return Vec2::new(-moved.dx, -moved.dy);
        }
        let mut travel = self.travel.get();
        let carried = |axis: usize, value: f64| if stretch[axis] { value } else { 0.0 };
        let total = Offset::new(
            wanted.dx + carried(0, travel.pan.dx),
            wanted.dy + carried(1, travel.pan.dy),
        );
        let moved = self.translate(total, true);
        let past = |axis: usize, total: f64, moved: f64| {
            let past = total - moved;
            if stretch[axis] && past.abs() > 1e-9 && past.is_finite() {
                past
            } else {
                0.0
            }
        };
        travel.pan = Offset::new(past(0, total.dx, moved.dx), past(1, total.dy, moved.dy));
        self.travel.set(travel);
        let focal = self.overlay.get().focal;
        self.show_travel(self.base(), focal);
        let consumed = Offset::new(
            if stretch[0] { wanted.dx } else { moved.dx },
            if stretch[1] { wanted.dy } else { moved.dy },
        );
        Vec2::new(-consumed.dx, -consumed.dy)
    }

    /// A pinch step: scale by `exp(log_ratio)` and rotate about `pivot`. The
    /// transform follows within the scale bounds; what a bound stops goes
    /// into the rubber band, shown about `pivot`.
    pub(super) fn pinch(&self, pivot: Offset<f64>, log_ratio: f64, rotation: f64) {
        let config = self.config.get();
        if !log_ratio.is_finite() {
            return;
        }
        let Some((viewport, boundary)) = self.geometry() else {
            return;
        };
        let base = self.base();
        let mut travel = self.travel.get();
        let total = travel.log_scale + log_ratio;
        let Some(next) = gesture_transform(
            base,
            pivot,
            Offset::ZERO,
            total.exp(),
            rotation,
            config.min_scale,
            config.max_scale,
            viewport,
            boundary,
        ) else {
            return;
        };
        let past = total - (uniform_scale(&next) / uniform_scale(&base)).ln();
        travel.log_scale = if past.is_finite() && past.abs() > 1e-9 {
            past
        } else {
            0.0
        };
        self.travel.set(travel);
        self.show_travel(next, pivot);
    }

    /// One step of `intent` toward a side, as its own assistive sequence.
    pub(super) fn pan_step(&self, intent: PanIntent) -> bool {
        let Some((viewport, _)) = self.geometry() else {
            return false;
        };
        let step = |dimension: f64| KEY_STEP.min((dimension * PAGE_FRACTION).max(0.0));
        let available = match intent {
            PanIntent::Left => Vec2::new(-step(viewport.width()), 0.0),
            PanIntent::Right => Vec2::new(step(viewport.width()), 0.0),
            PanIntent::Up => Vec2::new(0.0, -step(viewport.height())),
            PanIntent::Down => Vec2::new(0.0, step(viewport.height())),
        };
        self.interrupt();
        self.leaf
            .one_shot(ScrollDevice::Assistive, available, |offer| {
                self.consume(offer, [false, false])
            })
            != Vec2::ZERO
    }

    /// Zoom by `factor` about `focal` (the viewport's centre without one).
    /// Returns whether the transform changed.
    pub(super) fn zoom_by(&self, factor: f64, focal: Option<Offset<f64>>) -> bool {
        let config = self.config.get();
        if !config.scale_enabled || !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let Some((viewport, boundary)) = self.geometry() else {
            return false;
        };
        let focal = focal.unwrap_or_else(|| {
            Offset::new(
                f64::midpoint(viewport.min.x, viewport.max.x),
                f64::midpoint(viewport.min.y, viewport.max.y),
            )
        });
        let before = self.base();
        let Some(next) = gesture_transform(
            before,
            focal,
            Offset::ZERO,
            factor,
            0.0,
            config.min_scale,
            config.max_scale,
            viewport,
            boundary,
        ) else {
            return false;
        };
        if next == before {
            return false;
        }
        self.publish(next);
        true
    }

    /// One zoom step for `intent`, about the double tap's point if one
    /// performed it.
    pub(super) fn zoom_step(&self, intent: ZoomIntent) -> bool {
        let focal = self.focal_hint.take();
        self.interrupt();
        let factor = match intent {
            ZoomIntent::In => ZOOM_STEP,
            ZoomIntent::Out => ZOOM_STEP.recip(),
        };
        self.zoom_by(factor, focal)
    }

    /// Pan one assistive step toward `direction` (a physical channel unit
    /// vector), by an item or a page.
    pub(super) fn assistive_pan(&self, direction: Vec2, unit: SemanticsScrollUnit) {
        let Some((viewport, _)) = self.geometry() else {
            return;
        };
        let distance = |dimension: f64| {
            let page = (dimension * PAGE_FRACTION).max(0.0);
            match unit {
                SemanticsScrollUnit::Page => page,
                _ => KEY_STEP.min(page),
            }
        };
        let available = Vec2::new(
            direction.x * distance(viewport.width()),
            direction.y * distance(viewport.height()),
        );
        self.interrupt();
        let _ = self
            .leaf
            .one_shot(ScrollDevice::Assistive, available, |offer| {
                self.consume(offer, [false, false])
            });
    }

    /// An arrow key: one step of the keyboard sequence its press began.
    pub(super) fn key_pan(
        &self,
        event: &flui_platform::input::keyboard::KeyEvent,
    ) -> KeyEventResult {
        if event.state == KeyState::Up {
            self.leaf.key_released(event.code);
            return KeyEventResult::Ignored;
        }
        if event.composition == ImeComposition::Active
            || !command_modifiers(event.modifiers).is_empty()
        {
            return KeyEventResult::Ignored;
        }
        let direction = match &event.key {
            Key::Named(NamedKey::ArrowLeft) => Vec2::new(-1.0, 0.0),
            Key::Named(NamedKey::ArrowRight) => Vec2::new(1.0, 0.0),
            Key::Named(NamedKey::ArrowUp) => Vec2::new(0.0, -1.0),
            Key::Named(NamedKey::ArrowDown) => Vec2::new(0.0, 1.0),
            _ => return KeyEventResult::Ignored,
        };
        let Some((viewport, _)) = self.geometry() else {
            return KeyEventResult::Ignored;
        };
        let step = |dimension: f64| KEY_STEP.min((dimension * PAGE_FRACTION).max(0.0));
        let available = Vec2::new(
            direction.x * step(viewport.width()),
            direction.y * step(viewport.height()),
        );
        let repeated = event.state == KeyState::Down(KeyPress::AutoRepeat);
        let id = match self.leaf.key_sequence(event.code) {
            Some(id) if repeated => Some(id),
            _ => self.leaf.mint(),
        };
        self.interrupt();
        let consumed = match id {
            Some(id) => self.leaf.contactless_step(
                id,
                ScrollDevice::Keyboard,
                ContactlessInput::Key(event.code),
                ScrollCause::UserInput,
                available,
                |offer| self.consume(offer, [false, false]),
            ),
            None => self.consume(available, [false, false]),
        };
        if consumed == Vec2::ZERO {
            KeyEventResult::Ignored
        } else {
            KeyEventResult::Handled
        }
    }

    /// A plain wheel tick or a trackpad scroll under
    /// [`WheelScaleGate::CtrlWheel`]: a step of the binding's latch, which
    /// the viewer starts only when it can move.
    pub(super) fn wheel_pan(
        &self,
        data: &ScrollEvent,
        wheel: Option<&WheelPreferencesProvider>,
    ) -> bool {
        if !self.config.get().pan_enabled {
            return false;
        }
        let Some((viewport, _)) = self.geometry() else {
            return false;
        };
        let preferences = wheel.map_or_default(WheelPreferencesProvider::snapshot);
        let distances = WheelScrollDistances::default();
        let available = Vec2::new(
            wheel_axis_delta(
                Axis::Horizontal,
                data,
                viewport.width(),
                &preferences,
                distances,
            ),
            wheel_axis_delta(
                Axis::Vertical,
                data,
                viewport.height(),
                &preferences,
                distances,
            ),
        );
        if available == Vec2::ZERO {
            return false;
        }
        let Some(id) = self.leaf.dispatched_or_mint() else {
            return false;
        };
        if !self.leaf.leads(id)
            && self.translate(self.lock(Offset::new(-available.x, -available.y)), false)
                == Offset::ZERO
        {
            return false;
        }
        self.interrupt();
        let cause = match data.phase {
            Some(
                ScrollPhase::MomentumBegan
                | ScrollPhase::MomentumChanged
                | ScrollPhase::MomentumEnded,
            ) => ScrollCause::Momentum,
            _ => ScrollCause::UserInput,
        };
        let pointer = data.position.get();
        let pointer = Offset::new(pointer.x, pointer.y);
        let device = ScrollDevice::Pointer {
            kind: data.pointer.kind,
            device: data.pointer.device,
        };
        let _ = self.leaf.contactless_step(
            id,
            device,
            ContactlessInput::Wheel(pointer),
            cause,
            available,
            |offer| self.consume(offer, [false, false]),
        );
        true
    }
}
