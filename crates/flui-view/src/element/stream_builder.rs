//! `StreamBuilder` — build from the latest event of a stream.
//!
//! # Public shape
//!
//! Exported from `flui-view::element` and re-exported by `flui-widgets` plus its
//! prelude once the design passed its Flutter-parity gate. The keyed identity shape is signed
//! off by the repository owner; this repository has no separate api-design-lead
//! role. The state type is public only because Rust requires a public associated
//! `State` type for a public `StatefulView` implementation; it remains opaque.
//!
//! # Sibling of `FutureBuilder`
//!
//! Same seams (`RebuildHandle`, `AsyncDriver`, `AsyncSnapshot`), same
//! keyed identity, same shared [`Slot`]. The difference is the fold set and one
//! load-bearing subtlety about *when* the task is first polled.
//!
//! # Why this never polls eagerly
//!
//! `FutureBuilder` subscribes with `AsyncDriver::spawn_local_eager`, whose inline
//! poll reproduces Dart's synchronous `.then` (`SynchronousFuture`). A stream
//! must **not** do that.
//!
//! `_StreamBuilderBaseState._subscribe` (`.flutter/.../widgets/async.dart`) reads:
//!
//! ```text
//! _subscription = widget.stream!.listen(...);
//! _summary = widget.afterConnected(_summary);   // unconditional — no Done guard
//! ```
//!
//! `afterConnected` is `inState(waiting)` with no guard, and Dart's
//! `Stream.listen` never delivers an event synchronously — the first event always
//! arrives in a later microtask. So `Waiting` is *always* observed before the
//! first event. An eager inline poll could yield an item before `after_connected`
//! ran, and `after_connected` would then drag `Active` back to `Waiting`.
//! `spawn_local` (first poll on the next frame's driver step) is the faithful
//! shape, and also the simpler one.
//!
//! # Folds
//!
//! | Event | Fold | Result |
//! |---|---|---|
//! | subscribe | `after_connected` | `Waiting`, payload preserved |
//! | `Some(Ok(d))` | `after_data(d)` | `Active` + data, **error cleared** |
//! | `Some(Err(e))` | `after_error(e)` | `Active` + error, **data cleared** |
//! | `None` (end) | `after_done` | `Done`, last payload preserved |
//! | key change / dispose | `after_disconnected` | `None`, payload preserved |
//!
//! A Dart stream continues after an error unless `cancelOnError`; a Rust
//! `Stream<Item = Result<T, E>>` does the same, so an error leaves the state
//! `Active` and polling continues.

use std::{pin::Pin, rc::Rc, sync::Arc};

use flui_foundation::{AsyncSnapshot, ConnectionState};
use flui_scheduler::{AsyncDriver, TaskToken};
use futures_core::Stream;
use parking_lot::Mutex;

use super::async_slot::{InitialDataFactory, SharedSlot, Slot, SnapshotBuilder, apply_fold};
use crate::{
    RebuildHandle,
    context::{BuildContext, LifecycleContext},
    view::{IntoView, StatefulView, View, ViewState},
};

/// A boxed, `Send` stream of `Result<T, E>`.
pub type BoxedResultStream<T, E> = Pin<Box<dyn Stream<Item = Result<T, E>> + Send + 'static>>;

/// Produces the stream to listen to. `Fn`, not `FnOnce`: the view is cloned on
/// every rebuild. Called once per subscription.
pub type StreamFactory<T, E> = Rc<dyn Fn() -> BoxedResultStream<T, E>>;

/// Fold a stream event into the shared snapshot, honouring the generation guard.
///
/// Returns whether a rebuild must be scheduled.
fn apply_event<T, E>(
    slot: &SharedSlot<T, E>,
    generation: u64,
    event: Option<Result<T, E>>,
) -> bool {
    apply_fold(slot, generation, |snapshot| match event {
        Some(Ok(data)) => snapshot.after_data(data),
        Some(Err(error)) => snapshot.after_error(error),
        None => snapshot.after_done(),
    })
}

// ============================================================================
// VIEW
// ============================================================================

/// A view that builds itself from the latest interaction with a stream.
pub struct StreamBuilder<K, T, E> {
    /// Identity of the stream. `None` ⇒ no stream (Flutter's null stream).
    key: Option<K>,
    /// Creates the stream when the subscription starts.
    make: StreamFactory<T, E>,
    /// Optional seed value, applied only at `init_state`.
    initial_data: Option<InitialDataFactory<T>>,
    /// Builds the child from the snapshot.
    builder: SnapshotBuilder<T, E>,
}

impl<K: Clone, T, E> Clone for StreamBuilder<K, T, E> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            make: Rc::clone(&self.make),
            initial_data: self.initial_data.clone(),
            builder: Rc::clone(&self.builder),
        }
    }
}

impl<K: std::fmt::Debug, T, E> std::fmt::Debug for StreamBuilder<K, T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamBuilder")
            .field("key", &self.key)
            .field("has_initial_data", &self.initial_data.is_some())
            .finish_non_exhaustive()
    }
}

impl<K, T, E> StreamBuilder<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    /// Listen to the stream identified by `key`; `None` means no stream.
    pub fn keyed(
        key: Option<K>,
        make: StreamFactory<T, E>,
        builder: SnapshotBuilder<T, E>,
    ) -> Self {
        Self {
            key,
            make,
            initial_data: None,
            builder,
        }
    }

    /// Seed the snapshot before the first subscription.
    ///
    /// Flutter's `StreamBuilder.initialData`. Applied **only** at `init_state`; a
    /// later key change does not re-apply it.
    #[must_use]
    pub fn with_initial_data(mut self, initial_data: InitialDataFactory<T>) -> Self {
        self.initial_data = Some(initial_data);
        self
    }
}

impl<K, T, E> StatefulView for StreamBuilder<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    type State = StreamBuilderState<K, T, E>;

    fn create_state(&self) -> Self::State {
        // `ViewState::init_state` is handed a `BuildContext` but NOT the view, so
        // the configuration the first subscription needs is copied here.
        StreamBuilderState {
            slot: Arc::new(Mutex::new(Slot::new(AsyncSnapshot::nothing()))),
            handle: None,
            driver: None,
            token: None,
            key: None,
            initial_key: self.key.clone(),
            initial_make: Rc::clone(&self.make),
            initial_data: self.initial_data.clone(),
        }
    }
}

impl<K, T, E> View for StreamBuilder<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }
}

// ============================================================================
// STATE
// ============================================================================

/// Persistent state for [`StreamBuilder`] — **opaque**.
///
/// `pub` only because it is the `State` associated type of a public
/// [`StatefulView`] impl and Rust forbids a crate-private type there. It has no
/// public fields and no public methods; construct it only through
/// `StreamBuilder::create_state`.
pub struct StreamBuilderState<K, T, E> {
    /// The snapshot the builder reads, written by the task.
    slot: SharedSlot<T, E>,
    /// Captured in `init_state` — the only lifecycle hook handed a `BuildContext`.
    handle: Option<RebuildHandle>,
    /// The binding's driver, likewise captured in `init_state`.
    driver: Option<AsyncDriver>,
    /// Cancels the live subscription on drop.
    token: Option<TaskToken>,
    /// The key the live subscription was created for.
    key: Option<K>,
    /// The view's key at mount, read by `init_state` (which gets no view).
    initial_key: Option<K>,
    /// The view's factory at mount, likewise.
    initial_make: StreamFactory<T, E>,
    /// The view's `initialData` factory at mount. Applied once, never re-applied.
    initial_data: Option<InitialDataFactory<T>>,
}

impl<K, T, E> std::fmt::Debug for StreamBuilderState<K, T, E>
where
    K: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slot = self.slot.lock();
        f.debug_struct("StreamBuilderState")
            .field("key", &self.key)
            .field("connection_state", &slot.snapshot.connection_state())
            .field("generation", &slot.generation)
            .field("subscribed", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl<K, T, E> StreamBuilderState<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    /// Cancel the live subscription and invalidate its generation, so an event
    /// already in flight is discarded.
    ///
    /// Flutter's `_unsubscribe` calls `StreamSubscription.cancel()`. Dropping the
    /// token here does the same: the poll loop stops and the stream is dropped.
    fn unsubscribe(&mut self) {
        self.token = None; // Drop cancels.
        self.key = None;
        self.slot.lock().generation += 1;
    }

    /// Start a subscription for `key`.
    ///
    /// Mirrors `_StreamBuilderBaseState._subscribe`: listen, then
    /// `after_connected` — unconditionally. See the module docs for why this uses
    /// `spawn_local` rather than `spawn_local_eager`.
    fn subscribe(&mut self, key: K, make: &StreamFactory<T, E>) {
        let Some(driver) = self.driver.clone() else {
            tracing::warn!(
                "StreamBuilder: no async driver on this BuildContext; the stream \
                 will never be polled. Is the tree bound to a binding?"
            );
            return;
        };
        let handle = self.handle.clone().unwrap_or_else(RebuildHandle::inert);

        let generation = {
            let mut slot = self.slot.lock();
            slot.generation += 1;
            slot.generation
        };

        let mut stream = make();
        let slot_for_task = Arc::clone(&self.slot);

        // No eager poll: a stream must show `Waiting` before its first event.
        let token = driver.spawn_local(Box::pin(async move {
            loop {
                // `futures-core` gives the trait only — no `StreamExt::next()` —
                // so the stream is polled by hand through `poll_fn`. That is the
                // whole reason the dependency is trait-only.
                let event = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
                let is_end = event.is_none();

                // `StreamBuilder` never opens an inline window, so `false` here
                // means exactly one thing: the subscription was replaced or
                // disposed. Stop, and do not wake a frame for it.
                if !apply_event(&slot_for_task, generation, event) {
                    return;
                }

                handle.schedule(crate::RebuildReason::AsyncCompletion);

                if is_end {
                    return;
                }
            }
        }));

        self.slot.lock().fold(AsyncSnapshot::after_connected);

        self.token = Some(token);
        self.key = Some(key);
    }
}

impl<K, T, E> ViewState<StreamBuilder<K, T, E>> for StreamBuilderState<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    /// `_StreamBuilderBaseState.initState`: seed from `initial()`, then subscribe.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // Capture the capabilities here — the ONLY lifecycle hook handed a
        // context. `did_update_view` and `dispose` receive none.
        self.handle = Some(ctx.rebuild_handle());
        self.driver = ctx.async_driver();

        self.slot.lock().snapshot = match &self.initial_data {
            Some(initial_data) => AsyncSnapshot::with_data(ConnectionState::None, initial_data()),
            None => AsyncSnapshot::nothing(),
        };

        // An absent key is Flutter's null stream: no subscription, snapshot stays
        // where `initial` left it.
        if let Some(key) = self.initial_key.clone() {
            let make = Rc::clone(&self.initial_make);
            self.subscribe(key, &make);
        }
    }

    fn build(&self, view: &StreamBuilder<K, T, E>, ctx: &dyn BuildContext) -> impl IntoView {
        let slot = self.slot.lock();
        (view.builder)(ctx, &slot.snapshot)
    }

    /// `_StreamBuilderBaseState.didUpdateWidget`: an unchanged key is an early
    /// return; a changed one unsubscribes, applies `after_disconnected`
    /// (**preserving the payload**), and resubscribes. `initialData` is never
    /// re-applied.
    fn did_update_view(
        &mut self,
        old_view: &StreamBuilder<K, T, E>,
        new_view: &StreamBuilder<K, T, E>,
    ) {
        if old_view.key == new_view.key {
            return;
        }

        if self.token.is_some() || self.key.is_some() {
            self.unsubscribe();
            self.slot.lock().fold(AsyncSnapshot::after_disconnected);
        }

        if let Some(key) = new_view.key.clone() {
            self.subscribe(key, &new_view.make);
        }
    }

    fn dispose(&mut self) {
        self.unsubscribe();
    }
}
