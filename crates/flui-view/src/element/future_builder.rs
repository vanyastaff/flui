//! `FutureBuilder` — build from the latest state of a future.
//!
//! # Public shape
//!
//! Exported from `flui-view::element` and re-exported by `flui-widgets` plus its
//! prelude once the design passed its Flutter-parity gate. The keyed identity shape is signed
//! off by the repository owner; this repository has no separate api-design-lead
//! role. The state type is public only because Rust requires a public associated
//! `State` type for a public `StatefulView` implementation; it remains opaque.
//!
//! # How the seams compose
//!
//! - `RebuildHandle` — captured in `init_state`, called from the task's
//!   completion to schedule a rebuild. Never acquired in `build`.
//! - `AsyncDriver` — reached through [`LifecycleContext::async_driver`], which
//!   yields the driver *this binding's frame step polls*. The task is spawned
//!   with `spawn_local_eager`, so an immediately-ready future completes inline.
//! - [`AsyncSnapshot`] / [`ConnectionState`] — the state machine.
//!
//! # Identity is an explicit key
//!
//! Flutter compares `oldWidget.future == widget.future`. A Rust `Future` is
//! move-only, not `Clone`, not `Eq`, and cannot live in a `Clone` view. So the
//! view carries `key: Option<K>` plus a factory; the subscription is recreated
//! exactly when the key changes, and `None` means "no future" (Flutter's null
//! future). This also makes Flutter's worst `FutureBuilder` footgun —
//! constructing the future inside `build` — unrepresentable.
//!
//! **Divergence from the ADR sketch:** the ADR wrote `make: impl FnOnce() -> Fut`.
//! A view is cloned on every rebuild, so the factory must be `Fn`, not `FnOnce`.
//! It is called once per subscription.
//!
//! # The synchronous-completion window
//!
//! `init_state` runs inside `build_scope`, and the frame's driver step already
//! ran *before* `build_scope`. A plain `spawn_local` would therefore first poll
//! on the next frame, and the first build would show `Waiting` even for a ready
//! future — diverging from Flutter's `SynchronousFuture` behavior.
//! `AsyncDriver::spawn_local_eager` polls once inline at subscribe time,
//! exactly as Dart's synchronous `.then` runs inline inside `initState`.
//!
//! While that inline poll runs, `inline_window` is set: a completion landing in
//! it writes the snapshot but does **not** schedule a rebuild, because the build
//! that will read it has not happened yet. `after_subscribe` then leaves an
//! already-`Done` snapshot alone rather than dragging it back to `Waiting`.

use std::{future::Future, pin::Pin, rc::Rc, sync::Arc};

use flui_foundation::{AsyncSnapshot, ConnectionState};
use flui_scheduler::{AsyncDriver, TaskToken};
use parking_lot::Mutex;

use super::async_slot::{InitialDataFactory, SharedSlot, Slot, SnapshotBuilder, apply_fold};
use crate::{
    RebuildHandle,
    context::{BuildContext, LifecycleContext},
    view::{IntoView, StatefulView, View, ViewState},
};

/// A boxed, `Send` future yielding `Result<T, E>`.
pub type BoxedResultFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'static>>;

/// Produces the future to await. `Fn`, not `FnOnce`: the view is cloned on every
/// rebuild. Called once per subscription.
pub type FutureFactory<T, E> = Rc<dyn Fn() -> BoxedResultFuture<T, E>>;

/// Fold a completion into the shared snapshot, honouring the generation guard.
///
/// `Ok` ⇒ `after_success` (Done + data); `Err` ⇒ `after_failure` (Done + error).
/// Returns whether a rebuild must be scheduled.
fn apply_completion<T, E>(slot: &SharedSlot<T, E>, generation: u64, result: Result<T, E>) -> bool {
    apply_fold(slot, generation, |snapshot| match result {
        Ok(data) => snapshot.after_success(data),
        Err(error) => snapshot.after_failure(error),
    })
}

// ============================================================================
// VIEW
// ============================================================================

/// A view that builds itself from the latest interaction with a future.
pub struct FutureBuilder<K, T, E> {
    /// Identity of the future. `None` ⇒ no future (Flutter's null future).
    key: Option<K>,
    /// Creates the future when the subscription starts.
    make: FutureFactory<T, E>,
    /// Optional seed value, applied only at `init_state`.
    initial_data: Option<InitialDataFactory<T>>,
    /// Builds the child from the snapshot.
    builder: SnapshotBuilder<T, E>,
}

impl<K: Clone, T, E> Clone for FutureBuilder<K, T, E> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            make: Rc::clone(&self.make),
            initial_data: self.initial_data.clone(),
            builder: Rc::clone(&self.builder),
        }
    }
}

impl<K: std::fmt::Debug, T, E> std::fmt::Debug for FutureBuilder<K, T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FutureBuilder")
            .field("key", &self.key)
            .field("has_initial_data", &self.initial_data.is_some())
            .finish_non_exhaustive()
    }
}

/// Bounds shared by the view and its state. `T`/`E` are `Send + 'static` because
/// the task moves them across the driver; neither needs `Clone`.
impl<K, T, E> FutureBuilder<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    /// Subscribe to the future identified by `key`; `None` means no future.
    pub fn keyed(
        key: Option<K>,
        make: FutureFactory<T, E>,
        builder: SnapshotBuilder<T, E>,
    ) -> Self {
        Self {
            key,
            make,
            initial_data: None,
            builder,
        }
    }

    /// Seed the snapshot with `initial_data` before the first subscription.
    ///
    /// Flutter's `FutureBuilder.initialData`. It is applied **only** at
    /// `init_state`; a later key change does not re-apply it.
    #[must_use]
    pub fn with_initial_data(mut self, initial_data: InitialDataFactory<T>) -> Self {
        self.initial_data = Some(initial_data);
        self
    }
}

impl<K, T, E> StatefulView for FutureBuilder<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    type State = FutureBuilderState<K, T, E>;

    fn create_state(&self) -> Self::State {
        // `ViewState::init_state` is handed a `BuildContext` but NOT the view, so
        // the configuration the first subscription needs is copied here. Later
        // rebuilds reach the fresh view through `did_update_view` / `build`.
        FutureBuilderState {
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

impl<K, T, E> View for FutureBuilder<K, T, E>
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

/// Persistent state for [`FutureBuilder`] — **opaque**.
///
/// `pub` only because it is the `State` associated type of a public
/// [`StatefulView`] impl and Rust forbids a crate-private type there. It has no
/// public fields and no public methods; construct it only through
/// `FutureBuilder::create_state`.
pub struct FutureBuilderState<K, T, E> {
    /// The snapshot the builder reads, written by the task.
    slot: SharedSlot<T, E>,
    /// Captured in `init_state` — the only lifecycle hook handed a `BuildContext`
    /// that runs before a completion can arrive. `did_update_view` and `dispose`
    /// receive no context, which is exactly why this must be owned.
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
    initial_make: FutureFactory<T, E>,
    /// The view's `initialData` factory at mount. Applied once, never re-applied.
    initial_data: Option<InitialDataFactory<T>>,
}

impl<K, T, E> std::fmt::Debug for FutureBuilderState<K, T, E>
where
    K: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slot = self.slot.lock();
        f.debug_struct("FutureBuilderState")
            .field("key", &self.key)
            .field("connection_state", &slot.snapshot.connection_state())
            .field("generation", &slot.generation)
            .field("subscribed", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl<K, T, E> FutureBuilderState<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    /// Cancel the live subscription, if any, and invalidate its generation so a
    /// completion already in flight is discarded.
    ///
    /// Flutter's `_unsubscribe`, which merely clears `_activeCallbackIdentity` —
    /// Dart cannot cancel a future. Dropping the token here stops the producer.
    fn unsubscribe(&mut self) {
        self.token = None; // Drop cancels.
        self.key = None;
        self.slot.lock().generation += 1;
    }

    /// Start a subscription for `key` using `make`.
    ///
    /// Mirrors `_FutureBuilderState._subscribe`: spawn, then move the snapshot to
    /// `Waiting` **unless the future already completed inline** (Flutter's
    /// `if (_snapshot.connectionState != ConnectionState.done)` guard for
    /// `SynchronousFuture`).
    fn subscribe(&mut self, key: K, make: &FutureFactory<T, E>) {
        let Some(driver) = self.driver.clone() else {
            // No binding installed a driver: nothing can poll the future. Report
            // it rather than spawn into a driver nobody drives.
            tracing::warn!(
                "FutureBuilder: no async driver on this BuildContext; the future \
                 will never be polled. Is the tree bound to a binding?"
            );
            return;
        };
        let handle = self.handle.clone().unwrap_or_else(RebuildHandle::inert);

        let generation = {
            let mut slot = self.slot.lock();
            slot.generation += 1;
            slot.inline_window = true;
            slot.generation
        };

        let future = make();
        let slot_for_task = Arc::clone(&self.slot);

        let token = driver.spawn_local_eager(Box::pin(async move {
            let result = future.await;
            if apply_completion(&slot_for_task, generation, result) {
                handle.schedule(crate::RebuildReason::AsyncCompletion);
            }
        }));

        {
            let mut slot = self.slot.lock();
            slot.inline_window = false;
            slot.fold(AsyncSnapshot::after_subscribe);
        }

        self.token = token;
        self.key = Some(key);
    }
}

impl<K, T, E> ViewState<FutureBuilder<K, T, E>> for FutureBuilderState<K, T, E>
where
    K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    T: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    /// `_FutureBuilderState.initState`: seed from `initialData`, then subscribe.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // Capture the capabilities here — the ONLY lifecycle hook handed a
        // context. `did_update_view` and `dispose` receive none.
        self.handle = Some(ctx.rebuild_handle());
        self.driver = ctx.async_driver();

        // `initial(Some(d))` is `with_data(None, d)`; `initial(None)` is `nothing()`.
        self.slot.lock().snapshot = match &self.initial_data {
            Some(initial_data) => AsyncSnapshot::with_data(ConnectionState::None, initial_data()),
            None => AsyncSnapshot::nothing(),
        };

        // An absent key is Flutter's null future: no subscription, snapshot stays
        // where `initial` left it.
        if let Some(key) = self.initial_key.clone() {
            let make = Rc::clone(&self.initial_make);
            self.subscribe(key, &make);
        }
    }

    fn build(&self, view: &FutureBuilder<K, T, E>, ctx: &dyn BuildContext) -> impl IntoView {
        let slot = self.slot.lock();
        (view.builder)(ctx, &slot.snapshot)
    }

    /// `_FutureBuilderState.didUpdateWidget`: an unchanged key is an early
    /// return; a changed one unsubscribes, drops to `None` **preserving the old
    /// data/error**, and resubscribes to `Waiting`. `initialData` is never
    /// re-applied.
    fn did_update_view(
        &mut self,
        old_view: &FutureBuilder<K, T, E>,
        new_view: &FutureBuilder<K, T, E>,
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
    include!("future_builder_lifetime_tests.rs");

    use super::*;

    use std::task::{Context, Poll, Waker};

    use flui_foundation::ElementId;
    use flui_scheduler::UpdateScheduler;

    use crate::view::{ErrorView, ViewExt};
    use crate::{BuildOwner, tree::ElementTree};

    /// Deliberately neither `Clone` nor `Copy`, to prove the surface needs neither.
    #[derive(Debug, PartialEq)]
    struct Payload(i32);

    /// Likewise for the error.
    #[derive(Debug, PartialEq)]
    struct Boom(&'static str);

    /// What a build observed, flattened so the test can assert without `Clone`.
    #[derive(Debug, PartialEq, Clone, Copy)]
    struct Seen {
        state: ConnectionState,
        data: Option<i32>,
        error: Option<&'static str>,
    }

    /// A future the test completes by hand.
    struct Controlled {
        result: Arc<Mutex<Option<Result<Payload, Boom>>>>,
        waker: Arc<Mutex<Option<Waker>>>,
    }

    impl Future for Controlled {
        type Output = Result<Payload, Boom>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let result = self.result.lock().take();
            if let Some(result) = result {
                Poll::Ready(result)
            } else {
                let _prev = self.waker.lock().replace(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    /// Handle to complete a `Controlled` future from the test.
    #[derive(Clone)]
    struct Completer {
        result: Arc<Mutex<Option<Result<Payload, Boom>>>>,
        waker: Arc<Mutex<Option<Waker>>>,
    }

    impl Completer {
        fn new() -> Self {
            Self {
                result: Arc::new(Mutex::new(None)),
                waker: Arc::new(Mutex::new(None)),
            }
        }

        fn factory(&self) -> FutureFactory<Payload, Boom> {
            let result = Arc::clone(&self.result);
            let waker = Arc::clone(&self.waker);
            Rc::new(move || {
                Box::pin(Controlled {
                    result: Arc::clone(&result),
                    waker: Arc::clone(&waker),
                })
            })
        }

        /// Complete from outside a frame, as a real async completion would.
        fn complete(&self, result: Result<Payload, Boom>) {
            *self.result.lock() = Some(result);
            if let Some(waker) = self.waker.lock().as_ref() {
                waker.wake_by_ref();
            }
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
    /// `UpdateScheduler::drive_async_tasks()` (the shared async step) then
    /// `BuildOwner::build_scope()`. Not a bespoke loop — it is `pump_frame`'s
    /// body minus the parts (clock, gestures, pipeline) a `FutureBuilder` cannot
    /// observe. `flui-view` cannot depend on `flui-testing` (that would cycle).
    struct Harness {
        owner: BuildOwner,
        tree: ElementTree,
        scheduler: UpdateScheduler,
        root: ElementId,
    }

    impl Harness {
        fn mount<K>(view: &FutureBuilder<K, Payload, Boom>) -> Self
        where
            K: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
        {
            let scheduler = UpdateScheduler::new();
            let mut owner = BuildOwner::new();
            owner.set_async_driver(scheduler.async_driver().clone());
            let mut tree = ElementTree::new();

            let root = tree.mount_root(view, &mut owner.element_owner_mut());
            owner.schedule_build_for(root, 0, crate::RebuildReason::InitialMount);
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
    }

    fn seen(log: &Arc<Mutex<Vec<Seen>>>) -> Vec<Seen> {
        log.lock().clone()
    }

    fn last(log: &Arc<Mutex<Vec<Seen>>>) -> Seen {
        *log.lock().last().expect("at least one build")
    }

    // ── absent future ───────────────────────────────────────────────────────

    // ── life cycle ──────────────────────────────────────────────────────────

    /// `'tracks life-cycle of Future to success'`: `Waiting` → `Done + data`,
    /// observed through the normal frame path.
    #[test]
    fn future_builder_pending_future_waits_then_completes_with_data() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let completer = Completer::new();
        let view = FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
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
            "the first build shows Waiting"
        );
        assert_eq!(harness.scheduler.pending_task_count(), 1);

        completer.complete(Ok(Payload(42)));
        harness.frame();

        assert_eq!(
            last(&log),
            Seen {
                state: ConnectionState::Done,
                data: Some(42),
                error: None
            },
            "the completion is observed in the frame that polls it"
        );
        assert_eq!(harness.scheduler.pending_task_count(), 0);
    }

    // ── update semantics ────────────────────────────────────────────────────

    /// A disposed `FutureBuilder` neither rebuilds nor resolves: dropping the
    /// `TaskToken` cancels the task, so its writer never runs.
    ///
    /// This proves **cancellation**, not the generation guard — the guard is
    /// unreachable through the widget precisely because cancellation gets there
    /// first. `apply_completion_*` below tests the guard directly.
    #[test]
    fn future_builder_dispose_cancels_and_never_rebuilds() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let completer = Completer::new();
        let view = FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
            recording_builder(Arc::clone(&log)),
        );
        let mut harness = Harness::mount(&view);
        let builds_before = seen(&log).len();

        // Disposing bumps the generation and cancels the token.
        harness
            .tree
            .remove(harness.root, &mut harness.owner.element_owner_mut());

        completer.complete(Ok(Payload(9)));
        harness.frame();

        assert_eq!(
            seen(&log).len(),
            builds_before,
            "a disposed FutureBuilder must not rebuild"
        );
        assert_eq!(
            harness.owner.pending_external_builds(),
            0,
            "no rebuild queued"
        );
    }

    // ── the generation guard, tested directly ───────────────────────────────

    // ── bounds ──────────────────────────────────────────────────────────────
}
