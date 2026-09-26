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

/// Lend `slot` to a typed closure through an erased graph.
///
/// The downcast is checked: a slot holding another type is
/// [`SignalError::TypeMismatch`]. A graph that returns `Ok` without calling
/// the reader breaks [`ReadGraph::read_erased`]'s contract and is refused as
/// [`SignalError::Released`]; a second call of the reader does nothing.
fn read_typed<T: 'static, R>(
    graph: &dyn ReadGraph,
    slot: SignalSlot,
    f: impl FnOnce(&T) -> R,
) -> Result<R, SignalError> {
    let mut f = Some(f);
    let mut out: Option<Result<R, SignalError>> = None;
    graph.read_erased(slot, &mut |value: &dyn Any| {
        let Some(f) = f.take() else {
            return;
        };
        out = Some(match value.downcast_ref::<T>() {
            Some(typed) => Ok(f(typed)),
            None => Err(SignalError::TypeMismatch {
                index: slot.index,
                expected: type_name::<T>(),
            }),
        });
    })?;
    out.unwrap_or(Err(SignalError::Released {
        index: slot.index,
        generation: slot.generation,
    }))
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
    /// reader. A refused read subscribes nobody.
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
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, SignalError> {
        let scope = cx.scope();
        let result = read_typed(scope.graph, self.slot, f)?;
        if let Some(sink) = scope.sink {
            sink.subscribe(self.slot);
        }
        Ok(result)
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
    pub fn with<S: ReadScope + ?Sized, R>(self, cx: &S, f: impl FnOnce(&T) -> R) -> R {
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
    pub fn peek<R>(self, graph: &dyn ReadGraph, f: impl FnOnce(&T) -> R) -> Result<R, SignalError> {
        read_typed(graph, self.slot, f)
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
    use std::cell::{Cell, RefCell};

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
        sink: Option<&'a Recorder>,
    }

    impl ReadScope for Cx<'_> {
        fn scope(&self) -> ScopeRef<'_> {
            ScopeRef::new(self.graph, self.sink.map(|s| s as &dyn ReaderSink))
        }
    }

    #[test]
    fn every_error_variant_displays_its_text() {
        let element = ElementId::new(3);
        let cases = [
            (
                SignalError::Released {
                    index: 1,
                    generation: 2,
                },
                "signal slot 1 generation 2 was released".to_owned(),
            ),
            (
                SignalError::ForeignGraph {
                    index: 1,
                    graph: 7,
                    this: 8,
                },
                "signal slot 1 belongs to graph 7, not to this one (8)".to_owned(),
            ),
            (
                SignalError::TypeMismatch {
                    index: 4,
                    expected: "alloc::string::String",
                },
                "signal slot 4 holds a value of another type than alloc::string::String".to_owned(),
            ),
            (
                SignalError::WrittenDuringBuild { element },
                format!("signal written during the build of {element:?}"),
            ),
            (
                SignalError::CreatedDuringBuild { element },
                format!("signal created during the build of {element:?}"),
            ),
            (
                SignalError::Reentrant { index: 5 },
                "signal slot 5 accessed re-entrantly from its own read/write closure".to_owned(),
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }

    #[test]
    fn a_handle_of_the_wrong_type_is_a_typed_error_and_subscribes_nobody() {
        let graph = OneSlot::new(1, 7u32);
        let sink = Recorder::default();
        let cx = Cx {
            graph: &graph,
            sink: Some(&sink),
        };
        let wrong = Signal::<String>::from_slot(graph.slot());
        let expected = SignalError::TypeMismatch {
            index: 0,
            expected: type_name::<String>(),
        };

        assert_eq!(wrong.try_get(&cx), Err(expected));
        assert_eq!(wrong.peek(&graph, String::len), Err(expected));
        assert!(
            sink.0.borrow().is_empty(),
            "a refused read subscribes nobody"
        );

        let right = Signal::<u32>::from_slot(graph.slot());
        assert_eq!(right.try_get(&cx), Ok(7));
        assert_eq!(*sink.0.borrow(), [graph.slot()]);
    }

    #[test]
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
    fn reads_go_through_references_and_boxes_of_a_scope() {
        let graph = OneSlot::new(1, 5u32);
        let cx = Cx {
            graph: &graph,
            sink: None,
        };
        let sig = Signal::<u32>::from_slot(graph.slot());
        let by_ref: &Cx<'_> = &cx;
        let by_ref_ref: &&Cx<'_> = &by_ref;
        let boxed: Box<dyn ReadScope + '_> = Box::new(Cx {
            graph: &graph,
            sink: None,
        });
        let as_dyn: &dyn ReadScope = &cx;

        assert_eq!(sig.get(&cx), 5);
        assert_eq!(sig.get(by_ref_ref), 5);
        assert_eq!(sig.get(&boxed), 5);
        assert_eq!(sig.get(as_dyn), 5);
        assert_eq!(sig.with(&by_ref, |v| v + 1), 6);
    }

    /// A graph that breaks `read_erased`'s contract: calls the reader
    /// `calls` times and returns `Ok`.
    struct Misbehaving {
        calls: usize,
    }

    impl ReadGraph for Misbehaving {
        fn graph_id(&self) -> u32 {
            9
        }

        fn read_erased(
            &self,
            _slot: SignalSlot,
            read: &mut dyn FnMut(&dyn Any),
        ) -> Result<(), SignalError> {
            for _ in 0..self.calls {
                read(&1u32);
            }
            Ok(())
        }
    }

    #[test]
    fn a_graph_that_never_calls_the_reader_is_refused_not_trusted() {
        let slot = SignalSlot::new(9, 3, 4);
        let sig = Signal::<u32>::from_slot(slot);
        assert_eq!(
            sig.peek(&Misbehaving { calls: 0 }, |v| *v),
            Err(SignalError::Released {
                index: 3,
                generation: 4
            })
        );
    }

    #[test]
    fn a_graph_that_calls_the_reader_twice_runs_the_closure_once() {
        let sig = Signal::<u32>::from_slot(SignalSlot::new(9, 0, 0));
        let runs = Cell::new(0);
        let read = sig.peek(&Misbehaving { calls: 2 }, |v| {
            runs.set(runs.get() + 1);
            *v
        });
        assert_eq!(read, Ok(1));
        assert_eq!(runs.get(), 1);
    }

    #[test]
    fn detach_and_attach_keep_the_slot() {
        let slot = SignalSlot::new(1, 2, 3);
        let sig = Signal::<u8>::from_slot(slot);
        assert_eq!(sig.detach().slot(), slot);
        assert_eq!(sig.detach().attach(), sig);
        assert_eq!((slot.graph(), slot.index(), slot.generation()), (1, 2, 3));
    }

    #[test]
    fn a_scope_ref_debug_shows_only_the_graph_id_and_whether_it_subscribes() {
        let graph = OneSlot::new(4, 0u8);
        let sink = Recorder::default();
        let text = format!("{:?}", ScopeRef::new(&graph, Some(&sink)));
        assert_eq!(text, "ScopeRef { graph: 4, subscribes: true }");
    }
}
