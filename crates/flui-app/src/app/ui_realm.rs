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
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_animation::Vsync;
use flui_engine::{EngineError, RasterBackend};
use flui_foundation::{PresentationId, RealmId};
use flui_interaction::{FocusManager, GestureBinding, InteractionLane, TextInputOwner};
use flui_layer::Scene;
#[cfg(test)]
use flui_platform::traits::PlatformTextInput;
use flui_platform::traits::{DragDropEvent, PlatformInput, PlatformWindow};
use flui_rendering::binding::RendererBinding as _;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{AppLifecycleState, LocalPostFrameLane, SchedulerPhase};
use flui_semantics::SemanticsActionRequest;
use flui_types::{HapticFeedback, Size, geometry::px};
use flui_view::WidgetsBinding;
use flui_widgets::{FocusRoot, GestureArenaScope, NavigatorCommand, VsyncScope};
use parking_lot::{Mutex, RwLock};

use super::presentation::PresentationState;
use super::runtime::{RealmServices, SchedulerRef};
use crate::bindings::RenderingFlutterBinding;

/// Default bound of the owner inbox, matching the pipeline dirty-channel
/// precedent (`DEFAULT_DIRTY_CHANNEL_CAPACITY`)). Observable at
/// runtime via `UiCommandSender::capacity`; not part of the public API.
const DEFAULT_COMMAND_CAPACITY: usize = 256;

/// Claim flag for the at-most-one-instance transitional guard.
static REALM_CLAIMED: AtomicBool = AtomicBool::new(false);

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
///
/// Two addressing classes, by design, not oversight:
///
/// - **Presentation-scoped** commands ([`Self::SemanticsAction`]) carry an
///   explicit `presentation_id` stamp and are validated against the live
///   presentation at drain time, because the realm's inbox outlives any one
///   presentation incarnation within it (the eventual element-forest case)
///   and a sender may have been vended for a presentation that no longer
///   owns this realm's inbox.
/// - **Realm-scoped** commands (`HotReload`, [`Self::Navigation`])
///   carry no stamp of their own and stay bound by channel identity alone:
///   a recreated realm mints new channels, and a sender into the dead realm
///   already gets `OwnerGone` at send — re-stamping realm identity on top of
///   that would duplicate a structural fact the channel already enforces.
pub(crate) enum UiCommand {
    /// Apply a hot-reload reassemble on the owner at the next Idle drain.
    // Only constructed by `request_hot_reload`, whose consumer is the
    // desktop runner — absent from the wasm lib check.
    #[cfg(feature = "hot-reload")]
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
    HotReload(flui_hot_reload::HotReloadTier),
    /// Resolve and invoke an accessibility action on the owner thread,
    /// addressed to the exact presentation that was live when the sender
    /// stamped it.
    SemanticsAction {
        /// The presentation this action was stamped for.
        presentation_id: PresentationId,
        /// The stable node identity and action to resolve.
        request: SemanticsActionRequest,
    },
    /// Apply a typed navigator mutation on the owner thread.
    Navigation(NavigatorCommand),
}

impl std::fmt::Debug for UiCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "hot-reload")]
            UiCommand::HotReload(tier) => {
                f.debug_tuple("UiCommand::HotReload").field(tier).finish()
            }
            UiCommand::SemanticsAction {
                presentation_id,
                request,
            } => f
                .debug_struct("UiCommand::SemanticsAction")
                .field("presentation_id", presentation_id)
                .field("request", request)
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
    /// The presentation this sender stamps onto every presentation-scoped
    /// command it sends (set once, at [`UiRealm::construct`]). For the
    /// eventual element forest, senders become vended per-presentation with
    /// different stamps into the same realm inbox — this field is already
    /// the right shape; only the vending point changes.
    presentation_id: PresentationId,
    /// Fired after every successful state change so an idle event loop
    /// produces the drain that observes it — the enqueue-then-wake contract,
    /// same as `PipelineOwnerHandle`'s notifier.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for UiCommandSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiCommandSender")
            .field("capacity", &self.capacity)
            .field("presentation_id", &self.presentation_id)
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
    // The desktop runner (`cfg(not(target_arch = "wasm32"))`) is the only
    // non-test consumer, so the wasm lib check sees this as dead.
    #[cfg(feature = "hot-reload")]
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
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
        self.send(UiCommand::SemanticsAction {
            presentation_id: self.presentation_id,
            request,
        })
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
    // The desktop runner (`cfg(not(target_arch = "wasm32"))`) is the only
    // non-test consumer, so the wasm lib check sees this as dead.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
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
    /// Owner-local widget framework state. It is deliberately absent from any
    /// process-global host; every widget-tree operation enters through this
    /// realm and activates this binding's GlobalKey registry.
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
    /// Render tree, layout/paint pipeline coordination, and per-realm
    /// semantics-enablement fan-out. A direct value, not `RwLock`-wrapped:
    /// every field `RenderingFlutterBinding` itself owns already carries its
    /// own interior mutability (`RwLock`/`AtomicBool`), so an outer lock
    /// here would only ever be uncontended — this realm is the single
    /// owner-thread-confined caller, never shared across threads. Moved
    /// here from the retired `AppBinding` (ADR-0027 step 3): the renderer's
    /// per-field disposition (render_views, semantics fan-out,
    /// first-frame-deferral counters) is per-realm state, not a process-wide
    /// concern — sharing it across realms once `AppRuntime` hosts more than
    /// one was exactly the hazard this placement designs out.
    renderer: RenderingFlutterBinding,
    /// Controller registry for implicit animations (`VsyncScope`-driven).
    /// Moved here from the retired `AppBinding` — interim home (ADR-0027 §8:
    /// a scheduler-owning change re-homes controllers to `UpdateScheduler`
    /// later); frame-relative, so per-realm now instead of process-wide.
    /// `Mutex`, not `RwLock`: `set_vsync` replaces the whole handle through
    /// `&self`, and the per-frame `tick_all`/`has_running` calls operate on
    /// a cloned `Vsync` handle (sharing the inner `Arc<Mutex<VsyncInner>>`),
    /// so this lock is only ever held for the length of a clone or a swap.
    vsync_slot: Mutex<Vsync>,
    /// Wall-clock origin for the production `now_secs` computation, moved
    /// here from the retired `AppBinding`: frame times are realm-relative.
    /// `now_secs()` = `start.elapsed().as_secs_f64()`, stored once here so
    /// every frame this realm produces shares one monotonically-increasing
    /// origin instead of drifting between the Vsync tick and elsewhere.
    start: web_time::Instant,
    /// Whether a redraw has been requested since the last
    /// [`Self::mark_rendered`] — a clone of `AppRuntime`'s own
    /// `needs_redraw` flag (production; a fresh, unshared flag for the
    /// `#[cfg(test)]` `for_test` constructor), so this realm's own frame
    /// methods (`attach_root_widget*`, `handle_input_entered`) can flag-only
    /// request a redraw without reaching back into the loop-scoped runtime.
    /// [`Self::wake`] below shares the SAME underlying atomic through
    /// `AppRuntime`'s `frame_wake_callback`, so either side observes the
    /// other's writes.
    needs_redraw: Arc<AtomicBool>,
    /// Platform wake: sets `needs_redraw` and pokes the installed window so
    /// an idle event loop wakes up. In production this is `AppRuntime`'s
    /// `frame_wake_callback()`; retained here (not just cloned into
    /// [`UiCommandSender`]) so this realm's own frame methods can wake the
    /// loop directly (the vsync/gesture-deadline continuation and the
    /// render-retry path all need this, exactly as the retired
    /// `AppBinding::wake_frame` did).
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Test-only injectable clock, stored as the f64 bits in a u64 atomic.
    /// See the retired `AppBinding::now_secs_override`'s identical doc.
    #[cfg(test)]
    now_secs_override: AtomicU64,
    rx: Receiver<UiCommand>,
    /// Prototype for [`Self::command_sender`]: crossbeam receivers cannot
    /// mint senders, so the runtime keeps one sender to clone from. Holding
    /// it here does not keep the channel alive past the runtime: `rx` drops
    /// with the runtime and every outstanding sender turns `OwnerGone`.
    // The desktop runner (`cfg(not(target_arch = "wasm32"))`) is the only
    // non-test consumer, so the wasm lib check sees this as dead.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
    sender_prototype: UiCommandSender,
    redraw_pending: Arc<AtomicBool>,
    /// Whether this instance owns the transitional process-wide claim.
    claimed: bool,
    /// The scheduler this realm was constructed against, resolved once by
    /// the caller (see `RealmServices::resolve`) and retained only for the
    /// idle-only commit-gate phase probe in [`Self::drain_commands`] — the
    /// last remaining reach that used to be a bare `Scheduler::instance()`
    /// call inside this type.
    scheduler: SchedulerRef,
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

/// Outcome of one build+layout+paint pass, distinguishing "nothing was
/// dirty" from "the pipeline failed" — both produce no layer tree, but only
/// the latter must force a retry rather than being treated as a settled,
/// up-to-date frame (see [`UiRealm::render_frame_entered`]'s retry gate).
/// Moved here from the retired `AppBinding`.
enum FramePaintOutcome {
    /// A fresh layer tree was painted and turned into a `Scene`.
    Painted(Arc<Scene>),
    /// Nothing was dirty this frame; no new content to composite.
    Idle,
    /// The build/layout/paint transaction failed (e.g. a render object
    /// panicked and was caught by `catch_unwind`); the frame was dropped and
    /// must be retried.
    Errored,
}

/// Moved here from the retired `AppBinding`, unchanged.
fn preserve_first_input_panic(
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    candidate: Option<Box<dyn std::any::Any + Send>>,
    phase: &'static str,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if first.is_none() {
        *first = Some(candidate);
    } else {
        tracing::error!(
            phase,
            "input phase panicked after an earlier phase; only the first panic is resumed"
        );
        // A panic payload may itself panic while being dropped. Leaking only
        // the secondary exceptional payload keeps the original failure stable.
        std::mem::forget(candidate);
    }
}

/// Whether [`UiRealm::handle_input_entered`] must drop `input` outright,
/// before ever reaching the per-kind dispatch above, given the
/// presentation's current lifecycle (ADR-0037 §9). Moved here from the
/// retired `AppBinding`, unchanged — see its own doc for the per-lifecycle
/// rationale.
///
/// Explicit and exhaustive over **both** axes — lifecycle and input kind —
/// with no wildcard on the input axis: adding a `PlatformInput` variant
/// breaks this match at compile time instead of silently falling through a
/// `_` arm.
fn input_dropped_by_lifecycle(
    lifecycle: super::presentation::PresentationLifecycle,
    input: &PlatformInput,
) -> bool {
    use super::presentation::PresentationLifecycle;
    match lifecycle {
        PresentationLifecycle::Created
        | PresentationLifecycle::Closing
        | PresentationLifecycle::Closed => true,
        PresentationLifecycle::Suspended => match input {
            PlatformInput::Pointer(_) | PlatformInput::DragDrop(_) => true,
            PlatformInput::Keyboard(_) | PlatformInput::Ime(_) => false,
        },
        PresentationLifecycle::SurfaceAttached => false,
    }
}

/// A safe, content-free discriminator for a [`PlatformInput`] — never the
/// payload itself. STYLE.md forbids logging text-input/IME and drag-and-drop
/// payloads; this is the only thing about an input event that may reach a
/// trace/log line. Moved here from the retired `AppBinding`, unchanged.
fn input_kind(input: &PlatformInput) -> &'static str {
    match input {
        PlatformInput::Pointer(_) => "pointer",
        PlatformInput::Keyboard(_) => "keyboard",
        PlatformInput::Ime(_) => "ime",
        PlatformInput::DragDrop(_) => "drag_drop",
    }
}

/// A safe, content-free discriminator for a [`DragDropEvent`] — never the
/// carried offer/payload. Moved here from the retired `AppBinding`, unchanged.
fn drag_drop_kind(event: &DragDropEvent) -> &'static str {
    match event {
        DragDropEvent::Entered { .. } => "entered",
        DragDropEvent::Moved { .. } => "moved",
        DragDropEvent::Dropped { .. } => "dropped",
        DragDropEvent::Exited { .. } => "exited",
    }
}

impl UiRealm {
    /// Construct the runtime with the default inbox capacity.
    ///
    /// `wake` is the platform wake: it must deliver a wake to the owner's
    /// event loop without spawning a thread — in production this is
    /// `AppRuntime::frame_wake_callback()`. `needs_redraw` is a clone of
    /// that same runtime's flag (see [`Self::needs_redraw`]'s field doc).
    /// `device_pixel_ratio` is applied to the freshly built pipeline BEFORE
    /// this constructor returns — the window's constraints are set later,
    /// but the scale must already agree so the first frame's `RenderView`
    /// configuration and layout do not disagree on it.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::AlreadyExists`] while another runtime is live
    /// (transitional at-most-one guard, see module docs).
    pub(crate) fn new(
        wake: Arc<dyn Fn() + Send + Sync>,
        window: Arc<dyn PlatformWindow>,
        device_pixel_ratio: f32,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        Self::with_capacity(
            DEFAULT_COMMAND_CAPACITY,
            wake,
            window,
            device_pixel_ratio,
            needs_redraw,
        )
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
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
        window: Arc<dyn PlatformWindow>,
        device_pixel_ratio: f32,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        assert!(capacity > 0, "UiRealm inbox capacity must be non-zero");
        if REALM_CLAIMED.swap(true, Ordering::AcqRel) {
            return Err(UiRealmError::AlreadyExists);
        }
        let (realm_id, presentation_id) = super::runtime::next_identity();
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        pipeline.write().set_device_pixel_ratio(device_pixel_ratio);
        let presentation = PresentationState::new(presentation_id, pipeline, window);
        let services = RealmServices::resolve();
        match Self::construct(
            capacity,
            wake,
            realm_id,
            presentation,
            true,
            services,
            needs_redraw,
        ) {
            Ok(realm) => Ok(realm),
            Err(error) => {
                REALM_CLAIMED.store(false, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Builds the realm from already-resolved pieces. Takes `services:
    /// RealmServices` rather than reaching for `Scheduler::instance()`
    /// itself — the last two `Scheduler::instance()` calls this function
    /// used to make (`local_post_frame_lane()`, `async_driver()`) are now
    /// the caller's job (`RealmServices::resolve`, in `runtime.rs`), which
    /// is what makes `UiRealm` perform zero `::instance()` calls.
    fn construct(
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
        realm_id: RealmId,
        presentation: PresentationState,
        claimed: bool,
        services: RealmServices,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        let presentation_id = presentation.id();
        let RealmServices {
            local_post_frame,
            async_driver,
            scheduler,
        } = services;
        let interaction_lane = InteractionLane::try_new()?;
        let widgets = WidgetsBinding::with_focus_manager(presentation.focus_manager());
        widgets.set_pipeline_owner(Arc::clone(presentation.pipeline()));
        widgets.with_build_owner_mut(|owner| {
            owner.set_async_driver(async_driver);
            owner.set_post_frame_handle(local_post_frame.post_frame_handle());
            owner.set_interaction_dispatch_handle(interaction_lane.dispatch_handle());
            owner.set_text_input_handle(presentation.text_input_handle());
        });

        // Renderer, sharing the SAME PipelineOwner as the presentation (one
        // fact, one place) — moved here from the retired
        // `AppBinding::new`/`RenderingFlutterBinding::new_with_pipeline` pair.
        let renderer =
            RenderingFlutterBinding::new_with_pipeline(Arc::clone(presentation.pipeline()));

        // Idle-wake wiring: a dirty mark (mark_needs_layout / mark_needs_paint)
        // fires this callback so a quiescent event loop produces the frame —
        // moved verbatim from `AppBinding::new`. Lock order is safe: the
        // callback fires while the CALLER holds the pipeline-owner lock, and
        // `wake` only touches `Send + Sync` runtime-level state — never this
        // realm's own `widgets`/`renderer`.
        let visual_wake = Arc::clone(&wake);
        presentation
            .pipeline()
            .write()
            .set_on_need_visual_update(move || visual_wake());

        // Semantics-enabled fan-out -> this presentation's own `SemanticsHost`:
        // now that the renderer and the presentation are co-located on one
        // `UiRealm`, the listener can capture a cheap `Arc<AtomicBool>`
        // clone of the host's enablement flag directly, instead of the flag
        // having nowhere to route to.
        let semantics_flag = presentation
            .semantics_host()
            .platform_semantics_enabled_handle();
        renderer.add_semantics_enabled_listener(Arc::new(move |enabled| {
            semantics_flag.store(enabled, Ordering::Relaxed);
        }));

        Ok(Self {
            realm_id,
            widgets,
            local_post_frame,
            interaction_lane,
            presentation,
            renderer,
            vsync_slot: Mutex::new(Vsync::new()),
            start: web_time::Instant::now(),
            needs_redraw,
            wake: Arc::clone(&wake),
            #[cfg(test)]
            now_secs_override: AtomicU64::new(0),
            rx,
            sender_prototype: UiCommandSender {
                tx,
                capacity,
                redraw_pending: Arc::clone(&redraw_pending),
                presentation_id,
                wake,
            },
            redraw_pending,
            claimed,
            scheduler,
            _owner_affine: PhantomData,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::for_test_with_text_input(None)
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_text_input(
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let (realm_id, presentation_id) = super::runtime::next_identity();
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let presentation =
            PresentationState::new_for_test(presentation_id, pipeline, platform_text_input);
        // The no-op `wake` still must set THIS SAME `needs_redraw` flag —
        // in production the two are the same fact through AppRuntime's
        // `frame_wake_callback` (see `Self::needs_redraw`'s field doc); a
        // disconnected no-op here would silently break every test that
        // calls `wake_frame()`/relies on the vsync/gesture continuation
        // setting `needs_redraw` (there is no window to poke in a test, so
        // only the flag half applies).
        let needs_redraw = Arc::new(AtomicBool::new(false));
        let wake_needs_redraw = Arc::clone(&needs_redraw);
        let wake: Arc<dyn Fn() + Send + Sync> =
            Arc::new(move || wake_needs_redraw.store(true, Ordering::Relaxed));
        Self::construct(
            DEFAULT_COMMAND_CAPACITY,
            wake,
            realm_id,
            presentation,
            false,
            RealmServices::resolve(),
            needs_redraw,
        )
        .expect("test UiRealm should create an interaction lane")
    }

    /// Test-only: a clone of this realm's exact pipeline `Arc` — the same
    /// one its `renderer` and `widgets` share (one fact, one place).
    #[cfg(test)]
    pub(crate) fn pipeline_for_test(&self) -> Arc<RwLock<PipelineOwner>> {
        Arc::clone(self.presentation.pipeline())
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

    /// The current presentation's lifecycle state, for the input-gate
    /// checks at the physical owner (`AppBinding::handle_input_entered`).
    #[must_use]
    pub(crate) fn presentation_lifecycle(&self) -> super::presentation::PresentationLifecycle {
        self.presentation.lifecycle()
    }

    /// A new cross-thread sender into this runtime's inbox.
    #[must_use]
    // The desktop runner (`cfg(not(target_arch = "wasm32"))`) is the only
    // non-test consumer, so the wasm lib check sees this as dead.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
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
    #[expect(
        dead_code,
        reason = "the IME/text-input test module (attach_dispatch_and_active_detach_round_trip_through_the_platform \
                  and siblings, migrated from the retired AppBinding's test module) is deferred, not yet re-homed here"
    )]
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
    #[cfg(feature = "hot-reload")]
    #[must_use]
    pub(crate) fn apply_hot_reload(&self, tier: flui_hot_reload::HotReloadTier) -> bool {
        self.presentation.apply_hot_reload(&self.widgets, tier)
    }

    /// Apply a hot reload at the given tier (Flutter parity entry point),
    /// requesting a redraw if it actually changed anything. Moved here from
    /// the retired `AppBinding::perform_hot_reload_entered`.
    #[cfg(feature = "hot-reload")]
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
    pub(crate) fn perform_hot_reload_entered(&self, tier: flui_hot_reload::HotReloadTier) {
        if self.apply_hot_reload(tier) {
            self.request_redraw();
        }
    }

    // ========================================================================
    // Renderer, vsync, frame clock (moved from the retired `AppBinding`)
    // ========================================================================

    /// The render tree, layout/paint pipeline coordination, and semantics
    /// fan-out for this realm's single presentation.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production frame/input paths read self.renderer directly; \
                      this accessor exists for tests and future external callers"
        )
    )]
    pub(crate) fn renderer(&self) -> &RenderingFlutterBinding {
        &self.renderer
    }

    /// Re-dirty this realm's root so the next frame actually produces
    /// content instead of finding nothing to do — the per-realm target of
    /// `AppRuntime`'s frames-disabled->enabled re-enable listener (see
    /// `runtime.rs`'s `install_frames_reenable_redirty_listener`). FLUI has
    /// no retained-scene layer to fall back on, so a `Hidden`/`Paused` ->
    /// `Resumed`/`Inactive` transition needs the same explicit re-dirty
    /// `allow_first_frame` needs after a deferral lifts.
    pub(crate) fn redirty_root_for_frames_reenable(&self) {
        crate::bindings::redirty_pipeline_root(self.renderer.root_pipeline_owner());
    }

    /// A clone of the shared controller registry for implicit animations.
    /// See the retired `AppBinding::vsync`'s doc for the full invariant.
    pub(crate) fn vsync(&self) -> Vsync {
        self.vsync_slot.lock().clone()
    }

    /// Replace this realm's registry with a pre-existing shared `Vsync`. See
    /// the retired `AppBinding::set_vsync`'s doc for the customization
    /// invariant this must preserve.
    #[expect(
        dead_code,
        reason = "no production caller yet, and no test exercises the \
                  custom-registry substitution path -- an app-author escape \
                  hatch that has no wiring point since UiRealm is pub(crate)-only"
    )]
    pub(crate) fn set_vsync(&self, vsync: Vsync) {
        *self.vsync_slot.lock() = vsync;
    }

    /// Whether at least one registered implicit-animation controller is
    /// currently running.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "draw_frame_entered reads has_running() on its own local \
                      Vsync clone directly, not through this wrapper; kept for \
                      tests and future external callers"
        )
    )]
    pub(crate) fn has_vsync_running(&self) -> bool {
        self.vsync_slot.lock().has_running()
    }

    /// Current virtual seconds for the Vsync tick. See the retired
    /// `AppBinding::now_secs`'s doc for the production/test split.
    fn now_secs(&self) -> f64 {
        #[cfg(test)]
        {
            let bits = self.now_secs_override.load(Ordering::Relaxed);
            if bits != 0 {
                return f64::from_bits(bits);
            }
        }
        self.start.elapsed().as_secs_f64()
    }

    /// Inject a deterministic virtual `now_secs` for test frames. See the
    /// retired `AppBinding::set_now_secs_for_test`'s doc.
    #[cfg(test)]
    pub(crate) fn set_now_secs_for_test(&self, secs: f64) {
        let bits = secs.to_bits();
        let stored = if bits == 0 { 1u64 } else { bits };
        self.now_secs_override.store(stored, Ordering::Relaxed);
    }

    /// Clear the test clock override, reverting to wall-clock time.
    #[cfg(test)]
    #[expect(
        dead_code,
        reason = "no test in this file's migrated set needs to revert to \
                  wall-clock mid-test; kept for parity with the retired \
                  AppBinding::clear_now_secs_for_test"
    )]
    pub(crate) fn clear_now_secs_for_test(&self) {
        self.now_secs_override.store(0, Ordering::Relaxed);
    }

    // ========================================================================
    // Frame wake (moved from the retired `AppBinding`)
    // ========================================================================

    /// Request a redraw (flag only — does not poke the platform window).
    /// See [`Self::needs_redraw`]'s field doc for why this and
    /// [`Self::wake_frame`] share one underlying atomic with `AppRuntime`.
    pub(crate) fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Whether a redraw is needed.
    pub(crate) fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark the frame as rendered, clearing the redraw flag.
    pub(crate) fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// Wake the platform event loop so the next frame is rendered — sets
    /// `needs_redraw` AND pokes the installed window. See the retired
    /// `AppBinding::wake_frame`'s doc for the deadlock-safety argument,
    /// which still holds: this only ever touches `Send + Sync` state
    /// captured in [`Self::wake`], never this realm's own locks.
    pub(crate) fn wake_frame(&self) {
        (self.wake)();
    }

    /// A cloned, `'static` handle to this realm's wake capability — for a
    /// caller that must move a wake past this realm's own borrow (e.g. an
    /// `async move` block spawned from inside a frame callback, which
    /// outlives the synchronous `&UiRealm` the callback was given).
    /// `wake_frame()` above is for every same-scope caller instead.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(
            dead_code,
            reason = "the only production caller is the web bootstrap's GPU \
                      device-recovery spawn_local (runner.rs's bootstrap_web), \
                      invisible outside a wasm32 build"
        )
    )]
    pub(crate) fn wake_handle(&self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::clone(&self.wake)
    }

    // ========================================================================
    // First-frame deferral and frame accounting (moved from the retired
    // `AppBinding`, forwarding to the per-realm renderer / per-presentation
    // counters respectively)
    // ========================================================================

    /// See [`RenderingFlutterBinding::defer_first_frame`].
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- an app-author async-init \
                      deferral has no wiring point since UiRealm is \
                      pub(crate)-only; the retired AppBinding had the same gap"
        )
    )]
    pub(crate) fn defer_first_frame(&self) {
        self.renderer.defer_first_frame();
    }

    /// See [`RenderingFlutterBinding::allow_first_frame`].
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- see defer_first_frame's doc"
        )
    )]
    pub(crate) fn allow_first_frame(&self) {
        self.renderer.allow_first_frame();
    }

    /// See [`RenderingFlutterBinding::send_frames_to_engine`] (via the
    /// `RendererBinding` trait).
    pub(crate) fn send_frames_to_engine(&self) -> bool {
        self.renderer.send_frames_to_engine()
    }

    /// Total frames rendered successfully by this presentation.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "draw_frame_entered reads self.presentation.frames_rendered() \
                      directly for the frame-number computation, not through \
                      this wrapper; kept for tests and future external callers"
        )
    )]
    pub(crate) fn frames_rendered(&self) -> u64 {
        self.presentation.frames_rendered()
    }

    /// Frames dropped due to surface errors on this presentation.
    #[expect(
        dead_code,
        reason = "no production caller yet, and no test reads it back -- \
                  forwards PresentationState::frames_dropped, also uncalled"
    )]
    pub(crate) fn frames_dropped(&self) -> u64 {
        self.presentation.frames_dropped()
    }

    /// Turn this presentation's performance overlay on or off. See the
    /// retired `AppBinding::set_performance_overlay`'s doc.
    pub(crate) fn set_performance_overlay(&self, enabled: bool) {
        self.presentation.set_performance_overlay(enabled);
    }

    /// Perform haptic feedback on this presentation's window. See the
    /// retired `AppBinding::perform_haptic_feedback`'s doc.
    #[expect(
        dead_code,
        reason = "no production caller yet, and this specific forwarding \
                  wrapper is untested (the haptics tests exercise \
                  PresentationState::perform_haptic_feedback directly)"
    )]
    pub(crate) fn perform_haptic_feedback(&self, feedback: HapticFeedback) {
        self.presentation.perform_haptic_feedback(feedback);
    }

    /// Apply a new device pixel ratio to this realm's render pipeline (the
    /// resize path; construction applies the initial ratio directly).
    pub(crate) fn set_device_pixel_ratio(&self, device_pixel_ratio: f32) {
        self.renderer
            .root_pipeline_owner()
            .write()
            .set_device_pixel_ratio(device_pixel_ratio);
    }

    /// Check if there is pending work: a pending build, pending gesture
    /// motion/deadlines, or a dirty render node. The runner's wake gate
    /// (`needs_redraw() || has_pending_work()`) reads this every frame.
    pub(crate) fn has_pending_work(&self) -> bool {
        self.widgets.has_pending_builds()
            || self.gestures().has_pending_motion()
            || self.gestures().has_pending_deadlines()
            || self.renderer.root_pipeline_owner().read().has_dirty_nodes()
    }

    // ========================================================================
    // Root attach (moved from the retired `AppBinding`)
    // ========================================================================

    /// Attach a root widget.
    ///
    /// See the retired `AppBinding::attach_root_widget`'s doc for the full
    /// root-bootstrap, implicit-animation auto-wrap, and gesture-arena
    /// auto-wrap invariants — unchanged by this move.
    ///
    /// # Errors
    ///
    /// Forwards every [`flui_view::AttachError`] the underlying
    /// [`flui_view::WidgetsBinding::attach_root_widget`] returns.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "desktop/mobile runners use the sized attach variant"
        )
    )]
    pub(crate) fn attach_root_widget<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|realm| realm.attach_root_widget_entered(view))
    }

    fn attach_root_widget_entered<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        // Install this realm's three exact owner-driven capabilities.
        // GestureArenaScope is outermost so every descendant recognizer
        // shares one binding-driven arena, VsyncScope carries the animation
        // registry, and FocusRoot publishes this presentation's exact focus
        // tree. These wrappers have no render object, so the render root is
        // unchanged.
        let focused = FocusRoot::new(view.clone());
        let animated = VsyncScope::new(self.vsync(), focused);
        let wrapped = GestureArenaScope::new(self.gestures().arena().clone(), animated);
        self.widgets.attach_root_widget(&wrapped)?;
        self.request_redraw();
        tracing::debug!("Root widget attached");
        Ok(())
    }

    /// Attach a root widget sizing the root view to an explicit logical
    /// `width` × `height` — the platform window's surface size. See the
    /// retired `AppBinding::attach_root_widget_with_size`'s doc.
    ///
    /// # Errors
    ///
    /// Forwards every [`flui_view::AttachError`] from
    /// [`flui_view::WidgetsBinding::attach_root_widget_with_size`].
    pub(crate) fn attach_root_widget_with_size<V>(
        &self,
        view: &V,
        width: f32,
        height: f32,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|realm| realm.attach_root_widget_with_size_entered(view, width, height))
    }

    fn attach_root_widget_with_size_entered<V>(
        &self,
        view: &V,
        width: f32,
        height: f32,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        let focused = FocusRoot::new(view.clone());
        let animated = VsyncScope::new(self.vsync(), focused);
        let wrapped = GestureArenaScope::new(self.gestures().arena().clone(), animated);
        self.widgets
            .attach_root_widget_with_size(&wrapped, width, height)?;
        self.request_redraw();
        tracing::debug!(width, height, "Root widget attached (sized)");
        Ok(())
    }

    // ========================================================================
    // Frame production (moved from the retired `AppBinding`)
    // ========================================================================

    /// Draw a frame and return the produced `Scene`, if any. Test-only —
    /// production drives frames through [`Self::render_frame_entered`].
    #[cfg(test)]
    pub(crate) fn draw_frame(&self, constraints: BoxConstraints) -> Option<Arc<Scene>> {
        match self.enter(|realm| realm.draw_frame_entered(constraints)) {
            FramePaintOutcome::Painted(scene) => Some(scene),
            FramePaintOutcome::Idle | FramePaintOutcome::Errored => None,
        }
    }

    /// The complete build+layout+paint pipeline for one frame.
    ///
    /// **Frame-phase parity (critical):** this ordering — the vsync tick
    /// block, the gesture-deadline tick, the build phase, the
    /// layout/paint/scene-creation phase, and the pipeline-failure error
    /// path — moves VERBATIM from the retired `AppBinding::draw_frame_entered`.
    /// Reordering any of it is out of scope for the change that moved it
    /// here; the frame-loop tests below are the parity oracle.
    fn draw_frame_entered(&self, constraints: BoxConstraints) -> FramePaintOutcome {
        // Vsync tick — MUST precede the build phase (Phase 1). See the
        // retired `AppBinding::draw_frame_entered`'s doc for the full
        // disjoint-controller-set argument this ordering depends on.
        let now = self.now_secs();
        {
            let vsync = self.vsync_slot.lock().clone();
            vsync.tick_all(now);

            // Frame continuation: if any controller is still running after
            // this tick, request the NEXT frame so the runner gate stays
            // open for the full animation duration.
            if vsync.has_running() {
                self.wake_frame();
            }
        }

        // Gesture-deadline tick + keep-alive — also MUST precede the build
        // phase, for the identical ordering argument as the vsync tick.
        self.gestures().tick_deadlines();
        if self.gestures().has_pending_deadlines() {
            self.wake_frame();
        }

        // The async-driver step lives in `Scheduler::handle_begin_frame`'s
        // mid-frame slot, not here — see the retired `AppBinding`'s doc for
        // why (this pipeline runs in `PersistentCallbacks`, where
        // `drive_async_tasks` debug-asserts it must never poll).

        // Phase 1: Build (WidgetsBinding)
        {
            let w = self.widgets();
            if w.has_pending_builds() {
                w.draw_frame();
            }
        }

        // Phase 2 & 3: Layout, Compositing, Paint, Semantics through the
        // typestate-driven orchestrator.
        let mut pipeline_errored = false;
        let (layer_tree, link_registry) = {
            {
                self.renderer
                    .root_pipeline_owner()
                    .write()
                    .set_root_constraints(Some(constraints));
            }
            let result = self
                .widgets()
                .run_frame_with_layout_builders(self.presentation.pipeline());
            let link_registry = self
                .renderer
                .root_pipeline_owner()
                .write()
                .take_link_registry();
            match result {
                Ok(layer_tree) => (layer_tree, link_registry),
                Err(e) => {
                    tracing::error!(error = ?e, "draw_frame: pipeline failed, dropping frame");
                    pipeline_errored = true;
                    (None, link_registry)
                }
            }
        };

        // Production<->headless convergence point: service lazy-sliver child
        // requests accumulated by `run_frame`'s layout pass.
        {
            let w = self.widgets();
            w.service_child_requests(self.presentation.pipeline());
        }

        // Phase 4: Create Scene from LayerTree
        let size = constraints.constrain(Size::ZERO);
        let frame_number = self.presentation.frames_rendered() + 1;

        if let Some(mut layer_tree) = layer_tree {
            self.presentation
                .attach_performance_overlay(&mut layer_tree);

            let root = layer_tree.root();
            let scene = Scene::with_links(
                size,
                layer_tree,
                root,
                link_registry.unwrap_or_default(),
                frame_number,
            );
            #[expect(
                clippy::arc_with_non_send_sync,
                reason = "Scene: Send but !Sync due to CompositionCallback (FnOnce + Send + 'static, no Sync). Sole reader is the owner thread; relaxing the callback bound is tracked under the engine composition redesign."
            )]
            let arc = Arc::new(scene);
            FramePaintOutcome::Painted(arc)
        } else if pipeline_errored {
            FramePaintOutcome::Errored
        } else {
            FramePaintOutcome::Idle
        }
    }

    /// Render while the platform dispatcher already owns the realm entry.
    /// See the retired `AppBinding::render_frame_entered`'s doc for the full
    /// step-by-step rationale (first-frame deferral gate, damage tracking,
    /// device-loss/surface-lost handling, the retry-vs-settle distinction) —
    /// unchanged by this move.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn render_frame_entered<R: RasterBackend>(&self, renderer: &mut R) -> bool {
        self.gestures().drain_deferred_arena_resolutions();
        self.gestures().flush_pending_moves();

        let (width, height) = renderer.size();
        let dpr = {
            self.renderer
                .root_pipeline_owner()
                .read()
                .device_pixel_ratio()
        };
        let constraints =
            BoxConstraints::tight(Size::new(px(width as f32 / dpr), px(height as f32 / dpr)));
        let outcome = self.draw_frame_entered(constraints);

        self.gestures()
            .mouse_tracker()
            .update_all_devices(|position| {
                let mut result = flui_interaction::routing::HitTestResult::new();
                self.renderer.hit_test_in_view(&mut result, position, 0);
                result
            });

        let send_to_engine = self.send_frames_to_engine();
        let errored = matches!(outcome, FramePaintOutcome::Errored);
        if send_to_engine && !errored {
            self.renderer.mark_first_frame_sent();
        }

        let mut presented = false;
        let mut retry_needed = errored;
        if send_to_engine
            && let FramePaintOutcome::Painted(ref scene) = outcome
            && scene.has_content()
        {
            renderer.mark_full_repaint();
            match renderer.render_scene(scene) {
                Ok(did_present) => {
                    presented = did_present;
                    if did_present {
                        self.presentation.record_frame_rendered();
                        tracing::trace!(
                            frame = scene.frame_number(),
                            total = self.presentation.frames_rendered(),
                            "Frame rendered successfully"
                        );
                    } else {
                        tracing::trace!(
                            frame = scene.frame_number(),
                            "Frame skipped: no damage or surface occluded (no present)"
                        );
                    }
                }
                Err(EngineError::SurfaceLost) => {
                    self.presentation.record_frame_dropped();
                    retry_needed = true;
                    tracing::debug!("Surface lost; frame dropped — retry armed via wake_frame()");
                }
                Err(EngineError::DeviceLost) => {
                    self.presentation.record_frame_dropped();
                    tracing::warn!(
                        "GPU device lost — recovery will be attempted by the platform runner"
                    );
                }
                Err(EngineError::SurfaceValidation) => {
                    self.presentation.record_frame_dropped();
                    tracing::error!(
                        "Surface validation error — surface misconfig; external reconfigure required"
                    );
                }
                Err(e) => {
                    self.presentation.record_frame_dropped();
                    tracing::error!(error = ?e, "Render error (non-recoverable this frame)");
                }
            }
        }

        if retry_needed {
            self.wake_frame();
        } else {
            self.mark_rendered();
        }

        presented
    }

    // ========================================================================
    // Input dispatch (moved from the retired `AppBinding`)
    // ========================================================================

    /// Handle a platform input event while this realm is already entered.
    /// See the retired `AppBinding::handle_input_entered`'s doc for the full
    /// per-kind routing and lifecycle-gate rationale — unchanged by this
    /// move.
    pub(crate) fn handle_input_entered(&self, input: PlatformInput) {
        if input_dropped_by_lifecycle(self.presentation_lifecycle(), &input) {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = self.presentation_id().as_u64(),
                lifecycle = ?self.presentation_lifecycle(),
                input_kind = input_kind(&input),
                "dropping input due to presentation lifecycle"
            );
            return;
        }
        match input {
            PlatformInput::Ime(ime_event) => {
                self.text_input().dispatch(&ime_event);
                self.request_redraw();
            }
            PlatformInput::Pointer(pointer_event) => {
                let routing_panic = catch_unwind(AssertUnwindSafe(|| {
                    self.gestures()
                        .handle_pointer_event(&pointer_event, |position| {
                            let mut result = flui_interaction::routing::HitTestResult::new();
                            let offset = flui_types::Offset::new(position.dx, position.dy);
                            self.renderer.hit_test_in_view(&mut result, offset, 0);
                            if !result.is_empty() {
                                tracing::debug!(hits = result.len(), "Hit test found targets");
                            }
                            result
                        });
                }))
                .err();
                let deferred_panic = catch_unwind(AssertUnwindSafe(|| {
                    self.gestures().drain_deferred_arena_resolutions();
                }))
                .err();

                let mut first_panic = None;
                preserve_first_input_panic(&mut first_panic, routing_panic, "pointer routing");
                preserve_first_input_panic(
                    &mut first_panic,
                    deferred_panic,
                    "deferred arena resolution",
                );
                self.request_redraw();
                if let Some(payload) = first_panic {
                    resume_unwind(payload);
                }
            }
            PlatformInput::Keyboard(keyboard_event) => {
                self.focus_manager().dispatch_key_event(&keyboard_event);
                self.request_redraw();
            }
            PlatformInput::DragDrop(drag_drop_event) => {
                tracing::debug!(
                    drag_drop_kind = drag_drop_kind(&drag_drop_event),
                    "drag-and-drop input received; realm routing not implemented yet (ADR-0038), dropping"
                );
            }
        }
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
            self.scheduler.phase(),
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
                #[cfg(feature = "hot-reload")]
                UiCommand::HotReload(tier) => {
                    if self.presentation.apply_hot_reload(&self.widgets, tier) {
                        self.redraw_pending.store(true, Ordering::Release);
                    }
                    report.invoked += 1;
                }
                UiCommand::SemanticsAction {
                    presentation_id,
                    request,
                } => {
                    if presentation_id != self.presentation.id() {
                        tracing::trace!(
                            { flui_foundation::diagnostics::PRESENTATION_ID } =
                                self.presentation.id().as_u64(),
                            stamped_presentation_id = presentation_id.as_u64(),
                            "dropping semantics action stamped for a stale presentation incarnation"
                        );
                        report.dropped_stale += 1;
                        continue;
                    }
                    match self.presentation.dispatch_semantics_action(request) {
                        Ok(()) => {
                            report.invoked += 1;
                        }
                        Err(error) => {
                            // Flutter deliberately ignores actions for stale
                            // views/nodes because screen readers may lag behind
                            // the latest semantics update.
                            tracing::trace!(
                                { flui_foundation::diagnostics::PRESENTATION_ID } =
                                    self.presentation.id().as_u64(),
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
    use std::num::NonZeroU32;
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
        UiRealm::new(wake, test_window(), 1.0, Arc::new(AtomicBool::new(false)))
    }

    fn new_runtime_with_capacity(
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<UiRealm, UiRealmError> {
        UiRealm::with_capacity(
            capacity,
            wake,
            test_window(),
            1.0,
            Arc::new(AtomicBool::new(false)),
        )
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
        let realm = UiRealm::for_test();
        let pipeline = realm.pipeline_for_test();
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
        let realm = UiRealm::for_test();
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root =
            semantics_owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        semantics_owner.set_root(Some(root));
        realm
            .pipeline_for_test()
            .write()
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

    /// Distinct from `stale_semantics_action_is_gracefully_dropped` above:
    /// that test forges a stale *node* id against a live presentation; this
    /// one forges a stale *presentation* stamp against a request whose node
    /// would otherwise resolve — proving the stamp check runs first and
    /// never lets the request reach `dispatch_semantics_action` (no
    /// pipeline borrow at all).
    ///
    /// If reverted: remove the `presentation_id` comparison from
    /// `drain_commands` and this fails (`invoked == 1` instead of
    /// `dropped_stale == 1`).
    #[test]
    fn semantics_action_with_stale_presentation_stamp_is_dropped() {
        let realm = UiRealm::for_test();
        let render_id = RenderId::new(1);
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_in_handler = Arc::clone(&invoked);
        let mut node = SemanticsNode::new().with_source_render_id(render_id);
        // The action handler is real and would succeed: this test's stamp
        // check must be what drops the request, not an unrelated resolution
        // failure (e.g. a node with no registered action at all, which
        // would drop for the wrong reason and pass even with the stamp
        // check deleted).
        node.config_mut().add_action(
            SemanticsAction::Tap,
            Arc::new(move |_action, _arguments| {
                invoked_in_handler.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root = semantics_owner.insert(node);
        semantics_owner.set_root(Some(root));
        realm
            .pipeline_for_test()
            .write()
            .set_semantics_owner(Some(semantics_owner));

        let live = realm.presentation_id();
        let forged = PresentationId::new_gen(
            live.index(),
            NonZeroU32::new(live.generation().get() + 1).expect("nonzero"),
        );
        realm
            .command_sender()
            .send(UiCommand::SemanticsAction {
                presentation_id: forged,
                request: SemanticsActionRequest::new(
                    AccessibilityNodeId::from(render_id),
                    SemanticsAction::Tap,
                ),
            })
            .expect("realm inbox has room");

        let report = realm.drain_commands();
        assert_eq!(report.invoked, 0);
        assert_eq!(report.dropped_stale, 1);
        assert_eq!(
            invoked.load(Ordering::SeqCst),
            0,
            "a stale-stamped action must never reach the handler at all"
        );
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
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        let filler = || NavigatorCommand::maybe_pop(navigator.command_target());

        sender.send_navigation(filler()).expect("first fits");
        sender.send_navigation(filler()).expect("second fits");
        let overflow = sender
            .send_navigation(filler())
            .expect_err("third command is rejected");
        assert!(matches!(
            overflow,
            CommandSendError::ChannelFull { capacity: 2, .. }
        ));
        // Draining frees the inbox again.
        let _ = runtime.drain_commands();
        sender.send_navigation(filler()).expect("room after drain");
    }

    #[test]
    fn dropped_runtime_yields_owner_gone() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        drop(runtime);
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        assert!(matches!(
            sender.send_navigation(NavigatorCommand::maybe_pop(navigator.command_target())),
            Err(CommandSendError::OwnerGone { .. })
        ));
    }

    #[test]
    fn channel_full_retry_preserves_the_rejected_payload() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = new_runtime_with_capacity(1, noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        let filler_navigator = NavigatorHandle::new();
        filler_navigator.seed_initial(test_route("/"));
        sender
            .send_navigation(NavigatorCommand::maybe_pop(
                filler_navigator.command_target(),
            ))
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

        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        sender
            .send_navigation(NavigatorCommand::maybe_pop(navigator.command_target()))
            .expect("inbox has room");
        sender
            .send_navigation(NavigatorCommand::maybe_pop(navigator.command_target()))
            .expect("inbox has room");

        assert_eq!(wake_count.load(Ordering::Relaxed), 2);
        let _ = runtime.drain_commands();
    }

    fn test_route(name: &'static str) -> SimpleRoute<i32> {
        SimpleRoute::new(move |_ctx| SizedBox::new(1.0, 1.0).into_view().boxed()).named(name)
    }

    /// A hot-reload command mutates the exact presentation owned by this
    /// realm and arms the realm's own redraw request. There is no process
    /// singleton left for it to resolve instead: `UiRealm::for_test`
    /// constructs a fully independent realm (its own pipeline, its own
    /// `needs_redraw` flag), so this test's realm is structurally the only
    /// thing the command can reach.
    #[cfg(feature = "hot-reload")]
    #[test]
    fn hot_reload_command_applies_to_the_owned_presentation() {
        let realm = UiRealm::for_test();

        realm
            .command_sender()
            .request_hot_reload(flui_hot_reload::HotReloadTier::HotReload)
            .expect("inbox has room");

        let report = realm.drain_commands();

        assert_eq!(
            report.invoked, 1,
            "the hot-reload command must be applied, not dropped as stale"
        );
        // `drain_commands`'s `HotReload` arm arms the coalesced
        // `redraw_pending` flag (`take_redraw_request`) directly on this
        // exact realm -- the point being tested: it never resolves a
        // process-wide instance, only the realm the sender was vended from.
        assert!(realm.take_redraw_request());
    }

    /// Full restart is owned by the process supervisor, so the presentation
    /// records the command as handled without arming a UI redraw.
    #[cfg(feature = "hot-reload")]
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

    // ========================================================================
    // Frame pipeline, first-frame deferral, and Vsync — migrated from the
    // retired `AppBinding`'s own test module (`binding.rs`, deleted alongside
    // it). These are the frame-loop parity oracle: `draw_frame_entered`'s
    // internal ordering (vsync tick before build, the async-driver
    // mid-frame slot, the pipeline-failure retry path) and the first-frame
    // deferral gate moved to `UiRealm` verbatim; only the receiver syntax
    // changed (`binding.draw_frame(&realm, c)` -> `realm.draw_frame(c)`),
    // never the assertions themselves.
    //
    // NOT migrated in this change (tracked as deferred, not silently
    // dropped): the gesture-arena/pointer-dispatch tests
    // (`shell_installed_arena_resolves_nested_tap_detectors_to_one_winner`,
    // `root_gesture_scope_arbitrates_overlapping_detectors_once`,
    // `realm_input_dispatch_keeps_gesture_state_isolated`,
    // `pointer_input_boundary_drains_a_lone_deferred_winner`,
    // `long_press_fires_at_its_deadline_with_no_further_input`,
    // `resampled_contact_motion_keeps_the_frame_wake_gate_open`), the two
    // scheduler-wake-hook-stealing tests (re-homed to `runner.rs` against
    // the `install_platform_realm`-based once-per-thread seam),
    // `frames_reenable_redirties_root_so_next_frame_paints_not_idle`
    // (re-homed to `runner.rs`, same reason), the IME/text-input module,
    // and the haptics/clipboard/performance-overlay modules (re-homed to
    // `presentation.rs`/`runtime.rs`, whose state now owns them).
    mod frame_pipeline_and_vsync {
        use std::sync::atomic::{AtomicBool as StdAtomicBool, AtomicUsize};

        use flui_engine::EngineError;
        use flui_foundation::HasInstance;
        use flui_types::geometry::px;

        use super::*;

        /// Minimal leaf view/element so a headless `attach_root_widget` has
        /// something to mount without pulling in a widget crate.
        #[derive(Clone)]
        struct LeafView;

        impl flui_view::RenderView for LeafView {
            type Protocol = flui_rendering::protocol::BoxProtocol;
            type RenderObject = flui_objects::RenderSizedBox;

            fn create_render_object(
                &self,
                _ctx: &flui_view::RenderObjectContext<'_>,
            ) -> Self::RenderObject {
                flui_objects::RenderSizedBox::shrink()
            }

            fn update_render_object(
                &self,
                _ctx: &flui_view::RenderObjectContext<'_>,
                render_object: &mut Self::RenderObject,
            ) {
                *render_object = flui_objects::RenderSizedBox::shrink();
            }
        }

        impl flui_view::View for LeafView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::render_variable(self)
            }
        }

        fn test_constraints() -> BoxConstraints {
            BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
        }

        #[test]
        fn dirty_mark_fires_wake_via_notifier() {
            let realm = UiRealm::for_test();
            let pipeline = realm.pipeline_for_test();

            let id = pipeline
                .write()
                .insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >);
            pipeline.write().clear_all_dirty_nodes();
            realm.mark_rendered();

            pipeline.write().mark_needs_layout(id);
            assert!(
                realm.needs_redraw(),
                "an owner dirty mark must wake the realm via the visual-update \
                 notifier wired in UiRealm::construct",
            );
        }

        #[test]
        fn cross_thread_dirty_handle_wakes_owner_binding_not_worker_tls() {
            let realm = UiRealm::for_test();
            realm.mark_rendered();

            let handle = realm.pipeline_for_test().read().handle();
            std::thread::spawn(move || {
                handle
                    .request_mark_dirty(
                        flui_foundation::RenderId::new(1),
                        flui_rendering::pipeline::DirtyKind::Paint,
                    )
                    .expect("dirty request should enqueue");
            })
            .join()
            .expect("worker thread should not panic");

            assert!(
                realm.needs_redraw(),
                "cross-thread dirty requests must wake the owner realm captured \
                 during UiRealm construction, not resolve a worker-local TLS realm"
            );
        }

        #[test]
        fn test_needs_redraw() {
            let realm = UiRealm::for_test();

            realm.mark_rendered();
            assert!(!realm.needs_redraw());

            realm.request_redraw();
            assert!(realm.needs_redraw());

            realm.mark_rendered();
            assert!(!realm.needs_redraw());
        }

        #[test]
        fn test_renderer_initialized() {
            let realm = UiRealm::for_test();
            // Verify the renderer sub-binding is accessible (created during
            // UiRealm::construct).
            let _renderer = realm.renderer();
        }

        /// E2/E3 regression: `UiRealm` hands its shared `PipelineOwner` to the
        /// `WidgetsBinding` it owns, so `attach_root_widget` actually
        /// bootstraps the root render tree.
        #[test]
        fn attach_root_widget_bootstraps_shared_render_tree() {
            let realm = UiRealm::for_test();
            realm
                .enter(|realm| realm.attach_root_widget(&LeafView))
                .expect("attach succeeds");
            assert!(
                realm.pipeline_for_test().read().root_id().is_some(),
                "UiRealm must pass its PipelineOwner to the widgets binding so the \
                 root render tree bootstraps; without it the window renders nothing",
            );
        }

        /// Root-hop parent-link regression: after a standard bootstrap
        /// (`attach_root_widget` + a build/layout/paint `draw_frame`), the
        /// mounted leaf's render node must have a working parent link back
        /// to the root, not just the root's child-list entry.
        #[test]
        fn transform_to_resolves_through_the_root_hop_after_standard_bootstrap() {
            let realm = UiRealm::for_test();
            realm
                .enter(|realm| realm.attach_root_widget(&LeafView))
                .expect("attach succeeds");
            let _ = realm.draw_frame(test_constraints());

            let owner = realm.pipeline_for_test();
            let owner = owner.read();
            let root_id = owner.root_id().expect("root id set by attach_root_widget");
            let root_node = owner
                .render_tree()
                .get(root_id)
                .expect("root render node resolves");
            let leaf_id = *root_node
                .children()
                .first()
                .expect("LeafView must have mounted one render child under the root");

            assert_eq!(
                owner
                    .render_tree()
                    .get(leaf_id)
                    .and_then(flui_rendering::storage::RenderNode::parent),
                Some(root_id),
                "the leaf's render node must carry a parent link back to the root"
            );

            let transform = owner.transform_to(leaf_id, root_id);
            assert!(
                transform.is_some(),
                "transform_to(leaf, root) must resolve through the root hop; None means the \
                 ancestor walk broke at the very first step"
            );
            assert_eq!(
                transform,
                Some(flui_types::Matrix4::IDENTITY),
                "LeafView (RenderSizedBox::shrink(), zero offset) composes to the identity \
                 transform into root space"
            );
        }

        /// Wiring test: `draw_frame` must invoke
        /// `WidgetsBinding::service_child_requests`, which drains the
        /// pipeline's `pending_child_requests` buffer.
        #[test]
        fn draw_frame_invokes_service_child_requests() {
            let realm = UiRealm::for_test();
            let pipeline = realm.pipeline_for_test();

            let sliver_id = pipeline
                .write()
                .insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >);
            pipeline
                .write()
                .push_pending_child_request_for_test(sliver_id, 0);
            {
                let mut guard = pipeline.write();
                let drained = guard.take_pending_child_requests();
                assert_eq!(drained.len(), 1, "seed must be present before draw_frame");
                guard.push_pending_child_request_for_test(sliver_id, 0);
            }

            let _ = realm.draw_frame(test_constraints());

            let remaining = pipeline.write().take_pending_child_requests();
            assert!(
                remaining.is_empty(),
                "draw_frame must drain pending_child_requests via service_child_requests; \
                 {} request(s) remained undrained — wiring is absent",
                remaining.len(),
            );
        }

        /// Serializes the tests that drive the process-global
        /// `Scheduler::instance()`.
        static SINGLETON_FRAME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        /// The production frame path polls the async driver **exactly once**,
        /// on the `Scheduler::instance()` singleton, in the mid-frame slot —
        /// and the pipeline runs afterwards, in the persistent slot.
        #[test]
        fn the_production_frame_polls_the_singletons_async_driver_once_before_the_pipeline() {
            let _serialized = SINGLETON_FRAME_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let realm = UiRealm::for_test();
            let scheduler = flui_scheduler::Scheduler::instance();

            let polls = Arc::new(AtomicUsize::new(0));
            let polls_for_task = Arc::clone(&polls);
            let _token = scheduler.spawn_local(Box::pin(async move {
                polls_for_task.fetch_add(1, Ordering::Release);
            }));
            assert_eq!(
                polls.load(Ordering::Acquire),
                0,
                "spawn must not poll inline"
            );

            let polled_before_pipeline = Arc::new(StdAtomicBool::new(false));
            let flag = Arc::clone(&polled_before_pipeline);
            let polls_probe = Arc::clone(&polls);

            scheduler.drive_frame(flui_scheduler::Instant::now(), || {
                flag.store(polls_probe.load(Ordering::Acquire) == 1, Ordering::Release);
                let _ = realm.draw_frame(test_constraints());
            });

            assert!(
                polled_before_pipeline.load(Ordering::Acquire),
                "the async driver must be polled before the pipeline runs"
            );
            assert_eq!(
                polls.load(Ordering::Acquire),
                1,
                "exactly one driver poll per frame"
            );
        }

        /// `draw_frame` no longer polls the driver itself.
        #[test]
        fn draw_frame_does_not_poll_the_async_driver_itself() {
            let _serialized = SINGLETON_FRAME_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let realm = UiRealm::for_test();
            let ran = Arc::new(StdAtomicBool::new(false));
            let ran_for_task = Arc::clone(&ran);
            let _token = flui_scheduler::Scheduler::instance().spawn_local(Box::pin(async move {
                ran_for_task.store(true, Ordering::Release);
            }));

            let _ = realm.draw_frame(test_constraints());

            assert!(
                !ran.load(Ordering::Acquire),
                "the driver step belongs to Scheduler::handle_begin_frame, not to the pipeline"
            );
        }

        /// **The production-path acceptance test.** A post-frame callback on
        /// the `Scheduler::instance()` singleton observes the geometry this
        /// realm's real pipeline committed **in the same frame**.
        #[test]
        fn production_post_frame_callback_observes_this_frames_committed_layout() {
            let _serialized = SINGLETON_FRAME_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            use flui_rendering::prelude::Leaf;
            use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, PaintCx, RenderBox};

            #[derive(Debug, Default)]
            struct FixedBox;
            impl flui_foundation::Diagnosticable for FixedBox {}
            impl RenderBox for FixedBox {
                type Arity = Leaf;
                type ParentData = BoxParentData;
                fn perform_layout(
                    &mut self,
                    _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
                ) -> flui_types::Size {
                    flui_types::Size::new(px(40.0), px(24.0))
                }
                fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
            }

            let realm = UiRealm::for_test();
            let pipeline = realm.pipeline_for_test();

            let root = {
                let mut owner = pipeline.write();
                let root =
                    owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(FixedBox));
                owner.set_root_id(Some(root));
                root
            };

            assert_eq!(
                pipeline.read().box_size(root),
                None,
                "nothing is laid out before the first frame"
            );

            let observed = Arc::new(RwLock::new(None));
            let calls = Arc::new(AtomicUsize::new(0));
            let observed_cb = Arc::clone(&observed);
            let calls_cb = Arc::clone(&calls);
            let pipeline_cb = Arc::clone(&pipeline);

            let scheduler = flui_scheduler::Scheduler::instance();
            scheduler.add_post_frame_callback(Box::new(move |_timing| {
                calls_cb.fetch_add(1, Ordering::SeqCst);
                *observed_cb.write() = pipeline_cb.read().box_size(root);
            }));

            scheduler.drive_frame(flui_scheduler::Instant::now(), || {
                let _ =
                    realm.draw_frame(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)));
            });

            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                *observed.read(),
                Some(flui_types::Size::new(px(40.0), px(24.0))),
                "the production post-frame callback must observe THIS frame's layout"
            );
        }

        /// Wiring test: `draw_frame` must run the shared layout<->build
        /// fixpoint (`BuildOwner::run_frame_with_layout_builders`), not a bare
        /// `PipelineOwner::run_frame`.
        #[test]
        fn draw_frame_invokes_the_layout_builder_seam() {
            let realm = UiRealm::for_test();

            realm.widgets().with_build_owner_mut(|owner| {
                let _cell = owner.register_layout_builder_for_test(
                    flui_foundation::RenderId::new(1),
                    flui_foundation::ElementId::new(1),
                );
                assert_eq!(owner.layout_builder_count(), 1);
            });

            let _ = realm.draw_frame(test_constraints());

            realm.widgets().with_build_owner_mut(|owner| {
                assert_eq!(
                    owner.layout_builder_count(),
                    0,
                    "draw_frame must run service_layout_builders (via the shared \
                     run_frame_with_layout_builders helper), which prunes the stale entry"
                );
            });
        }

        /// Wake-gate contract: after a frame marks a render node dirty,
        /// `has_pending_work()` must return `true` so the runner gate
        /// schedules the settling frame; once no nodes are dirty,
        /// `has_pending_work()` is `false` and the app can go idle.
        #[test]
        fn wake_gate_schedules_settling_frame_after_dirty_mark() {
            let realm = UiRealm::for_test();
            let pipeline = realm.pipeline_for_test();

            realm.mark_rendered();
            assert!(!realm.needs_redraw(), "precondition: needs_redraw clear");

            let node_id = pipeline
                .write()
                .insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >);
            pipeline.write().clear_all_dirty_nodes();
            assert!(
                !realm.has_pending_work(),
                "baseline: no pending work after clearing dirty nodes",
            );

            pipeline.write().mark_needs_layout(node_id);
            assert!(
                realm.has_pending_work(),
                "a dirty layout node must make has_pending_work() true so the runner \
                 schedules the settling frame",
            );

            pipeline.write().clear_all_dirty_nodes();
            assert!(
                !realm.has_pending_work(),
                "after clearing dirty nodes has_pending_work() must be false so a \
                 settled lazy-list app does not loop forever",
            );
        }

        // ---- Input lifecycle gate (ADR-0037 §9) ------------------------------

        /// `Suspended` drops pointer input only — gesture-arena protection —
        /// while keyboard/IME continue to flow. A flaky or absent occlusion
        /// signal (the web backend wires none) must never become a
        /// keystroke blackout.
        ///
        /// If reverted: remove the `Suspended` pointer-only arm from
        /// `input_dropped_by_lifecycle` and the pointer assertions below
        /// fail (the arena receives the down and `needs_redraw` is armed
        /// even though the presentation is suspended).
        #[test]
        fn pointer_input_is_dropped_while_suspended_but_keyboard_flows() {
            use flui_interaction::events::{PointerType, make_down_event};

            let realm = UiRealm::for_test();
            realm.handle_presentation_lifecycle(AppLifecycleState::Hidden);

            let position = flui_types::Offset::new(px(50.0), px(50.0));
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
                    position,
                    PointerType::Mouse,
                )));
            });
            assert_eq!(
                realm.gestures().active_pointer_count(),
                0,
                "a pointer down while suspended must never reach the gesture arena"
            );
            assert!(
                !realm.needs_redraw(),
                "a dropped pointer event must never arm a redraw"
            );

            // Keyboard/IME keep flowing while suspended: falling through to
            // the ordinary dispatch path is observable because that path
            // (unlike the pointer early-return above) always requests a
            // redraw.
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Ime(flui_types::ImeEvent::Commit(
                    "suspended-ime".to_string(),
                )));
            });
            assert!(
                realm.needs_redraw(),
                "IME input must keep flowing while the presentation is only suspended"
            );

            // Resume: pointer flows again.
            realm.mark_rendered();
            realm.handle_presentation_lifecycle(AppLifecycleState::Resumed);
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
                    position,
                    PointerType::Mouse,
                )));
            });
            assert_eq!(
                realm.gestures().active_pointer_count(),
                1,
                "pointer input must reach the arena again once resumed"
            );
        }

        /// `Closing`/`Closed` is a hard gate: every input kind is dropped,
        /// not just pointer.
        ///
        /// If reverted: remove the `Closing | Closed` hard-gate arm from
        /// `input_dropped_by_lifecycle` and the IME assertion below fails (a
        /// "closed" presentation still dispatches and arms a redraw).
        #[test]
        fn all_input_dropped_after_close() {
            use flui_interaction::events::{PointerType, make_down_event};

            let realm = UiRealm::for_test();
            realm.handle_presentation_lifecycle(AppLifecycleState::Detached);

            let position = flui_types::Offset::new(px(50.0), px(50.0));
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
                    position,
                    PointerType::Mouse,
                )));
            });
            assert_eq!(
                realm.gestures().active_pointer_count(),
                0,
                "pointer input must never reach the arena once closed"
            );
            assert!(
                !realm.needs_redraw(),
                "no input at all may reach dispatch once closed"
            );

            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Ime(flui_types::ImeEvent::Commit(
                    "closed-ime".to_string(),
                )));
            });
            assert!(
                !realm.needs_redraw(),
                "IME input must also be dropped once closed — the hard gate covers every kind"
            );
        }

        // ---- Vsync wiring (production frame continuation) -------------------

        fn make_controller(duration_ms: u64) -> flui_animation::AnimationController {
            use std::time::Duration;
            flui_animation::AnimationController::new(
                Duration::from_millis(duration_ms),
                Arc::new(flui_scheduler::Scheduler::new()),
            )
        }

        /// V1 — Frame continuation (the key test): a running controller
        /// registered in the realm's Vsync must keep the runner gate
        /// schedulable across every mid-animation frame, and the gate must
        /// go idle once the controller completes.
        #[test]
        fn vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle() {
            use flui_animation::{Animation, AnimationStatus};

            let realm = UiRealm::for_test();
            let vsync = realm.vsync();

            let controller = make_controller(100);
            vsync.register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            let constraints = test_constraints();

            realm.set_now_secs_for_test(0.0);
            realm.mark_rendered();
            let _ = realm.draw_frame(constraints);
            assert!(
                realm.needs_redraw() || realm.has_pending_work(),
                "V1: the runner gate must be open after an anchor frame",
            );

            realm.set_now_secs_for_test(0.05);
            realm.mark_rendered();
            let _ = realm.draw_frame(constraints);
            assert!(
                realm.needs_redraw() || realm.has_pending_work(),
                "V1: runner gate must remain open at t=0.05s",
            );
            let mid_value = controller.value();
            assert!(
                mid_value > 0.1 && mid_value < 0.95,
                "V1: sanity — controller is mid-run at t=50ms (value={mid_value})",
            );

            realm.set_now_secs_for_test(0.20);
            realm.mark_rendered();
            let _ = realm.draw_frame(constraints);
            assert_eq!(controller.status(), AnimationStatus::Completed);

            assert!(
                !realm.needs_redraw(),
                "V1: the runner gate must be CLOSED after settle",
            );
            assert!(!realm.has_vsync_running());

            controller.dispose();
        }

        /// V2 — Value advances across injected-time frames.
        #[test]
        fn vsync_value_advances_across_frames() {
            use flui_animation::Animation;

            let realm = UiRealm::for_test();
            let vsync = realm.vsync();
            let controller = make_controller(200);
            vsync.register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            let constraints = test_constraints();

            realm.set_now_secs_for_test(0.0);
            let _ = realm.draw_frame(constraints);
            let v0 = controller.value();

            realm.set_now_secs_for_test(0.10);
            let _ = realm.draw_frame(constraints);
            let v1 = controller.value();

            assert!(
                v1 > v0,
                "V2: controller value must increase (v0={v0}, v1={v1})"
            );
            assert!(
                (v1 - 0.5).abs() < 0.05,
                "V2: at t=100ms/200ms run ~0.5 (got {v1})"
            );

            controller.dispose();
        }

        /// V3 — Exactly-once-per-frame (no double-advance).
        #[test]
        fn vsync_tick_exactly_once_per_frame() {
            use flui_animation::{Animation, AnimationStatus};

            let realm = UiRealm::for_test();
            let vsync = realm.vsync();
            let controller = make_controller(100);
            vsync.register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            let constraints = test_constraints();

            realm.set_now_secs_for_test(0.0);
            let _ = realm.draw_frame(constraints);

            realm.set_now_secs_for_test(0.05);
            let _ = realm.draw_frame(constraints);
            assert_ne!(
                controller.status(),
                AnimationStatus::Completed,
                "V3: must NOT be complete at t=50ms (100ms duration)",
            );

            realm.set_now_secs_for_test(0.15);
            let _ = realm.draw_frame(constraints);
            assert_eq!(controller.status(), AnimationStatus::Completed);

            controller.dispose();
        }

        use flui_view::{IntoView, StatefulView, ViewState};

        /// Test-local view that captures the auto-injected `VsyncScope` in
        /// `init_state`, registers a caller-supplied controller, and starts
        /// it running.
        #[derive(Clone)]
        struct VsyncProbeView {
            controller_to_register: flui_animation::AnimationController,
        }

        struct VsyncProbeState {
            controller: flui_animation::AnimationController,
        }

        impl StatefulView for VsyncProbeView {
            type State = VsyncProbeState;

            fn create_state(&self) -> Self::State {
                VsyncProbeState {
                    controller: self.controller_to_register.clone(),
                }
            }
        }

        impl ViewState<VsyncProbeView> for VsyncProbeState {
            fn init_state(&mut self, ctx: &dyn flui_view::BuildContext) {
                use flui_view::BuildContextExt as _;
                if let Some(vsync) =
                    ctx.get::<flui_widgets::VsyncScope, _>(|scope| scope.vsync().clone())
                {
                    vsync.register(self.controller.clone());
                    self.controller.forward().ok();
                }
            }

            fn build(
                &self,
                _view: &VsyncProbeView,
                _ctx: &dyn flui_view::BuildContext,
            ) -> impl IntoView {
                LeafView
            }
        }

        impl flui_view::View for VsyncProbeView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateful(self)
            }
        }

        fn make_vsync_probe() -> (VsyncProbeView, flui_animation::AnimationController) {
            use std::time::Duration;
            let controller = flui_animation::AnimationController::new(
                Duration::from_millis(200),
                Arc::new(flui_scheduler::Scheduler::new()),
            );
            let view = VsyncProbeView {
                controller_to_register: controller.clone(),
            };
            (view, controller)
        }

        /// A1 — Auto-wrap causes registration after the first build pass.
        #[test]
        fn a1_autowrap_causes_registration_after_build_pass() {
            let realm = UiRealm::for_test();
            let (probe, controller) = make_vsync_probe();

            realm
                .attach_root_widget(&probe)
                .expect("a fresh UiRealm must accept its first root widget");

            assert!(
                realm.vsync().is_empty(),
                "A1 precondition: controller must not be registered before the first build pass",
            );

            let _ = realm.draw_frame(test_constraints());

            assert!(
                !realm.vsync().is_empty(),
                "A1: after a build pass the controller registered in init_state must appear \
                 in realm.vsync()",
            );

            controller.dispose();
        }

        /// A2 — End-to-end tick: auto-wrap -> register -> tick -> value advances.
        #[test]
        fn a2_autowrap_end_to_end_tick_advances_controller_value() {
            use flui_animation::Animation as _;

            let realm = UiRealm::for_test();
            let (probe, controller) = make_vsync_probe();
            realm
                .attach_root_widget(&probe)
                .expect("a fresh UiRealm must accept its first root widget");

            realm.set_now_secs_for_test(0.0);
            let _ = realm.draw_frame(test_constraints());
            assert!(!realm.vsync().is_empty());

            realm.set_now_secs_for_test(0.1);
            let _ = realm.draw_frame(test_constraints());
            let value_after_anchor = controller.value();

            realm.set_now_secs_for_test(0.2);
            let _ = realm.draw_frame(test_constraints());
            let value_at_50_percent = controller.value();

            assert!(
                value_at_50_percent > value_after_anchor,
                "A2: controller value must advance from the anchor frame to t=0.2s \
                 (anchor={value_after_anchor}, t=200ms={value_at_50_percent})",
            );

            controller.dispose();
        }

        /// A3 — No-animation root: auto-wrap registers nothing itself.
        #[test]
        fn a3_no_animation_root_vsync_stays_empty_after_build_pass() {
            let realm = UiRealm::for_test();
            realm
                .attach_root_widget(&LeafView)
                .expect("a fresh UiRealm must accept its first root widget");

            let _ = realm.draw_frame(test_constraints());

            assert!(
                realm.vsync().is_empty(),
                "A3: a root with no implicitly-animated widgets must not register anything",
            );
        }

        /// V4 — No-animation app idles cheaply.
        #[test]
        fn vsync_empty_does_not_keep_gate_open() {
            let realm = UiRealm::for_test();
            assert!(realm.vsync().is_empty(), "precondition: Vsync is empty");

            let constraints = test_constraints();
            realm.set_now_secs_for_test(1.0);
            realm.mark_rendered();

            let _ = realm.draw_frame(constraints);

            assert!(
                !realm.has_vsync_running(),
                "V4: has_vsync_running() must be false when no controllers are registered",
            );
        }

        // ---- render_frame_entered retry / first-frame-deferral semantics ----

        struct ScriptedRasterBackend {
            outcome: Option<Result<bool, EngineError>>,
            render_scene_calls: u32,
        }

        impl ScriptedRasterBackend {
            fn new(outcome: Result<bool, EngineError>) -> Self {
                Self {
                    outcome: Some(outcome),
                    render_scene_calls: 0,
                }
            }
        }

        impl RasterBackend for ScriptedRasterBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                self.render_scene_calls += 1;
                self.outcome
                    .take()
                    .expect("render_scene called more than once in a single-frame test")
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
            }
            fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
            fn mark_full_repaint(&mut self) {}
            fn has_damage(&self) -> bool {
                true
            }
            fn size(&self) -> (u32, u32) {
                (800, 600)
            }
            fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
                Ok(())
            }
        }

        fn mount_root() -> UiRealm {
            let realm = UiRealm::for_test();
            realm
                .enter(|realm| realm.attach_root_widget(&LeafView))
                .expect("attach succeeds");
            realm
        }

        #[test]
        fn surface_lost_keeps_needs_redraw_armed_for_a_retry() {
            let realm = mount_root();
            let mut backend = ScriptedRasterBackend::new(Err(EngineError::SurfaceLost));

            realm.mark_rendered();
            let presented = realm.render_frame_entered(&mut backend);

            assert!(!presented, "a SurfaceLost frame never reaches present()");
            assert_eq!(
                backend.render_scene_calls, 1,
                "precondition: the mounted scene actually reached render_scene"
            );
            assert!(
                realm.needs_redraw(),
                "a dropped SurfaceLost frame must re-arm needs_redraw so the next wake \
                 actually retries"
            );
        }

        #[test]
        fn a_successful_frame_still_clears_needs_redraw() {
            let realm = mount_root();
            let mut backend = ScriptedRasterBackend::new(Ok(true));

            realm.request_redraw();
            let presented = realm.render_frame_entered(&mut backend);

            assert!(presented, "Ok(true) means render_scene reached present()");
            assert!(
                !realm.needs_redraw(),
                "a successfully presented frame must clear needs_redraw"
            );
        }

        struct CountingRasterBackend {
            render_scene_calls: u32,
        }

        impl CountingRasterBackend {
            fn new() -> Self {
                Self {
                    render_scene_calls: 0,
                }
            }
        }

        impl RasterBackend for CountingRasterBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                self.render_scene_calls += 1;
                Ok(true)
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
            }
            fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
            fn mark_full_repaint(&mut self) {}
            fn has_damage(&self) -> bool {
                true
            }
            fn size(&self) -> (u32, u32) {
                (800, 600)
            }
            fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
                Ok(())
            }
        }

        #[test]
        fn deferred_first_frame_runs_the_pipeline_but_withholds_the_scene() {
            let realm = mount_root();
            let mut backend = CountingRasterBackend::new();

            realm.defer_first_frame();
            realm.mark_rendered();

            let presented = realm.render_frame_entered(&mut backend);

            assert!(!presented, "a deferred first frame must never present");
            assert_eq!(
                backend.render_scene_calls, 0,
                "the scene must never reach render_scene while deferred"
            );
            assert_eq!(realm.frames_rendered(), 0);
            assert!(
                !realm.needs_redraw(),
                "deferred is not errored: it must not spam a retry wake"
            );
        }

        #[test]
        fn allow_first_frame_alone_presents_the_previously_withheld_content() {
            let realm = mount_root();
            let mut backend = CountingRasterBackend::new();

            realm.defer_first_frame();
            let withheld = realm.render_frame_entered(&mut backend);
            assert!(
                !withheld,
                "precondition: the first frame is withheld while deferred"
            );
            assert_eq!(backend.render_scene_calls, 0);

            realm.allow_first_frame();

            let presented = realm.render_frame_entered(&mut backend);

            assert!(
                presented,
                "allow_first_frame alone (no external re-dirty) must make the withheld \
                 content reach present() on the next pumped frame"
            );
            assert_eq!(backend.render_scene_calls, 1);
            assert_eq!(realm.frames_rendered(), 1);
        }

        #[test]
        fn nested_defer_allow_only_presents_after_the_last_allow() {
            let realm = mount_root();
            let mut backend = CountingRasterBackend::new();

            realm.defer_first_frame();
            realm.defer_first_frame();

            assert!(!realm.render_frame_entered(&mut backend));
            assert_eq!(backend.render_scene_calls, 0);

            realm.allow_first_frame();
            assert!(
                !realm.render_frame_entered(&mut backend),
                "one matching allow of two nested defers must not yet open the gate"
            );
            assert_eq!(backend.render_scene_calls, 0);

            realm.allow_first_frame();
            assert!(
                realm.render_frame_entered(&mut backend),
                "the last matching allow must open the gate"
            );
            assert_eq!(backend.render_scene_calls, 1);
        }

        #[test]
        #[should_panic(expected = "allow_first_frame called without matching defer_first_frame")]
        fn allow_first_frame_without_matching_defer_panics() {
            let realm = UiRealm::for_test();
            realm.allow_first_frame();
        }

        /// Root `RenderBox` whose layout panics — the exact catch_unwind path
        /// any third-party panic in production widget code reaches.
        #[derive(Debug)]
        struct PanicOnLayoutBox;

        impl flui_foundation::Diagnosticable for PanicOnLayoutBox {}

        impl flui_rendering::traits::RenderBox for PanicOnLayoutBox {
            type Arity = flui_rendering::prelude::Leaf;
            type ParentData = flui_rendering::prelude::BoxParentData;

            fn perform_layout(
                &mut self,
                _ctx: &mut flui_rendering::context::BoxLayoutContext<
                    '_,
                    Self::Arity,
                    Self::ParentData,
                >,
            ) -> flui_types::Size {
                panic!("PanicOnLayoutBox::perform_layout -- intentional test panic");
            }
        }

        fn mount_panicking_root() -> UiRealm {
            let realm = UiRealm::for_test();
            let pipeline = realm.pipeline_for_test();
            let mut owner = pipeline.write();
            let root_id = owner.insert(Box::new(PanicOnLayoutBox)
                as Box<
                    dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
                >);
            owner.set_root_id(Some(root_id));
            drop(owner);
            realm
        }

        #[test]
        fn errored_first_frame_does_not_latch_first_frame_sent() {
            let realm = mount_panicking_root();
            let mut backend = CountingRasterBackend::new();

            let prev_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let presented = realm.render_frame_entered(&mut backend);
            std::panic::set_hook(prev_hook);

            assert!(!presented, "an errored frame must never present");
            assert_eq!(backend.render_scene_calls, 0);

            assert!(realm.send_frames_to_engine());

            realm.defer_first_frame();
            assert!(
                !realm.send_frames_to_engine(),
                "an errored first frame must not latch first_frame_sent"
            );
        }

        #[test]
        fn first_frame_sent_latch_short_circuits_later_defers() {
            let realm = mount_root();
            let mut backend = CountingRasterBackend::new();

            let presented = realm.render_frame_entered(&mut backend);
            assert!(
                presented,
                "precondition: the first frame presents with no active deferral"
            );
            assert!(realm.send_frames_to_engine());

            realm.defer_first_frame();
            assert!(
                realm.send_frames_to_engine(),
                "a defer registered AFTER the first frame was sent must not re-close the gate"
            );
        }
    }
}
