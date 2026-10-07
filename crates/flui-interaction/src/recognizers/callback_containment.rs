//! Callback delivery and capture retirement without guarded user code.

use crate::{
    arena::{GestureArena, GestureArenaEntry, SweepModel},
    retain::Retain,
    routing::RoutePanic,
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
    pub(crate) fn call<T: ?Sized>(&mut self, callback: Option<Rc<T>>, invoke: impl FnOnce(&T)) {
        if self.incoming_failure {
            callback.retain();
            return;
        }
        if let Some(callback) = callback.as_ref() {
            let candidate = RoutePanic::capture(|| invoke(callback.as_ref()));
            RoutePanic::preserve_first(&mut self.first, candidate, "recognizer callback");
        }
        retire_callback(callback, &mut self.first);
    }
    pub(crate) fn retire<T: ?Sized>(&mut self, callback: Option<Rc<T>>) {
        if self.incoming_failure {
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
    if arena.sweep_model() == SweepModel::SelfDriven {
        entry.abandon_without_self();
    } else {
        entry.reject_without_self();
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

/// Retire immutable callback fields individually, retaining the tail after failure.
macro_rules! retire_callbacks {
    ($owner:ident; $($field:ident),+ $(,)?) => {{
        let mut retirement = $crate::recognizers::callback_containment::CallbackSequence::new();
        $(retirement.retire($owner.$field.take());)+
        retirement.finish();
    }};
}
pub(crate) use retire_callbacks;
