//! Fingers resting on a touchpad, and a multi-finger swipe across it.

use core::num::NonZeroU8;

use flui_foundation::geometry::Offset;

use super::PointerSource;
use super::value::{InputValueError, PointerPosition, Quantity, finite};
use crate::EventTime;
use crate::keyboard::Modifiers;

/// Where a touchpad hold is.
///
/// A hold is `Begin`, then `End` or `Cancelled`, all with one [`PointerSource::id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "fingers come to rest on a touchpad, then lift or start a gesture"
)]
pub enum TouchpadHoldPhase {
    /// Fingers came to rest on the touchpad without moving. A fling under the pointer (a
    /// scrollable's, an interactive viewer's inertia) stops here, as the platform's own
    /// momentum does, and stays stopped whichever phase follows.
    Begin,
    /// The fingers lifted without moving: the hold was the whole gesture.
    End,
    /// The fingers started a gesture (a scroll, a pinch, a swipe), or the platform took the
    /// hold back. The gesture that follows arrives as its own events.
    Cancelled,
}

/// Fingers resting on a touchpad: Wayland's `zwp_pointer_gesture_hold_v1`, an AppKit scroll in
/// `NSEventPhaseMayBegin`, a precision touchpad's DirectManipulation viewport running before
/// its manipulation passes a slop.
///
/// Built with [`TouchpadHoldEvent::new`] and [`with_modifiers`](Self::with_modifiers).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TouchpadHoldEvent {
    pointer: PointerSource,
    /// When the platform observed the event.
    pub time: EventTime,
    /// Where the pointer rests.
    pub position: PointerPosition,
    /// Where the hold is.
    pub phase: TouchpadHoldPhase,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

impl TouchpadHoldEvent {
    /// A hold event at `position` with no modifiers. The pointer's kind is set to
    /// [`PointerKind::Trackpad`](super::PointerKind::Trackpad) whatever `pointer` carried.
    #[must_use]
    pub const fn new(
        pointer: PointerSource,
        time: EventTime,
        position: PointerPosition,
        phase: TouchpadHoldPhase,
    ) -> Self {
        let mut pointer = pointer;
        pointer.kind = super::PointerKind::Trackpad;
        Self {
            pointer,
            time,
            position,
            phase,
            modifiers: Modifiers::NONE,
        }
    }

    /// This event with `modifiers` held.
    #[must_use]
    pub const fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// The hold's pointer; its kind is always
    /// [`PointerKind::Trackpad`](super::PointerKind::Trackpad).
    #[must_use]
    pub const fn pointer(&self) -> &PointerSource {
        &self.pointer
    }
}

/// How far a touchpad swipe's fingers have travelled **since its start**, in logical pixels,
/// positive right and down.
///
/// Cumulative, as [`PanZoomTransform`](super::PanZoomTransform) is: an update replaces the
/// previous one, and a consumer that joins mid-gesture sees the whole travel. A backend whose
/// platform reports steps (Wayland, XInput 2.4) sums them.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Offset;
/// use flui_platform::pointer::SwipeOffset;
///
/// let travelled = SwipeOffset::try_new(Offset::new(-120.0, 4.0))?;
/// assert_eq!(travelled.get().dx, -120.0);
/// assert!(SwipeOffset::try_new(Offset::new(f64::NAN, 0.0)).is_err());
/// # Ok::<(), flui_platform::pointer::InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwipeOffset(Offset<f64>);

impl SwipeOffset {
    /// No travel: where every swipe starts.
    pub const ZERO: Self = Self(Offset::new(0.0, 0.0));

    /// The travel `offset`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when a component is NaN or infinite.
    pub fn try_new(offset: Offset<f64>) -> Result<Self, InputValueError> {
        let dx = finite(Quantity::SwipeOffset, offset.dx)?;
        let dy = finite(Quantity::SwipeOffset, offset.dy)?;
        Ok(Self(Offset::new(dx, dy)))
    }

    /// The travel, finite in both components.
    #[must_use]
    pub const fn get(self) -> Offset<f64> {
        self.0
    }
}

impl Default for SwipeOffset {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Where a touchpad swipe is.
#[derive(Debug, Clone, Copy, PartialEq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a swipe starts, moves, then ends or is taken back"
)]
pub enum TouchpadSwipePhase {
    /// The fingers began to swipe together; the travel is [`SwipeOffset::ZERO`].
    Start,
    /// The fingers moved: the travel since [`Start`](Self::Start).
    Update(SwipeOffset),
    /// The fingers lifted. A consumer acts on the travel and the speed it saw.
    End,
    /// The platform took the swipe back. A consumer ends it without acting.
    Cancelled,
}

/// One event of a multi-finger touchpad swipe (three fingers or more on every platform that
/// reports one): Wayland's `zwp_pointer_gesture_swipe_v1`, XInput 2.4's `XI_GestureSwipe*`.
///
/// A swipe is `Start`, any number of `Update`s, then `End` or `Cancelled`, all with the same
/// [`PointerSource::id`] and finger count. It is a gesture of its own, never a
/// [`ScrollEvent`](super::ScrollEvent): a two-finger pan is the scroll, and the pointer does
/// not move while the fingers swipe, so [`position`](Self::position) stays where the swipe
/// started.
///
/// Built with [`TouchpadSwipeEvent::new`] and [`with_modifiers`](Self::with_modifiers).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TouchpadSwipeEvent {
    pointer: PointerSource,
    /// When the platform observed the event.
    pub time: EventTime,
    /// Where the pointer rests while the fingers swipe.
    pub position: PointerPosition,
    /// How many fingers swipe.
    pub fingers: NonZeroU8,
    /// Where the swipe is, with its travel.
    pub phase: TouchpadSwipePhase,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

impl TouchpadSwipeEvent {
    /// A swipe event of `fingers` at `position` with no modifiers. The pointer's kind is set
    /// to [`PointerKind::Trackpad`](super::PointerKind::Trackpad) whatever `pointer` carried.
    #[must_use]
    pub const fn new(
        pointer: PointerSource,
        time: EventTime,
        position: PointerPosition,
        fingers: NonZeroU8,
        phase: TouchpadSwipePhase,
    ) -> Self {
        let mut pointer = pointer;
        pointer.kind = super::PointerKind::Trackpad;
        Self {
            pointer,
            time,
            position,
            fingers,
            phase,
            modifiers: Modifiers::NONE,
        }
    }

    /// This event with `modifiers` held.
    #[must_use]
    pub const fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// The swipe's pointer; its kind is always
    /// [`PointerKind::Trackpad`](super::PointerKind::Trackpad).
    #[must_use]
    pub const fn pointer(&self) -> &PointerSource {
        &self.pointer
    }
}
