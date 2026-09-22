//! Realm-scoped signals — ADR-0074 phase-2 prototype (feature `signals`).
//!
//! One [`Reactive`] graph per `BuildOwner` (so per realm): an arena of
//! generational slots holding [`Signal`] values, [`Computed`] computations and
//! [`Effect`] closures, plus the **reader registry** that makes the whole thing
//! worth having:
//!
//! - reading a signal inside `build` (through [`Signal::get`] / [`Signal::with`],
//!   which take the `&dyn BuildContext`) records the building element as a
//!   reader of that slot — the same class of edge as `depend_on` for an
//!   inherited provider, and re-derived from scratch on every build of that
//!   element (a build that no longer reads a signal stops depending on it);
//! - writing a signal ([`Signal::set`] / [`Signal::update`]) schedules exactly
//!   the reader elements on the owner's existing external inbox
//!   ([`RebuildReason::SignalChange`]), re-checks dependent computed values and marks
//!   their readers only if the memo's output changed, and queues dependent
//!   effects for the next effects phase.
//!
//! Nothing here touches the element tree: the only side effect on it is the
//! same `ExternalBuildScheduler::schedule` call a `RebuildHandle` makes, so the
//! depth-ordered drain, the mid-drain absorb budget and the `catch_unwind`
//! around `build` all apply unchanged.
//!
//! Equality is opt-in, never implied: a plain `set` marks readers even when the
//! value is equal (there is no `PartialEq` bound on `T`); only
//! [`Signal::set_if_changed`] and [`Computed`] compare, and both say so in their
//! bounds.
//!
//! Threading: everything in this module is `!Send + !Sync` — realm-affine like
//! the element tree. A cross-thread write is a realm command executed on the
//! owner thread (`UiCommand::SignalWrite` in `flui-app`), never a shared cell.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use flui_foundation::{ElementId, RebuildReason};
use smallvec::SmallVec;

use crate::owner::ExternalBuildScheduler;

/// Index + generation of a slot in a [`Reactive`] arena. `Copy`, `'static`,
/// and meaningless outside the graph that minted it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignalSlot {
    index: u32,
    generation: u32,
}

/// Why a signal operation could not be carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignalError {
    /// The slot was released (its owning element unmounted, or the effect was
    /// dropped) and possibly reused; the handle's generation no longer matches.
    #[error("signal slot {index} generation {generation} was released")]
    Released {
        /// Arena index of the stale handle.
        index: u32,
        /// Generation the stale handle carries.
        generation: u32,
    },
    /// A write was attempted while an element was building. Writes from
    /// `build` are refused (ADR-0074 §5.2): they are the unbounded-loop hazard.
    #[error("signal written during the build of {element:?}")]
    WrittenDuringBuild {
        /// The element whose build was running.
        element: ElementId,
    },
}

type ComputeFn = Rc<dyn Fn(&Reactive) -> Box<dyn Any>>;
type EqFn = Rc<dyn Fn(&dyn Any, &dyn Any) -> bool>;
type EffectFn = Rc<RefCell<dyn FnMut(&Reactive)>>;

enum Kind {
    Signal,
    Computed {
        compute: ComputeFn,
        eq: EqFn,
        stale: bool,
    },
    Effect {
        run: EffectFn,
        pending: bool,
    },
}

struct Node {
    generation: u32,
    live: bool,
    /// The current value for a signal or memo; `None` for an effect, a freed
    /// slot, or a computed that has never been computed.
    value: Option<Box<dyn Any>>,
    kind: Kind,
    /// Elements that read this slot during their last build.
    element_readers: SmallVec<[ElementId; 4]>,
    /// Computed/effect nodes that read this slot during their last run.
    computed_readers: SmallVec<[u32; 2]>,
    /// Slots this memo/effect read during its last run (to clear on re-run).
    sources: SmallVec<[u32; 4]>,
}

#[derive(Default)]
struct Inner {
    nodes: Vec<Node>,
    free: Vec<u32>,
    /// Slots each element read during its last build.
    element_reads: HashMap<ElementId, SmallVec<[u32; 4]>>,
    /// Slots created on behalf of an element, released when it unmounts.
    owned_by_element: HashMap<ElementId, SmallVec<[u32; 2]>>,
    /// Computed/effect nodes currently computing, innermost last.
    tracking: Vec<u32>,
    /// The element whose `build` is running, if any.
    building: Option<ElementId>,
    scheduler: Option<ExternalBuildScheduler>,
    pending_effects: Vec<u32>,
}

/// The reactive graph of one `BuildOwner` (one realm). Cheap to clone (an
/// `Rc`); `!Send + !Sync` by construction.
#[derive(Clone, Default)]
pub struct Reactive {
    inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for Reactive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("Reactive")
            .field("slots", &inner.nodes.len())
            .field("free", &inner.free.len())
            .field("pending_effects", &inner.pending_effects.len())
            .finish()
    }
}

impl Reactive {
    /// A graph with no scheduler: writes still update values and re-check computed values,
    /// but no element is scheduled until [`Reactive::set_scheduler`] is called.
    pub fn new() -> Self {
        Self::default()
    }

    /// Route reader scheduling through `scheduler` (the owner's external
    /// inbox). The `BuildOwner` refreshes this whenever its frame-request
    /// callback changes.
    pub(crate) fn set_scheduler(&self, scheduler: ExternalBuildScheduler) {
        self.inner.borrow_mut().scheduler = Some(scheduler);
    }

    // ------------------------------------------------------------------ slots

    fn alloc(
        &self,
        value: Option<Box<dyn Any>>,
        kind: Kind,
        owner: Option<ElementId>,
    ) -> SignalSlot {
        let mut inner = self.inner.borrow_mut();
        let index = if let Some(index) = inner.free.pop() {
            let node = &mut inner.nodes[index as usize];
            node.generation = node.generation.wrapping_add(1);
            node.live = true;
            node.value = value;
            node.kind = kind;
            node.element_readers.clear();
            node.computed_readers.clear();
            node.sources.clear();
            index
        } else {
            inner.nodes.push(Node {
                generation: 0,
                live: true,
                value,
                kind,
                element_readers: SmallVec::new(),
                computed_readers: SmallVec::new(),
                sources: SmallVec::new(),
            });
            (inner.nodes.len() - 1) as u32
        };
        let generation = inner.nodes[index as usize].generation;
        if let Some(owner) = owner {
            inner.owned_by_element.entry(owner).or_default().push(index);
        }
        SignalSlot { index, generation }
    }

    fn check(inner: &Inner, slot: SignalSlot) -> Result<(), SignalError> {
        match inner.nodes.get(slot.index as usize) {
            Some(node) if node.live && node.generation == slot.generation => Ok(()),
            _ => Err(SignalError::Released {
                index: slot.index,
                generation: slot.generation,
            }),
        }
    }

    /// Create a signal owned by the graph itself (released with the realm).
    pub fn signal<T: 'static>(&self, value: T) -> Signal<T> {
        Signal {
            slot: self.alloc(Some(Box::new(value)), Kind::Signal, None),
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// Create a signal released when `owner` unmounts. This is what
    /// `BuildContext::signal` uses from `init_state`.
    pub fn signal_owned_by<T: 'static>(&self, owner: ElementId, value: T) -> Signal<T> {
        Signal {
            slot: self.alloc(Some(Box::new(value)), Kind::Signal, Some(owner)),
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// Create a computed. `compute` reads its sources with [`Signal::track`] /
    /// [`Computed::track`]; the memo re-checks when any of them is written and
    /// notifies its own readers only when the output differs (`PartialEq`).
    /// Lazy: nothing runs until the first read.
    pub fn computed<T: PartialEq + 'static>(
        &self,
        compute: impl Fn(&Reactive) -> T + 'static,
    ) -> Computed<T> {
        self.computed_owned_by_opt(None, compute)
    }

    /// [`Reactive::computed`] released when `owner` unmounts.
    pub fn computed_owned_by<T: PartialEq + 'static>(
        &self,
        owner: ElementId,
        compute: impl Fn(&Reactive) -> T + 'static,
    ) -> Computed<T> {
        self.computed_owned_by_opt(Some(owner), compute)
    }

    fn computed_owned_by_opt<T: PartialEq + 'static>(
        &self,
        owner: Option<ElementId>,
        compute: impl Fn(&Reactive) -> T + 'static,
    ) -> Computed<T> {
        let compute: ComputeFn = Rc::new(move |r| Box::new(compute(r)) as Box<dyn Any>);
        let eq: EqFn = Rc::new(
            |a, b| match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        );
        let slot = self.alloc(
            None,
            Kind::Computed {
                compute,
                eq,
                stale: true,
            },
            owner,
        );
        Computed {
            slot,
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// Register an effect. It runs at the next effects phase
    /// ([`Reactive::run_effects`]) and again after any of the slots it tracked
    /// is written. Dropping the returned handle unregisters it.
    pub fn effect(&self, run: impl FnMut(&Reactive) + 'static) -> Effect {
        self.effect_owned_by_opt(None, run)
    }

    /// [`Reactive::effect`] released when `owner` unmounts (dropping the handle
    /// earlier also releases it).
    pub fn effect_owned_by(
        &self,
        owner: ElementId,
        run: impl FnMut(&Reactive) + 'static,
    ) -> Effect {
        self.effect_owned_by_opt(Some(owner), run)
    }

    fn effect_owned_by_opt(
        &self,
        owner: Option<ElementId>,
        run: impl FnMut(&Reactive) + 'static,
    ) -> Effect {
        let run: EffectFn = Rc::new(RefCell::new(run));
        let slot = self.alloc(None, Kind::Effect { run, pending: true }, owner);
        self.inner.borrow_mut().pending_effects.push(slot.index);
        Effect {
            slot,
            reactive: self.clone(),
        }
    }

    /// Release a slot: its value drops, its readers forget it, later handle
    /// use reports [`SignalError::Released`].
    pub fn release(&self, slot: SignalSlot) {
        let mut inner = self.inner.borrow_mut();
        if Self::check(&inner, slot).is_err() {
            return;
        }
        Self::release_index(&mut inner, slot.index);
    }

    fn release_index(inner: &mut Inner, index: u32) {
        let sources = std::mem::take(&mut inner.nodes[index as usize].sources);
        for source in sources {
            if let Some(node) = inner.nodes.get_mut(source as usize) {
                node.computed_readers.retain(|reader| *reader != index);
            }
        }
        let readers = std::mem::take(&mut inner.nodes[index as usize].element_readers);
        for element in readers {
            if let Some(reads) = inner.element_reads.get_mut(&element) {
                reads.retain(|slot| *slot != index);
            }
        }
        let node = &mut inner.nodes[index as usize];
        node.live = false;
        node.value = None;
        node.kind = Kind::Signal;
        node.computed_readers.clear();
        inner.pending_effects.retain(|pending| *pending != index);
        inner.free.push(index);
    }

    // ------------------------------------------------------------ elements

    /// Called by the build drain right before `element`'s `build`: forget every
    /// slot it read last time so this build re-derives the set from scratch.
    pub(crate) fn begin_element_build(&self, element: ElementId) {
        let mut inner = self.inner.borrow_mut();
        Self::forget_element_reads(&mut inner, element);
        inner.building = Some(element);
    }

    /// Called by the build drain right after `element`'s `build` returned (or
    /// unwound).
    pub(crate) fn end_element_build(&self, element: ElementId) {
        let mut inner = self.inner.borrow_mut();
        if inner.building == Some(element) {
            inner.building = None;
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
        if Self::check(&inner, slot).is_err() {
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

    /// The element unmounted: it reads nothing any more, and every slot created
    /// on its behalf is released.
    pub(crate) fn release_element(&self, element: ElementId) {
        let mut inner = self.inner.borrow_mut();
        Self::forget_element_reads(&mut inner, element);
        if let Some(owned) = inner.owned_by_element.remove(&element) {
            for index in owned {
                if inner
                    .nodes
                    .get(index as usize)
                    .is_some_and(|node| node.live)
                {
                    Self::release_index(&mut inner, index);
                }
            }
        }
    }

    /// Elements currently registered as readers of `slot` (test/diagnostic
    /// probe).
    pub fn readers_of(&self, slot: SignalSlot) -> Vec<ElementId> {
        let inner = self.inner.borrow();
        match Self::check(&inner, slot) {
            Ok(()) => inner.nodes[slot.index as usize].element_readers.to_vec(),
            Err(_) => Vec::new(),
        }
    }

    // --------------------------------------------------------------- reads

    /// Read `slot`'s value. If a computed or effect is computing, it becomes a
    /// dependent of `slot`. A stale memo is recomputed first.
    fn read<T: 'static, R>(
        &self,
        slot: SignalSlot,
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, SignalError> {
        self.ensure_fresh(slot)?;
        let mut inner = self.inner.borrow_mut();
        Self::check(&inner, slot)?;
        if let Some(&tracker) = inner.tracking.last() {
            let node = &mut inner.nodes[slot.index as usize];
            if !node.computed_readers.contains(&tracker) {
                node.computed_readers.push(tracker);
            }
            let tracker_node = &mut inner.nodes[tracker as usize];
            if !tracker_node.sources.contains(&slot.index) {
                tracker_node.sources.push(slot.index);
            }
        }
        let value = inner.nodes[slot.index as usize]
            .value
            .as_deref()
            .and_then(|value| value.downcast_ref::<T>())
            .expect("BUG: signal slot holds a value of another type");
        Ok(f(value))
    }

    /// Read without any dependency registration.
    fn peek<T: 'static, R>(
        &self,
        slot: SignalSlot,
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, SignalError> {
        self.ensure_fresh(slot)?;
        let inner = self.inner.borrow();
        Self::check(&inner, slot)?;
        let value = inner.nodes[slot.index as usize]
            .value
            .as_deref()
            .and_then(|value| value.downcast_ref::<T>())
            .expect("BUG: signal slot holds a value of another type");
        Ok(f(value))
    }

    /// Recompute a stale memo (and nothing else). Returns whether the value
    /// changed, which only matters to [`Reactive::mark`].
    fn ensure_fresh(&self, slot: SignalSlot) -> Result<bool, SignalError> {
        let (compute, eq) = {
            let inner = self.inner.borrow();
            Self::check(&inner, slot)?;
            match &inner.nodes[slot.index as usize].kind {
                Kind::Computed {
                    compute,
                    eq,
                    stale: true,
                } => (Rc::clone(compute), Rc::clone(eq)),
                _ => return Ok(false),
            }
        };
        Ok(self.recompute(slot.index, &compute, &eq))
    }

    fn recompute(&self, index: u32, compute: &ComputeFn, eq: &EqFn) -> bool {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.tracking.contains(&index) {
                // A computed value reading itself (directly or through another
                // one) is a cycle; the stale value stands rather than recursing.
                return false;
            }
            Self::clear_sources(&mut inner, index);
            inner.tracking.push(index);
        }
        let new_value = compute(self);
        let mut inner = self.inner.borrow_mut();
        inner.tracking.pop();
        let node = &mut inner.nodes[index as usize];
        let changed = match node.value.as_deref() {
            Some(old) => !eq(old, new_value.as_ref()),
            None => true,
        };
        node.value = Some(new_value);
        if let Kind::Computed { stale, .. } = &mut node.kind {
            *stale = false;
        }
        changed
    }

    fn clear_sources(inner: &mut Inner, index: u32) {
        let sources = std::mem::take(&mut inner.nodes[index as usize].sources);
        for source in sources {
            if let Some(node) = inner.nodes.get_mut(source as usize) {
                node.computed_readers.retain(|reader| *reader != index);
            }
        }
    }

    // -------------------------------------------------------------- writes

    fn write<T: 'static, R>(
        &self,
        slot: SignalSlot,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, SignalError> {
        let result = {
            let mut inner = self.inner.borrow_mut();
            Self::check(&inner, slot)?;
            if let Some(element) = inner.building {
                return Err(SignalError::WrittenDuringBuild { element });
            }
            let value = inner.nodes[slot.index as usize]
                .value
                .as_deref_mut()
                .and_then(|value| value.downcast_mut::<T>())
                .expect("BUG: signal slot holds a value of another type");
            f(value)
        };
        self.mark(slot.index);
        Ok(result)
    }

    /// `slot` changed: schedule its element readers, re-check dependent computed values
    /// (marking *their* readers only if their output changed), queue dependent
    /// effects.
    fn mark(&self, index: u32) {
        let mut queue: SmallVec<[u32; 8]> = SmallVec::new();
        queue.push(index);
        while let Some(current) = queue.pop() {
            let (elements, nodes, scheduler) = {
                let inner = self.inner.borrow();
                let Some(node) = inner.nodes.get(current as usize) else {
                    continue;
                };
                (
                    node.element_readers.clone(),
                    node.computed_readers.clone(),
                    inner.scheduler.clone(),
                )
            };
            if let Some(scheduler) = scheduler {
                for element in elements {
                    scheduler.schedule(element, RebuildReason::SignalChange);
                }
            }
            for reader in nodes {
                let action = {
                    let mut inner = self.inner.borrow_mut();
                    let Some(node) = inner.nodes.get_mut(reader as usize) else {
                        continue;
                    };
                    let mut queue_effect = false;
                    let action = match &mut node.kind {
                        Kind::Computed { compute, eq, stale } => {
                            *stale = true;
                            Some((Rc::clone(compute), Rc::clone(eq)))
                        }
                        Kind::Effect { pending, .. } => {
                            if !*pending {
                                *pending = true;
                                queue_effect = true;
                            }
                            None
                        }
                        Kind::Signal => None,
                    };
                    if queue_effect {
                        inner.pending_effects.push(reader);
                    }
                    action
                };
                if let Some((compute, eq)) = action
                    && self.recompute(reader, &compute, &eq)
                {
                    queue.push(reader);
                }
            }
        }
    }

    // ------------------------------------------------------------- effects

    /// Run every pending effect once. The frame order is build → effects →
    /// layout → paint (ADR-0074 §5.4); a signal an effect writes marks readers
    /// into the owner's inbox, i.e. for the **next** frame.
    pub fn run_effects(&self) {
        let pending = std::mem::take(&mut self.inner.borrow_mut().pending_effects);
        for index in pending {
            let run = {
                let mut inner = self.inner.borrow_mut();
                match inner.nodes.get_mut(index as usize) {
                    Some(Node {
                        live: true,
                        kind: Kind::Effect { run, pending },
                        ..
                    }) => {
                        *pending = false;
                        Rc::clone(run)
                    }
                    _ => continue,
                }
            };
            {
                let mut inner = self.inner.borrow_mut();
                Self::clear_sources(&mut inner, index);
                inner.tracking.push(index);
            }
            (run.borrow_mut())(self);
            self.inner.borrow_mut().tracking.pop();
        }
    }

    /// Number of effects queued for the next effects phase (test probe).
    pub fn pending_effect_count(&self) -> usize {
        self.inner.borrow().pending_effects.len()
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

impl<T: 'static> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal").field("slot", &self.slot).finish()
    }
}

impl<T: 'static> Signal<T> {
    /// The arena slot behind this handle.
    pub fn slot(self) -> SignalSlot {
        self.slot
    }

    /// The `Send + Sync` form of this handle, for a closure that will run on
    /// the owner thread later (`UiCommand::SignalWrite`).
    pub fn detach(self) -> SignalSender<T> {
        SignalSender {
            slot: self.slot,
            _t: PhantomData,
        }
    }

    /// Read during `build`: the building element becomes a reader.
    pub fn get(self, cx: &dyn crate::BuildContext) -> T
    where
        T: Clone,
    {
        self.with(cx, T::clone)
    }

    /// Borrowed read during `build`: the building element becomes a reader.
    pub fn with<R>(self, cx: &dyn crate::BuildContext, f: impl FnOnce(&T) -> R) -> R {
        cx.signal_read(self.slot);
        cx.reactive()
            .read(self.slot, f)
            .expect("BUG: signal read through a BuildContext after its slot was released")
    }

    /// Read inside a computed or effect: that computation becomes a dependent.
    /// Outside one it is a plain read.
    pub fn track<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError> {
        r.read(self.slot, f)
    }

    /// Read without registering anything.
    pub fn peek<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError> {
        r.peek(self.slot, f)
    }

    /// Replace the value and mark every reader — equal or not.
    pub fn set(self, r: &Reactive, value: T) -> Result<(), SignalError> {
        r.write(self.slot, |slot: &mut T| *slot = value)
    }

    /// Mutate in place and mark every reader.
    pub fn update<R>(self, r: &Reactive, f: impl FnOnce(&mut T) -> R) -> Result<R, SignalError> {
        r.write(self.slot, f)
    }

    /// Replace the value only if it differs; an equal write marks nobody.
    /// Returns whether a write happened.
    pub fn set_if_changed(self, r: &Reactive, value: T) -> Result<bool, SignalError>
    where
        T: PartialEq,
    {
        if r.peek(self.slot, |current: &T| *current == value)? {
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
    /// another graph reports [`SignalError::Released`].
    pub fn attach(self) -> Signal<T> {
        Signal {
            slot: self.slot,
            _t: PhantomData,
            _local: PhantomData,
        }
    }
}

/// A `Copy` handle to a cached computation; readers are notified only when
/// its output changes.
pub struct Computed<T: 'static> {
    slot: SignalSlot,
    _t: PhantomData<fn() -> T>,
    /// Realm-affine: a handle is only meaningful on the thread that owns its
    /// [`Reactive`], so it is `!Send + !Sync` like the graph itself.
    _local: PhantomData<*const ()>,
}

impl<T: 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for Computed<T> {}

impl<T: 'static> fmt::Debug for Computed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Computed")
            .field("slot", &self.slot)
            .finish()
    }
}

impl<T: 'static> Computed<T> {
    /// The arena slot behind this handle.
    pub fn slot(self) -> SignalSlot {
        self.slot
    }

    /// Read during `build`: the building element becomes a reader.
    pub fn get(self, cx: &dyn crate::BuildContext) -> T
    where
        T: Clone,
    {
        self.with(cx, T::clone)
    }

    /// Borrowed read during `build`.
    pub fn with<R>(self, cx: &dyn crate::BuildContext, f: impl FnOnce(&T) -> R) -> R {
        cx.signal_read(self.slot);
        cx.reactive()
            .read(self.slot, f)
            .expect("BUG: memo read through a BuildContext after its slot was released")
    }

    /// Read inside another memo or an effect.
    pub fn track<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError> {
        r.read(self.slot, f)
    }

    /// Read without registering anything (still recomputes if stale).
    pub fn peek<R>(self, r: &Reactive, f: impl FnOnce(&T) -> R) -> Result<R, SignalError> {
        r.peek(self.slot, f)
    }
}

/// RAII handle for a registered effect; dropping it unregisters the effect.
pub struct Effect {
    slot: SignalSlot,
    reactive: Reactive,
}

impl fmt::Debug for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Effect")
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

impl Effect {
    /// The arena slot behind this handle.
    pub fn slot(&self) -> SignalSlot {
        self.slot
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        self.reactive.release(self.slot);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use parking_lot::Mutex;

    use flui_foundation::RebuildReasons;

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
        r.end_element_build(e1);

        // The next build of e1 reads nothing.
        r.begin_element_build(e1);
        r.end_element_build(e1);

        a.set(&r, 1).unwrap();
        assert!(scheduled(&inbox).is_empty());
        assert!(r.readers_of(a.slot()).is_empty());
    }

    #[test]
    fn writes_during_build_are_refused() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(0u8);
        let e1 = ElementId::new(1);
        r.begin_element_build(e1);
        assert_eq!(
            a.set(&r, 1),
            Err(SignalError::WrittenDuringBuild { element: e1 })
        );
        r.end_element_build(e1);
        assert_eq!(a.set(&r, 1), Ok(()));
    }

    #[test]
    fn memo_notifies_readers_only_when_its_output_changes() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(3u32);
        let is_even = r.computed(move |r| a.track(r, |v| v % 2 == 0).unwrap());
        let e1 = ElementId::new(1);
        assert!(!is_even.peek(&r, |v| *v).unwrap());
        r.register_element_reader(is_even.slot(), e1);

        a.set(&r, 5).unwrap(); // still odd
        assert!(scheduled(&inbox).is_empty());

        a.set(&r, 6).unwrap(); // flips
        assert_eq!(scheduled(&inbox), vec![e1]);
        assert!(is_even.peek(&r, |v| *v).unwrap());
    }

    #[test]
    fn memo_chains_propagate_through_changed_outputs_only() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(1u32);
        let doubled = r.computed(move |r| a.track(r, |v| v * 2).unwrap());
        let big = r.computed(move |r| doubled.track(r, |v| *v > 10).unwrap());
        let e1 = ElementId::new(1);
        assert!(!big.peek(&r, |v| *v).unwrap());
        r.register_element_reader(big.slot(), e1);

        a.set(&r, 2).unwrap(); // doubled 4, big false
        assert!(scheduled(&inbox).is_empty());
        a.set(&r, 6).unwrap(); // doubled 12, big true
        assert_eq!(scheduled(&inbox), vec![e1]);
    }

    #[test]
    fn effects_run_in_the_effects_phase_and_rerun_on_source_writes() {
        let (r, _) = graph_with_inbox();
        let a = r.signal(0u32);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let effect = r.effect(move |r| log.borrow_mut().push(a.track(r, |v| *v).unwrap()));
        assert!(seen.borrow().is_empty(), "effects never run at creation");

        r.run_effects();
        assert_eq!(*seen.borrow(), vec![0]);

        a.set(&r, 7).unwrap();
        assert_eq!(
            *seen.borrow(),
            vec![0],
            "a write queues the effect, it does not run it"
        );
        r.run_effects();
        assert_eq!(*seen.borrow(), vec![0, 7]);

        drop(effect);
        a.set(&r, 8).unwrap();
        r.run_effects();
        assert_eq!(
            *seen.borrow(),
            vec![0, 7],
            "a dropped effect is unregistered"
        );
    }

    #[test]
    fn an_effect_write_lands_in_the_inbox_not_in_a_rerun_loop() {
        let (r, inbox) = graph_with_inbox();
        let a = r.signal(0u32);
        let b = r.signal(0u32);
        let e1 = ElementId::new(1);
        r.register_element_reader(b.slot(), e1);
        let _effect = r.effect(move |r| {
            let v = a.track(r, |v| *v).unwrap();
            b.set(r, v + 1).unwrap();
        });
        r.run_effects();
        assert_eq!(scheduled(&inbox), vec![e1]);
        assert_eq!(b.peek(&r, |v| *v).unwrap(), 1);
        assert_eq!(
            r.pending_effect_count(),
            0,
            "writing b does not re-queue an effect that only tracks a"
        );
    }

    #[test]
    fn unmounting_an_element_releases_what_it_owned_and_its_reads() {
        let (r, inbox) = graph_with_inbox();
        let e1 = ElementId::new(1);
        let shared = r.signal(0u8);
        let owned = r.signal_owned_by(e1, 0u8);
        r.register_element_reader(shared.slot(), e1);

        r.release_element(e1);

        assert_eq!(
            owned.peek(&r, |v| *v),
            Err(SignalError::Released {
                index: owned.slot().index,
                generation: 0
            })
        );
        shared.set(&r, 1).unwrap();
        assert!(scheduled(&inbox).is_empty());

        // The freed slot is reused with a new generation; the old handle stays dead.
        let fresh = r.signal(9u8);
        assert_eq!(fresh.slot().index, owned.slot().index);
        assert_ne!(fresh.slot().generation, owned.slot().generation);
        assert!(owned.peek(&r, |v| *v).is_err());
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
