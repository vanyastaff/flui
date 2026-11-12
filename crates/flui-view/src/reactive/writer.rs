//! The write capability of the reactive graph (ADR-0086).
//!
//! A signal write takes a [`WriteTarget`]. Application code meets exactly one:
//! the `&mut EventContext<'_>` a framework event callback receives. The only way to
//! open one is [`WriterSource::write`]. Stateful widgets acquire a
//! [`WriterSource`] through [`LifecycleContext::writer_source`]; render views
//! use [`crate::RenderObjectContext::writer_source`] while registering their
//! owner-local handlers. Neither method is available on the build context
//! (ADR-0078). So a write in `build` has no writer to name:
//!
//! ```rust,ignore
//! RawButton::new(Text::new("+")).on_press(move |ctx| count.update(ctx, |n| *n += 1))
//! ```
//!
//! The types narrow the run-time guard; they do not replace it. A widget that
//! holds a `WriterSource` can still open a write synchronously inside its own
//! `build` (a callback it invokes there), and the guard refuses that write
//! with [`SignalError::WrittenDuringBuild`].
//!
//! The graph itself is not a write target. Host code that writes outside a
//! widget (a cross-thread `UiCommand::SignalWrite`, a test harness) opens an
//! [`EventContext`] through the owner's source,
//! [`BuildOwner::writer_source`](crate::BuildOwner::writer_source), like any
//! callback.
//!
//! [`LifecycleContext::writer_source`]: crate::LifecycleContext::writer_source

use std::any::Any;
use std::fmt;
use std::ops::{Deref, DerefMut};

use super::{Reactive, ReadGraph, SignalError, SignalSlot};

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
/// through the [`EventContext`] a [`WriterSource`] opens.
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

impl ReadGraph for Writer {
    fn graph_id(&self) -> u32 {
        self.graph.graph_id()
    }

    fn read_erased(
        &self,
        slot: SignalSlot,
        read: &mut dyn FnMut(&dyn Any),
    ) -> Result<(), SignalError> {
        self.graph.read_erased(slot, read)
    }
}

/// What an event callback receives: `&mut EventContext<'_>`, borrowed for one
/// dispatch and dropped when the callback returns.
///
/// It dereferences to the [`Writer`] and carries nothing else: no tree
/// position, no UI runtime id. A signal write takes it directly:
/// `count.set(ctx, 3)`, `count.update(ctx, |n| *n += 1)`. Passing `ctx` to a
/// write reborrows it, so one callback can make several writes.
/// Current values can be inspected with `signal.peek(ctx, |value| ...)`.
/// These reads neither subscribe an element nor schedule a rebuild.
pub struct EventContext<'a> {
    writer: &'a mut Writer,
}

/// Why an owner-bound operation cannot use the supplied event context.
///
/// Check before changing non-signal state: refusing a later signal write
/// cannot undo an earlier controller or form mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EventContextError {
    /// The handle is not attached to a live widget.
    #[error("the event target is detached")]
    Detached,
    /// The event and target belong to different presentations.
    #[error("the event context belongs to another presentation")]
    ForeignPresentation,
    /// The owning presentation is currently building an element.
    #[error("an event operation was attempted during the build of {element:?}")]
    WrittenDuringBuild {
        /// The element whose build is in progress.
        element: flui_foundation::ElementId,
    },
}

/// A framework refusal from a callback that combines signal and owner-bound
/// operations. Individual operations keep their precise error types; use
/// `Result<(), EventError>` as the callback return type to propagate both
/// with `?`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EventError {
    /// A signal could not be read or written.
    #[error(transparent)]
    Signal(#[from] SignalError),
    /// An owner-bound operation rejected the event context.
    #[error(transparent)]
    Context(#[from] EventContextError),
}

impl<'a> EventContext<'a> {
    pub(crate) fn new(writer: &'a mut Writer) -> Self {
        Self { writer }
    }
}

impl Deref for EventContext<'_> {
    type Target = Writer;

    fn deref(&self) -> &Writer {
        self.writer
    }
}

impl DerefMut for EventContext<'_> {
    fn deref_mut(&mut self) -> &mut Writer {
        self.writer
    }
}

impl fmt::Debug for EventContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventContext")
            .field("graph", &self.writer.graph.id())
            .finish()
    }
}

impl ReadGraph for EventContext<'_> {
    fn graph_id(&self) -> u32 {
        self.writer.graph_id()
    }

    fn read_erased(
        &self,
        slot: SignalSlot,
        read: &mut dyn FnMut(&dyn Any),
    ) -> Result<(), SignalError> {
        self.writer.read_erased(slot, read)
    }
}

/// An owned, `'static`, `!Send` capability that opens an [`EventContext`].
///
/// Acquire it in `create_state` (or `init_state`) with
/// [`LifecycleContext::writer_source`](crate::LifecycleContext::writer_source)
/// and keep it in the state. A widget wraps a recognizer callback with it
/// (`source.write(|ctx| user_callback(ctx))`), and so does code that receives
/// a callback shaped `Fn()` or `Fn(&str)` and has to write from it.
///
/// It is bound to the graph of one `BuildOwner`, so to one presentation: a
/// signal minted by another presentation's graph is refused with
/// [`SignalError::ForeignGraph`].
///
/// The writer an [`EventContext`] lends cannot outlive the closure that received
/// it:
///
/// ```compile_fail,E0521
/// use flui_view::{Writer, WriterSource};
///
/// fn keep(source: &WriterSource) -> &'static mut Writer {
///     let mut out = None;
///     source.write(|ctx| out = Some(&mut **ctx));
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

    /// Validate a caller's context before an owner-bound operation mutates
    /// state. This checks presentation identity and the signal build guard,
    /// but is not a transaction or proof that the target widget is mounted.
    /// The target must separately invalidate its stored source on disposal.
    pub fn check_context(&self, ctx: &EventContext<'_>) -> Result<(), EventContextError> {
        if self.graph.inner.borrow().closed {
            return Err(EventContextError::Detached);
        }
        if self.graph.id() != ctx.writer.graph.id() {
            return Err(EventContextError::ForeignPresentation);
        }
        if let Some(element) = self.graph.inner.borrow().building {
            return Err(EventContextError::WrittenDuringBuild { element });
        }
        Ok(())
    }

    /// Open one [`EventContext`] for the duration of `f`.
    ///
    /// The run-time guard still applies: a write made through it while an
    /// element is building is refused with
    /// [`SignalError::WrittenDuringBuild`].
    pub fn write<R>(&self, f: impl FnOnce(&mut EventContext<'_>) -> R) -> R {
        let mut writer = Writer::new(self.graph.clone());
        let mut ctx = EventContext::new(&mut writer);
        f(&mut ctx)
    }
}

impl fmt::Debug for WriterSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterSource")
            .field("graph", &self.graph.id())
            .finish()
    }
}

/// What a signal write accepts: in application code, the `&mut EventContext<'_>`
/// an event callback receives.
///
/// Sealed: `EventContext` and the `Writer` it lends are the only implementors.
/// The graph is not one, so naming it is never a way to write.
///
/// ```compile_fail,E0277
/// use flui_view::{Reactive, SignalWriteExt};
///
/// let graph = Reactive::new();
/// let count = graph.signal(0u32);
/// let _ = count.set(&graph, 1);
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot write a signal",
    label = "not a write context",
    note = "a signal write takes the `&mut EventContext` an event callback receives: `move |ctx| count.set(ctx, v)`",
    note = "outside a callback, acquire a `WriterSource` in `init_state` and call `source.write(|ctx| ..)`; `build` has none"
)]
pub trait WriteTarget: sealed::Sealed {
    /// The graph the write lands in. Callable only inside `flui-view`.
    #[doc(hidden)]
    fn graph(&self, token: sealed::Token) -> &Reactive;
}

impl sealed::Sealed for Writer {}
impl WriteTarget for Writer {
    fn graph(&self, _: sealed::Token) -> &Reactive {
        &self.graph
    }
}

impl sealed::Sealed for EventContext<'_> {}
impl WriteTarget for EventContext<'_> {
    fn graph(&self, _: sealed::Token) -> &Reactive {
        &self.writer.graph
    }
}

/// What an event callback may return: `()`, or a `Result` with
/// [`SignalError`], [`EventContextError`] or their union [`EventError`].
/// Thus `move |ctx| count.set(ctx, v)` needs no `let _`.
///
/// Sealed. A refused write is not lost silently: [`EventOutcome::report`]
/// logs it at the dispatch boundary, the same way the guard logs a write
/// refused during `build`. Automatic reporting is deliberately limited to
/// framework refusals. Application and domain errors must be handled by the
/// application, rather than implicitly discarded into a warning log.
pub trait EventOutcome: sealed::Sealed {
    /// Hand the outcome to the dispatch boundary: a refused write becomes a
    /// `tracing::warn!` on the `flui.signals` target.
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
                target: "flui.signals",
                %error,
                "an event callback's signal write was refused"
            );
        }
    }
}

impl<T> sealed::Sealed for Result<T, EventContextError> {}
impl<T> EventOutcome for Result<T, EventContextError> {
    fn report(self) {
        if let Err(error) = self {
            tracing::warn!(
                target: "flui.signals",
                %error,
                "an event callback's owner-bound operation was refused"
            );
        }
    }
}

impl<T> sealed::Sealed for Result<T, EventError> {}
impl<T> EventOutcome for Result<T, EventError> {
    fn report(self) {
        match self {
            Ok(_) => {}
            Err(EventError::Signal(error)) => Err::<(), _>(error).report(),
            Err(EventError::Context(error)) => Err::<(), _>(error).report(),
        }
    }
}

/// Fix the higher-ranked signature of an event closure bound with `let`.
///
/// A closure written inline where `Fn(&mut EventContext<'_>)` is expected gets the
/// right signature from that bound. A closure bound with `let` first does
/// not: nothing tells it what `ctx` is, the write infers `&_` for it, and
/// passing it on fails with E0631, "type mismatch in closure arguments".
/// Wrap it here (annotating `|ctx: &mut EventContext<'_>|` also works):
///
/// ```
/// use flui_view::{EventContext, Signal, SignalError, SignalWriteExt, WriterSource, callback};
///
/// fn wire(source: &WriterSource, a: Signal<u32>, b: Signal<u32>) -> Result<(), SignalError> {
///     let press = callback(move |ctx| {
///         a.set(ctx, 1)?;
///         b.update(ctx, |n| *n += 1)
///     });
///     source.write(|ctx| press(ctx))
/// }
/// ```
pub fn callback<F, R>(f: F) -> F
where
    F: Fn(&mut EventContext<'_>) -> R + 'static,
    R: EventOutcome,
{
    f
}

/// [`callback`] for an event callback that also receives a value: a
/// `let`-bound closure shaped `|ctx, details| ..`.
///
/// The value is passed by value (`DragUpdateDetails`, `bool`, a `DeviceId`).
/// For a borrowed argument (`&str`, `&KeyEvent`), use [`callback_ref`].
///
/// ```
/// use flui_view::{Signal, SignalError, SignalWriteExt, WriterSource, callback_with};
///
/// fn wire(source: &WriterSource, a: Signal<u32>, b: Signal<u32>) -> Result<(), SignalError> {
///     let moved = callback_with(move |ctx, delta: u32| {
///         a.update(ctx, |n| *n += delta)?;
///         b.set(ctx, delta)
///     });
///     source.write(|ctx| moved(ctx, 4))
/// }
/// ```
pub fn callback_with<A, F, R>(f: F) -> F
where
    F: Fn(&mut EventContext<'_>, A) -> R + 'static,
    R: EventOutcome,
{
    f
}

/// [`callback`] for an event callback that also receives a borrowed value: a
/// `let`-bound closure shaped `|ctx, text| ..` where `text: &str`.
///
/// [`callback_with`] cannot express it, because its argument type is one
/// fixed type and a borrowed argument has to accept every lifetime.
///
/// ```
/// use flui_view::{Signal, SignalError, SignalWriteExt, WriterSource, callback_ref};
///
/// fn wire(
///     source: &WriterSource,
///     text: Signal<String>,
///     edits: Signal<u32>,
/// ) -> Result<(), SignalError> {
///     let changed = callback_ref(move |ctx, value: &str| {
///         text.set(ctx, value.to_owned())?;
///         edits.update(ctx, |n| *n += 1)
///     });
///     let typed = String::from("hello");
///     source.write(|ctx| changed(ctx, &typed))
/// }
/// ```
pub fn callback_ref<T, F, R>(f: F) -> F
where
    T: ?Sized,
    F: for<'a> Fn(&mut EventContext<'_>, &'a T) -> R + 'static,
    R: EventOutcome,
{
    f
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_foundation::{ElementId, RebuildReason};

    use super::*;
    use crate::owner::{ExternalBuildInbox, ExternalBuildScheduler};
    use crate::reactive::SignalWriteExt;

    static_assertions::assert_impl_all!(WriterSource: Clone);
    static_assertions::assert_not_impl_any!(WriterSource: Send, Sync);
    static_assertions::assert_not_impl_any!(Writer: Send, Sync, Clone);
    static_assertions::assert_not_impl_any!(EventContext<'static>: Send, Sync, Clone);

    fn graph_with_inbox() -> (Reactive, Arc<ExternalBuildInbox>) {
        let inbox = Arc::new(ExternalBuildInbox::default());
        let reactive = Reactive::new();
        reactive.set_scheduler(ExternalBuildScheduler::from_parts(Arc::clone(&inbox), None));
        (reactive, inbox)
    }

    fn scheduled(inbox: &ExternalBuildInbox) -> Vec<ElementId> {
        let mut ids: Vec<_> = inbox.lock().ids().to_vec();
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

        assert_eq!(source.write(|ctx| a.set(ctx, 10)), Ok(()));

        assert_eq!(scheduled(&inbox), vec![e1]);
        assert!(
            inbox
                .lock()
                .get(e1)
                .expect("e1 is queued")
                .contains(RebuildReason::SignalChange)
        );
        assert_eq!(a.peek(&r, |v| *v), Ok(10));
    }
}
