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
    context::BuildContext,
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

                handle.schedule();

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
    fn init_state(&mut self, ctx: &dyn BuildContext) {
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};

    use flui_foundation::ElementId;
    use flui_scheduler::Scheduler;

    use crate::view::{ErrorView, ViewExt};
    use crate::{BuildOwner, tree::ElementTree};

    /// Deliberately neither `Clone` nor `Copy`.
    #[derive(Debug, PartialEq)]
    struct Payload(i32);

    /// Likewise for the error.
    #[derive(Debug, PartialEq)]
    struct Boom(&'static str);

    /// One queued stream event; `None` ends the stream.
    type Event = Option<Result<Payload, Boom>>;

    /// What a build observed, flattened so the test can assert without `Clone`.
    #[derive(Debug, PartialEq, Clone, Copy)]
    struct Seen {
        state: ConnectionState,
        data: Option<i32>,
        error: Option<&'static str>,
    }

    /// Shared queue behind a `Controlled` stream.
    #[derive(Default)]
    struct Channel {
        events: Mutex<VecDeque<Event>>,
        waker: Mutex<Option<Waker>>,
        /// How many times the factory was called — i.e. subscriptions created.
        subscriptions: AtomicUsize,
    }

    /// A stream the test feeds by hand.
    struct Controlled {
        channel: Arc<Channel>,
    }

    impl Stream for Controlled {
        type Item = Result<Payload, Boom>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if let Some(event) = self.channel.events.lock().pop_front() {
                return Poll::Ready(event);
            }
            *self.channel.waker.lock() = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    /// Test-side handle: create the factory, then push events.
    #[derive(Clone)]
    struct Sender {
        channel: Arc<Channel>,
    }

    impl Sender {
        fn new() -> Self {
            Self {
                channel: Arc::new(Channel::default()),
            }
        }

        fn factory(&self) -> StreamFactory<Payload, Boom> {
            let channel = Arc::clone(&self.channel);
            Rc::new(move || {
                channel.subscriptions.fetch_add(1, Ordering::Relaxed);
                Box::pin(Controlled {
                    channel: Arc::clone(&channel),
                })
            })
        }

        fn subscriptions(&self) -> usize {
            self.channel.subscriptions.load(Ordering::Relaxed)
        }

        /// Push an event and wake the task, exactly as a real producer would.
        fn push(&self, event: Event) {
            self.channel.events.lock().push_back(event);
            if let Some(waker) = self.channel.waker.lock().as_ref() {
                waker.wake_by_ref();
            }
        }

        fn data(&self, value: i32) {
            self.push(Some(Ok(Payload(value))));
        }

        fn error(&self, message: &'static str) {
            self.push(Some(Err(Boom(message))));
        }

        fn end(&self) {
            self.push(None);
        }
    }

    /// Records every snapshot the builder was handed.
    fn recording_builder(log: Arc<Mutex<Vec<Seen>>>) -> SnapshotBuilder<Payload, Boom> {
        Rc::new(move |_ctx, snapshot| {
            log.lock().push(Seen {
                state: snapshot.connection_state(),
                data: snapshot.data().map(|payload| payload.0),
                error: snapshot.error().map(|boom| boom.0),
            });
            ErrorView::new("leaf").into_view().boxed()
        })
    }

    /// Drives the exact steps a binding's frame drives, in the same order:
    /// `Scheduler::drive_async_tasks()` (the shared async step) then
    /// `BuildOwner::build_scope()` — `pump_frame`'s body minus the parts a
    /// `StreamBuilder` cannot observe. `flui-view` cannot depend on
    /// `flui-binding` (that would cycle).
    struct Harness {
        owner: BuildOwner,
        tree: ElementTree,
        scheduler: Scheduler,
        root: ElementId,
    }

    impl Harness {
        fn mount<K>(view: &StreamBuilder<K, Payload, Boom>) -> Self
        where
            K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
        {
            let scheduler = Scheduler::new();
            let mut owner = BuildOwner::new();
            owner.set_async_driver(scheduler.async_driver().clone());
            let mut tree = ElementTree::new();

            let root = tree.mount_root(view, &mut owner.element_owner_mut());
            owner.schedule_build_for(root, 0);
            // The mount build: `init_state` runs here.
            owner.build_scope(&mut tree);

            Self {
                owner,
                tree,
                scheduler,
                root,
            }
        }

        /// One frame, in the binding's order.
        fn frame(&mut self) {
            self.scheduler.drive_async_tasks();
            self.owner.build_scope(&mut self.tree);
        }

        /// A rebuild with a new view — the reconcile path that reaches
        /// `did_update_view`.
        fn update<K>(&mut self, view: &StreamBuilder<K, Payload, Boom>)
        where
            K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
        {
            self.tree
                .update(self.root, view, &mut self.owner.element_owner_mut());
            let depth = self.tree.get(self.root).map_or(0, |node| node.depth);
            self.tree.mark_needs_build(self.root);
            self.owner.schedule_build_for(self.root, depth);
            self.frame();
        }
    }

    fn seen(log: &Arc<Mutex<Vec<Seen>>>) -> Vec<Seen> {
        log.lock().clone()
    }

    fn last(log: &Arc<Mutex<Vec<Seen>>>) -> Seen {
        *log.lock().last().expect("at least one build")
    }

    fn active(data: Option<i32>, error: Option<&'static str>) -> Seen {
        Seen {
            state: ConnectionState::Active,
            data,
            error,
        }
    }

    // ── absent stream ───────────────────────────────────────────────────────

    /// No key ⇒ no stream: `ConnectionState::None`, nothing spawned.
    #[test]
    fn stream_builder_absent_stream_is_none() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::<u32, _, _>::keyed(
            None,
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );

        let harness = Harness::mount(&view);
        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::None,
                data: None,
                error: None
            }
        );
        assert_eq!(harness.scheduler.pending_task_count(), 0, "nothing spawned");
        assert_eq!(sender.subscriptions(), 0);
    }

    /// `'runs the builder using given initial data'` with no stream.
    #[test]
    fn stream_builder_absent_stream_preserves_initial_data() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::<u32, _, _>::keyed(
            None,
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(7)));

        let _harness = Harness::mount(&view);
        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::None,
                data: Some(7),
                error: None
            }
        );
    }

    // ── life cycle ──────────────────────────────────────────────────────────

    /// Subscribing shows `Waiting` before any event — Flutter's unconditional
    /// `afterConnected`. A stream never delivers synchronously, so this must hold
    /// even when the producer already has an event queued.
    #[test]
    fn stream_builder_shows_waiting_before_the_first_event() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        // Queue an event BEFORE mounting: an eager first poll would consume it
        // and skip `Waiting`.
        sender.data(1);

        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Waiting,
                data: None,
                error: None
            },
            "a stream must show Waiting before its first event, even one already queued"
        );

        harness.frame();
        assert_eq!(last(&log), active(Some(1), None));
    }

    /// `'tracks events and errors of stream until completion'`:
    /// `Waiting` → `Active(d)` → `Active(err)` → `Active(d)` → `Done`.
    #[test]
    fn stream_builder_life_cycle_events_errors_then_done() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);
        assert_eq!(last(&log).state, ConnectionState::Waiting);

        sender.data(1);
        harness.frame();
        assert_eq!(last(&log), active(Some(1), None));

        sender.error("mid");
        harness.frame();
        assert_eq!(
            last(&log),
            active(None, Some("mid")),
            "after_error clears the stale value"
        );

        sender.data(2);
        harness.frame();
        assert_eq!(
            last(&log),
            active(Some(2), None),
            "after_data clears the stale error"
        );

        sender.end();
        harness.frame();
        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Done,
                data: Some(2),
                error: None
            },
            "after_done preserves the last value"
        );
        assert_eq!(harness.scheduler.pending_task_count(), 0, "task finished");
    }

    /// `after_done` preserves a trailing error, not just a value.
    #[test]
    fn stream_builder_done_preserves_the_last_error() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        sender.error("boom");
        harness.frame();
        assert_eq!(last(&log), active(None, Some("boom")));

        sender.end();
        harness.frame();
        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Done,
                data: None,
                error: Some("boom")
            }
        );
    }

    /// The `initialData` seed survives `Waiting` and is replaced by the first event.
    #[test]
    fn stream_builder_initial_data_survives_waiting() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(9)));

        let mut harness = Harness::mount(&view);
        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Waiting,
                data: Some(9),
                error: None
            }
        );

        sender.data(1);
        harness.frame();
        assert_eq!(last(&log), active(Some(1), None));
    }

    // ── update semantics ────────────────────────────────────────────────────

    /// An unchanged key is an early return: no resubscribe, snapshot untouched.
    #[test]
    fn stream_builder_same_key_does_not_resubscribe() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        sender.data(3);
        harness.frame();
        assert_eq!(last(&log), active(Some(3), None));
        assert_eq!(sender.subscriptions(), 1);

        let same = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        harness.update(&same);

        assert_eq!(sender.subscriptions(), 1, "no resubscribe");
        assert_eq!(
            last(&log),
            active(Some(3), None),
            "the snapshot is untouched"
        );
    }

    /// `'gracefully handles transition to other stream'` +
    /// `'ignores initialData when reconfiguring'`: a new key hops
    /// `Active` → `None` → `Waiting`, preserving the old value, and the seed is
    /// not re-applied.
    #[test]
    fn stream_builder_key_change_preserves_old_data_and_ignores_initial_data() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let first = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            first.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(99)));
        let mut harness = Harness::mount(&view);

        first.data(1);
        harness.frame();
        assert_eq!(last(&log), active(Some(1), None));

        let second = Sender::new();
        let next = StreamBuilder::keyed(
            Some(2_u32),
            second.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(99)));
        harness.update(&next);

        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Waiting,
                data: Some(1),
                error: None
            },
            "the old value stays visible while the new stream is Waiting, and \
             initialData (99) is NOT re-applied"
        );

        second.data(2);
        harness.frame();
        assert_eq!(last(&log), active(Some(2), None));
    }

    /// The same hop starting from an error.
    #[test]
    fn stream_builder_key_change_preserves_old_error() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let first = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            first.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        first.error("old");
        harness.frame();

        let second = Sender::new();
        let next = StreamBuilder::keyed(
            Some(2_u32),
            second.factory(),
            recording_builder(Arc::clone(&log)),
        );
        harness.update(&next);

        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Waiting,
                data: None,
                error: Some("old")
            }
        );
    }

    /// `'gracefully handles transition to null stream'`: the task is cancelled and
    /// the snapshot drops to `None`, keeping the old payload.
    #[test]
    fn stream_builder_transition_to_absent_stream_cancels_and_preserves_payload() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        sender.data(4);
        harness.frame();
        assert_eq!(last(&log), active(Some(4), None));
        assert_eq!(harness.scheduler.pending_task_count(), 1);

        let none = StreamBuilder::<u32, _, _>::keyed(
            None,
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        harness.update(&none);

        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::None,
                data: Some(4),
                error: None
            }
        );
        assert_eq!(harness.scheduler.pending_task_count(), 0, "task cancelled");
    }

    /// An event from the OLD stream, delivered after a key change, must not touch
    /// the new subscription's snapshot.
    #[test]
    fn stream_builder_stale_event_after_key_change_cannot_mutate_the_snapshot() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let first = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            first.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        let second = Sender::new();
        let next = StreamBuilder::keyed(
            Some(2_u32),
            second.factory(),
            recording_builder(Arc::clone(&log)),
        );
        harness.update(&next);
        assert_eq!(last(&log).state, ConnectionState::Waiting);
        assert_eq!(
            harness.scheduler.pending_task_count(),
            1,
            "the old task was cancelled; only the new one is live"
        );

        // The old producer emits now.
        first.data(111);
        harness.frame();
        harness.frame();

        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Waiting,
                data: None,
                error: None
            },
            "a stale event must not resolve the new subscription"
        );
    }

    /// A disposed `StreamBuilder` neither rebuilds nor resolves.
    ///
    /// This proves **cancellation**, not the generation guard — the guard is
    /// unreachable through the widget because cancellation gets there first.
    /// `apply_event_*` below tests the guard directly.
    #[test]
    fn stream_builder_dispose_cancels_and_never_rebuilds() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);
        let builds_before = seen(&log).len();

        harness
            .tree
            .remove(harness.root, &mut harness.owner.element_owner_mut());

        sender.data(9);
        harness.frame();

        assert_eq!(
            seen(&log).len(),
            builds_before,
            "a disposed StreamBuilder must not rebuild"
        );
        assert_eq!(
            harness.owner.pending_external_builds(),
            0,
            "no rebuild queued"
        );
    }

    // ── the generation guard, tested directly ───────────────────────────────

    fn fresh_slot() -> SharedSlot<Payload, Boom> {
        let mut slot = Slot::new(AsyncSnapshot::nothing());
        slot.generation = 7;
        Arc::new(Mutex::new(slot))
    }

    #[test]
    fn apply_event_data_is_active_and_schedules() {
        let slot = fresh_slot();
        assert!(apply_event(&slot, 7, Some(Ok(Payload(1)))));
        let guard = slot.lock();
        assert_eq!(guard.snapshot.connection_state(), ConnectionState::Active);
        assert_eq!(guard.snapshot.data(), Some(&Payload(1)));
    }

    #[test]
    fn apply_event_error_is_active_and_clears_data() {
        let slot = fresh_slot();
        slot.lock().snapshot = AsyncSnapshot::with_data(ConnectionState::Active, Payload(1));

        assert!(apply_event(&slot, 7, Some(Err(Boom("x")))));
        let guard = slot.lock();
        assert_eq!(guard.snapshot.connection_state(), ConnectionState::Active);
        assert_eq!(guard.snapshot.error(), Some(&Boom("x")));
        assert!(!guard.snapshot.has_data());
    }

    #[test]
    fn apply_event_end_is_done_and_preserves_payload() {
        let slot = fresh_slot();
        slot.lock().snapshot = AsyncSnapshot::with_data(ConnectionState::Active, Payload(5));

        assert!(apply_event(&slot, 7, None));
        let guard = slot.lock();
        assert_eq!(guard.snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(guard.snapshot.data(), Some(&Payload(5)));
    }

    /// An event from a subscription that has since been replaced (or disposed) is
    /// discarded: the snapshot is untouched and no rebuild is asked for.
    #[test]
    fn apply_event_with_a_stale_generation_is_discarded() {
        let slot = fresh_slot();
        slot.lock().snapshot = AsyncSnapshot::with_data(ConnectionState::Waiting, Payload(1));

        assert!(!apply_event(&slot, 6, Some(Ok(Payload(999)))));

        let guard = slot.lock();
        assert_eq!(guard.snapshot.connection_state(), ConnectionState::Waiting);
        assert_eq!(guard.snapshot.data(), Some(&Payload(1)));
    }

    // ── bounds ──────────────────────────────────────────────────────────────

    /// Compile-proof: `Payload` and `Boom` implement neither `Clone` nor `Copy`,
    /// and they flow through the constructor, the stream item, the fold, and the
    /// builder.
    #[test]
    fn stream_builder_needs_no_clone_on_t_or_e() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sender = Sender::new();
        let view = StreamBuilder::keyed(
            Some(()),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);

        sender.data(1);
        harness.frame();
        assert_eq!(last(&log).data, Some(1));
    }
}
