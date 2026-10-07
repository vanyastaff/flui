//! Direct access to owned pointer events with a computed movement delta.
//!
//! Raw delivery borrows the original event, preserving its identity, timestamp,
//! sensors, coalesced samples and predictions without a second event vocabulary.

use crate::events::{
    PointerEvent, PointerEventExt, PointerId, PointerInfo, PointerKind, get_event_time,
    get_pointer_info,
};
use flui_foundation::geometry::Offset;
use flui_platform_api::EventTime;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

/// A borrowed raw dispatch and its computed movement delta.
#[derive(Debug, Clone, Copy)]
pub struct RawPointerEvent<'event> {
    event: &'event PointerEvent,
    delta: Option<Offset<f64>>,
}

impl<'event> RawPointerEvent<'event> {
    /// The complete source event, including all measured and predicted samples.
    #[must_use]
    pub fn event(&self) -> &'event PointerEvent {
        self.event
    }

    /// The contact identity, absent for device lifecycle events.
    #[must_use]
    pub fn pointer(&self) -> Option<PointerId> {
        self.event.pointer_id()
    }

    /// The reported position, when this event carries one.
    #[must_use]
    pub fn position(&self) -> Option<Offset<f64>> {
        self.event.position()
    }

    /// Movement since the previous known position, or `None` when unknown or overflowing.
    /// A Down has the defined initial delta zero.
    #[must_use]
    pub fn delta(&self) -> Option<Offset<f64>> {
        self.delta
    }

    /// The device kind reported by the source.
    #[must_use]
    pub fn device_kind(&self) -> Option<PointerKind> {
        self.event.pointer_kind()
    }

    /// The source's production timestamp, including epoch zero.
    #[must_use]
    pub fn timestamp(&self) -> Option<EventTime> {
        get_event_time(self.event)
    }

    /// Whether a contact began.
    #[must_use]
    pub fn is_down(&self) -> bool {
        matches!(self.event, PointerEvent::Down(_))
    }
    /// Whether a pointer moved.
    #[must_use]
    pub fn is_move(&self) -> bool {
        matches!(self.event, PointerEvent::Move(_))
    }
    /// Whether a contact ended with a release.
    #[must_use]
    pub fn is_up(&self) -> bool {
        matches!(self.event, PointerEvent::Up(_))
    }
    /// Whether a contact was cancelled.
    #[must_use]
    pub fn is_cancel(&self) -> bool {
        matches!(self.event, PointerEvent::Cancel(_))
    }
}

/// A raw callback borrows the source only for its synchronous dispatch.
pub type RawInputCallback = Rc<dyn for<'event> Fn(RawPointerEvent<'event>)>;

#[derive(Debug)]
struct PointerTrackingState {
    last_position: Offset<f64>,
    is_down: bool,
    pointer: PointerInfo,
}

/// Owner-local raw input tracking and synchronous borrowed delivery.
#[derive(Clone)]
pub struct RawInputHandler {
    tracking: Rc<RefCell<HashMap<PointerId, PointerTrackingState>>>,
    callback: Rc<RefCell<Option<RawInputCallback>>>,
    enabled: Rc<Cell<bool>>,
}

impl Default for RawInputHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RawInputHandler {
    /// Create an enabled handler without a callback.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracking: Rc::new(RefCell::new(HashMap::new())),
            callback: Rc::new(RefCell::new(None)),
            enabled: Rc::new(Cell::new(true)),
        }
    }

    /// Replace the callback; outgoing captures retire after the borrow is released.
    pub fn set_callback(&self, callback: impl for<'event> Fn(RawPointerEvent<'event>) + 'static) {
        let _previous = self.callback.borrow_mut().replace(Rc::new(callback));
    }

    /// Clear the callback; outgoing captures retire after the borrow is released.
    pub fn clear_callback(&self) {
        let _previous = self.callback.borrow_mut().take();
    }

    /// Enable or disable raw delivery.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
    }
    /// Whether raw delivery is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Commit tracking before invoking the callback without an internal borrow.
    /// Replacement or clearing through this same handle applies to the next event.
    pub fn handle_event<'event>(
        &self,
        event: &'event PointerEvent,
    ) -> Option<RawPointerEvent<'event>> {
        if !self.enabled.get() {
            return None;
        }
        let raw = RawPointerEvent {
            event,
            delta: self.update_tracking(event),
        };
        let callback = self.callback.borrow().clone();
        if let Some(callback) = callback {
            callback(raw);
        }
        Some(raw)
    }

    fn update_tracking(&self, event: &PointerEvent) -> Option<Offset<f64>> {
        if let PointerEvent::DeviceRemoved(device) = event {
            self.tracking
                .borrow_mut()
                .retain(|_, state| state.pointer.device != Some(device.device));
            return None;
        }
        let pointer = *get_pointer_info(event)?;
        let mut tracking = self.tracking.borrow_mut();
        if matches!(event, PointerEvent::Cancel(_)) {
            tracking.remove(&pointer.id);
            return None;
        }
        let position = event.position()?;
        match event {
            PointerEvent::Down(_) => {
                tracking.insert(
                    pointer.id,
                    PointerTrackingState {
                        last_position: position,
                        is_down: true,
                        pointer,
                    },
                );
                Some(Offset::ZERO)
            }
            PointerEvent::Up(_) => tracking
                .remove(&pointer.id)
                .and_then(|state| finite_delta(position, state.last_position)),
            PointerEvent::Move(_) | PointerEvent::ButtonChange(_) => {
                if let Some(state) = tracking.get_mut(&pointer.id) {
                    let delta = finite_delta(position, state.last_position);
                    state.last_position = position;
                    state.pointer = pointer;
                    delta
                } else if matches!(event, PointerEvent::Move(_)) {
                    tracking.insert(
                        pointer.id,
                        PointerTrackingState {
                            last_position: position,
                            is_down: false,
                            pointer,
                        },
                    );
                    None
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Number of tracked contacts and hover pointers.
    #[must_use]
    pub fn tracked_pointer_count(&self) -> usize {
        self.tracking.borrow().len()
    }
    /// Number of contacts still down.
    #[must_use]
    pub fn active_pointer_count(&self) -> usize {
        self.tracking
            .borrow()
            .values()
            .filter(|state| state.is_down)
            .count()
    }
    /// Whether this contact is down.
    #[must_use]
    pub fn is_pointer_down(&self, pointer: PointerId) -> bool {
        self.tracking
            .borrow()
            .get(&pointer)
            .is_some_and(|state| state.is_down)
    }
    /// Last known measured position for a tracked pointer.
    #[must_use]
    pub fn pointer_position(&self, pointer: PointerId) -> Option<Offset<f64>> {
        self.tracking
            .borrow()
            .get(&pointer)
            .map(|state| state.last_position)
    }
    /// Forget tracking without retiring the callback.
    pub fn reset(&self) {
        self.tracking.borrow_mut().clear();
    }
}

fn finite_delta(position: Offset<f64>, previous: Offset<f64>) -> Option<Offset<f64>> {
    let delta = Offset::new(position.dx - previous.dx, position.dy - previous.dy);
    (delta.dx.is_finite() && delta.dy.is_finite()).then_some(delta)
}

impl std::fmt::Debug for RawInputHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RawInputHandler")
            .field("tracked_pointers", &self.tracked_pointer_count())
            .field("active_pointers", &self.active_pointer_count())
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

/// Which synchronous input paths receive an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Gesture routing only.
    #[default]
    Gesture,
    /// Raw delivery only.
    Raw,
    /// Both gesture routing and raw delivery.
    Both,
}

impl InputMode {
    /// Whether gesture routing is enabled.
    #[must_use]
    pub fn has_gestures(self) -> bool {
        matches!(self, Self::Gesture | Self::Both)
    }
    /// Whether raw delivery is enabled.
    #[must_use]
    pub fn has_raw(self) -> bool {
        matches!(self, Self::Raw | Self::Both)
    }
}
