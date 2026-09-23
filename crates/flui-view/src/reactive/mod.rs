//! Realm-scoped signals — ADR-0074 (feature `signals`).
//!
//! One [`Reactive`] graph per `BuildOwner` (so per realm): an arena of
//! generational slots holding [`Signal`] values, plus the **reader registry**
//! that makes the whole thing worth having:
//!
//! - reading a signal inside `build` (through [`Signal::get`] / [`Signal::with`]
//!   / their `try_` forms, which take the `&dyn BuildContext`) records the
//!   building element as a reader of that slot — the same class of edge as
//!   `depend_on` for an inherited provider, re-derived from scratch on every
//!   build of that element (a build that no longer reads a signal stops
//!   depending on it);
//! - writing a signal ([`Signal::set`] / [`Signal::update`]) schedules exactly
//!   the reader elements on the owner's existing external inbox
//!   ([`RebuildReason::SignalChange`]).
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
//! [`Signal::set_if_changed`] compares, and says so in its bounds.
//!
//! # Refusals at run time
//!
//! The static half is refusal trigger 24 (`scripts/check-signal-write-scope.sh`;
//! advisory, because a textual scanner cannot tell a synchronously invoked
//! closure from a deferred one). The binding half is here: while an element's
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
//! # Threading
//!
//! Everything here is `!Send + !Sync` — realm-affine like the element tree. A
//! handle carries the id of the graph that minted it, so a handle from realm
//! A used against realm B is [`SignalError::ForeignGraph`], not a silent read
//! of someone else's slot. A cross-thread write is a realm command executed on
//! the owner thread (`UiCommand::SignalWrite` in `flui-app`, carrying a
//! [`SignalSender`]), never a shared cell.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_foundation::{ElementId, RebuildReason};
use smallvec::SmallVec;

use crate::owner::ExternalBuildScheduler;

/// Process-wide counter that gives every [`Reactive`] graph a distinct id, so
/// a [`SignalSlot`] is meaningful only against the graph that minted it.
static NEXT_GRAPH_ID: AtomicU32 = AtomicU32::new(1);

/// Graph id + index + generation of a slot in a [`Reactive`] arena. `Copy`,
/// `'static`; every operation re-checks all three, so a stale or foreign
/// handle is an error, never a read of another value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignalSlot {
    graph: u32,
    index: u32,
    generation: u32,
}

/// Why a signal operation could not be carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignalError {
    /// The slot was released (its owning element unmounted) and possibly
    /// reused; the handle's generation no longer matches.
    #[error("signal slot {index} generation {generation} was released")]
    Released {
        /// Arena index of the stale handle.
        index: u32,
        /// Generation the stale handle carries.
        generation: u32,
    },
    /// The handle was minted by another realm's graph.
    #[error("signal slot {index} belongs to graph {graph}, not to this one ({this})")]
    ForeignGraph {
        /// Arena index of the handle.
        index: u32,
        /// The graph that minted it.
        graph: u32,
        /// The graph the operation ran against.
        this: u32,
    },
    /// A write was attempted while an element was building (ADR-0074 §5.2):
    /// it would re-mark readers of the frame that is still building.
    #[error("signal written during the build of {element:?}")]
    WrittenDuringBuild {
        /// The element whose build was running.
        element: ElementId,
    },
    /// A slot was created while an element was building: one slot per
    /// rebuild is a leak. Create in `init_state` and hold the handle.
    #[error("signal created during the build of {element:?}")]
    CreatedDuringBuild {
        /// The element whose build was running.
        element: ElementId,
    },
    /// The slot is being read or written by an enclosing closure on this
    /// same slot (`a.with(.., |_| a.set(..))`); its value is out on loan.
    #[error("signal slot {index} accessed re-entrantly from its own read/write closure")]
    Reentrant {
        /// Arena index of the slot.
        index: u32,
    },
}

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
    /// is scheduled until [`Reactive::set_scheduler`] is called (the
    /// `BuildOwner` does that at construction and again when its
    /// frame-request callback changes).
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: NEXT_GRAPH_ID.fetch_add(1, Ordering::Relaxed),
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
        let slot = SignalSlot {
            graph: self.id,
            index,
            generation: inner.nodes[index as usize].generation,
        };
        if let Some(owner) = owner {
            inner.owned_by_element.entry(owner).or_default().push(slot);
        }
        tracing::trace!(target: "flui::signals", slot = ?slot, owner = ?owner, "signal created");
        Ok(slot)
    }

    fn check(&self, inner: &Inner, slot: SignalSlot) -> Result<(), SignalError> {
        if slot.graph != self.id {
            return Err(SignalError::ForeignGraph {
                index: slot.index,
                graph: slot.graph,
                this: self.id,
            });
        }
        match inner.nodes.get(slot.index as usize) {
            Some(node) if node.live && node.generation == slot.generation => Ok(()),
            _ => Err(SignalError::Released {
                index: slot.index,
                generation: slot.generation,
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
    /// is already stale.
    pub fn release(&self, slot: SignalSlot) {
        let mut inner = self.inner.borrow_mut();
        if self.check(&inner, slot).is_err() {
            return;
        }
        Self::release_index(&mut inner, slot.index);
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
        let node = &mut inner.nodes[slot.index as usize];
        if !node.element_readers.contains(&element) {
            node.element_readers.push(element);
        }
        let reads = inner.element_reads.entry(element).or_default();
        if !reads.contains(&slot.index) {
            reads.push(slot.index);
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
                    Self::release_index(&mut inner, slot.index);
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
            Ok(()) => inner.nodes[slot.index as usize].element_readers.to_vec(),
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
                    owners.insert(slot.index, *element);
                }
            }
        }
        inner
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.live)
            .map(|(index, node)| SlotInfo {
                slot: SignalSlot {
                    graph: self.id,
                    index: index as u32,
                    generation: node.generation,
                },
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
        let value = inner.nodes[slot.index as usize]
            .value
            .take()
            .ok_or(SignalError::Reentrant { index: slot.index })?;
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
            inner.nodes[slot.index as usize].value = Some(value);
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
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, SignalError> {
        let loan = self.loan(slot)?;
        let typed = loan
            .value
            .as_deref()
            .and_then(<dyn Any>::downcast_ref::<T>)
            .expect("BUG: a live slot of this graph holds a value of another type");
        let result = f(typed);
        drop(loan);
        Ok(result)
    }

    fn write<T: 'static, R>(
        &self,
        slot: SignalSlot,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, SignalError> {
        self.refuse_if_building(slot)?;
        let mut loan = self.loan(slot)?;
        let typed = loan
            .value
            .as_deref_mut()
            .and_then(<dyn Any>::downcast_mut::<T>)
            .expect("BUG: a live slot of this graph holds a value of another type");
        let result = f(typed);
        drop(loan);
        self.mark(slot);
        Ok(result)
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
                inner.nodes[slot.index as usize].element_readers.clone(),
                inner.scheduler.clone(),
            )
        };
        tracing::debug!(
            target: "flui::signals",
            slot = ?slot,
            readers = readers.len(),
            scheduled = scheduler.is_some(),
            "signal written"
        );
        if let Some(scheduler) = scheduler {
            for element in readers {
                scheduler.schedule(element, RebuildReason::SignalChange);
            }
        }
    }
}

/// A `Copy` handle to a value in a [`Reactive`] arena.
pub struct Signal<T: 'static> {
    slot: SignalSlot,
    _t: PhantomData<fn() -> T>,
    /// Realm-affine: a handle is only meaningful on the thread that owns its
    /// [`Reactive`], so it is `!Send + !Sync` like the graph itself.
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

impl<T: 'static> Signal<T> {
    fn from_slot(slot: SignalSlot) -> Self {
        Self {
            slot,
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// The arena slot behind this handle.
    #[must_use]
    pub fn slot(self) -> SignalSlot {
        self.slot
    }

    /// The `Send + Sync` form of this handle, for a closure that will run on
    /// the owner thread later (`UiCommand::SignalWrite`).
    #[must_use]
    pub fn detach(self) -> SignalSender<T> {
        SignalSender {
            slot: self.slot,
            _t: PhantomData,
        }
    }

    /// Read during `build`: the building element becomes a reader.
    ///
    /// # Errors
    ///
    /// [`SignalError::Released`] for a stale handle (its owning element
    /// unmounted — a sliver band evicted, a route popped),
    /// [`SignalError::ForeignGraph`] for another realm's handle,
    /// [`SignalError::Reentrant`] from inside this slot's own closure.
    pub fn try_with<R>(
        self,
        cx: &dyn crate::BuildContext,
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, SignalError> {
        let graph = cx.reactive();
        let result = graph.read(self.slot, f)?;
        cx.signal_read(self.slot);
        Ok(result)
    }

    /// [`Signal::try_with`], cloning the value.
    ///
    /// # Errors
    ///
    /// As [`Signal::try_with`].
    pub fn try_get(self, cx: &dyn crate::BuildContext) -> Result<T, SignalError>
    where
        T: Clone,
    {
        self.try_with(cx, T::clone)
    }

    /// Borrowed read during `build`; the building element becomes a reader.
    ///
    /// # Panics
    ///
    /// On a stale handle (its owning element unmounted), a handle from another
    /// realm, or a re-entrant read of this same slot — see
    /// [`Signal::try_with`] for the non-panicking form.
    pub fn with<R>(self, cx: &dyn crate::BuildContext, f: impl FnOnce(&T) -> R) -> R {
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
    pub fn get(self, cx: &dyn crate::BuildContext) -> T
    where
        T: Clone,
    {
        self.with(cx, T::clone)
    }

    /// Read without registering anything (callbacks, tests, realm commands).
    ///
    /// # Errors
    ///
    /// As [`Signal::try_with`].
    pub fn peek<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError> {
        r.read(self.slot, f)
    }

    /// Replace the value and mark every reader — equal or not.
    ///
    /// # Errors
    ///
    /// [`SignalError::WrittenDuringBuild`] from inside a `build`, plus the
    /// handle errors of [`Signal::try_with`].
    pub fn set(self, r: &Reactive, value: T) -> Result<(), SignalError> {
        r.write(self.slot, |slot: &mut T| *slot = value)
    }

    /// Mutate in place and mark every reader.
    ///
    /// # Errors
    ///
    /// As [`Signal::set`].
    pub fn update<R>(self, r: &Reactive, f: impl FnOnce(&mut T) -> R) -> Result<R, SignalError> {
        r.write(self.slot, f)
    }

    /// Replace the value only if it differs; an equal write marks nobody.
    /// Returns whether a write happened.
    ///
    /// # Errors
    ///
    /// As [`Signal::set`].
    pub fn set_if_changed(self, r: &Reactive, value: T) -> Result<bool, SignalError>
    where
        T: PartialEq,
    {
        r.refuse_if_building(self.slot)?;
        if r.read(self.slot, |current: &T| *current == value)? {
            return Ok(false);
        }
        self.set(r, value).map(|()| true)
    }
}

/// The `Send + Sync` form of a [`Signal`] handle for crossing a thread
/// boundary: it carries only the slot, and can do nothing until it is
/// re-attached on the owner thread (inside a `UiCommand::SignalWrite`
/// closure, ADR-0074 §5.8), where [`SignalSender::attach`] hands back the
/// realm-affine [`Signal`].
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
    pub fn attach(self) -> Signal<T> {
        Signal::from_slot(self.slot)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use flui_foundation::RebuildReasons;
    use parking_lot::Mutex;

    use super::*;

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

    #[test]
    fn equal_write_still_marks_and_set_if_changed_does_not() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(5u32);
        let e1 = ElementId::new(1);
        r.register_element_reader(a.slot(), e1);

        assert!(!a.set_if_changed(&r, 5).unwrap());
        assert!(scheduled(&inbox).is_empty());

        a.set(&r, 5).unwrap();
        assert_eq!(scheduled(&inbox), vec![e1]);
    }

    #[test]
    fn a_rebuild_re_derives_the_read_set_from_scratch() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(0u8);
        let e1 = ElementId::new(1);
        r.begin_element_build(e1);
        r.register_element_reader(a.slot(), e1);
        r.end_element_build(e1, true);

        r.begin_element_build(e1);
        r.end_element_build(e1, true);

        a.set(&r, 1).unwrap();
        assert!(scheduled(&inbox).is_empty());
        assert!(r.readers_of(a.slot()).is_empty());
    }

    #[test]
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

    #[test]
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
            r.try_signal(7u8).map(|s| s.slot().index),
            Err(SignalError::CreatedDuringBuild { element: e1 })
        );
        assert_eq!(a.peek(&r, |v| *v), Ok(0), "reads stay legal during build");
        r.end_element_build(e1, true);
        assert_eq!(a.set(&r, 1), Ok(()));
        assert!(r.try_signal(7u8).is_ok());
    }

    #[test]
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
        assert!(scheduled(&inbox).is_empty());

        let fresh = r.signal(9u8);
        assert_eq!(
            fresh.slot().index,
            owned.slot().index,
            "the freed slot is reused"
        );
        assert_ne!(fresh.slot().generation, owned.slot().generation);
        assert!(owned.peek(&r, |v| *v).is_err(), "the old handle stays dead");
    }

    #[test]
    fn an_element_unmount_never_releases_a_slot_another_owner_reused() {
        // E owns a slot, releases it early, F reuses the index with a new
        // generation, then E unmounts: F's slot must survive.
        let (r, _) = graph_with_inbox();
        let e = ElementId::new(1);
        let f = ElementId::new(2);
        let e_slot = r.signal_owned_by(e, 1u8);
        r.release(e_slot.slot());
        let f_slot = r.signal_owned_by(f, 2u8);
        assert_eq!(
            f_slot.slot().index,
            e_slot.slot().index,
            "test setup: index reused"
        );

        r.release_element(e);

        assert_eq!(f_slot.peek(&r, |v| *v), Ok(2), "F's live slot is untouched");
        assert!(matches!(
            e_slot.peek(&r, |v| *v),
            Err(SignalError::Released { .. })
        ));
    }

    #[test]
    fn nested_access_to_other_slots_is_fine_and_to_the_same_slot_is_reentrant() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(1u32);
        let b = r.signal(2u32);
        let sum = a.peek(&r, |x| *x + b.peek(&r, |y| *y).unwrap()).unwrap();
        assert_eq!(sum, 3);
        a.peek(&r, |x| b.set(&r, *x * 10).unwrap()).unwrap();
        assert_eq!(b.peek(&r, |v| *v), Ok(10));
        a.update(&r, |x| {
            *x += b
                .update(&r, |y| {
                    *y += 1;
                    *y
                })
                .unwrap();
        })
        .unwrap();
        assert_eq!(a.peek(&r, |v| *v), Ok(12));

        let inner = a.peek(&r, |_| a.peek(&r, |v| *v)).unwrap();
        assert_eq!(
            inner,
            Err(SignalError::Reentrant {
                index: a.slot().index
            })
        );
        let inner = a.update(&r, |_| a.set(&r, 0)).unwrap();
        assert_eq!(
            inner,
            Err(SignalError::Reentrant {
                index: a.slot().index
            })
        );
        assert_eq!(
            a.peek(&r, |v| *v),
            Ok(12),
            "the loaned value came back intact"
        );
    }

    #[test]
    fn releasing_a_slot_from_inside_its_own_read_closure_drops_the_value_afterwards() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(String::from("alive"));
        let seen = a.peek(&r, |v| {
            r.release(a.slot());
            v.clone()
        });
        assert_eq!(seen.as_deref(), Ok("alive"));
        assert!(matches!(
            a.peek(&r, String::len),
            Err(SignalError::Released { .. })
        ));
        assert_eq!(r.live_slot_count(), 0);
    }

    #[test]
    fn a_handle_from_another_graph_is_refused_not_read() {
        let (a_graph, _) = graph_with_inbox();
        let (b_graph, _) = graph_with_inbox();
        let a = a_graph.signal(1u32);
        let _b = b_graph.signal(2u32); // same index 0, same generation 0
        assert!(matches!(
            a.peek(&b_graph, |v| *v),
            Err(SignalError::ForeignGraph { .. })
        ));
        assert!(matches!(
            a.set(&b_graph, 5),
            Err(SignalError::ForeignGraph { .. })
        ));
        assert_eq!(a.peek(&a_graph, |v| *v), Ok(1));
        assert_eq!(
            a.detach().attach(),
            a,
            "detach/attach preserves the full identity"
        );
    }

    #[test]
    fn snapshot_lists_live_slots_with_readers_and_owners() {
        let (r, _) = graph_with_inbox();
        let e1 = ElementId::new(1);
        let owned = r.signal_owned_by(e1, 0u8);
        let free = r.signal(0u8);
        r.register_element_reader(free.slot(), e1);
        let mut info = r.snapshot();
        info.sort_by_key(|i| i.slot.index);
        assert_eq!(info.len(), 2);
        assert_eq!(info[0].slot, owned.slot());
        assert_eq!(info[0].owner, Some(e1));
        assert_eq!(info[1].readers, vec![e1]);
        assert_eq!(info[1].owner, None);
    }

    #[test]
    fn a_panicking_closure_returns_the_loaned_value_and_marks_nobody() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(3u32);
        let e1 = ElementId::new(1);
        r.register_element_reader(a.slot(), e1);

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
        assert!(write.is_err());
        assert_eq!(
            a.peek(&r, |v| *v),
            Ok(99),
            "the (partially) written value came back; the slot is usable"
        );
        assert!(
            scheduled(&inbox).is_empty(),
            "a panicking write marks nobody"
        );
        a.set(&r, 5).unwrap();
        assert_eq!(
            scheduled(&inbox),
            vec![e1],
            "and the slot still schedules afterwards"
        );
    }

    #[test]
    fn an_equal_set_if_changed_inside_build_is_still_refused() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(1u32);
        let e1 = ElementId::new(1);
        r.begin_element_build(e1);
        assert_eq!(
            a.set_if_changed(&r, 1),
            Err(SignalError::WrittenDuringBuild { element: e1 })
        );
        r.end_element_build(e1, true);
        assert_eq!(a.set_if_changed(&r, 1), Ok(false));
    }

    #[test]
    fn no_partial_eq_bound_on_plain_signals() {
        struct Opaque(#[allow(dead_code)] Vec<u8>);
        let (r, _) = graph_with_inbox();
        let s = r.signal(Opaque(vec![1]));
        s.update(&r, |o| o.0.push(2)).unwrap();
        assert_eq!(s.peek(&r, |o| o.0.len()).unwrap(), 2);
    }
}
