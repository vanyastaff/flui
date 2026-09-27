//! The stored shapes of this crate's event callbacks (ADR-0086).
//!
//! Every public `on_*` setter takes `Fn(&mut EventCx<'_>[, value]) -> R`
//! with `R: EventOutcome`, and stores the callback already adapted to report
//! its outcome: a refused signal write is logged at the dispatch boundary
//! instead of being dropped.

use std::rc::Rc;

use flui_sdk::view::{EventCx, EventOutcome};

/// A stored no-argument event callback: a press, a tap, a delete.
pub(crate) type PressCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

/// A stored event callback that also receives a value: a toggle's next
/// value, a selected index.
pub(crate) type ValueCallback<T> = Rc<dyn Fn(&mut EventCx<'_>, T)>;

/// Store a no-argument event callback, adapted to report its outcome.
pub(crate) fn press_callback<F, R>(callback: F) -> PressCallback
where
    F: Fn(&mut EventCx<'_>) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>| callback(cx).report())
}

/// Store an event callback that also receives a value, adapted to report
/// its outcome.
pub(crate) fn value_callback<T, F, R>(callback: F) -> ValueCallback<T>
where
    T: 'static,
    F: Fn(&mut EventCx<'_>, T) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>, value: T| callback(cx, value).report())
}
