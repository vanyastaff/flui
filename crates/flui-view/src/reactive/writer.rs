//! The write capability of the reactive graph (ADR-0086).
//!
//! A signal write takes a [`WriteTarget`]. Application code meets exactly one:
//! the `&mut EventCx<'_>` a framework event callback receives. The only way to
//! open one is [`WriterSource::write`], and the only way to get a
//! [`WriterSource`] is [`LifecycleContext::writer_source`], which `build`
//! cannot reach (ADR-0078). So a write in `build` has no writer to name:
//!
//! ```rust,ignore
//! RawButton::new(Text::new("+")).on_press(move |cx| count.update(cx, |n| *n += 1))
//! ```
//!
//! The types narrow the run-time guard; they do not replace it. A widget that
//! holds a `WriterSource` can still open a write synchronously inside its own
//! `build` (a callback it invokes there), and the guard refuses that write
//! with [`SignalError::WrittenDuringBuild`].
//!
//! [`Reactive`] is still a write target, so the code that writes through the
//! graph directly (tests, benches, `flui-app`'s `UiCommand::SignalWrite`)
//! compiles unchanged. ADR-0086 §8 step 3 removes it together with both
//! `reactive()` accessors.
//!
//! [`LifecycleContext::writer_source`]: crate::LifecycleContext::writer_source

use std::fmt;
use std::ops::{Deref, DerefMut};

use super::{Reactive, SignalError};

pub(super) mod sealed {
    /// Only this module's write targets and event outcomes implement the
    /// traits sealed by it.
    pub trait Sealed {}

    /// Proof that a call comes from inside `flui-view`: the graph accessor of
    /// [`WriteTarget`](super::WriteTarget) takes one, so a write target is a
    /// bound, never a way to reach the graph.
    #[derive(Clone, Copy, Debug)]
    pub struct Token(());

    impl Token {
        pub(crate) const fn new() -> Self {
            Self(())
        }
    }
}

/// The write capability for one dispatch. It has no public constructor and
/// is neither `Clone` nor `Send`: application code only ever borrows one,
/// through the [`EventCx`] a [`WriterSource`] opens.
pub struct Writer {
    graph: Reactive,
}

impl Writer {
    pub(crate) fn new(graph: Reactive) -> Self {
        Self { graph }
    }
}

impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("graph", &self.graph.id())
            .finish()
    }
}

/// What an event callback receives: `&mut EventCx<'_>`, borrowed for one
/// dispatch and dropped when the callback returns.
///
/// It dereferences to the [`Writer`] and carries nothing else: no tree
/// position, no realm id. A signal write takes it directly:
/// `count.set(cx, 3)`, `count.update(cx, |n| *n += 1)`. Passing `cx` to a
/// write reborrows it, so one callback can make several writes.
pub struct EventCx<'a> {
    writer: &'a mut Writer,
}

impl<'a> EventCx<'a> {
    pub(crate) fn new(writer: &'a mut Writer) -> Self {
        Self { writer }
    }
}

impl Deref for EventCx<'_> {
    type Target = Writer;

    fn deref(&self) -> &Writer {
        self.writer
    }
}

impl DerefMut for EventCx<'_> {
    fn deref_mut(&mut self) -> &mut Writer {
        self.writer
    }
}

impl fmt::Debug for EventCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventCx")
            .field("graph", &self.writer.graph.id())
            .finish()
    }
}

/// An owned, `'static`, `!Send` capability that opens an [`EventCx`].
///
/// Acquire it in `init_state` with
/// [`LifecycleContext::writer_source`](crate::LifecycleContext::writer_source)
/// and keep it in the state. A widget wraps a recognizer callback with it
/// (`source.write(|cx| user_callback(cx))`), and so does code that receives
/// a callback shaped `Fn()` or `Fn(&str)` and has to write from it.
///
/// It is bound to the graph of one `BuildOwner`, so to one presentation: a
/// signal minted by another presentation's graph is refused with
/// [`SignalError::ForeignGraph`].
///
/// The writer an [`EventCx`] lends cannot outlive the closure that received
/// it:
///
/// ```compile_fail,E0521
/// use flui_view::{Writer, WriterSource};
///
/// fn keep(source: &WriterSource) -> &'static mut Writer {
///     let mut out = None;
///     source.write(|cx| out = Some(&mut **cx));
///     out.expect("written")
/// }
/// ```
#[derive(Clone)]
pub struct WriterSource {
    graph: Reactive,
}

impl WriterSource {
    pub(crate) fn new(graph: Reactive) -> Self {
        Self { graph }
    }

    /// Open one [`EventCx`] for the duration of `f`.
    ///
    /// The run-time guard still applies: a write made through it while an
    /// element is building is refused with
    /// [`SignalError::WrittenDuringBuild`].
    pub fn write<R>(&self, f: impl FnOnce(&mut EventCx<'_>) -> R) -> R {
        let mut writer = Writer::new(self.graph.clone());
        let mut cx = EventCx::new(&mut writer);
        f(&mut cx)
    }
}

impl fmt::Debug for WriterSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterSource")
            .field("graph", &self.graph.id())
            .finish()
    }
}

/// What a signal write accepts: in application code, the `&mut EventCx<'_>`
/// an event callback receives.
///
/// Sealed. [`Reactive`] is an implementor only until ADR-0086 §8 step 3
/// removes it together with both `reactive()` accessors.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot write a signal",
    label = "not a write context",
    note = "a signal write takes the `&mut EventCx` an event callback receives: `move |cx| count.set(cx, v)`",
    note = "outside a callback, acquire a `WriterSource` in `init_state` and call `source.write(|cx| ..)`; `build` has none"
)]
pub trait WriteTarget: sealed::Sealed {
    /// The graph the write lands in. Callable only inside `flui-view`.
    #[doc(hidden)]
    fn graph(&self, token: sealed::Token) -> &Reactive;
}

impl sealed::Sealed for Reactive {}
impl WriteTarget for Reactive {
    fn graph(&self, _: sealed::Token) -> &Reactive {
        self
    }
}

impl sealed::Sealed for Writer {}
impl WriteTarget for Writer {
    fn graph(&self, _: sealed::Token) -> &Reactive {
        &self.graph
    }
}

impl sealed::Sealed for EventCx<'_> {}
impl WriteTarget for EventCx<'_> {
    fn graph(&self, _: sealed::Token) -> &Reactive {
        &self.writer.graph
    }
}

/// What an event callback may return: `()`, or the `Result` of a signal
/// write, so `move |cx| count.set(cx, v)` needs no `let _`.
///
/// Sealed. A refused write is not lost silently: [`EventOutcome::report`]
/// logs it at the dispatch boundary, the same way the guard logs a write
/// refused during `build`.
pub trait EventOutcome: sealed::Sealed {
    /// Hand the outcome to the dispatch boundary: a refused write becomes a
    /// `tracing::warn!` on the `flui::signals` target.
    fn report(self);
}

impl sealed::Sealed for () {}
impl EventOutcome for () {
    fn report(self) {}
}

impl<T> sealed::Sealed for Result<T, SignalError> {}
impl<T> EventOutcome for Result<T, SignalError> {
    fn report(self) {
        if let Err(error) = self {
            tracing::warn!(
                target: "flui::signals",
                %error,
                "an event callback's signal write was refused"
            );
        }
    }
}

/// Fix the higher-ranked signature of an event closure bound with `let`.
///
/// A closure written inline where `Fn(&mut EventCx<'_>)` is expected gets the
/// right signature from that bound. A closure bound with `let` first does
/// not: nothing tells it what `cx` is, the write infers `&_` for it, and
/// passing it on fails with E0631, "type mismatch in closure arguments".
/// Wrap it here (annotating `|cx: &mut EventCx<'_>|` also works):
///
/// ```
/// use flui_view::{EventCx, Signal, SignalError, SignalWriteExt, WriterSource, callback};
///
/// fn wire(source: &WriterSource, a: Signal<u32>, b: Signal<u32>) -> Result<(), SignalError> {
///     let press = callback(move |cx| {
///         a.set(cx, 1)?;
///         b.update(cx, |n| *n += 1)
///     });
///     source.write(|cx| press(cx))
/// }
/// ```
pub fn callback<F, R>(f: F) -> F
where
    F: Fn(&mut EventCx<'_>) -> R + 'static,
    R: EventOutcome,
{
    f
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use flui_foundation::{ElementId, RebuildReason, RebuildReasons};
    use parking_lot::Mutex;

    use super::*;
    use crate::owner::ExternalBuildScheduler;
    use crate::reactive::{Signal, SignalWriteExt};

    static_assertions::assert_impl_all!(WriterSource: Clone);
    static_assertions::assert_not_impl_any!(WriterSource: Send, Sync);
    static_assertions::assert_not_impl_any!(Writer: Send, Sync, Clone);
    static_assertions::assert_not_impl_any!(EventCx<'static>: Send, Sync, Clone);

    fn is_static<T: 'static>() {}

    #[test]
    fn a_writer_source_is_static() {
        is_static::<WriterSource>();
    }

    fn graph_with_inbox() -> (Reactive, Arc<Mutex<HashMap<ElementId, RebuildReasons>>>) {
        let inbox = Arc::new(Mutex::new(HashMap::new()));
        let reactive = Reactive::new();
        reactive.set_scheduler(ExternalBuildScheduler::from_parts(Arc::clone(&inbox), None));
        (reactive, inbox)
    }

    fn scheduled(inbox: &Mutex<HashMap<ElementId, RebuildReasons>>) -> Vec<ElementId> {
        let mut ids: Vec<_> = inbox.lock().keys().copied().collect();
        ids.sort();
        ids
    }

    #[test]
    fn a_write_through_an_event_cx_marks_exactly_the_readers() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(1u32);
        let b = r.signal(2u32);
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);
        r.register_element_reader(a.slot(), e1);
        r.register_element_reader(b.slot(), e2);
        let source = WriterSource::new(r.clone());

        assert_eq!(source.write(|cx| a.set(cx, 10)), Ok(()));

        assert_eq!(scheduled(&inbox), vec![e1]);
        assert!(inbox.lock()[&e1].contains(RebuildReason::SignalChange));
        assert_eq!(a.peek(&r, |v| *v), Ok(10));
    }

    #[test]
    fn one_event_cx_serves_several_writes() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(1u32);
        let b = r.signal(2u32);
        let source = WriterSource::new(r.clone());

        let result = source.write(|cx| {
            a.set(cx, 5)?;
            b.update(cx, |n| *n += 1)?;
            a.set_if_changed(cx, 5)
        });

        assert_eq!(result, Ok(false));
        assert_eq!((a.peek(&r, |v| *v), b.peek(&r, |v| *v)), (Ok(5), Ok(3)));
    }

    #[test]
    fn a_write_opened_while_an_element_builds_is_refused() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(1u32);
        let e1 = ElementId::new(1);
        r.register_element_reader(a.slot(), e1);
        let source = WriterSource::new(r.clone());

        r.begin_element_build(e1);
        let result = source.write(|cx| a.set(cx, 10));
        r.end_element_build(e1, true);

        assert_eq!(result, Err(SignalError::WrittenDuringBuild { element: e1 }));
        assert_eq!(a.peek(&r, |v| *v), Ok(1), "the value is unchanged");
        assert!(scheduled(&inbox).is_empty(), "nothing is scheduled");
    }

    #[test]
    fn a_writer_refuses_a_signal_of_another_graph() {
        let (mine, _) = graph_with_inbox();
        let (theirs, _) = graph_with_inbox();
        let foreign = theirs.signal(1u32);
        let source = WriterSource::new(mine);

        assert!(matches!(
            source.write(|cx| foreign.set(cx, 2)),
            Err(SignalError::ForeignGraph { .. })
        ));
        assert_eq!(foreign.peek(&theirs, |v| *v), Ok(1));
    }

    #[test]
    fn a_default_signal_write_is_unbound() {
        let (r, _) = graph_with_inbox();
        let source = WriterSource::new(r.clone());
        let unbound = Signal::<u32>::default();

        assert_eq!(
            source.write(|cx| unbound.set(cx, 1)),
            Err(SignalError::Unbound)
        );
        assert_eq!(unbound.set(&r, 1), Err(SignalError::Unbound));
        assert_eq!(unbound.peek(&r, |v| *v), Err(SignalError::Unbound));
    }

    /// Pins the transitional `&Reactive` target: removing it (ADR-0086 §8
    /// step 3) is a deliberate, visible change to this test.
    #[test]
    fn the_reactive_target_and_the_event_cx_write_the_same_slot() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(0u32);
        let source = WriterSource::new(r.clone());

        a.set(&r, 1).expect("a write through the graph");
        source
            .write(|cx| a.update(cx, |n| *n += 10))
            .expect("a write through an event context");

        assert_eq!(a.peek(&r, |v| *v), Ok(11));
    }

    #[test]
    fn a_refused_outcome_is_reported_and_an_accepted_one_is_silent() {
        // `report` consumes the outcome without panicking either way; the
        // warning itself goes to the `flui::signals` target.
        Ok::<(), SignalError>(()).report();
        Err::<(), _>(SignalError::Unbound).report();
        ().report();
    }

    #[test]
    fn debug_shows_only_the_graph_id() {
        let r = Reactive::new();
        let source = WriterSource::new(r.clone());
        assert_eq!(
            format!("{source:?}"),
            format!("WriterSource {{ graph: {} }}", r.id())
        );
        source.write(|cx| {
            assert_eq!(
                format!("{cx:?}"),
                format!("EventCx {{ graph: {} }}", r.id())
            );
        });
    }
}
