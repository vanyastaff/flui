//! The read side of the reactive graph (ADR-0085 §2).
//!
//! The graph itself (the slot arena, the reader registry, writes and the
//! build-time guard) lives in `flui-view`. This module holds only what a
//! *reader* has to name, so a crate at or above foundation can take a
//! [`Signal<T>`] and read it through any context that implements
//! [`ReadScope`], without depending on the crate that owns the graph:
//!
//! - [`SignalSlot`], [`Signal<T>`] and [`SignalSender<T>`]: `Copy` handles;
//! - [`SignalError`]: why an operation on a handle was refused;
//! - [`ReadGraph`]: the read-only, object-safe face of a graph;
//! - [`ReaderSink`]: where a read records its subscriber, bound to one reader
//!   by whoever minted it;
//! - [`ReadScope`] and [`ScopeRef`]: what a context hands a read.
//!
//! # What this module may not name
//!
//! Nothing here writes a slot, creates one, releases one or drives a build.
//! [`ReadGraph`] has no such method, and [`ScopeRef`] exposes neither its graph
//! nor its sink, so a scope can be passed to a read and nothing else. A
//! subscription is recorded only through a [`ReaderSink`], and the sinks that
//! subscribe a real reader are private to the crate that owns the graph: a
//! scope built outside it can read, but cannot subscribe anyone.
//!
//! The only build-phase vocabulary is two [`SignalError`] variants carrying
//! the [`ElementId`] foundation already defines. Reader identities (elements,
//! render objects) stay in the graph's crate.
//!
//! # Handles of the wrong type
//!
//! [`Signal::from_slot`] and [`SignalSlot::new`] are public only because the
//! graph's crate needs them across the crate boundary. A handle forged with
//! the wrong `T` is refused with [`SignalError::TypeMismatch`], never read as
//! another type and never a `BUG:` panic.

use std::any::{Any, type_name};
use std::fmt;
use std::marker::PhantomData;

use crate::ElementId;

// ============================================================================
// Slot and errors
// ============================================================================

/// Graph id, index and generation of a slot in a reactive graph. `Copy` and
/// `'static`; every operation re-checks all three, so a stale or foreign
/// handle is an error, never a read of another value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignalSlot {
    graph: u32,
    index: u32,
    generation: u32,
}

impl SignalSlot {
    /// The graph id of an unbound handle ([`Signal::default`]). A graph never
    /// takes it as its id, so a slot naming it is refused as
    /// [`SignalError::Unbound`] by every read and write.
    pub const UNBOUND_GRAPH: u32 = 0;

    /// A slot handle. Minted by the graph that owns the arena; a handle built
    /// any other way is checked like every handle and refused unless it names
    /// a live slot of the graph it is used against.
    #[doc(hidden)]
    #[must_use]
    pub const fn new(graph: u32, index: u32, generation: u32) -> Self {
        Self {
            graph,
            index,
            generation,
        }
    }

    /// The id of the graph that minted this slot. Graph ids come from a
    /// process-wide monotonic counter, so a realm uses this to route a
    /// cross-thread write to the one graph that can accept it (ADR-0085 §1),
    /// and to refuse a slot none of its graphs minted.
    #[must_use]
    pub const fn graph(self) -> u32 {
        self.graph
    }

    /// Whether this slot names no graph ([`SignalSlot::UNBOUND_GRAPH`]).
    #[must_use]
    pub const fn is_unbound(self) -> bool {
        self.graph == Self::UNBOUND_GRAPH
    }

    /// The arena index of the slot.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The generation of the arena entry this handle was minted for.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Why a signal operation could not be carried out.
///
/// `Display` and `Error` are written by hand: foundation takes no `thiserror`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignalError {
    /// The slot was released (its owning element unmounted) and possibly
    /// reused; the handle's generation no longer matches.
    Released {
        /// Arena index of the stale handle.
        index: u32,
        /// Generation the stale handle carries.
        generation: u32,
    },
    /// The handle was minted by another graph.
    ForeignGraph {
        /// Arena index of the handle.
        index: u32,
        /// The graph that minted it.
        graph: u32,
        /// The graph the operation ran against.
        this: u32,
    },
    /// The slot holds a value of another type than the handle names: the
    /// handle was built with [`Signal::from_slot`] over a slot minted for a
    /// different `T`.
    TypeMismatch {
        /// Arena index of the slot.
        index: u32,
        /// The type the handle names.
        expected: &'static str,
    },
    /// A write was attempted while an element was building (ADR-0074 §5.2):
    /// it would re-mark readers of the frame that is still building.
    WrittenDuringBuild {
        /// The element whose build was running.
        element: ElementId,
    },
    /// A slot was created while an element was building: one slot per
    /// rebuild is a leak. Create in `init_state` and hold the handle.
    CreatedDuringBuild {
        /// The element whose build was running.
        element: ElementId,
    },
    /// The slot is being read or written by an enclosing closure on this
    /// same slot (`a.with(.., |_| a.set(..))`); its value is out on loan.
    Reentrant {
        /// Arena index of the slot.
        index: u32,
    },
    /// The handle was never bound to a graph: it is a [`Signal::default`]
    /// placeholder that `init_state` did not replace with a created signal.
    Unbound,
}

impl fmt::Display for SignalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Released { index, generation } => {
                write!(
                    f,
                    "signal slot {index} generation {generation} was released"
                )
            }
            Self::ForeignGraph { index, graph, this } => write!(
                f,
                "signal slot {index} belongs to graph {graph}, not to this one ({this})"
            ),
            Self::TypeMismatch { index, expected } => write!(
                f,
                "signal slot {index} holds a value of another type than {expected}"
            ),
            Self::WrittenDuringBuild { element } => {
                write!(f, "signal written during the build of {element:?}")
            }
            Self::CreatedDuringBuild { element } => {
                write!(f, "signal created during the build of {element:?}")
            }
            Self::Reentrant { index } => write!(
                f,
                "signal slot {index} accessed re-entrantly from its own read/write closure"
            ),
            Self::Unbound => {
                f.write_str("signal handle was never bound to a graph (create it in init_state)")
            }
        }
    }
}

impl std::error::Error for SignalError {}

// ============================================================================
// Graph face, sink, scope
// ============================================================================

/// The read-only, object-safe face of a reactive graph: pure reads, with no
/// subscribe, write, create or release method, and no way back to the
/// concrete graph type. A `&dyn ReadGraph` is therefore not a path to a write.
pub trait ReadGraph {
    /// This graph's id, as carried by every slot it mints.
    fn graph_id(&self) -> u32;

    /// Lend `slot`'s value to `read` for the duration of the call.
    ///
    /// On `Ok`, `read` was called exactly once with the slot's value. An
    /// implementation that breaks this is refused by the typed reads as
    /// [`SignalError::Released`], never trusted.
    ///
    /// # Errors
    ///
    /// [`SignalError::ForeignGraph`], [`SignalError::Released`] or
    /// [`SignalError::Reentrant`]; `read` is not called then.
    fn read_erased(
        &self,
        slot: SignalSlot,
        read: &mut dyn FnMut(&dyn Any),
    ) -> Result<(), SignalError>;
}

/// Where a read records its subscriber.
///
/// A sink is bound to one reader by whoever minted it, so the read names the
/// slot and nothing else: it cannot subscribe an arbitrary reader. The sinks
/// of a real graph are private to the crate that owns it.
pub trait ReaderSink {
    /// Record the sink's reader as a subscriber of `slot`. A stale or foreign
    /// slot is ignored.
    fn subscribe(&self, slot: SignalSlot);
}

/// What a [`ReadScope`] hands a read: the graph to resolve against and the
/// sink to subscribe through, if any. Opaque on purpose: a holder can pass it
/// to a signal read, but cannot pull the graph or the sink back out of it.
#[derive(Clone, Copy)]
pub struct ScopeRef<'a> {
    graph: &'a dyn ReadGraph,
    sink: Option<&'a dyn ReaderSink>,
}

impl<'a> ScopeRef<'a> {
    /// Reads resolve against `graph`; each slot read is passed to `sink`
    /// (`None`: a read that subscribes nobody, such as a lifecycle hook
    /// outside `build`).
    #[must_use]
    pub fn new(graph: &'a dyn ReadGraph, sink: Option<&'a dyn ReaderSink>) -> Self {
        Self { graph, sink }
    }
}

impl fmt::Debug for ScopeRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopeRef")
            .field("graph", &self.graph.graph_id())
            .field("subscribes", &self.sink.is_some())
            .finish()
    }
}

/// A context a signal can be read through (ADR-0085 §2).
///
/// `flui-view`'s `BuildContext` has it as a supertrait, so `count.get(cx)`
/// with `cx: &dyn BuildContext` keeps its spelling. Read-only by
/// construction: the one thing it yields is an opaque [`ScopeRef`], so it
/// cannot hand out a writable graph, write, or create a slot.
pub trait ReadScope {
    /// The graph and sink for reads made through this context.
    fn scope(&self) -> ScopeRef<'_>;
}

/// A reference to a scope is a scope: `&&dyn BuildContext` (a context
/// captured by reference in a closure) reads like `&dyn BuildContext`, as
/// deref coercion allowed when reads took `&dyn BuildContext`.
impl<S: ReadScope + ?Sized> ReadScope for &S {
    fn scope(&self) -> ScopeRef<'_> {
        (**self).scope()
    }
}

/// A boxed scope is a scope, for the same reason as `&S`.
impl<S: ReadScope + ?Sized> ReadScope for Box<S> {
    fn scope(&self) -> ScopeRef<'_> {
        (**self).scope()
    }
}

// ============================================================================
// Signal handles
// ============================================================================

/// A `Copy` handle to a value in a reactive graph.
///
/// `!Send + !Sync`: a handle is only meaningful on the thread that owns its
/// graph, which is realm-affine. Reading subscribes the scope's sink; writing
/// and creating go through the graph's owner (`flui-view`). Use
/// [`Signal::detach`] to carry the handle across threads.
pub struct Signal<T: 'static> {
    slot: SignalSlot,
    _t: PhantomData<fn() -> T>,
    /// Realm-affine, like the graph itself.
    _local: PhantomData<*const ()>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for Signal<T> {}

impl<T: 'static> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot
    }
}
impl<T: 'static> Eq for Signal<T> {}

impl<T: 'static> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal").field("slot", &self.slot).finish()
    }
}

/// An unbound placeholder, so a `ViewState` can hold `Signal<T>` rather than
/// `Option<Signal<T>>` between `create_state` and `init_state`, where the
/// real handle is created (`self.count = cx.signal(0)`).
///
/// It names graph id 0, which no graph mints (graph ids start at 1 and skip 0
/// on wrap), so every read and every write through it is
/// [`SignalError::Unbound`], never a read of another slot.
impl<T: 'static> Default for Signal<T> {
    fn default() -> Self {
        Self::from_slot(SignalSlot::new(SignalSlot::UNBOUND_GRAPH, 0, 0))
    }
}

/// Lend `slot` to a typed closure through an erased graph.
///
/// The downcast is checked: a slot holding another type is
/// [`SignalError::TypeMismatch`]. A graph that returns `Ok` without calling
/// the reader breaks [`ReadGraph::read_erased`]'s contract and is refused as
/// [`SignalError::Released`]; a second call of the reader does nothing. An
/// unbound handle ([`Signal::default`]) is [`SignalError::Unbound`] before any
/// graph is asked.
fn read_typed<T: 'static, R>(
    graph: &dyn ReadGraph,
    sink: Option<&dyn ReaderSink>,
    slot: SignalSlot,
    mut f: impl FnMut(&T) -> R,
) -> Result<R, SignalError> {
    if slot.is_unbound() {
        return Err(SignalError::Unbound);
    }
    let (out, graph_outcome) = invoke_reader(graph, slot, &mut f);
    // Keep the reader outside its caught invocation. Consuming an owned
    // `FnOnce` there would destroy its captures while a reader panic is still
    // unwinding, before `catch_unwind` can return. Once either the reader or
    // the graph has panicked, the opaque capture bundle cannot be destroyed
    // safely either: generated closure drop glue may drop a second hostile
    // capture while the first capture's destructor is unwinding. Leak that
    // exceptional-path envelope instead. On every non-panicking path it is
    // still destroyed normally, and a destructor panic remains observable.
    let disposed = dispose_reader(
        f,
        matches!(&out, Some(Ok(Err(_)))) || graph_outcome.is_err(),
    );
    let outcome = finish_read(out, graph_outcome, disposed, slot)?;
    finish_subscription(outcome, sink, slot)
}

type ReaderInvocation<R> = Option<Result<std::thread::Result<R>, SignalError>>;
type GraphReadOutcome = std::thread::Result<Result<(), SignalError>>;

fn invoke_reader<T: 'static, R>(
    graph: &dyn ReadGraph,
    slot: SignalSlot,
    f: &mut impl FnMut(&T) -> R,
) -> (ReaderInvocation<R>, GraphReadOutcome) {
    let mut called = false;
    let mut out: Option<Result<std::thread::Result<R>, SignalError>> = None;
    let graph_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        graph.read_erased(slot, &mut |value: &dyn Any| {
            if std::mem::replace(&mut called, true) {
                return;
            }
            out = Some(match value.downcast_ref::<T>() {
                Some(typed) => Ok(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || f(typed),
                ))),
                None => Err(SignalError::TypeMismatch {
                    index: slot.index,
                    expected: type_name::<T>(),
                }),
            });
        })
    }));
    (out, graph_outcome)
}

fn finish_read<R>(
    out: ReaderInvocation<R>,
    graph_outcome: GraphReadOutcome,
    disposed: std::thread::Result<()>,
    slot: SignalSlot,
) -> Result<std::thread::Result<R>, SignalError> {
    let outcome = match out {
        Some(Ok(Err(payload))) => {
            if let Err(secondary) = graph_outcome {
                discard_panic_payload(secondary);
            }
            if let Err(secondary) = disposed {
                discard_panic_payload(secondary);
            }
            Err(payload)
        }
        Some(Ok(Ok(result))) => match graph_outcome {
            Ok(Ok(())) => match disposed {
                Ok(()) => Ok(result),
                Err(payload) => {
                    discard_secondary(result);
                    Err(payload)
                }
            },
            Ok(Err(error)) => {
                discard_secondary(result);
                if let Err(payload) = disposed {
                    std::panic::resume_unwind(payload);
                }
                return Err(error);
            }
            Err(payload) => {
                discard_secondary(result);
                if let Err(secondary) = disposed {
                    discard_panic_payload(secondary);
                }
                std::panic::resume_unwind(payload)
            }
        },
        Some(Err(read_error)) => match graph_outcome {
            Ok(Ok(())) => {
                if let Err(payload) = disposed {
                    std::panic::resume_unwind(payload);
                }
                return Err(read_error);
            }
            Ok(Err(graph_error)) => {
                if let Err(payload) = disposed {
                    std::panic::resume_unwind(payload);
                }
                return Err(graph_error);
            }
            Err(payload) => {
                if let Err(secondary) = disposed {
                    discard_panic_payload(secondary);
                }
                std::panic::resume_unwind(payload)
            }
        },
        None => match graph_outcome {
            Ok(Ok(())) => {
                if let Err(payload) = disposed {
                    std::panic::resume_unwind(payload);
                }
                return Err(SignalError::Released {
                    index: slot.index,
                    generation: slot.generation,
                });
            }
            Ok(Err(error)) => {
                if let Err(payload) = disposed {
                    std::panic::resume_unwind(payload);
                }
                return Err(error);
            }
            Err(payload) => {
                if let Err(secondary) = disposed {
                    discard_panic_payload(secondary);
                }
                std::panic::resume_unwind(payload)
            }
        },
    };
    Ok(outcome)
}

fn dispose_reader<F>(reader: F, has_primary_panic: bool) -> std::thread::Result<()> {
    if has_primary_panic {
        std::mem::forget(reader);
        Ok(())
    } else {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(reader)))
    }
}

fn finish_subscription<R>(
    outcome: std::thread::Result<R>,
    sink: Option<&dyn ReaderSink>,
    slot: SignalSlot,
) -> Result<R, SignalError> {
    let subscription = sink.map(|sink| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.subscribe(slot)))
    });
    match outcome {
        Ok(result) => match subscription {
            None | Some(Ok(())) => Ok(result),
            Some(Err(payload)) => {
                // `R` is arbitrary user-owned state. Dispose it before
                // resuming the subscription panic, but do not let its
                // destructor replace that earlier failure.
                discard_secondary(result);
                std::panic::resume_unwind(payload)
            }
        },
        Err(payload) => {
            if let Some(Err(secondary)) = subscription {
                discard_panic_payload(secondary);
            }
            std::panic::resume_unwind(payload)
        }
    }
}

/// Retire a value while another failure already has chronological priority.
///
/// `T` is opaque. Its generated drop glue may destroy another field while a
/// first field's destructor is unwinding, which aborts before `catch_unwind`
/// can regain control. The only generic continuation-safe operation is to
/// leak the exceptional-path value.
fn discard_secondary<T>(value: T) {
    std::mem::forget(value);
}

/// Dispose a secondary panic payload without risking a double-panic abort.
fn discard_panic_payload(payload: Box<dyn Any + Send>) {
    // A panic payload is opaque and may itself contain multiple hostile
    // destructors. Retiring it through `drop` cannot be made unwind-safe.
    std::mem::forget(payload);
}

impl<T: 'static> Signal<T> {
    /// A typed handle over `slot`, for the graph that minted `slot` with a
    /// value of type `T`. A handle whose `T` disagrees with the slot is
    /// refused on every operation with [`SignalError::TypeMismatch`].
    #[doc(hidden)]
    #[must_use]
    pub const fn from_slot(slot: SignalSlot) -> Self {
        Self {
            slot,
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// The arena slot behind this handle.
    #[must_use]
    pub const fn slot(self) -> SignalSlot {
        self.slot
    }

    /// The `Send + Sync` form of this handle, for a closure that will run on
    /// the owner thread later (`UiCommand::SignalWrite` in `flui-app`).
    #[must_use]
    pub const fn detach(self) -> SignalSender<T> {
        SignalSender {
            slot: self.slot,
            _t: PhantomData,
        }
    }

    /// Read through `cx`; during `build` the building element becomes a
    /// reader. Once the graph and value type are validated, that subscription
    /// is recorded after the graph releases its read loan and before a panic
    /// from `f` resumes, so a recovered build can still be invalidated by a
    /// later write. A refused read subscribes nobody. The reader is accepted
    /// as [`FnMut`] even though it is invoked at most once: retaining it across
    /// the protected invocation lets a successful read destroy its captures
    /// normally and a panicking read retain the opaque bundle rather than run
    /// aggregate drop glue during recovery.
    ///
    /// # Errors
    ///
    /// [`SignalError::Released`] for a stale handle (its owning element
    /// unmounted: a sliver band evicted, a route popped),
    /// [`SignalError::ForeignGraph`] for another graph's handle,
    /// [`SignalError::TypeMismatch`] for a handle of the wrong `T`,
    /// [`SignalError::Reentrant`] from inside this slot's own closure.
    pub fn try_with<S: ReadScope + ?Sized, R>(
        self,
        cx: &S,
        f: impl FnMut(&T) -> R,
    ) -> Result<R, SignalError> {
        let scope = cx.scope();
        read_typed(scope.graph, scope.sink, self.slot, f)
    }

    /// [`Signal::try_with`], cloning the value.
    ///
    /// # Errors
    ///
    /// As [`Signal::try_with`].
    pub fn try_get<S: ReadScope + ?Sized>(self, cx: &S) -> Result<T, SignalError>
    where
        T: Clone,
    {
        self.try_with(cx, T::clone)
    }

    /// Borrowed read through `cx`; during `build` the building element
    /// becomes a reader.
    ///
    /// # Panics
    ///
    /// On a stale handle (its owning element unmounted), a handle from
    /// another graph, a handle of the wrong `T`, or a re-entrant read of this
    /// same slot; see [`Signal::try_with`] for the non-panicking form.
    pub fn with<S: ReadScope + ?Sized, R>(self, cx: &S, f: impl FnMut(&T) -> R) -> R {
        match self.try_with(cx, f) {
            Ok(result) => result,
            Err(error) => panic!("Signal::with: {error} (use try_with for a fallible read)"),
        }
    }

    /// [`Signal::with`], cloning the value.
    ///
    /// # Panics
    ///
    /// As [`Signal::with`].
    #[must_use]
    pub fn get<S: ReadScope + ?Sized>(self, cx: &S) -> T
    where
        T: Clone,
    {
        self.with(cx, T::clone)
    }

    /// Read without subscribing anyone (callbacks, tests, realm commands).
    ///
    /// # Errors
    ///
    /// As [`Signal::try_with`].
    pub fn peek<R>(self, graph: &dyn ReadGraph, f: impl FnMut(&T) -> R) -> Result<R, SignalError> {
        read_typed(graph, None, self.slot, f)
    }
}

/// The `Send + Sync` form of a [`Signal`] handle for crossing a thread
/// boundary: it carries only the slot, and can do nothing until it is
/// re-attached on the owner thread (inside a `UiCommand::SignalWrite`
/// closure, ADR-0074 §5.8), where [`SignalSender::attach`] hands back the
/// realm-affine [`Signal`]. The realm routes that command by
/// [`SignalSlot::graph`] of [`SignalSender::slot`] to the presentation whose
/// graph minted it (ADR-0085 §1).
pub struct SignalSender<T: 'static> {
    slot: SignalSlot,
    _t: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for SignalSender<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for SignalSender<T> {}

impl<T: 'static> fmt::Debug for SignalSender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignalSender")
            .field("slot", &self.slot)
            .finish()
    }
}

impl<T: 'static> SignalSender<T> {
    /// Re-attach on the owner thread. The handle is only meaningful against
    /// the graph that minted the original signal; any operation through
    /// another graph reports [`SignalError::ForeignGraph`].
    #[must_use]
    pub const fn attach(self) -> Signal<T> {
        Signal::from_slot(self.slot)
    }

    /// The arena slot behind this handle. Readable on any thread: a realm
    /// routes the write this sender carries by its [`SignalSlot::graph`].
    #[must_use]
    pub const fn slot(self) -> SignalSlot {
        self.slot
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A one-slot graph: slot index 0, generation 0, of graph `id`.
    struct OneSlot {
        id: u32,
        value: Box<dyn Any>,
    }

    impl OneSlot {
        fn new<T: 'static>(id: u32, value: T) -> Self {
            Self {
                id,
                value: Box::new(value),
            }
        }

        fn slot(&self) -> SignalSlot {
            SignalSlot::new(self.id, 0, 0)
        }
    }

    impl ReadGraph for OneSlot {
        fn graph_id(&self) -> u32 {
            self.id
        }

        fn read_erased(
            &self,
            slot: SignalSlot,
            read: &mut dyn FnMut(&dyn Any),
        ) -> Result<(), SignalError> {
            if slot.graph() != self.id {
                return Err(SignalError::ForeignGraph {
                    index: slot.index(),
                    graph: slot.graph(),
                    this: self.id,
                });
            }
            if slot.index() != 0 || slot.generation() != 0 {
                return Err(SignalError::Released {
                    index: slot.index(),
                    generation: slot.generation(),
                });
            }
            read(&*self.value);
            Ok(())
        }
    }

    struct PreReadPanickingGraph(u32);

    impl ReadGraph for PreReadPanickingGraph {
        fn graph_id(&self) -> u32 {
            self.0
        }

        fn read_erased(
            &self,
            _slot: SignalSlot,
            _read: &mut dyn FnMut(&dyn Any),
        ) -> Result<(), SignalError> {
            panic!("graph probe");
        }
    }

    #[derive(Default)]
    struct Recorder(RefCell<Vec<SignalSlot>>);

    impl ReaderSink for Recorder {
        fn subscribe(&self, slot: SignalSlot) {
            self.0.borrow_mut().push(slot);
        }
    }

    /// A context over a graph and an optional sink.
    struct Cx<'a> {
        graph: &'a dyn ReadGraph,
        sink: Option<&'a dyn ReaderSink>,
    }

    impl ReadScope for Cx<'_> {
        fn scope(&self) -> ScopeRef<'_> {
            ScopeRef::new(self.graph, self.sink)
        }
    }

    struct PanickingSink;

    impl ReaderSink for PanickingSink {
        fn subscribe(&self, _slot: SignalSlot) {
            panic!("subscription probe");
        }
    }

    struct DropBomb(&'static str);

    impl Drop for DropBomb {
        fn drop(&mut self) {
            panic!("{}", self.0);
        }
    }

    fn a_user_read_panic_keeps_priority_over_a_subscription_panic() {
        let graph = OneSlot::new(1, 7u32);
        let sink = PanickingSink;
        let cx = Cx {
            graph: &graph,
            sink: Some(&sink),
        };
        let signal = Signal::<u32>::from_slot(graph.slot());

        let subscription_only = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal.with(&cx, |value| *value)
        }));
        let payload = subscription_only.expect_err("the subscription panic must resume");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"subscription probe"));

        let dual_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal.with(&cx, |_| panic!("reader probe"));
        }));
        let payload = dual_panic.expect_err("the original read panic must resume");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"reader probe"),
            "a secondary subscription panic must not replace the user failure"
        );
    }

    fn reader_panic_keeps_priority_over_its_captures_destructor_panic() {
        let graph = OneSlot::new(1, 7u32);
        let sink = Recorder::default();
        let cx = Cx {
            graph: &graph,
            sink: Some(&sink),
        };
        let signal = Signal::<u32>::from_slot(graph.slot());
        let captured = DropBomb("reader capture destructor probe");

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal.with(&cx, move |_| {
                let _capture_stays_owned_by_the_reader = &captured;
                panic!("reader probe");
            });
        }));

        let payload = outcome.expect_err("the reader panic must resume");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"reader probe"),
            "the capture destructor panic must remain secondary"
        );
        assert_eq!(
            *sink.0.borrow(),
            [graph.slot()],
            "the valid read must still subscribe before its panic resumes"
        );
    }

    fn graph_panic_keeps_priority_over_the_unread_closures_destructor_panic() {
        let graph = PreReadPanickingGraph(1);
        let signal = Signal::<u32>::from_slot(SignalSlot::new(graph.graph_id(), 0, 0));
        let captured = DropBomb("unread closure destructor probe");

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = signal.peek(&graph, move |_| {
                let _ = &captured;
            });
        }));

        let payload = outcome.expect_err("the graph panic must resume");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"graph probe"),
            "the unread closure's destructor panic must remain secondary"
        );
    }

    fn a_foreign_or_stale_read_subscribes_nobody() {
        let graph = OneSlot::new(1, 7u32);
        let sink = Recorder::default();
        let cx = Cx {
            graph: &graph,
            sink: Some(&sink),
        };
        let foreign = Signal::<u32>::from_slot(SignalSlot::new(2, 0, 0));
        let stale = Signal::<u32>::from_slot(SignalSlot::new(1, 0, 1));
        assert!(matches!(
            foreign.try_get(&cx),
            Err(SignalError::ForeignGraph { .. })
        ));
        assert!(matches!(
            stale.try_get(&cx),
            Err(SignalError::Released { .. })
        ));
        assert!(sink.0.borrow().is_empty());
    }

    #[test]
    fn read_scope_failure_priority() {
        crate::test_cases::run_cases(&[
            (
                "a user read panic keeps priority over a subscription panic",
                a_user_read_panic_keeps_priority_over_a_subscription_panic,
            ),
            (
                "reader panic keeps priority over its captures destructor panic",
                reader_panic_keeps_priority_over_its_captures_destructor_panic,
            ),
            (
                "graph panic keeps priority over the unread closures destructor panic",
                graph_panic_keeps_priority_over_the_unread_closures_destructor_panic,
            ),
            (
                "a foreign or stale read subscribes nobody",
                a_foreign_or_stale_read_subscribes_nobody,
            ),
        ]);
    }
}
