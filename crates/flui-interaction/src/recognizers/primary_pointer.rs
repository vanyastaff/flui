//! `PrimaryPointerGestureRecognizer` — canonical trait for recognizers that
//! track only the first pointer that hits.
//!
//! Flutter parity: [`recognizer.dart:611+`](../../../.flutter/flutter-master/packages/flutter/lib/src/gestures/recognizer.dart)
//! `abstract class PrimaryPointerGestureRecognizer extends OneSequenceGestureRecognizer`.
//!
//! Concrete implementers (after the U16-U17 migration wave): Tap, LongPress.
//!
//! Note: this trait was previously a 481-LOC scaffold with zero `impl ... for`
//! blocks (deleted in U3 commit f10d21b5). U13 re-introduces it as a properly
//! shaped, sealed canonical trait extending OneSequenceGestureRecognizer.

use std::time::Duration;

use flui_types::{Offset, geometry::Pixels};

use crate::{
    recognizers::one_sequence::OneSequenceGestureRecognizer,
    sealed::gesture_recognizer::Sealed as GestureRecognizerSealed,
};

/// Trait for recognizers that track a single primary pointer.
///
/// Extends [`OneSequenceGestureRecognizer`] with primary-pointer-specific
/// semantics: the recognizer only competes for the first pointer that lands
/// inside its hit region, and tracks an optional pre-acceptance deadline.
pub trait PrimaryPointerGestureRecognizer:
    OneSequenceGestureRecognizer + GestureRecognizerSealed
{
    /// The position of the primary pointer at down event.
    fn initial_position(&self) -> Option<Offset<Pixels>>;

    /// Deadline before which the recognizer must decide.
    ///
    /// Flutter parity: `recognizer.dart:644 deadline`. `None` means no
    /// pre-acceptance timeout (e.g. Tap fires immediately on Up).
    fn deadline(&self) -> Option<Duration> {
        None
    }

    /// Called when the pre-acceptance deadline elapses without resolution.
    ///
    /// Default: resolve with [`GestureDisposition::Rejected`]. Concrete
    /// recognizers may override (e.g. LongPress accepts on deadline). Flutter
    /// parity: `recognizer.dart:646+ didExceedDeadline(position)`.
    fn did_exceed_deadline(&self) {
        use crate::arena::GestureDisposition;
        self.resolve(GestureDisposition::Rejected);
    }

    /// Called for events on the primary pointer.
    ///
    /// Flutter parity: `recognizer.dart:684+ @protected void handlePrimaryPointer(PointerEvent event)`.
    fn handle_primary_pointer(&self, event: &crate::events::PointerEvent);
}
