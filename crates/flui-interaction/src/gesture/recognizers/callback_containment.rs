//! Callback delivery and capture retirement without guarded user code.

use crate::{
    containment::RoutePanic,
    gesture::arena::{GestureArena, GestureArenaEntry, SweepModel},
    retain::{Owned, Retain},
};
use std::rc::Rc;

pub(crate) fn retire_callback<T: ?Sized>(callback: Option<Rc<T>>, first: &mut Option<RoutePanic>) {
    if first.is_some() || std::thread::panicking() {
        callback.retain();
    } else {
        let candidate = RoutePanic::capture(|| drop(callback));
        RoutePanic::preserve_first(first, candidate, "recognizer callback retirement");
    }
}

fn retire_delivery_snapshot<T: ?Sized>(
    callback: Option<Rc<T>>,
    first: &mut Option<RoutePanic>,
    incoming_failure: bool,
) {
    if first.is_some() || incoming_failure {
        // The immutable field still owns this callback. Keep the failed
        // delivery's ownership too, so a later healthy owner Drop cannot
        // destroy its opaque captures outside the failure that retired it.
        Owned(callback).retain();
    } else {
        retire_callback(callback, first);
    }
}

/// Callbacks belonging to one committed transition.
pub(crate) struct CallbackSequence {
    first: Option<RoutePanic>,
    incoming_failure: bool,
    preserving: bool,
}

impl CallbackSequence {
    pub(crate) fn new() -> Self {
        Self {
            first: None,
            incoming_failure: std::thread::panicking(),
            preserving: false,
        }
    }
    pub(crate) fn preserving_failure(preserving: bool) -> Self {
        Self {
            preserving,
            ..Self::new()
        }
    }
    pub(crate) fn call<T: ?Sized>(&mut self, callback: Option<Rc<T>>, invoke: impl FnOnce(&T)) {
        if self.incoming_failure {
            Owned(callback).retain();
            return;
        }
        if let Some(callback) = callback.as_ref() {
            let candidate = RoutePanic::capture(|| invoke(callback.as_ref()));
            RoutePanic::preserve_first(&mut self.first, candidate, "recognizer callback");
        }
        if self.preserving && self.first.is_none() {
            // An earlier producer failure changes retirement policy, but a
            // healthy callback's immutable owner still has sole capture custody.
            callback.retain();
        } else {
            retire_delivery_snapshot(callback, &mut self.first, self.incoming_failure);
        }
    }
    pub(crate) fn retire<T: ?Sized>(&mut self, callback: Option<Rc<T>>) {
        if self.incoming_failure || self.preserving {
            callback.retain();
        } else {
            retire_callback(callback, &mut self.first);
        }
    }
    pub(crate) fn finish(self) {
        finish_containment(self.first, self.incoming_failure);
    }
}

pub(crate) fn finish_containment(first: Option<RoutePanic>, incoming_failure: bool) {
    if let Some(panic) = first {
        if incoming_failure {
            panic.retain();
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
    retire_delivery_snapshot(callback, &mut first, incoming_failure);
    finish_containment(first, incoming_failure);
}

/// Retire immutable callback fields individually, retaining the tail after failure.
macro_rules! retire_callbacks {
    ($owner:ident; $($field:ident),+ $(,)?) => {{
        let mut retirement = $crate::gesture::recognizers::callback_containment::CallbackSequence::new();
        $(retirement.retire($owner.$field.take());)+
        retirement.finish();
    }};
}
pub(crate) use retire_callbacks;
