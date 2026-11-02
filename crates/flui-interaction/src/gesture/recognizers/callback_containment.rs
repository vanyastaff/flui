//! Callback delivery and capture retirement without guarded user code.

use crate::{
    containment::RoutePanic,
    gesture::arena::{GestureArena, GestureArenaEntry, SweepModel},
};
use std::rc::Rc;

/// Destroy a withdrawn callback under containment: a panic of its destructor
/// is the first failure, or is discarded behind one already caught
/// (ADR-0281 §3).
pub(crate) fn retire_callback<T: ?Sized>(callback: Option<Rc<T>>, first: &mut Option<RoutePanic>) {
    let candidate = RoutePanic::capture(|| drop(callback));
    RoutePanic::preserve_first(first, candidate, "recognizer callback retirement");
}

/// Callbacks belonging to one committed transition.
pub(crate) struct CallbackSequence {
    first: Option<RoutePanic>,
    incoming_failure: bool,
}

impl CallbackSequence {
    pub(crate) fn new() -> Self {
        Self {
            first: None,
            incoming_failure: std::thread::panicking(),
        }
    }
    /// Run `invoke` on `callback` unless the thread is already unwinding, then
    /// destroy this delivery's snapshot under containment.
    pub(crate) fn call<T: ?Sized>(&mut self, callback: Option<Rc<T>>, invoke: impl FnOnce(&T)) {
        if !self.incoming_failure
            && let Some(callback) = callback.as_ref()
        {
            let candidate = RoutePanic::capture(|| invoke(callback.as_ref()));
            RoutePanic::preserve_first(&mut self.first, candidate, "recognizer callback");
        }
        retire_callback(callback, &mut self.first);
    }
    pub(crate) fn retire<T: ?Sized>(&mut self, callback: Option<Rc<T>>) {
        retire_callback(callback, &mut self.first);
    }
    pub(crate) fn finish(self) {
        finish_containment(self.first, self.incoming_failure);
    }
}

/// Raise the first failure, or discard it when the thread is already
/// unwinding: raising a second panic there would abort.
pub(crate) fn finish_containment(first: Option<RoutePanic>, incoming_failure: bool) {
    if let Some(panic) = first {
        if incoming_failure {
            panic.discard();
        } else {
            panic.resume();
        }
    }
}

pub(crate) fn withdraw_cancelled(entry: &GestureArenaEntry, arena: &GestureArena) {
    arena.run_owned_recovery(|recovery| {
        withdraw_cancelled_with_recovery(entry, arena, recovery);
    });
}

pub(crate) fn withdraw_cancelled_with_recovery(
    entry: &GestureArenaEntry,
    arena: &GestureArena,
    recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
) {
    if arena.sweep_model() == SweepModel::SelfDriven {
        entry.abandon_without_self_with_recovery(recovery);
    } else {
        entry.reject_without_self_with_recovery(recovery);
    }
}

pub(crate) fn invoke_callback<T: ?Sized>(
    callback: Option<Rc<T>>,
    before: impl FnOnce(),
    invoke: impl FnOnce(&T),
) {
    let incoming_failure = std::thread::panicking();
    let mut first = RoutePanic::capture(before);
    if first.is_none()
        && !incoming_failure
        && let Some(callback) = callback.as_ref()
    {
        first = RoutePanic::capture(|| invoke(callback.as_ref()));
    }
    retire_callback(callback, &mut first);
    finish_containment(first, incoming_failure);
}

/// Retire immutable callback fields individually, destroying the tail after a
/// failure under containment.
macro_rules! retire_callbacks {
    ($owner:ident; $($field:ident),+ $(,)?) => {{
        let mut retirement = $crate::gesture::recognizers::callback_containment::CallbackSequence::new();
        $(retirement.retire($owner.$field.take());)+
        retirement.finish();
    }};
}
pub(crate) use retire_callbacks;
