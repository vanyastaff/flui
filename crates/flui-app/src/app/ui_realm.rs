//! `UiRealm` — the owner-affine UI-session composition root.
//!
//! One realm, one owner: the runtime is the single logical owner of a
//! window's UI state and is structurally `!Send + !Sync`. Everything that
//! crosses a thread boundary does so as a typed [`UiCommandSender`]
//! capability feeding a **bounded** inbox, whose contents the owner commits
//! **only while the scheduler phase is Idle** — at frame boundaries, never
//! inside the frame transaction. This is the generalization of the
//! `RebuildHandle`/`PipelineOwnerHandle` pattern: enqueue-and-wake, never
//! touch the tree.
//!
//! # Transitional coupling
//!
//! Until singleton retirement (in a prior iteration), the runtime
//! coexists with the process-global `AppBinding`/`Scheduler` graph rather
//! than owning those subsystems. A per-window type over process-global
//! internals would be a lying API, so construction enforces **at most one
//! live runtime per process** ([`UiRealmError::AlreadyExists`]); the
//! guard retires with the singletons. Each incarnation still gets a fresh
//! generational [`RealmId`], so results stamped for a dead runtime are
//! droppable by identity, not by convention.

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::{HasInstance, PresentationId, RealmId};
use flui_interaction::{FocusManager, GestureBinding, InteractionLane, TextInputOwner};
#[cfg(test)]
use flui_platform::traits::PlatformTextInput;
use flui_platform::traits::PlatformWindow;
use flui_scheduler::{AppLifecycleState, LocalPostFrameLane, Scheduler, SchedulerPhase};
use flui_semantics::SemanticsActionRequest;
use flui_view::WidgetsBinding;
use flui_widgets::NavigatorCommand;

use super::presentation::PresentationState;

/// Default bound of the owner inbox, matching the pipeline dirty-channel
/// precedent (`DEFAULT_DIRTY_CHANNEL_CAPACITY`)). Observable at
/// runtime via `UiCommandSender::capacity`; not part of the public API.
const DEFAULT_COMMAND_CAPACITY: usize = 256;

/// Claim flag for the at-most-one-instance transitional guard.
static REALM_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Monotonic incarnation counter: every successfully constructed runtime
/// gets a fresh `RealmId` generation, so a recreated realm never compares
/// equal to its predecessor.
static NEXT_INCARNATION: AtomicU32 = AtomicU32::new(1);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors constructing a [`UiRealm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub(crate) enum UiRealmError {
    /// A `UiRealm` is already live in this process.
    ///
    /// Transitional: the runtime still fronts process-global binding state,
    /// so a second instance would alias it while claiming isolation. The
    /// guard retires with the singletons.
    #[error(
        "a UiRealm is already live in this process; the at-most-one guard \
         holds until singleton retirement (in a prior iteration)"
    )]
    AlreadyExists,
    /// The owner-local interaction lane could not be created.
    #[error("failed to create the realm interaction lane: {0}")]
    InteractionLane(#[from] flui_interaction::InteractionDispatchError),
}

/// Errors returned by [`UiCommandSender`] sends.
///
/// Same shape as the pipeline dirty-channel errors: bounded channels surface
/// backpressure as a typed value, and a dropped owner is a typed value — the
/// producer decides what to do, nothing blocks, nothing grows unbounded.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandSendError {
    /// The inbox is full; the producer must back off (retry next frame,
    /// drop, or escalate — its call).
    #[error("realm command inbox full ({capacity} capacity); back off and retry")]
    ChannelFull {
        /// Configured inbox capacity.
        capacity: usize,
        /// Rejected command, returned intact so the framework can retry.
        rejected: UiCommand,
    },

    /// The owning [`UiRealm`] has been dropped; this sender is now
    /// permanently inert and the producer should stop sending.
    #[error("ui realm dropped; command sender is no longer valid")]
    OwnerGone {
        /// Rejected command, returned intact to the framework caller.
        rejected: UiCommand,
    },
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "payload recovery is exercised by the protocol tests"
    )
)]
impl CommandSendError {
    fn into_rejected(self) -> UiCommand {
        match self {
            Self::ChannelFull { rejected, .. } | Self::OwnerGone { rejected } => rejected,
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A command enqueued for the owner thread.
pub(crate) enum UiCommand {
    /// Apply a hot-reload reassemble on the owner at the next Idle drain.
    HotReload(flui_hot_reload::HotReloadTier),
    /// Resolve and invoke an accessibility action on the owner thread.
    SemanticsAction(SemanticsActionRequest),
    /// Apply a typed navigator mutation on the owner thread.
    Navigation(NavigatorCommand),
}

impl std::fmt::Debug for UiCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UiCommand::HotReload(tier) => {
                f.debug_tuple("UiCommand::HotReload").field(tier).finish()
            }
            UiCommand::SemanticsAction(request) => f
                .debug_tuple("UiCommand::SemanticsAction")
                .field(request)
                .finish(),
            UiCommand::Navigation(command) => f
                .debug_tuple("UiCommand::Navigation")
                .field(command)
                .finish(),
        }
    }
}

/// What one [`UiRealm::drain_commands`] pass did, for observability
/// and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub(crate) struct DrainReport {
    /// Owner commands successfully applied.
    pub invoked: usize,
    /// Commands whose typed owner target is stale or no longer live.
    pub dropped_stale: usize,
}

// ---------------------------------------------------------------------------
// Sender
// ---------------------------------------------------------------------------

/// Cross-thread capability into a [`UiRealm`]'s inbox.
///
/// `Clone + Send + Sync`. A sender can enqueue a command and wake the owner;
/// it can never obtain a reference into any tree, invoke a lifecycle
/// callback, or run build/layout/paint. Every enqueued command
/// executes on the owner thread, at the next Idle drain.
#[derive(Clone)]
pub(crate) struct UiCommandSender {
    tx: Sender<UiCommand>,
    capacity: usize,
    redraw_pending: Arc<AtomicBool>,
    /// Fired after every successful state change so an idle event loop
    /// produces the drain that observes it — the enqueue-then-wake contract,
    /// same as `PipelineOwnerHandle`'s notifier.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for UiCommandSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiCommandSender")
            .field("capacity", &self.capacity)
            .field("pending", &self.tx.len())
            .field(
                "redraw_pending",
                &self.redraw_pending.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl UiCommandSender {
    /// Enqueue a hot-reload request for the realm owner.
    ///
    /// Unlike direct platform dispatch, this capability is safe to call from
    /// any thread: delivery occurs at the owner's next Idle drain and the
    /// normal enqueue-and-wake contract pumps that drain.
    pub(crate) fn request_hot_reload(
        &self,
        tier: flui_hot_reload::HotReloadTier,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::HotReload(tier))
    }

    /// Enqueue an accessibility action for owner-local semantics resolution.
    ///
    /// The sender itself selects the target realm/presentation; the request
    /// carries only the stable node identity exported by that presentation's
    /// snapshot. Delivery is bounded, FIFO, and committed only at the next
    /// Idle drain.
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "vended to the platform accessibility adapter in the AccessKit slice"
        )
    )]
    pub(crate) fn send_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::SemanticsAction(request))
    }

    /// Enqueue a typed navigation command for owner-thread application.
    ///
    /// This is the ADR-0027 cross-thread navigation ingress. The sender only
    /// accepts the closed [`NavigatorCommand`] vocabulary; it does not expose a
    /// generic "run this closure on the UI thread" API.
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "typed navigation command sender is wired before public runtime vending"
        )
    )]
    pub(crate) fn send_navigation(
        &self,
        command: NavigatorCommand,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::Navigation(command))
    }

    /// Request a redraw of the realm's presentation, coalesced: any number of pending
    /// requests collapse into one flag read by the owner at the next drain
    /// (the `needs_redraw` precedent — idempotent dirty marks).
    ///
    /// Infallible and idempotent by design: the flag outlives the runtime,
    /// and a request against a dropped runtime is a harmless no-op (the wake
    /// has no loop left to wake).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "typed redraw capability is not yet vended externally"
        )
    )]
    pub(crate) fn request_redraw(&self) {
        // `swap` (not store) so only the first request in a burst pays the
        // wake; a pending frame absorbs repeated wakes anyway, this just
        // skips redundant platform calls.
        if !self.redraw_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }

    /// The inbox's configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn send(&self, command: UiCommand) -> Result<(), CommandSendError> {
        match self.tx.try_send(command) {
            Ok(()) => {
                (self.wake)();
                Ok(())
            }
            Err(TrySendError::Full(rejected)) => Err(CommandSendError::ChannelFull {
                capacity: self.capacity,
                rejected,
            }),
            Err(TrySendError::Disconnected(rejected)) => {
                Err(CommandSendError::OwnerGone { rejected })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

/// The per-window owner: receives the inbox, drains it at Idle, and is
/// structurally confined to its construction thread.
///
/// `!Send + !Sync` by construction (raw-pointer `PhantomData` marker) — the
/// compiler, not convention, keeps the owner on its thread. Cross-thread
/// access goes through [`UiCommandSender`] only.
pub(crate) struct UiRealm {
    realm_id: RealmId,
    /// Owner-local widget framework state. It is deliberately absent from the
    /// process-global `AppBinding`; every widget-tree operation enters through
    /// this realm and activates this binding's GlobalKey registry.
    widgets: WidgetsBinding,
    /// Owner-local callback queue, activated with the realm's other TLS scope.
    local_post_frame: LocalPostFrameLane,
    /// Owner-local interaction callback storage, activated with the realm scope.
    interaction_lane: InteractionLane,
    /// The current UI-owner presentation domain.
    ///
    /// The realm has one presentation until the element tree becomes a forest
    /// with root-scoped capabilities. The nominal identity exists now so no
    /// command or resource needs to overload `RealmId` or a native window id.
    presentation: PresentationState,
    rx: Receiver<UiCommand>,
    /// Prototype for [`Self::command_sender`]: crossbeam receivers cannot
    /// mint senders, so the runtime keeps one sender to clone from. Holding
    /// it here does not keep the channel alive past the runtime: `rx` drops
    /// with the runtime and every outstanding sender turns `OwnerGone`.
    sender_prototype: UiCommandSender,
    redraw_pending: Arc<AtomicBool>,
    /// Whether this instance owns the transitional process-wide claim.
    claimed: bool,
    /// `*const ()` is `!Send + !Sync`; `PhantomData` of it makes the runtime
    /// so at zero cost (thread-affinity marker).
    _owner_affine: PhantomData<*const ()>,
}

impl std::fmt::Debug for UiRealm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiRealm")
            .field("realm_id", &self.realm_id)
            .field("presentation_id", &self.presentation.id())
            .field("pending_commands", &self.rx.len())
            .field(
                "redraw_pending",
                &self.redraw_pending.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl UiRealm {
    /// Construct the runtime with the default inbox capacity.
    ///
    /// `wake` is the platform wake: it must deliver a wake to the owner's
    /// event loop without spawning a thread — in the current
    /// desktop runner that is `AppBinding::wake_frame`.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::AlreadyExists`] while another runtime is live
    /// (transitional at-most-one guard, see module docs).
    pub(crate) fn new(
        app: &super::binding::AppBinding,
        wake: Arc<dyn Fn() + Send + Sync>,
        window: Arc<dyn PlatformWindow>,
    ) -> Result<Self, UiRealmError> {
        Self::with_capacity(app, DEFAULT_COMMAND_CAPACITY, wake, window)
    }

    /// [`Self::new`] with an explicit inbox capacity.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::AlreadyExists`] while another runtime is live.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` (a zero-capacity inbox could never accept
    /// a command; every sender would spuriously report backpressure).
    pub(crate) fn with_capacity(
        app: &super::binding::AppBinding,
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
        window: Arc<dyn PlatformWindow>,
    ) -> Result<Self, UiRealmError> {
        assert!(capacity > 0, "UiRealm inbox capacity must be non-zero");
        if REALM_CLAIMED.swap(true, Ordering::AcqRel) {
            return Err(UiRealmError::AlreadyExists);
        }
        let (realm_id, presentation_id) = Self::next_identity();
        let presentation =
            PresentationState::new(presentation_id, app.render_pipeline_arc(), window);
        match Self::construct(capacity, wake, realm_id, presentation, true) {
            Ok(realm) => Ok(realm),
            Err(error) => {
                REALM_CLAIMED.store(false, Ordering::Release);
                Err(error)
            }
        }
    }

    fn construct(
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
        realm_id: RealmId,
        presentation: PresentationState,
        claimed: bool,
    ) -> Result<Self, UiRealmError> {
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        let local_post_frame = Scheduler::instance().local_post_frame_lane();
        let interaction_lane = InteractionLane::try_new()?;
        let widgets = WidgetsBinding::with_focus_manager(presentation.focus_manager());
        widgets.set_pipeline_owner(Arc::clone(presentation.pipeline()));
        widgets.with_build_owner_mut(|owner| {
            owner.set_async_driver(Scheduler::instance().async_driver().clone());
            owner.set_post_frame_handle(local_post_frame.post_frame_handle());
            owner.set_interaction_dispatch_handle(interaction_lane.dispatch_handle());
            owner.set_text_input_handle(presentation.text_input_handle());
        });
        Ok(Self {
            realm_id,
            widgets,
            local_post_frame,
            interaction_lane,
            presentation,
            rx,
            sender_prototype: UiCommandSender {
                tx,
                capacity,
                redraw_pending: Arc::clone(&redraw_pending),
                wake,
            },
            redraw_pending,
            claimed,
            _owner_affine: PhantomData,
        })
    }

    fn next_identity() -> (RealmId, PresentationId) {
        let incarnation = NEXT_INCARNATION.fetch_add(1, Ordering::Relaxed);
        let generation = NonZeroU32::new(incarnation)
            .expect("BUG: incarnation counter starts at 1 and only increments");
        // Slot 0 is the single-window slot; a real multi-window `AppRuntime`
        // registry mints slots once it exists — the shape is the deliverable,
        // single-window the only instantiation for now.
        (
            RealmId::new_gen(0, generation),
            PresentationId::new_gen(0, generation),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test(app: &super::binding::AppBinding) -> Self {
        Self::for_test_with_text_input(app, None)
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_text_input(
        app: &super::binding::AppBinding,
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let (realm_id, presentation_id) = Self::next_identity();
        let presentation = PresentationState::new_for_test(
            presentation_id,
            app.render_pipeline_arc(),
            platform_text_input,
        );
        Self::construct(
            DEFAULT_COMMAND_CAPACITY,
            Arc::new(|| {}),
            realm_id,
            presentation,
            false,
        )
        .expect("test UiRealm should create an interaction lane")
    }

    /// This incarnation's generational realm identity.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.realm_id
    }

    /// Current presentation incarnation.
    #[must_use]
    pub fn presentation_id(&self) -> PresentationId {
        self.presentation.id()
    }

    /// A new cross-thread sender into this runtime's inbox.
    #[must_use]
    pub fn command_sender(&self) -> UiCommandSender {
        self.sender_prototype.clone()
    }

    /// Enter this realm's owner scope.
    ///
    /// The GlobalKey registry is active for the entire dynamic extent of `f`,
    /// including lifecycle/build callbacks. Nested entry is stack-shaped and
    /// panic unwinding restores the previously active realm.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        self.local_post_frame.enter(|| {
            self.interaction_lane
                .enter(|| self.widgets.with_global_key_registry(|| f(self)))
        })
    }

    /// Owner-local widgets binding. Crate-private so callers cannot bypass the
    /// guarded realm entry boundary.
    pub(crate) fn widgets(&self) -> &WidgetsBinding {
        &self.widgets
    }

    /// Gesture state for the realm's current single presentation.
    ///
    /// Crate-private so platform input can only reach it through the entered
    /// realm dispatch path rather than exposing a second public owner seam.
    pub(crate) fn gestures(&self) -> &GestureBinding {
        self.presentation.gestures()
    }

    /// Focus state for the realm's current presentation.
    #[must_use]
    pub(crate) fn focus_manager(&self) -> Rc<FocusManager> {
        self.presentation.focus_manager()
    }

    /// Text-input state for the realm's current single presentation.
    pub(crate) fn text_input(&self) -> &TextInputOwner {
        self.presentation.text_input()
    }

    /// Weak text-input capability for this exact presentation.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn text_input_handle(&self) -> flui_interaction::TextInputHandle {
        self.presentation.text_input_handle()
    }

    /// Keep presentation-owned resources aligned with the synthesized
    /// application lifecycle delivered by the platform runner.
    pub(crate) fn handle_presentation_lifecycle(&self, state: AppLifecycleState) {
        match state {
            AppLifecycleState::Resumed | AppLifecycleState::Inactive => {
                self.presentation.resume();
            }
            AppLifecycleState::Hidden | AppLifecycleState::Paused => {
                self.presentation.suspend();
            }
            AppLifecycleState::Detached => {
                self.presentation.close();
            }
        }
    }

    /// Reassemble this realm's element tree and exact presentation pipeline.
    #[must_use]
    pub(crate) fn apply_hot_reload(&self, tier: flui_hot_reload::HotReloadTier) -> bool {
        self.presentation.apply_hot_reload(&self.widgets, tier)
    }

    /// Consume the coalesced redraw request, if any.
    ///
    /// The runner merges this into its dirty gate each frame; reading clears
    /// the flag so the next request wakes again.
    #[must_use]
    pub fn take_redraw_request(&self) -> bool {
        self.redraw_pending.swap(false, Ordering::AcqRel)
    }

    /// Drain the closed command inbox on the owner thread in strict FIFO
    /// order.
    ///
    /// Call only at frame boundaries — immediately before entering
    /// `drive_frame` and/or after it returns — never inside the frame
    /// transaction. Enforced in debug builds against the transitional global
    /// scheduler's phase; the thread affinity itself is structural
    /// (`UiRealm: !Send + !Sync`), not asserted.
    pub fn drain_commands(&self) -> DrainReport {
        debug_assert_eq!(
            Scheduler::instance().phase(),
            SchedulerPhase::Idle,
            "UiRealm::drain_commands must run at a frame boundary (Idle), \
             never inside the frame transaction"
        );
        let mut report = DrainReport::default();
        // Bound the pass by the pre-read length: `try_iter` is NOT a
        // snapshot — it keeps yielding messages that arrive during
        // iteration, so an unbounded loop could be extended indefinitely by
        // a producer keeping pace (or by a drained command re-enqueueing
        // through a sender clone). Commands sent during this drain land in
        // the NEXT drain — deterministic batches, no owner starvation.
        let pending = self.rx.len();
        for _ in 0..pending {
            let Ok(command) = self.rx.try_recv() else {
                break;
            };
            match command {
                UiCommand::HotReload(tier) => {
                    if self.presentation.apply_hot_reload(&self.widgets, tier) {
                        self.redraw_pending.store(true, Ordering::Release);
                    }
                    report.invoked += 1;
                }
                UiCommand::SemanticsAction(request) => {
                    match self.presentation.dispatch_semantics_action(request) {
                        Ok(()) => {
                            report.invoked += 1;
                        }
                        Err(error) => {
                            // Flutter deliberately ignores actions for stale
                            // views/nodes because screen readers may lag behind
                            // the latest semantics update.
                            tracing::trace!(
                                presentation_id = ?self.presentation.id(),
                                ?error,
                                "dropping semantics action against a stale snapshot"
                            );
                            report.dropped_stale += 1;
                        }
                    }
                }
                UiCommand::Navigation(command) => match command.apply_on_owner() {
                    Ok(_) => {
                        report.invoked += 1;
                    }
                    Err(error) => {
                        tracing::trace!(
                            ?error,
                            "dropping navigation command that no longer reaches its owner"
                        );
                        report.dropped_stale += 1;
                    }
                },
            }
        }
        report
    }
}

impl Drop for UiRealm {
    fn drop(&mut self) {
        self.presentation.close();
        if self.claimed {
            REALM_CLAIMED.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    use flui_foundation::RenderId;
    use flui_semantics::{
        AccessibilityNodeId, SemanticsAction, SemanticsActionRequest, SemanticsNode, SemanticsOwner,
    };
    use flui_view::prelude::*;
    use flui_widgets::{NavigatorCommand, NavigatorHandle, SimpleRoute, SizedBox};

    use super::*;

    static_assertions::assert_not_impl_any!(UiRealm: Send, Sync);

    /// Serializes tests that claim the process-global `REALM_CLAIMED`
    /// flag (the repo rule for tests mutating shared binding state —
    /// AGENTS.md "Testing Quirks"). nextest gives each test its own
    /// process, but `cargo test` / IDE runners share one.
    static REALM_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn noop_wake() -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(|| {})
    }

    fn counting_wake() -> (Arc<dyn Fn() + Send + Sync>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let count_in_wake = Arc::clone(&count);
        (
            Arc::new(move || {
                count_in_wake.fetch_add(1, Ordering::Relaxed);
            }),
            count,
        )
    }

    fn test_window() -> Arc<dyn PlatformWindow> {
        flui_platform::headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window")
    }

    fn new_runtime(wake: Arc<dyn Fn() + Send + Sync>) -> Result<UiRealm, UiRealmError> {
        let app = super::super::binding::AppBinding::new();
        UiRealm::new(&app, wake, test_window())
    }

    fn new_runtime_with_capacity(
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<UiRealm, UiRealmError> {
        let app = super::super::binding::AppBinding::new();
        UiRealm::with_capacity(&app, capacity, wake, test_window())
    }

    #[test]
    fn senders_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UiCommandSender>();
    }

    #[test]
    fn realm_entry_activates_its_global_key_registry() {
        let _claim = REALM_TEST_LOCK.lock();
        let realm = new_runtime(noop_wake()).expect("runtime");
        let key = flui_view::GlobalKey::<()>::new();
        let element = flui_foundation::ElementId::new(17);
        realm
            .widgets()
            .with_build_owner_mut(|owner| owner.register_global_key(key.id(), element));

        assert_eq!(
            key.current_element(),
            None,
            "no realm is active outside enter"
        );
        realm.enter(|_| {
            assert_eq!(key.current_element(), Some(element));
        });
        assert_eq!(
            key.current_element(),
            None,
            "enter restores quiescent state"
        );
    }

    #[test]
    fn presentation_and_widget_tree_share_the_exact_focus_owner() {
        let _claim = REALM_TEST_LOCK.lock();
        let realm = new_runtime(noop_wake()).expect("runtime");

        let presentation_focus = realm.focus_manager();
        let widget_focus = realm
            .widgets()
            .with_build_owner(flui_view::BuildOwner::focus_manager);

        assert!(
            Rc::ptr_eq(&presentation_focus, &widget_focus),
            "keyboard dispatch and every BuildContext must address one focus tree"
        );
    }

    #[test]
    fn at_most_one_runtime_second_construction_fails_typed() {
        let _claim = REALM_TEST_LOCK.lock();
        let first = new_runtime(noop_wake()).expect("first runtime claims");
        let second = new_runtime(noop_wake());
        assert!(matches!(second, Err(UiRealmError::AlreadyExists)));
        drop(first);
        let third = new_runtime(noop_wake()).expect("claim released on drop");
        drop(third);
    }

    #[test]
    fn recreated_runtime_gets_fresh_realm_id() {
        let _claim = REALM_TEST_LOCK.lock();
        let first = new_runtime(noop_wake()).expect("first runtime");
        let first_id = first.realm_id();
        drop(first);
        let second = new_runtime(noop_wake()).expect("second incarnation");
        assert_ne!(
            first_id,
            second.realm_id(),
            "a recreated window must never compare equal to its predecessor"
        );
    }

    #[test]
    fn cross_thread_navigation_command_drains_on_owner_thread() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();

        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        let pushed = navigator.push(test_route("/details"));
        let target = navigator.command_target();

        std::thread::spawn(move || {
            sender
                .send_navigation(NavigatorCommand::pop(target))
                .expect("inbox has room");
        })
        .join()
        .expect("sender thread did not panic");

        let report = runtime.drain_commands();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.dropped_stale, 0);
        assert_eq!(navigator.route_ids().len(), 1);
        assert_eq!(pushed.try_take(), Some(None));
    }

    #[test]
    fn semantics_action_commits_on_the_owner_after_releasing_the_pipeline_lock() {
        let app = super::super::binding::AppBinding::new();
        let realm = UiRealm::for_test(&app);
        let pipeline = app.render_pipeline_arc();
        let weak_pipeline = Arc::downgrade(&pipeline);
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_in_handler = Arc::clone(&invoked);
        let lock_was_free = Arc::new(AtomicBool::new(false));
        let lock_was_free_in_handler = Arc::clone(&lock_was_free);
        let render_id = RenderId::new(7);
        let target = AccessibilityNodeId::from(render_id);

        let mut node = SemanticsNode::new().with_source_render_id(render_id);
        node.config_mut().add_action(
            SemanticsAction::Tap,
            Arc::new(move |action, arguments| {
                assert_eq!(action, SemanticsAction::Tap);
                assert!(arguments.is_none());
                invoked_in_handler.fetch_add(1, Ordering::SeqCst);

                let pipeline = weak_pipeline
                    .upgrade()
                    .expect("bound pipeline must outlive the action");
                let guard = pipeline.try_write();
                lock_was_free_in_handler.store(guard.is_some(), Ordering::SeqCst);
            }),
        );
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root = semantics_owner.insert(node);
        semantics_owner.set_root(Some(root));
        pipeline.write().set_semantics_owner(Some(semantics_owner));

        let sender = realm.command_sender();
        std::thread::spawn(move || {
            sender
                .send_semantics_action(SemanticsActionRequest::new(target, SemanticsAction::Tap))
                .expect("realm inbox has room");
        })
        .join()
        .expect("platform action sender did not panic");

        assert_eq!(
            invoked.load(Ordering::SeqCst),
            0,
            "cross-thread input must wait for the owner's Idle commit point"
        );
        let report = realm.drain_commands();

        assert_eq!(report.invoked, 1);
        assert_eq!(report.dropped_stale, 0);
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
        assert!(
            lock_was_free.load(Ordering::SeqCst),
            "semantics handlers must run after the PipelineOwner read guard is released"
        );
    }

    #[test]
    fn stale_semantics_action_is_gracefully_dropped() {
        let app = super::super::binding::AppBinding::new();
        let realm = UiRealm::for_test(&app);
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root =
            semantics_owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        semantics_owner.set_root(Some(root));
        app.render_pipeline_mut()
            .set_semantics_owner(Some(semantics_owner));

        realm
            .command_sender()
            .send_semantics_action(SemanticsActionRequest::new(
                AccessibilityNodeId::from(RenderId::new(99)),
                SemanticsAction::Tap,
            ))
            .expect("realm inbox has room");

        let report = realm.drain_commands();
        assert_eq!(report.invoked, 0);
        assert_eq!(report.dropped_stale, 1);
    }

    #[test]
    fn dead_navigation_target_is_dropped_at_commit() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        let target = {
            let navigator = NavigatorHandle::new();
            navigator.command_target()
        };

        sender
            .send_navigation(NavigatorCommand::maybe_pop(target))
            .expect("inbox has room");

        let report = runtime.drain_commands();
        assert_eq!(report.invoked, 0);
        assert_eq!(report.dropped_stale, 1);
    }

    #[test]
    fn inbox_reports_backpressure_at_capacity() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime_with_capacity(2, noop_wake()).expect("runtime with tiny inbox");
        let sender = runtime.command_sender();
        sender
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect("first fits");
        sender
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect("second fits");
        let overflow = sender
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect_err("third command is rejected");
        assert!(matches!(
            overflow,
            CommandSendError::ChannelFull { capacity: 2, .. }
        ));
        // Draining frees the inbox again.
        let _ = runtime.drain_commands();
        sender
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect("room after drain");
    }

    #[test]
    fn dropped_runtime_yields_owner_gone() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        drop(runtime);
        assert!(matches!(
            sender.request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart),
            Err(CommandSendError::OwnerGone { .. })
        ));
    }

    #[test]
    fn channel_full_retry_preserves_the_rejected_payload() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime_with_capacity(1, noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        sender
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect("fills inbox");

        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        let pushed = navigator.push(test_route("/details"));
        let rejected = sender
            .send_navigation(NavigatorCommand::pop(navigator.command_target()))
            .expect_err("inbox full")
            .into_rejected();

        let _ = runtime.drain_commands();
        sender.send(rejected).expect("retry fits");
        let _ = runtime.drain_commands();
        assert_eq!(navigator.route_ids().len(), 1);
        assert_eq!(pushed.try_take(), Some(None));
    }

    #[test]
    fn redraw_requests_coalesce_to_one_flag_and_one_wake() {
        let _claim = REALM_TEST_LOCK.lock();
        let (wake, wake_count) = counting_wake();
        let runtime = new_runtime(wake).expect("runtime");
        let sender = runtime.command_sender();

        sender.request_redraw();
        sender.request_redraw();
        sender.request_redraw();
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "a burst of redraw requests pays exactly one wake"
        );
        assert!(runtime.take_redraw_request(), "flag observed once");
        assert!(!runtime.take_redraw_request(), "reading clears the flag");

        sender.request_redraw();
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            2,
            "after the owner consumes the flag, the next request wakes again"
        );
    }

    #[test]
    fn every_send_wakes_the_owner() {
        let _claim = REALM_TEST_LOCK.lock();
        let (wake, wake_count) = counting_wake();
        let runtime = new_runtime(wake).expect("runtime");
        let sender = runtime.command_sender();

        sender
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect("inbox has room");

        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        sender
            .send_navigation(NavigatorCommand::maybe_pop(navigator.command_target()))
            .expect("inbox has room");

        assert_eq!(wake_count.load(Ordering::Relaxed), 2);
        let _ = runtime.drain_commands();
    }

    fn test_route(name: &'static str) -> SimpleRoute<i32> {
        SimpleRoute::new(move |_ctx| SizedBox::new(1.0, 1.0).into_view().boxed()).named(name)
    }

    /// Serializes tests that read/write `AppBinding::instance()` (the
    /// process-singleton), per the repo rule for tests mutating shared
    /// binding state (AGENTS.md "Testing quirks"). nextest gives each test
    /// its own process; `cargo test` runs them on threads in one process,
    /// where two tests each asserting on the singleton's `needs_redraw` flag
    /// could interleave.
    static SINGLETON_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A hot-reload command mutates the exact presentation owned by this
    /// realm and arms the realm's own redraw request. It never resolves a
    /// process singleton or another app instance.
    #[test]
    fn hot_reload_command_applies_to_the_owned_presentation() {
        let _serialized = SINGLETON_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Deliberately NOT `AppBinding::instance()`.
        let bound_app = super::super::binding::AppBinding::new();
        let realm = UiRealm::for_test(&bound_app);

        let singleton = super::super::binding::AppBinding::instance();
        bound_app.mark_rendered();
        singleton.mark_rendered();

        realm
            .command_sender()
            .request_hot_reload(flui_hot_reload::HotReloadTier::HotReload)
            .expect("inbox has room");

        let report = realm.drain_commands();

        assert_eq!(
            report.invoked, 1,
            "the hot-reload command must be applied, not dropped as stale"
        );
        assert!(realm.take_redraw_request());
        assert!(
            !singleton.needs_redraw(),
            "hot reload must not reach for AppBinding::instance()"
        );
        assert!(!bound_app.needs_redraw());
    }

    /// Full restart is owned by the process supervisor, so the presentation
    /// records the command as handled without arming a UI redraw.
    #[test]
    fn full_restart_command_does_not_arm_a_presentation_redraw() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime(noop_wake()).expect("runtime");

        runtime
            .command_sender()
            .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
            .expect("inbox has room");

        let report = runtime.drain_commands();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.dropped_stale, 0);
        assert!(!runtime.take_redraw_request());
    }
}
