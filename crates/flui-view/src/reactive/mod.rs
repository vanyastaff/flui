//! Realm-scoped signals — ADR-0074, placed by ADR-0085.
//!
//! One [`Reactive`] graph per `BuildOwner` (so per presentation, and every
//! presentation belongs to a realm): an arena of generational slots holding
//! [`Signal`] values, plus the **reader registry** that makes the whole thing
//! worth having:
//!
//! - reading a signal inside `build` (through [`Signal::get`] / [`Signal::with`]
//!   / their `try_` forms, which take any `&S` where `S: ReadScope + ?Sized`,
//!   `BuildContext` included) records the building element as a reader of that
//!   slot — the same class of edge as `depend_on` for an inherited provider,
//!   re-derived from scratch on every build of that element (a build that no
//!   longer reads a signal stops depending on it);
//! - writing a signal ([`SignalWriteExt::set`] / [`SignalWriteExt::update`])
//!   schedules exactly the reader elements on the owner's existing external
//!   inbox ([`RebuildReason::SignalChange`]).
//!
//! # The read contract
//!
//! The handles, the error type and the read contract live in
//! [`flui_foundation::read_scope`] and are re-exported here, so render and
//! animation code can name a `Signal<T>` without depending on this crate. The
//! graph implements [`ReadGraph`] (pure reads, enough for [`Signal::peek`]) and
//! never [`ReaderSink`]: the only sink that subscribes an element is the
//! private `ElementReads`, which `make_build_ctx` mints for the element about
//! to build. A scope built anywhere else reads but subscribes nobody.
//!
//! Nothing here touches the element tree: the only side effect on it is the
//! same `ExternalBuildScheduler::schedule` call a `RebuildHandle` makes, so the
//! depth-ordered drain, the mid-drain absorb budget and the `catch_unwind`
//! around `build` all apply unchanged.
//!
//! Derived values and effects are **not** part of this module: ADR-0075
//! (Proposed) designs them separately, with the glitch-freedom and panic
//! safety the first prototype did not have.
//!
//! Equality is opt-in, never implied: a plain `set` marks readers even when the
//! value is equal (there is no `PartialEq` bound on `T`); only
//! [`SignalWriteExt::set_if_changed`] compares, and says so in its bounds.
//!
//! # Refusals at run time
//!
//! The guard lives in the binding: while an element's
//! `build` runs (`build_or_recover`, the one choke point every element kind
//! builds through, arms it), a **write** ([`SignalError::WrittenDuringBuild`])
//! and a **slot creation** ([`SignalError::CreatedDuringBuild`]) are refused
//! with a typed error and a `tracing::warn!`. Reads are the sanctioned
//! subscription path.
//!
//! # Re-entrancy
//!
//! A read or write closure runs with **no** borrow of the graph held: the
//! slot's value is taken out on loan, the closure runs, the value is put back
//! if the slot is still the same generation. Nested access to a *different*
//! slot is therefore fine (`a.with(cx, |_| b.get(cx))`); nested access to the
//! *same* slot finds the value on loan and gets [`SignalError::Reentrant`],
//! never a `RefCell` panic.
//!
//! # Unwind consistency
//!
//! An `update` closure is not a transaction. If it mutates the value and then
//! panics, the partial value is put back, every registered reader is durably
//! enqueued as one batch, and the original panic resumes, provided the closure
//! did not explicitly release that same slot. Rolling arbitrary `T` back would
//! require a separate snapshot/transaction contract; leaving a committed value
//! invisible to the UI is not an acceptable substitute. Explicit release still
//! wins by destroying the slot and its reader set.
//!
//! # Threading
//!
//! Everything here is `!Send + !Sync` — realm-affine like the element tree. A
//! handle carries the id of the graph that minted it, so a handle from realm
//! A used against realm B is [`SignalError::ForeignGraph`], not a silent read
//! of someone else's slot. A cross-thread write is a realm command executed on
//! the owner thread (`UiCommand::SignalWrite` in `flui-app`, carrying a
//! [`SignalSender`]), never a shared cell. The realm routes that command by
//! [`SignalSlot::graph`] to the presentation whose graph minted the slot
//! (ADR-0085 §1).

use std::any::{Any, type_name};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

pub use flui_foundation::read_scope::{
    ReadGraph, ReadScope, ReaderSink, ScopeRef, Signal, SignalError, SignalSender, SignalSlot,
};
use flui_foundation::{ElementId, RebuildReason};
use smallvec::SmallVec;

#[cfg(test)]
use crate::owner::ExternalBuildInbox;
use crate::owner::ExternalBuildScheduler;

mod writer;
pub use writer::{
    EventContextError, EventCx, EventError, EventOutcome, WriteTarget, Writer, WriterSource,
    callback, callback_ref, callback_with,
};

/// Process-wide counter that gives every [`Reactive`] graph a distinct id, so
/// a [`SignalSlot`] is meaningful only against the graph that minted it. Id 0
/// ([`SignalSlot::UNBOUND_GRAPH`]) is never handed out: it names the unbound
/// handle [`Signal::default`].
static NEXT_GRAPH_ID: AtomicU32 = AtomicU32::new(1);

struct Node {
    generation: u32,
    live: bool,
    /// The value; `None` while a freed slot, or while a read/write closure
    /// holds it on loan (see the module docs on re-entrancy).
    value: Option<Box<dyn Any>>,
    /// Elements that read this slot during their last build.
    element_readers: SmallVec<[ElementId; 4]>,
}

#[derive(Default)]
struct Inner {
    nodes: Vec<Node>,
    free: Vec<u32>,
    /// Slots each element read during its last build.
    element_reads: HashMap<ElementId, SmallVec<[u32; 4]>>,
    /// Slots created on behalf of an element (full handles, so a slot that
    /// was released and reused in between is recognised by generation).
    owned_by_element: HashMap<ElementId, SmallVec<[SignalSlot; 2]>>,
    /// The element whose `build` is running, if any.
    building: Option<ElementId>,
    /// The read set of the element now building, taken out by
    /// `begin_element_build`: dropped when the build completes, restored when
    /// it unwinds (a panicking build says nothing about what it reads).
    previous_reads: SmallVec<[u32; 4]>,
    scheduler: Option<ExternalBuildScheduler>,
}

/// The reactive graph of one `BuildOwner` (one realm). Cheap to clone (an
/// `Rc`); `!Send + !Sync` by construction.
#[derive(Clone)]
pub struct Reactive {
    id: u32,
    inner: Rc<RefCell<Inner>>,
}

impl Default for Reactive {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Reactive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("Reactive")
            .field("id", &self.id)
            .field("slots", &inner.nodes.len())
            .field("free", &inner.free.len())
            .field("building", &inner.building)
            .finish()
    }
}

/// A snapshot of one live slot, for tests, tooling and agents
/// ([`Reactive::snapshot`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    /// The slot's handle.
    pub slot: SignalSlot,
    /// Elements registered as readers.
    pub readers: Vec<ElementId>,
    /// The element that owns the slot's lifetime, if any.
    pub owner: Option<ElementId>,
}

impl Reactive {
    /// A graph with no scheduler: writes still update values, but no element
    /// is scheduled until the crate-private `set_scheduler` is called (the
    /// `BuildOwner` does that at construction and again when its
    /// frame-request callback changes).
    #[must_use]
    pub fn new() -> Self {
        // Skip the unbound id if the counter ever wraps.
        let id = loop {
            let id = NEXT_GRAPH_ID.fetch_add(1, Ordering::Relaxed);
            if id != SignalSlot::UNBOUND_GRAPH {
                break id;
            }
        };
        Self {
            id,
            inner: Rc::new(RefCell::new(Inner::default())),
        }
    }

    /// This graph's id, as carried by every slot it mints.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Route reader scheduling through `scheduler` (the owner's external
    /// inbox).
    pub(crate) fn set_scheduler(&self, scheduler: ExternalBuildScheduler) {
        self.inner.borrow_mut().scheduler = Some(scheduler);
    }

    // ------------------------------------------------------------------ slots

    fn alloc<T: 'static>(
        &self,
        value: T,
        owner: Option<ElementId>,
    ) -> Result<SignalSlot, SignalError> {
        let mut inner = self.inner.borrow_mut();
        if let Some(element) = inner.building {
            tracing::warn!(
                target: "flui::signals",
                element = ?element,
                owner = ?owner,
                "refused: a signal was created during build (create it in init_state and hold the handle)"
            );
            return Err(SignalError::CreatedDuringBuild { element });
        }
        let value: Box<dyn Any> = Box::new(value);
        let index = if let Some(index) = inner.free.pop() {
            let node = &mut inner.nodes[index as usize];
            node.generation = node.generation.wrapping_add(1);
            node.live = true;
            node.value = Some(value);
            node.element_readers.clear();
            index
        } else {
            inner.nodes.push(Node {
                generation: 0,
                live: true,
                value: Some(value),
                element_readers: SmallVec::new(),
            });
            (inner.nodes.len() - 1) as u32
        };
        let slot = SignalSlot::new(self.id, index, inner.nodes[index as usize].generation);
        if let Some(owner) = owner {
            inner.owned_by_element.entry(owner).or_default().push(slot);
        }
        tracing::trace!(target: "flui::signals", slot = ?slot, owner = ?owner, "signal created");
        Ok(slot)
    }

    fn check(&self, inner: &Inner, slot: SignalSlot) -> Result<(), SignalError> {
        if slot.is_unbound() {
            return Err(SignalError::Unbound);
        }
        if slot.graph() != self.id {
            return Err(SignalError::ForeignGraph {
                index: slot.index(),
                graph: slot.graph(),
                this: self.id,
            });
        }
        match inner.nodes.get(slot.index() as usize) {
            Some(node) if node.live && node.generation == slot.generation() => Ok(()),
            _ => Err(SignalError::Released {
                index: slot.index(),
                generation: slot.generation(),
            }),
        }
    }

    /// Create a signal that lives as long as the graph — application-level
    /// state released with the realm. Widget-level state belongs to its
    /// element: use [`Reactive::signal_owned_by`] (or `cx.signal(..)` from
    /// `init_state`, which does that for you) so the slot is released when
    /// the element unmounts.
    ///
    /// # Errors
    ///
    /// [`SignalError::CreatedDuringBuild`] while an element is building.
    pub fn try_signal<T: 'static>(&self, value: T) -> Result<Signal<T>, SignalError> {
        self.alloc(value, None).map(Signal::from_slot)
    }

    /// [`Reactive::try_signal`], panicking on refusal.
    ///
    /// # Panics
    ///
    /// If called while an element is building (see
    /// [`SignalError::CreatedDuringBuild`]).
    #[must_use]
    pub fn signal<T: 'static>(&self, value: T) -> Signal<T> {
        match self.try_signal(value) {
            Ok(signal) => signal,
            Err(error) => {
                panic!("Reactive::signal: {error} (use try_signal for a fallible creation)")
            }
        }
    }

    /// Create a signal released when `owner` unmounts — the canonical form for
    /// widget-owned state (`BuildContextExt::signal` calls this with the
    /// building element).
    ///
    /// # Errors
    ///
    /// [`SignalError::CreatedDuringBuild`] while an element is building.
    pub fn try_signal_owned_by<T: 'static>(
        &self,
        owner: ElementId,
        value: T,
    ) -> Result<Signal<T>, SignalError> {
        self.alloc(value, Some(owner)).map(Signal::from_slot)
    }

    /// [`Reactive::try_signal_owned_by`], panicking on refusal.
    ///
    /// # Panics
    ///
    /// If called while an element is building.
    #[must_use]
    pub fn signal_owned_by<T: 'static>(&self, owner: ElementId, value: T) -> Signal<T> {
        match self.try_signal_owned_by(owner, value) {
            Ok(signal) => signal,
            Err(error) => panic!(
                "Reactive::signal_owned_by: {error} (use try_signal_owned_by for a fallible creation)"
            ),
        }
    }

    /// Release a slot explicitly: its value drops, its readers forget it, later
    /// handle use reports [`SignalError::Released`]. A no-op for a handle that
    /// is already stale. Release is authoritative even while the slot's value
    /// is loaned to its own read or update closure: the loaned value is dropped
    /// when that closure returns or unwinds, and no readers are invalidated.
    pub fn release(&self, slot: SignalSlot) {
        let mut inner = self.inner.borrow_mut();
        if self.check(&inner, slot).is_err() {
            return;
        }
        Self::release_index(&mut inner, slot.index());
        tracing::trace!(target: "flui::signals", slot = ?slot, "signal released");
    }

    fn release_index(inner: &mut Inner, index: u32) {
        let readers = std::mem::take(&mut inner.nodes[index as usize].element_readers);
        for element in readers {
            if let Some(reads) = inner.element_reads.get_mut(&element) {
                reads.retain(|slot| *slot != index);
            }
        }
        let node = &mut inner.nodes[index as usize];
        node.live = false;
        node.value = None;
        inner.free.push(index);
    }

    // ------------------------------------------------------------ elements

    /// Called by `build_or_recover` right before `element`'s `build`: forget
    /// every slot it read last time so this build re-derives the set from
    /// scratch, and arm the build-time refusals.
    pub(crate) fn begin_element_build(&self, element: ElementId) {
        let mut inner = self.inner.borrow_mut();
        inner.previous_reads = inner
            .element_reads
            .get(&element)
            .cloned()
            .unwrap_or_default();
        Self::forget_element_reads(&mut inner, element);
        inner.building = Some(element);
    }

    /// Called by `build_or_recover` right after `element`'s `build` returned
    /// (or unwound).
    ///
    /// `completed` is false when the build unwound: the reads it made before
    /// the panic are kept and the previous read set is restored on top, so a
    /// build that panics before reading a signal stays subscribed and a later
    /// write can rebuild it once the failing condition is fixed.
    pub(crate) fn end_element_build(&self, element: ElementId, completed: bool) {
        let mut inner = self.inner.borrow_mut();
        let previous = std::mem::take(&mut inner.previous_reads);
        if inner.building == Some(element) {
            inner.building = None;
        }
        if completed {
            return;
        }
        for index in previous {
            let Some(node) = inner.nodes.get_mut(index as usize) else {
                continue;
            };
            if !node.live {
                continue;
            }
            if !node.element_readers.contains(&element) {
                node.element_readers.push(element);
            }
            let reads = inner.element_reads.entry(element).or_default();
            if !reads.contains(&index) {
                reads.push(index);
            }
        }
    }

    fn forget_element_reads(inner: &mut Inner, element: ElementId) {
        let Some(reads) = inner.element_reads.remove(&element) else {
            return;
        };
        for index in reads {
            if let Some(node) = inner.nodes.get_mut(index as usize) {
                node.element_readers.retain(|reader| *reader != element);
            }
        }
    }

    /// Record that `element` (the one building right now) read `slot`.
    pub(crate) fn register_element_reader(&self, slot: SignalSlot, element: ElementId) {
        let mut inner = self.inner.borrow_mut();
        if self.check(&inner, slot).is_err() {
            return;
        }
        let node = &mut inner.nodes[slot.index() as usize];
        if !node.element_readers.contains(&element) {
            node.element_readers.push(element);
        }
        let reads = inner.element_reads.entry(element).or_default();
        if !reads.contains(&slot.index()) {
            reads.push(slot.index());
        }
    }

    /// The element unmounted: it reads nothing any more, and every slot
    /// created on its behalf **that is still the generation it created** is
    /// released. A slot the element released earlier and that a later owner
    /// reused is recognised by its generation and left alone.
    pub(crate) fn release_element(&self, element: ElementId) {
        let mut inner = self.inner.borrow_mut();
        Self::forget_element_reads(&mut inner, element);
        if let Some(owned) = inner.owned_by_element.remove(&element) {
            for slot in owned {
                if self.check(&inner, slot).is_ok() {
                    Self::release_index(&mut inner, slot.index());
                    tracing::trace!(
                        target: "flui::signals",
                        slot = ?slot,
                        ?element,
                        "signal released with its element"
                    );
                }
            }
        }
    }

    /// Elements currently registered as readers of `slot` (test/diagnostic
    /// probe).
    #[must_use]
    pub fn readers_of(&self, slot: SignalSlot) -> Vec<ElementId> {
        let inner = self.inner.borrow();
        match self.check(&inner, slot) {
            Ok(()) => inner.nodes[slot.index() as usize].element_readers.to_vec(),
            Err(_) => Vec::new(),
        }
    }

    /// Every live slot with its readers and owner — the introspection surface
    /// for devtools and agents.
    #[must_use]
    pub fn snapshot(&self) -> Vec<SlotInfo> {
        let inner = self.inner.borrow();
        let mut owners: HashMap<u32, ElementId> = HashMap::new();
        for (element, slots) in &inner.owned_by_element {
            for slot in slots {
                if self.check(&inner, *slot).is_ok() {
                    owners.insert(slot.index(), *element);
                }
            }
        }
        inner
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.live)
            .map(|(index, node)| SlotInfo {
                slot: SignalSlot::new(self.id, index as u32, node.generation),
                readers: node.element_readers.to_vec(),
                owner: owners.get(&(index as u32)).copied(),
            })
            .collect()
    }

    /// Number of live slots (test/diagnostic probe).
    #[must_use]
    pub fn live_slot_count(&self) -> usize {
        self.inner
            .borrow()
            .nodes
            .iter()
            .filter(|node| node.live)
            .count()
    }
}

// -------------------------------------------------------- take / put back

/// A slot's value on loan to a user closure. Dropping the loan puts the
/// value back (if the slot is still the same generation) — **on unwind
/// too**, so a panicking closure, which `build_or_recover` contains, does
/// not leave the slot permanently [`SignalError::Reentrant`].
struct Loan<'a> {
    graph: &'a Reactive,
    slot: SignalSlot,
    value: Option<Box<dyn Any>>,
}

impl Drop for Loan<'_> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.graph.put_back(self.slot, value);
        }
    }
}

impl Reactive {
    /// Take `slot`'s value out on loan. The borrow of the graph ends before
    /// the caller's closure runs.
    fn loan(&self, slot: SignalSlot) -> Result<Loan<'_>, SignalError> {
        let mut inner = self.inner.borrow_mut();
        self.check(&inner, slot)?;
        let value =
            inner.nodes[slot.index() as usize]
                .value
                .take()
                .ok_or(SignalError::Reentrant {
                    index: slot.index(),
                })?;
        Ok(Loan {
            graph: self,
            slot,
            value: Some(value),
        })
    }

    /// Return a loaned value, unless the slot was released (or reused) while
    /// it was out — then the value simply drops.
    fn put_back(&self, slot: SignalSlot, value: Box<dyn Any>) {
        let mut inner = self.inner.borrow_mut();
        if self.check(&inner, slot).is_ok() {
            inner.nodes[slot.index() as usize].value = Some(value);
        }
    }

    /// The write-side refusal shared by every mutating entry point, checked
    /// before anything else (so an equal `set_if_changed` inside `build` is
    /// still refused).
    fn refuse_if_building(&self, slot: SignalSlot) -> Result<(), SignalError> {
        let inner = self.inner.borrow();
        self.check(&inner, slot)?;
        if let Some(element) = inner.building {
            tracing::warn!(
                target: "flui::signals",
                slot = ?slot,
                ?element,
                "refused: a signal was written during build (write from a callback, init_state or a realm command)"
            );
            return Err(SignalError::WrittenDuringBuild { element });
        }
        Ok(())
    }

    fn read<T: 'static, R>(
        &self,
        slot: SignalSlot,
        mut f: impl FnMut(&T) -> R,
    ) -> Result<R, SignalError> {
        let loan = self.loan(slot)?;
        let typed = loan
            .value
            .as_deref()
            .and_then(<dyn Any>::downcast_ref::<T>)
            .ok_or(SignalError::TypeMismatch {
                index: slot.index(),
                expected: type_name::<T>(),
            })?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(typed)));
        let finalized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(loan)));
        match outcome {
            Ok(result) => {
                // Restore the loan before destroying the opaque capture
                // bundle. Generated closure drop glue can abort when two
                // captured fields panic; it must not strand the slot. A
                // contained capture panic keeps the pre-existing phase
                // priority over a loan-finalization panic.
                let disposed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(f)));
                match disposed {
                    Err(payload) => {
                        if let Err(secondary) = finalized {
                            discard_panic_payload(secondary);
                        }
                        discard_secondary(result);
                        std::panic::resume_unwind(payload)
                    }
                    Ok(()) => match finalized {
                        Ok(()) => Ok(result),
                        Err(payload) => {
                            discard_secondary(result);
                            std::panic::resume_unwind(payload)
                        }
                    },
                }
            }
            Err(payload) => {
                std::mem::forget(f);
                if let Err(secondary) = finalized {
                    discard_panic_payload(secondary);
                }
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// The type is checked before anything is marked, so a write through a
    /// handle of the wrong `T` changes nothing and schedules nobody.
    fn write<T: 'static, R>(
        &self,
        slot: SignalSlot,
        mut f: impl FnMut(&mut T) -> R,
    ) -> Result<R, SignalError> {
        // Preparation can execute tracing subscribers while refusing a build
        // write. Keep the opaque updater outside that unwind too: destroying
        // its captures during a telemetry panic could otherwise abort before
        // the caller regains control.
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.refuse_if_building(slot)?;
            self.loan(slot)
        }));
        let mut loan = match prepared {
            Ok(result) => result?,
            Err(payload) => {
                std::mem::forget(f);
                std::panic::resume_unwind(payload)
            }
        };
        let typed = loan
            .value
            .as_deref_mut()
            .and_then(<dyn Any>::downcast_mut::<T>)
            .ok_or(SignalError::TypeMismatch {
                index: slot.index(),
                expected: type_name::<T>(),
            })?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(typed)));
        // `f` must remain outside the caught invocation: consuming a `FnOnce`
        // there would destroy its captures while the updater panic is still
        // unwinding, and a panicking capture destructor would abort the
        // process before `catch_unwind` can return. A panicking updater's
        // opaque bundle is retained; a successful updater is destroyed only
        // after loan restoration and invalidation are durable.
        let finalized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(loan)));
        let invalidated =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.mark(slot)));
        match outcome {
            Ok(result) => {
                if let Err(payload) = finalized {
                    std::mem::forget(f);
                    discard_secondary(result);
                    if let Err(secondary) = invalidated {
                        discard_panic_payload(secondary);
                    }
                    std::panic::resume_unwind(payload)
                }
                if let Err(payload) = invalidated {
                    // Invalidation is chronologically earlier than opaque
                    // callback teardown. Retain the capture bundle so a
                    // destructor panic cannot replace this payload or abort
                    // inside aggregate drop glue.
                    std::mem::forget(f);
                    discard_secondary(result);
                    std::panic::resume_unwind(payload)
                }
                // All graph protocol state is durable before user captures
                // are destroyed. Even uncontainable aggregate drop glue can
                // no longer bypass loan restoration or reader invalidation.
                let disposed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(f)));
                match disposed {
                    Ok(()) => Ok(result),
                    Err(payload) => {
                        discard_secondary(result);
                        std::panic::resume_unwind(payload)
                    }
                }
            }
            Err(payload) => {
                std::mem::forget(f);
                if let Err(secondary) = finalized {
                    discard_panic_payload(secondary);
                }
                if let Err(secondary) = invalidated {
                    discard_panic_payload(secondary);
                }
                std::panic::resume_unwind(payload)
            }
        }
    }
}

/// Pure reads: enough for [`Signal::peek`] against a `&Reactive`. The graph is
/// deliberately not a [`ReaderSink`], so holding it (through
/// `BuildContext::reactive()`) is not a way to subscribe an element.
impl ReadGraph for Reactive {
    fn graph_id(&self) -> u32 {
        self.id
    }

    fn read_erased(
        &self,
        slot: SignalSlot,
        read: &mut dyn FnMut(&dyn Any),
    ) -> Result<(), SignalError> {
        let loan = self.loan(slot)?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(value) = loan.value.as_deref() {
                read(value);
            }
        }));
        let finalized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(loan)));
        match outcome {
            Ok(()) => match finalized {
                Ok(()) => Ok(()),
                Err(payload) => std::panic::resume_unwind(payload),
            },
            Err(payload) => {
                if let Err(secondary) = finalized {
                    discard_panic_payload(secondary);
                }
                std::panic::resume_unwind(payload)
            }
        }
    }
}

fn discard_secondary<T>(value: T) {
    // `T` may be an aggregate whose second field panics while the first
    // field's destructor is already unwinding. No outer catch can contain
    // that generated drop glue, so exceptional-path retirement must leak it.
    std::mem::forget(value);
}

fn discard_panic_payload(payload: Box<dyn Any + Send>) {
    std::mem::forget(payload);
}

fn retain_first_panic(first: &mut Option<Box<dyn Any + Send>>, outcome: std::thread::Result<()>) {
    if let Err(payload) = outcome {
        if first.is_none() {
            *first = Some(payload);
        } else {
            discard_panic_payload(payload);
        }
    }
}

impl Reactive {
    /// `slot` changed: schedule its element readers through the owner's inbox
    /// (one frame request for the burst; the inbox dedups by element).
    fn mark(&self, slot: SignalSlot) {
        let (readers, scheduler) = {
            let inner = self.inner.borrow();
            let Ok(()) = self.check(&inner, slot) else {
                return;
            };
            (
                inner.nodes[slot.index() as usize].element_readers.clone(),
                inner.scheduler.clone(),
            )
        };
        if let Some(scheduler) = &scheduler {
            scheduler.schedule_many(readers.iter().copied(), RebuildReason::SignalChange);
        }
        tracing::debug!(
            target: "flui::signals",
            slot = ?slot,
            readers = readers.len(),
            scheduled = scheduler.is_some(),
            "signal written"
        );
    }
}

mod sealed {
    /// Only the signal handles of this graph take the write extension.
    pub trait Sealed {}
}

impl<T: 'static> sealed::Sealed for Signal<T> {}

/// The write side of a [`Signal`]: `set`, `update` and `set_if_changed`
/// against the graph that minted it (ADR-0085 §2), through a [`WriteTarget`]
/// (ADR-0086).
///
/// In an event callback the target is the `&mut EventCx<'_>` the callback
/// receives: `move |cx| count.update(cx, |n| *n += 1)`. Passing `cx` reborrows
/// it, so the same `cx` serves several writes. `build` has no write target;
/// [`Reactive`] still is one until ADR-0086 §8 step 3 removes it.
///
/// An extension trait because [`Signal`] lives in `flui-foundation`, which
/// cannot name [`Reactive`]. It is sealed: the handles of this graph are the
/// only implementors. It is in [`crate::prelude`]; without the prelude, import
/// `flui_view::SignalWriteExt`.
pub trait SignalWriteExt<T: 'static>: Copy + sealed::Sealed {
    /// Replace the value and mark every reader — equal or not.
    ///
    /// The replacement is committed before the retired value is destroyed.
    /// Reader invalidation still runs after a contained destructor failure,
    /// so a panicking `T::drop` cannot hide the committed value from existing
    /// readers. A retired value gets its own panic boundary; pending values
    /// that remain after an earlier panic are retained because opaque
    /// aggregate drop glue cannot be contained generically. The
    /// chronologically first panic keeps priority.
    ///
    /// # Errors
    ///
    /// [`SignalError::WrittenDuringBuild`] from inside a `build`, plus the
    /// handle errors of [`Signal::try_with`] and [`SignalError::Unbound`] for
    /// a [`Signal::default`] handle.
    fn set<W: WriteTarget + ?Sized>(self, w: &W, value: T) -> Result<(), SignalError>;

    /// Mutate in place and mark every reader.
    ///
    /// If `f` panics after changing the value, the partial change remains,
    /// every reader is invalidated, and the original panic resumes, unless
    /// `f` explicitly releases this same slot. Release is authoritative: it
    /// destroys the loaned value and reader set, so there is no surviving
    /// commit to invalidate. This method does not provide transactional
    /// rollback. The updater is accepted as [`FnMut`] even though it is
    /// invoked exactly once: retaining it across the protected invocation
    /// lets successful capture destruction happen separately, while a panic
    /// retains the opaque capture bundle instead of risking aggregate drop
    /// glue during recovery.
    ///
    /// # Errors
    ///
    /// As [`SignalWriteExt::set`].
    fn update<W: WriteTarget + ?Sized, R>(
        self,
        w: &W,
        f: impl FnMut(&mut T) -> R,
    ) -> Result<R, SignalError>;

    /// Replace the value only if it differs; an equal write marks nobody.
    /// Returns whether a write happened. A comparison panic commits nothing;
    /// the proposed value is then retained on that exceptional path so its
    /// destructor cannot replace or double-panic over the comparison failure.
    ///
    /// # Errors
    ///
    /// As [`SignalWriteExt::set`].
    fn set_if_changed<W: WriteTarget + ?Sized>(self, w: &W, value: T) -> Result<bool, SignalError>
    where
        T: PartialEq;
}

impl<T: 'static> SignalWriteExt<T> for Signal<T> {
    fn set<W: WriteTarget + ?Sized>(self, w: &W, value: T) -> Result<(), SignalError> {
        let graph = w.graph(writer::sealed::Token::new());
        let slot = self.slot();
        let mut pending = Some(value);

        // Keep the proposed value outside every fallible preparation step.
        // If tracing or graph access panics, its destructor is a later,
        // separately-contained phase rather than a second panic during the
        // preparation unwind.
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            graph.refuse_if_building(slot)?;
            graph.loan(slot)
        }));
        let mut loan = match prepared {
            Ok(Ok(loan)) => loan,
            Ok(Err(error)) => return Err(error),
            Err(payload) => {
                discard_secondary(pending.take());
                std::panic::resume_unwind(payload)
            }
        };
        let typed = loan
            .value
            .as_deref_mut()
            .and_then(<dyn Any>::downcast_mut::<T>)
            .ok_or(SignalError::TypeMismatch {
                index: slot.index(),
                expected: type_name::<T>(),
            })?;

        // `replace` commits without running either destructor. Returning the
        // loan, invalidating readers, and retiring the old value are distinct
        // phases. Protocol state is complete before opaque aggregate drop
        // glue runs: Rust cannot generically contain two field destructors
        // that panic while their aggregate is being destroyed.
        let retired = std::mem::replace(
            typed,
            pending
                .take()
                .expect("BUG: a signal replacement is consumed only once"),
        );
        let finalized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(loan)));
        let invalidated =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| graph.mark(slot)));

        let mut first = None;
        retain_first_panic(&mut first, finalized);
        if let Some(payload) = first {
            discard_secondary(retired);
            if let Err(secondary) = invalidated {
                discard_panic_payload(secondary);
            }
            std::panic::resume_unwind(payload);
        }
        if let Err(payload) = invalidated {
            discard_secondary(retired);
            std::panic::resume_unwind(payload)
        }
        let disposed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(retired)));
        if let Err(payload) = disposed {
            std::panic::resume_unwind(payload)
        }
        Ok(())
    }

    fn update<W: WriteTarget + ?Sized, R>(
        self,
        w: &W,
        f: impl FnMut(&mut T) -> R,
    ) -> Result<R, SignalError> {
        w.graph(writer::sealed::Token::new()).write(self.slot(), f)
    }

    fn set_if_changed<W: WriteTarget + ?Sized>(self, w: &W, value: T) -> Result<bool, SignalError>
    where
        T: PartialEq,
    {
        let r = w.graph(writer::sealed::Token::new());
        let mut pending = Some(value);
        let compared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            r.refuse_if_building(self.slot())?;
            r.read(self.slot(), |current: &T| {
                current
                    == pending
                        .as_ref()
                        .expect("BUG: comparison does not consume the proposed value")
            })
        }));
        let equal = match compared {
            Ok(result) => result?,
            Err(payload) => {
                discard_secondary(pending.take());
                std::panic::resume_unwind(payload)
            }
        };
        if equal {
            return Ok(false);
        }
        self.set(
            r,
            pending
                .take()
                .expect("BUG: a signal replacement is consumed only once"),
        )
        .map(|()| true)
    }
}

/// The sink that subscribes one element: minted by `make_build_ctx` for the
/// element about to build, and by `ElementBuildContext` for its own element.
/// Private to this crate, so no scope built elsewhere can subscribe an
/// element.
#[derive(Clone)]
pub(crate) struct ElementReads {
    graph: Reactive,
    element: ElementId,
}

impl ElementReads {
    pub(crate) fn new(graph: Reactive, element: ElementId) -> Self {
        Self { graph, element }
    }

    /// The graph the element's reads resolve against.
    pub(crate) fn graph(&self) -> &Reactive {
        &self.graph
    }
}

impl ReaderSink for ElementReads {
    fn subscribe(&self, slot: SignalSlot) {
        self.graph.register_element_reader(slot, self.element);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    // The graph handle `BuildContext::reactive()` returns cannot subscribe
    // anyone; the element sink this crate mints can.
    static_assertions::assert_not_impl_any!(Reactive: ReaderSink);
    static_assertions::assert_impl_all!(ElementReads: ReaderSink);

    struct DropBomb(&'static str);

    impl Drop for DropBomb {
        fn drop(&mut self) {
            std::panic::panic_any(self.0);
        }
    }

    struct ArmedValue {
        name: &'static str,
        drop_armed: Arc<std::sync::atomic::AtomicBool>,
        equality_panics: bool,
    }

    impl PartialEq for ArmedValue {
        fn eq(&self, other: &Self) -> bool {
            assert!(
                !(self.equality_panics || other.equality_panics),
                "comparison probe"
            );
            self.name == other.name
        }
    }

    impl Drop for ArmedValue {
        fn drop(&mut self) {
            if self
                .drop_armed
                .swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                std::panic::panic_any(self.name);
            }
        }
    }

    fn graph_with_inbox() -> (Reactive, Arc<ExternalBuildInbox>) {
        let inbox = Arc::new(ExternalBuildInbox::default());
        let reactive = Reactive::new();
        reactive.set_scheduler(ExternalBuildScheduler::from_parts(Arc::clone(&inbox), None));
        (reactive, inbox)
    }

    fn scheduled(inbox: &ExternalBuildInbox) -> Vec<ElementId> {
        let mut ids: Vec<_> = inbox.lock().keys().copied().collect();
        ids.sort();
        ids
    }

    fn write_schedules_exactly_the_registered_readers() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(1u32);
        let b = r.signal(2u32);
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);
        r.register_element_reader(a.slot(), e1);
        r.register_element_reader(b.slot(), e2);

        a.set(&r, 10).unwrap();

        assert_eq!(scheduled(&inbox), vec![e1]);
        assert!(inbox.lock()[&e1].contains(RebuildReason::SignalChange));
        assert_eq!(a.peek(&r, |v| *v).unwrap(), 10);
    }

    fn a_build_that_unwinds_keeps_its_previous_read_set() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(0u8);
        let e1 = ElementId::new(1);
        r.begin_element_build(e1);
        r.register_element_reader(a.slot(), e1);
        r.end_element_build(e1, true);

        // The next build panics before reading `a`.
        r.begin_element_build(e1);
        r.end_element_build(e1, false);

        assert_eq!(r.readers_of(a.slot()), vec![e1], "still subscribed");
        a.set(&r, 1).unwrap();
        assert_eq!(scheduled(&inbox).len(), 1, "the write still rebuilds it");
    }

    fn writes_and_creations_during_build_are_refused() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(0u8);
        let e1 = ElementId::new(1);
        r.begin_element_build(e1);
        assert_eq!(
            a.set(&r, 1),
            Err(SignalError::WrittenDuringBuild { element: e1 })
        );
        assert_eq!(
            r.try_signal(7u8).map(|s| s.slot().index()),
            Err(SignalError::CreatedDuringBuild { element: e1 })
        );
        assert_eq!(a.peek(&r, |v| *v), Ok(0), "reads stay legal during build");
        r.end_element_build(e1, true);
        assert_eq!(a.set(&r, 1), Ok(()));
        assert!(r.try_signal(7u8).is_ok());
    }

    fn unmounting_an_element_releases_what_it_owned_and_its_reads() {
        let (r, inbox) = graph_with_inbox();
        let e1 = ElementId::new(1);
        let shared = r.signal(0u8);
        let owned = r.signal_owned_by(e1, 0u8);
        r.register_element_reader(shared.slot(), e1);

        r.release_element(e1);

        assert!(matches!(
            owned.peek(&r, |v| *v),
            Err(SignalError::Released { .. })
        ));
        shared.set(&r, 1).unwrap();
        assert_eq!(scheduled(&inbox), [] as [ElementId; 0]);

        let fresh = r.signal(9u8);
        assert_eq!(
            fresh.slot().index(),
            owned.slot().index(),
            "the freed slot is reused"
        );
        assert_ne!(fresh.slot().generation(), owned.slot().generation());
        assert!(owned.peek(&r, |v| *v).is_err(), "the old handle stays dead");
    }

    #[test]
    fn reactive_graph_contract_matrix() {
        crate::table_test::run_table(
            "reactive_graph_contract_matrix",
            &[
                (
                    "write_schedules_exactly_the_registered_readers",
                    write_schedules_exactly_the_registered_readers as fn(),
                ),
                (
                    "writes_and_creations_during_build_are_refused",
                    writes_and_creations_during_build_are_refused as fn(),
                ),
                (
                    "unmounting_an_element_releases_what_it_owned_and_its_reads",
                    unmounting_an_element_releases_what_it_owned_and_its_reads as fn(),
                ),
            ],
        );
    }

    fn releasing_a_slot_from_its_panicking_update_remains_authoritative() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(String::from("alive"));
        r.register_element_reader(a.slot(), ElementId::new(1));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = a.update(&r, |value| {
                value.push_str(" but loaned");
                r.release(a.slot());
                panic!("release wins");
            });
        }));

        let payload = outcome.expect_err("the updater panic must resume");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"release wins"));
        assert!(matches!(
            a.peek(&r, String::len),
            Err(SignalError::Released { .. })
        ));
        assert!(
            scheduled(&inbox).is_empty(),
            "a released slot has no surviving value or readers to invalidate"
        );
    }

    fn reader_panic_keeps_priority_over_a_released_values_destructor_panic() {
        let (r, _) = graph_with_inbox();
        let signal = r.signal(DropBomb("value destructor probe"));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = signal.peek(&r, |_| {
                r.release(signal.slot());
                panic!("reader probe");
            });
        }));

        let payload = outcome.expect_err("the reader panic must resume");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"reader probe"),
            "the released value's destructor panic must remain secondary"
        );
    }

    fn a_panicking_update_returns_the_loaned_value_and_marks_its_readers() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(3u32);
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);
        r.register_element_reader(a.slot(), e1);
        r.register_element_reader(a.slot(), e2);

        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            a.peek(&r, |_| panic!("reader panics"))
        }));
        assert!(read.is_err());
        assert_eq!(
            a.peek(&r, |v| *v),
            Ok(3),
            "the value came back after the read panic"
        );

        let write = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            a.update(&r, |v| {
                *v = 99;
                panic!("writer panics");
            })
        }));
        let payload = write.expect_err("writer panic must resume after invalidation");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"writer panics"));
        assert_eq!(
            a.peek(&r, |v| *v),
            Ok(99),
            "the (partially) written value came back; the slot is usable"
        );
        assert_eq!(
            scheduled(&inbox),
            vec![e1, e2],
            "a partially committed value must not stay invisible to its readers"
        );
        a.set(&r, 5).unwrap();
        assert_eq!(
            scheduled(&inbox),
            vec![e1, e2],
            "and the slot still schedules afterwards"
        );
    }

    fn updater_panic_keeps_priority_over_a_secondary_wake_panic() {
        let inbox = Arc::new(ExternalBuildInbox::default());
        let r = Reactive::new();
        r.set_scheduler(ExternalBuildScheduler::from_parts(
            Arc::clone(&inbox),
            Some(Arc::new(|| panic!("wake probe"))),
        ));
        let signal = r.signal(0u8);
        let readers = [ElementId::new(1), ElementId::new(2)];
        for reader in readers {
            r.register_element_reader(signal.slot(), reader);
        }

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal.update(&r, |value| {
                *value = 1;
                panic!("updater probe");
            })
        }));

        let payload = outcome.expect_err("the updater panic must resume");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"updater probe"));
        assert_eq!(scheduled(&inbox), readers);
        assert_eq!(signal.peek(&r, |value| *value), Ok(1));
    }

    fn updater_panic_keeps_priority_over_its_captures_destructor_panic() {
        let (r, inbox) = graph_with_inbox();
        let signal = r.signal(0u8);
        let reader = ElementId::new(1);
        r.register_element_reader(signal.slot(), reader);
        let first_capture = DropBomb("first updater capture destructor probe");
        let second_capture = DropBomb("second updater capture destructor probe");

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal.update(&r, move |value| {
                let _capture_bundle_stays_owned_by_the_updater = (&first_capture, &second_capture);
                *value = 1;
                panic!("updater probe");
            })
        }));

        let payload = outcome.expect_err("the updater panic must resume");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"updater probe"),
            "the capture destructor panic must remain secondary"
        );
        assert_eq!(signal.peek(&r, |value| *value), Ok(1));
        assert_eq!(
            scheduled(&inbox),
            vec![reader],
            "the partially committed update must still invalidate its reader"
        );
        signal
            .set(&r, 2)
            .expect("the signal remains usable after both contained panics");
        assert_eq!(signal.peek(&r, |value| *value), Ok(2));
    }

    fn invalidation_panic_retains_a_successful_updaters_capture_bundle() {
        let inbox = Arc::new(ExternalBuildInbox::default());
        let r = Reactive::new();
        r.set_scheduler(ExternalBuildScheduler::from_parts(
            Arc::clone(&inbox),
            Some(Arc::new(|| panic!("wake probe"))),
        ));
        let signal = r.signal(0u8);
        let reader = ElementId::new(1);
        r.register_element_reader(signal.slot(), reader);
        let capture_armed = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let capture = ArmedValue {
            name: "successful updater capture destructor probe",
            drop_armed: Arc::clone(&capture_armed),
            equality_panics: false,
        };

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal.update(&r, move |value| {
                let _capture_stays_owned_by_the_updater = &capture;
                *value = 1;
            })
        }));

        let payload = outcome.expect_err("the invalidation panic must resume");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"wake probe"));
        assert!(
            capture_armed.load(std::sync::atomic::Ordering::Relaxed),
            "opaque captures are retained once invalidation has failed"
        );
        assert_eq!(signal.peek(&r, |value| *value), Ok(1));
        assert_eq!(scheduled(&inbox), vec![reader]);
        capture_armed.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn reactive_unwind_matrix() {
        crate::table_test::run_table(
            "reactive_unwind_matrix",
            &[
                (
                    "a_build_that_unwinds_keeps_its_previous_read_set",
                    a_build_that_unwinds_keeps_its_previous_read_set as fn(),
                ),
                (
                    "releasing_a_slot_from_its_panicking_update_remains_authoritative",
                    releasing_a_slot_from_its_panicking_update_remains_authoritative as fn(),
                ),
                (
                    "reader_panic_keeps_priority_over_a_released_values_destructor_panic",
                    reader_panic_keeps_priority_over_a_released_values_destructor_panic as fn(),
                ),
                (
                    "a_panicking_update_returns_the_loaned_value_and_marks_its_readers",
                    a_panicking_update_returns_the_loaned_value_and_marks_its_readers as fn(),
                ),
                (
                    "updater_panic_keeps_priority_over_a_secondary_wake_panic",
                    updater_panic_keeps_priority_over_a_secondary_wake_panic as fn(),
                ),
                (
                    "updater_panic_keeps_priority_over_its_captures_destructor_panic",
                    updater_panic_keeps_priority_over_its_captures_destructor_panic as fn(),
                ),
                (
                    "invalidation_panic_retains_a_successful_updaters_capture_bundle",
                    invalidation_panic_retains_a_successful_updaters_capture_bundle as fn(),
                ),
            ],
        );
    }
}
