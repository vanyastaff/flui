//! Typestate pattern implementations
//!
//! This module provides zero-cost typestate patterns for compile-time
//! state machine verification.
//!
//! # Typestate Pattern
//!
//! The typestate pattern encodes state in the type system, making invalid
//! state transitions a compile error instead of a runtime error.
//!
//! ```rust,ignore
//! // State types (zero-sized)
//! struct Open;
//! struct Closed;
//!
//! struct Door<State> {
//!     _state: PhantomData<State>,
//! }
//!
//! impl Door<Closed> {
//!     fn open(self) -> Door<Open> { ... }
//! }
//!
//! impl Door<Open> {
//!     fn close(self) -> Door<Closed> { ... }
//!     fn walk_through(&self) { ... } // Only available when open!
//! }
//! ```

use std::marker::PhantomData;

// ============================================================================
// Arena States
// ============================================================================

/// Arena is open - members can be added.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ArenaOpen;

/// Arena is held - resolution is delayed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ArenaHeld;

/// Arena is closed - no more members, waiting for resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ArenaClosed;

/// Arena is resolved - winner determined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ArenaResolved;

/// Marker trait for arena states.
pub trait ArenaState: private::Sealed + Default {}

impl ArenaState for ArenaOpen {}
impl ArenaState for ArenaHeld {}
impl ArenaState for ArenaClosed {}
impl ArenaState for ArenaResolved {}

// ============================================================================
// Gesture States
// ============================================================================

/// Gesture recognizer is ready to track a pointer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GestureReady;

/// Gesture might be recognized (tracking pointer).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GesturePossible;

/// Gesture has started (recognized).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GestureStarted;

/// Gesture was accepted (won arena).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GestureAccepted;

/// Gesture was rejected (lost arena or cancelled).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GestureRejected;

/// Marker trait for gesture states.
pub trait GestureStateMarker: private::Sealed + Default {}

impl GestureStateMarker for GestureReady {}
impl GestureStateMarker for GesturePossible {}
impl GestureStateMarker for GestureStarted {}
impl GestureStateMarker for GestureAccepted {}
impl GestureStateMarker for GestureRejected {}

// ============================================================================
// Drag States
// ============================================================================

/// Drag has not started yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DragIdle;

/// Pointer is down, might become a drag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DragPending;

/// Drag is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DragActive;

/// Marker trait for drag states.
pub trait DragStateMarker: private::Sealed + Default {}

impl DragStateMarker for DragIdle {}
impl DragStateMarker for DragPending {}
impl DragStateMarker for DragActive {}

// ============================================================================
// Focus States
// ============================================================================

/// Element is not focused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Unfocused;

/// Element has focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Focused;

/// Marker trait for focus states.
pub trait FocusStateMarker: private::Sealed + Default {}

impl FocusStateMarker for Unfocused {}
impl FocusStateMarker for Focused {}

// ============================================================================
// State wrapper for runtime state tracking
// ============================================================================

/// A state wrapper that can transition between states at runtime.
///
/// This is useful when state transitions are determined by runtime values
/// but you still want type-level documentation of valid states.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::typestate::{State, GestureReady, GestureStarted};
///
/// let state: State<GestureReady> = State::new();
/// let state: State<GestureStarted> = state.transition();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct State<S> {
    _marker: PhantomData<S>,
}

impl<S: Default> Default for State<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> State<S> {
    /// Creates a new state wrapper.
    #[inline]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    /// Transitions to a new state.
    ///
    /// This consumes the current state, ensuring you can't use it after transition.
    #[inline]
    pub fn transition<T>(self) -> State<T> {
        State {
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Private sealed trait
// ============================================================================

mod private {
    pub trait Sealed {}

    impl Sealed for super::ArenaOpen {}
    impl Sealed for super::ArenaHeld {}
    impl Sealed for super::ArenaClosed {}
    impl Sealed for super::ArenaResolved {}

    impl Sealed for super::GestureReady {}
    impl Sealed for super::GesturePossible {}
    impl Sealed for super::GestureStarted {}
    impl Sealed for super::GestureAccepted {}
    impl Sealed for super::GestureRejected {}

    impl Sealed for super::DragIdle {}
    impl Sealed for super::DragPending {}
    impl Sealed for super::DragActive {}

    impl Sealed for super::Unfocused {}
    impl Sealed for super::Focused {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_transition() {
        let state: State<GestureReady> = State::new();
        let state: State<GesturePossible> = state.transition();
        let state: State<GestureStarted> = state.transition();
        let _state: State<GestureAccepted> = state.transition();
    }

    #[test]
    fn test_zero_sized() {
        // All state types should be zero-sized
        assert_eq!(std::mem::size_of::<ArenaOpen>(), 0);
        assert_eq!(std::mem::size_of::<GestureReady>(), 0);
        assert_eq!(std::mem::size_of::<DragIdle>(), 0);
        assert_eq!(std::mem::size_of::<Focused>(), 0);

        // State wrapper should also be zero-sized
        assert_eq!(std::mem::size_of::<State<GestureReady>>(), 0);
    }
}
