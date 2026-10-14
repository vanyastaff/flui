//! One owner-local animation sample a render object reads instead of its
//! animation, shared by the layout- and paint-driven transition objects.
//!
//! The animation is read only in `attach` and in the tick listener; layout,
//! paint and hit-testing read the cached sample, so no user `Animation::value`
//! runs inside a frame walk. A ticket taken before each read lets a reentrant,
//! newer reading win over the outer one, and a value the owner refuses (a
//! non-finite alignment, say) keeps the last admitted sample. The listener
//! holds a `Weak` to the proxy: a strong capture would form a proxy →
//! listener → proxy cycle that outlives the tree.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::{Listenable, ListenerId};
use flui_rendering::{RenderUpdateImpact, pipeline::LocalRenderInvalidationHandle};

/// The admitted value, the ticket of the reading that produced it, and
/// whether its invalidation request has yet to complete.
#[derive(Debug)]
struct SampleCell<T> {
    generation: Cell<u64>,
    value: RefCell<T>,
    undelivered: Cell<bool>,
    /// Whether a `request_update` is running: a notification its wake raises
    /// must not retry that same request, or the retry recurses without bound.
    delivering: Cell<bool>,
}

/// Marks a request in flight, and unmarks it when the request returns or
/// unwinds.
struct InFlight<'a>(&'a Cell<bool>, bool);

impl<'a> InFlight<'a> {
    fn begin(flag: &'a Cell<bool>) -> Self {
        Self(flag, flag.replace(true))
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}

/// A [`ProxyAnimation`] plus the last value its owner admitted.
#[derive(Debug)]
pub(crate) struct AnimationSample<T: Clone + PartialEq + std::fmt::Debug + 'static> {
    proxy: Rc<ProxyAnimation<T>>,
    cell: Rc<SampleCell<T>>,
    admit: fn(&T) -> bool,
    listener: Option<ListenerId>,
}

impl<T: Clone + PartialEq + std::fmt::Debug + 'static> AnimationSample<T> {
    /// Samples `proxy` now; an initial value `admit` refuses starts at
    /// `fallback`.
    pub(crate) fn new(proxy: ProxyAnimation<T>, fallback: T, admit: fn(&T) -> bool) -> Self {
        let value = proxy.value();
        let value = if admit(&value) { value } else { fallback };
        Self {
            proxy: Rc::new(proxy),
            cell: Rc::new(SampleCell {
                generation: Cell::new(0),
                value: RefCell::new(value),
                undelivered: Cell::new(false),
                delivering: Cell::new(false),
            }),
            admit,
            listener: None,
        }
    }

    /// The last admitted value.
    pub(crate) fn get(&self) -> T {
        self.cell.value.borrow().clone()
    }

    /// Subscribes to the proxy; each admitted change requests `impact`.
    /// Catches up with a change made while nothing listened.
    pub(crate) fn attach(
        &mut self,
        handle: &LocalRenderInvalidationHandle,
        impact: RenderUpdateImpact,
    ) {
        let weak: Weak<ProxyAnimation<T>> = Rc::downgrade(&self.proxy);
        let cell = Rc::clone(&self.cell);
        let admit = self.admit;
        let listener_handle = handle.clone();
        self.listener = Some(self.proxy.add_listener(Rc::new(move || {
            if let Some(proxy) = weak.upgrade() {
                refresh(&proxy, &cell, admit, &listener_handle, impact);
            }
        })));
        refresh(&self.proxy, &self.cell, self.admit, handle, impact);
    }

    /// Removes the subscription [`attach`](Self::attach) made.
    pub(crate) fn detach(&mut self) {
        if let Some(id) = self.listener.take() {
            self.proxy.remove_listener(id);
        }
    }
}

/// Reads the proxy and, when the reading is the newest, admitted and changed,
/// publishes it before requesting `impact`.
fn refresh<T: Clone + PartialEq + std::fmt::Debug + 'static>(
    proxy: &ProxyAnimation<T>,
    cell: &SampleCell<T>,
    admit: fn(&T) -> bool,
    handle: &LocalRenderInvalidationHandle,
    impact: RenderUpdateImpact,
) {
    // An exhausted ticket counter keeps the last sample permanently.
    let Some(generation) = cell.generation.get().checked_add(1) else {
        return;
    };
    cell.generation.set(generation);
    // Debt from an earlier failed wake is retried before the fallible read of
    // the user's animation, and whatever that read returns: a read that
    // unwinds, or a value the owner refuses, must not strand an accepted
    // sample. A wake that reenters supersedes this reading and owns what
    // follows.
    // A request already running is not retried: its own return settles it.
    if !cell.delivering.get() {
        deliver(cell, handle, impact, generation);
    }
    if cell.generation.get() != generation {
        return;
    }
    let value = proxy.value();
    if cell.generation.get() != generation || !admit(&value) {
        return;
    }
    if *cell.value.borrow() != value {
        let previous = cell.value.replace(value);
        // The outgoing value drops after the cell is released.
        drop(previous);
        cell.undelivered.set(true);
        // Publication precedes delivery: a frame the request wakes reads the
        // new sample.
        deliver(cell, handle, impact, generation);
    }
}

/// Requests `impact` while a published sample is still owed. The debt clears
/// only once the request returns, so a wake that unwinds leaves it for the
/// next notification, even of the same value. A reading the wake reentered
/// owns the debt from then on: its own failed wake must not be cleared by
/// this older request returning. A detached handle refuses; `attach` catches
/// up on reattach.
fn deliver<T>(
    cell: &SampleCell<T>,
    handle: &LocalRenderInvalidationHandle,
    impact: RenderUpdateImpact,
    generation: u64,
) {
    if !cell.undelivered.get() {
        return;
    }
    let in_flight = InFlight::begin(&cell.delivering);
    let delivered = handle.request_update(impact).is_ok();
    drop(in_flight);
    if delivered && cell.generation.get() == generation {
        cell.undelivered.set(false);
    }
}
