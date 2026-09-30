//! `OneSequenceGestureRecognizer` — canonical base trait for recognizers that
//! track a single gesture sequence at a time.
//!
//! Recognizers that implement this trait track per-pointer arena entries and
//! resolve them on `accept` / `reject` / `dispose`. Concrete implementers (after
//! the migration wave): Drag, Scale, ForcePress.
//!
//! Note: this trait was previously a 341-LOC scaffold with zero `impl ... for`
//! blocks (deleted in commit f10d21b5). It is re-introduced as a properly
//! shaped, sealed canonical trait ready for the migration wave.

use crate::{
    arena::GestureDisposition, ids::PointerId, recognizers::recognizer::GestureRecognizer,
    sealed::gesture_recognizer::Sealed as GestureRecognizerSealed,
};

/// Base trait for recognizers tracking a single gesture sequence at a time.
///
/// Sealed via [`crate::sealed::gesture_recognizer::Sealed`] — external crates
/// cannot implement directly (FLUI's recognizer set is curated; external
/// gesture recognizers go through `CustomGestureRecognizer`). Per Constitution
/// Principle 4 + *Rust for Rustaceans* "Sealed Traits".
pub trait OneSequenceGestureRecognizer: GestureRecognizer + GestureRecognizerSealed {
    /// Returns the slice of tracked pointer IDs.
    fn tracked_pointers(&self) -> Vec<PointerId>;

    // `handle_event` inherited from the supertrait `GestureRecognizer`; one
    // concrete impl satisfies both contracts.

    /// Resolve this recognizer's arena entries with the given disposition.
    ///
    /// Default impl: walks `tracked_pointers()`, resolves each via
    /// `resolve_pointer`.
    fn resolve(&self, disposition: GestureDisposition) {
        for pointer in self.tracked_pointers() {
            self.resolve_pointer(pointer, disposition);
        }
    }

    /// Resolve this recognizer's arena entry for a single pointer.
    fn resolve_pointer(&self, pointer: PointerId, disposition: GestureDisposition);

    /// Called when the number of tracked pointers transitions from 1 to 0.
    ///
    /// Default no-op. Concrete recognizers may override to finalize sequence
    /// state (e.g. emit on_drag_end).
    fn did_stop_tracking_last_pointer(&self, _pointer: PointerId) {}

    /// Stop tracking the given pointer (removes route + clears arena entry).
    ///
    /// Called implicitly by `resolve_pointer` and `dispose`.
    fn stop_tracking_pointer(&self, pointer: PointerId);
}
