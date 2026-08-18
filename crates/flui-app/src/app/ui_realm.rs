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
//! # Coexistence
//!
//! Singleton retirement is complete: every service a `UiRealm` consumes
//! (widgets binding, gesture arena, focus tree, scheduler) is owned by the
//! realm itself, resolved once at construction (`RealmServices::construct`,
//! `runtime.rs`) rather than reached ambiently. There is no process-global
//! graph left to alias, so nothing enforces at-most-one instance any more —
//! any number of realms may be constructed and driven concurrently, on one
//! thread or several (see `two_realms_coexist_same_thread` and
//! `two_realms_two_threads_no_shared_state` below). Each incarnation still
//! gets a fresh generational [`RealmId`], so results stamped for a dead
//! runtime are droppable by identity, not by convention.

use std::cell::Cell;
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
use flui_interaction::{FocusManager, GestureBinding, InteractionLane};
use flui_layer::Scene;
#[cfg(test)]
use flui_platform::traits::PlatformTextInput;
use flui_platform::traits::{DragDropEvent, PlatformInput, PlatformWindow};
use flui_rendering::binding::RendererBinding as _;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_scheduler::{
    AppLifecycleState, DemandKind, FrameSnapshot, Instant, LocalPostFrameLane, PresentOutcome,
    SchedulerPhase, UpdateScheduler,
};
use flui_semantics::SemanticsActionRequest;
use flui_types::{HapticFeedback, Size, geometry::px};
use flui_view::{GlobalKeyRegistryComposite, GlobalKeyScope, WidgetsBinding};
use flui_widgets::{FocusRoot, GestureArenaScope, NavigatorCommand, VsyncScope};
#[cfg(test)]
use parking_lot::RwLock;

use super::presentation::{PresentationState, RealmCapabilities};
use super::presentation_forest::PresentationForest;
use super::runtime::RealmServices;
use crate::bindings::RenderingFlutterBinding;

/// Default bound of the owner inbox, matching the pipeline dirty-channel
/// precedent (`DEFAULT_DIRTY_CHANNEL_CAPACITY`)). Observable at
/// runtime via `UiCommandSender::capacity`; not part of the public API.
const DEFAULT_COMMAND_CAPACITY: usize = 256;

/// Realm-level arbitration of which presentation currently owns OS keyboard
/// focus (issue #555's addressed-routing slice). One [`FocusManager`] exists per presentation
/// (`PresentationState::focus_manager`), but only ONE presentation's tree
/// should ever receive a keyboard event at a time — the one whose native
/// window the platform most recently reported as focused.
/// [`UiRealm::handle_input_addressed`]'s `Keyboard` arm routes there,
/// deliberately never to whichever presentation an individual event happens
/// to be stamped for: a stray or racing keyboard event addressed to a
/// presentation that is not (or no longer) the OS-focused one must still
/// land on the active one, matching a real OS's single-keyboard-focus model
/// rather than trusting the per-event address.
///
/// Defaults to the realm's initial presentation, so single-presentation
/// production topology (today's only shipped shape) observes no behavior
/// change: focus starts exactly where it always did, and only moves once a
/// genuine `WindowFocus(true)` event names a different, currently-hosted
/// presentation.
struct FocusCoordinator {
    active: Cell<PresentationId>,
}

impl FocusCoordinator {
    fn new(initial: PresentationId) -> Self {
        Self {
            active: Cell::new(initial),
        }
    }

    /// The presentation keyboard input currently routes to.
    fn active(&self) -> PresentationId {
        self.active.get()
    }

    /// Record that `id` is now the active presentation. Two write sides:
    /// `PlatformToUi::WindowFocus(true)` handling (`runner.rs`) calls this
    /// with the event's own addressed presentation when its native window
    /// gains OS focus; [`UiRealm::close_presentation_entered`] calls this
    /// with the surviving primary when `id` is the presentation being
    /// closed and it was the active one (never leave this pointing at a
    /// presentation the forest is about to drop).
    ///
    /// A `WindowFocus(false)` (focus LOST) never calls this: losing focus
    /// names no new owner, so the coordinator keeps pointing at whichever
    /// presentation held it most recently — the same retention Flutter's
    /// own single-view focus model has (losing OS focus does not un-focus
    /// the last-focused element; a *different* window gaining focus does).
    fn note_focus_gained(&self, id: PresentationId) {
        self.active.set(id);
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors constructing a [`UiRealm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub(crate) enum UiRealmError {
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
    /// Owner-local callback queue, activated with the realm's other TLS scope.
    local_post_frame: LocalPostFrameLane,
    /// Owner-local interaction callback storage, activated with the realm scope.
    interaction_lane: InteractionLane,
    /// The live source behind the root `MediaQuery` — the platform's resize
    /// and appearance signals write it, the root wrapper republishes it.
    /// Primary-presentation-scoped, like `attach_root_widget` itself.
    media_query: std::rc::Rc<crate::app::media_query_root::MediaQuerySource>,
    /// This realm's cross-tree `GlobalKey` uniqueness domain (ADR-0043 §1),
    /// installed into every presentation's `BuildOwner` at assembly time
    /// (`PresentationState::new`). Retained here (not just handed off once)
    /// so a later-installed presentation can share the same scope: both
    /// [`Self::assemble_presentation`] (production, issue #555's
    /// addressed-routing slice) and its `install_second_presentation_for_test`
    /// counterpart (the isolation test suite) read it back to assemble a
    /// second presentation sharing this exact scope.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "read back only by assemble_presentation, reachable only through \
                      runner.rs::install_presentation_alongside, itself desktop-only"
        )
    )]
    global_key_scope: GlobalKeyScope,
    /// The insertion-ordered set of UI-owner presentation domains this realm
    /// hosts (ADR-0043 §1 — `PresentationForest`). Production topology
    /// allows any number of presentations per realm since issue #555's addressed-routing slice
    /// lifted the forest's former `len()<=1` ratchet; see
    /// [`PresentationForest`]'s doc. Everything that builds, lays out,
    /// focuses, or presents for one surface lives in and dies with its own
    /// `PresentationState` inside this forest — the realm owns only what is
    /// genuinely cross-tree (this struct's other fields).
    presentations: PresentationForest,
    /// Realm-level arbitration of which presentation currently owns OS
    /// keyboard focus (ADR-0043 §4, issue #555's addressed-routing slice). See
    /// [`FocusCoordinator`]'s own doc.
    focus_coordinator: FocusCoordinator,
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
    /// Test-only injectable clock, stored as the f64 bits in a u64 atomic
    /// (rather than an `Option<f64>`/`Cell<f64>`) so [`Self::now_secs`] can
    /// read it with a single relaxed load; `0u64` is the "not set" sentinel
    /// (see [`Self::set_now_secs_for_test`] for why a genuine `t=0.0` is
    /// nudged to the smallest positive subnormal instead).
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
    /// This realm's OWN scheduler — the strong root every `WeakUpdateScheduler`
    /// this realm vends (tickers, `PostFrameHandle`s) upgrades against.
    /// Built fresh per realm by [`RealmServices::construct`], never a
    /// process-global singleton: when this realm drops, this field drops
    /// with it, and every retained weak handle starts failing closed.
    /// Read directly for the idle-only commit-gate phase probe in
    /// [`Self::drain_commands`] and vended to `runner.rs`'s lifecycle sites
    /// via [`Self::scheduler`].
    scheduler: UpdateScheduler,
    /// `*const ()` is `!Send + !Sync`; `PhantomData` of it makes the runtime
    /// so at zero cost (thread-affinity marker).
    _owner_affine: PhantomData<*const ()>,
}

impl std::fmt::Debug for UiRealm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiRealm")
            .field("realm_id", &self.realm_id)
            .field("presentation_id", &self.presentations.primary().id())
            .field("presentation_count", &self.presentations.len())
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
    /// A fresh layer tree was painted and turned into a `Scene`. Holds
    /// `Scene` by value, not `Arc<Scene>`: the sole reader
    /// (`render_frame_entered`, immediately below) borrows it and both are
    /// dropped in the same call stack — nothing shares this value or
    /// crosses a thread with it, so an `Arc` bought nothing here. `Scene`
    /// is not `Sync` (`CompositionCallback` is `FnOnce + Send`, never
    /// `Sync`), which is why wrapping it in `Arc` used to need a
    /// `#[expect(clippy::arc_with_non_send_sync)]` at the construction
    /// site below — removed along with the `Arc`. (`SceneSnapshot`'s own
    /// "never `Arc<Scene>`" contract in `scene_snapshot.rs` is a related
    /// but separate rule for the raster-mailbox seam; `SceneSnapshot` is
    /// not on this call path at all — production has no consumer of it
    /// yet.)
    Painted(Scene),
    /// Nothing was dirty this frame; no new content to composite.
    Idle,
    /// The build/layout/paint transaction failed (e.g. a render object
    /// panicked and was caught by `catch_unwind`); the frame was dropped and
    /// must be retried.
    Errored,
}

/// Whether [`UiRealm::record_submit_telemetry`] drains a presentation's
/// pending input epochs into the [`flui_scheduler::FrameSnapshot`] it
/// records, or merely reads them without consuming them.
///
/// `Retain` exists for exactly one shape: a submit failure whose own caller
/// has already armed a retry. **Three** arms in
/// [`UiRealm::render_frame_entered`] are that shape — `SurfaceLost`,
/// `DeviceLost` and `SurfaceValidation`. The latter two joined when device
/// loss gained a retry at all; before that they drained, and this sentence
/// named only `SurfaceLost`. Each is pinned by its own
/// `*_retry_preserves_the_original_input_epoch_for_the_presented_frame`
/// test, so flipping any of the three back to `Drain` turns one red — the
/// invariant used to be documentation alone. Draining on such an arm
/// leaves the
/// eventual retry's own real submit with nothing pending to attribute to
/// the frame that actually reaches the screen, so the inputs that arrived
/// before the failure would never be attributed to any frame at all. Every
/// other outcome (a real present, or a submit failure with no armed retry)
/// uses `Drain`, the same as before this type existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EpochDisposition {
    /// Drain pending epochs into the recorded snapshot (the common case).
    Drain,
    /// Read pending epochs into the recorded snapshot without consuming
    /// them, for a failed submit a retry will resubmit shortly.
    Retain,
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
/// presentation's current lifecycle. Moved here from the retired
/// `AppBinding`, unchanged — the match arms below are the per-lifecycle
/// rationale: a suspended presentation drops pointer/drag input (no gesture
/// arena to resolve into) but still accepts keyboard/IME (a background
/// window can keep text-input focus), while a not-yet-attached or closing
/// presentation drops everything.
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
    /// [`UiRealmError::InteractionLane`] if the owner-local interaction lane
    /// could not be created.
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
    /// [`UiRealmError::InteractionLane`] if the owner-local interaction lane
    /// could not be created.
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
        let identity = super::runtime::next_identity();
        let services = RealmServices::construct();
        Self::construct(
            capacity,
            wake,
            identity,
            window,
            Some(device_pixel_ratio),
            services,
            needs_redraw,
        )
    }

    /// Builds the realm from already-resolved pieces: identity, the
    /// presentation's window, and `services: RealmServices` — a fresh
    /// `UpdateScheduler` plus the `local_post_frame_lane()`/`async_driver()`
    /// handles derived from it, built by the caller (`RealmServices::
    /// construct`, in `runtime.rs`), which is what makes `UiRealm` perform
    /// zero `::instance()` calls and gives every realm its own scheduler
    /// strong root instead of sharing a process-global one.
    ///
    /// `device_pixel_ratio` is `None` only for the `#[cfg(test)]`
    /// constructors, which never touched it before this function existed
    /// (`PipelineOwner::new`'s own default applies) — preserved exactly,
    /// not silently changed to an explicit `1.0`.
    ///
    /// Assembles this realm's INITIAL presentation via
    /// [`PresentationState::new`], which installs the scope, the realm's
    /// shared dispatch handles, and this presentation's own focus/IME
    /// before anything attaches or mounts (ADR-0043 §1). Production
    /// topology is no longer limited to this one presentation:
    /// `PresentationForest`'s former `len()<=1` ratchet is lifted (issue
    /// #555's addressed-routing slice) — [`Self::install_presentation`]
    /// (paired with [`Self::assemble_presentation`]) is how a realm grows
    /// PAST this initial presentation, once the realm itself already
    /// exists.
    fn construct(
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
        (realm_id, presentation_id): (RealmId, PresentationId),
        window: Arc<dyn PlatformWindow>,
        device_pixel_ratio: Option<f32>,
        services: RealmServices,
        needs_redraw: Arc<AtomicBool>,
    ) -> Result<Self, UiRealmError> {
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        let RealmServices {
            local_post_frame,
            async_driver,
            scheduler,
        } = services;

        // The realm's scheduler fires the SAME platform wake its presentation
        // and command sender use. This is the edge an async completion travels:
        // a background task's `Waker::wake()` reaches `AsyncDriver`'s
        // request-frame, which reaches `UpdateScheduler::request_frame`, whose
        // `frame_scheduled` false->true transition fires this hook. Without it
        // that demand is a bare atomic store only an already-running pump can
        // observe, so an idle `ControlFlow::Wait` loop sleeps through it and
        // the future silently stops advancing until unrelated input arrives.
        //
        // Installed here rather than at each constructor because `construct`
        // is the single chokepoint every `UiRealm` is built through — a realm
        // with an unwired scheduler wake is not constructible.
        scheduler.set_on_frame_scheduled(Some(Arc::clone(&wake)));

        let interaction_lane = InteractionLane::try_new()?;
        let global_key_scope = GlobalKeyScope::new();

        let pipeline = PipelineCell::new(PipelineOwner::new());
        if let Some(device_pixel_ratio) = device_pixel_ratio {
            pipeline.with_mut(|owner| owner.set_device_pixel_ratio(device_pixel_ratio));
        }

        let presentation = PresentationState::new(
            presentation_id,
            pipeline,
            window,
            RealmCapabilities {
                global_key_scope: global_key_scope.clone(),
                async_driver,
                local_post_frame_handle: local_post_frame.local_handle(),
                interaction_dispatch_handle: interaction_lane.dispatch_handle(),
                scheduler: &scheduler,
                wake: Arc::clone(&wake),
                command_sender: UiCommandSender {
                    tx: tx.clone(),
                    capacity,
                    redraw_pending: Arc::clone(&redraw_pending),
                    presentation_id,
                    wake: Arc::clone(&wake),
                },
            },
        );

        Ok(Self {
            realm_id,
            local_post_frame,
            interaction_lane,
            media_query: std::rc::Rc::new(crate::app::media_query_root::MediaQuerySource::default()),
            global_key_scope,
            presentations: PresentationForest::single(presentation),
            focus_coordinator: FocusCoordinator::new(presentation_id),
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
        let identity = super::runtime::next_identity();
        let window = super::presentation::test_platform_window(platform_text_input);
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
            identity,
            window,
            None,
            RealmServices::construct(),
            needs_redraw,
        )
        .expect("test UiRealm should create an interaction lane")
    }

    /// Test-only: a clone of this realm's exact `PipelineCell` — the same
    /// one its primary presentation's `renderer` and `widgets` share (one
    /// fact, one place).
    #[cfg(test)]
    pub(crate) fn pipeline_for_test(&self) -> PipelineCell {
        self.presentations.primary().pipeline().clone()
    }

    /// This incarnation's generational realm identity.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.realm_id
    }

    /// Current presentation incarnation.
    #[must_use]
    pub fn presentation_id(&self) -> PresentationId {
        self.presentations.primary().id()
    }

    /// True if `id` names this realm's ONLY currently-hosted presentation.
    ///
    /// `runner.rs`'s `RealmTask::ClosePresentation` handling reads this
    /// BEFORE calling [`Self::close_presentation_entered`]: closing the
    /// realm's sole presentation would leave the forest empty, and every
    /// other method on this realm (`widgets()`, `presentation_id()`, a
    /// future frame pump) assumes `PresentationForest::primary()` always
    /// resolves — closing the last presentation is closing the REALM, and
    /// must route there instead (see that match arm's own doc).
    #[must_use]
    /// Whether `id` addresses this realm's primary presentation — the one
    /// whose widget tree hosts the root `MediaQuery`.
    pub(crate) fn is_primary_presentation(&self, id: PresentationId) -> bool {
        self.presentations.primary().id() == id
    }

    pub(crate) fn is_sole_presentation(&self, id: PresentationId) -> bool {
        self.presentations.len() == 1 && self.presentations.get(id).is_some()
    }

    /// The presentation id that would become this realm's PRIMARY if `id`
    /// were removed right now — WITHOUT actually removing it.
    ///
    /// `runner.rs`'s `RealmTask::ClosePresentation` handling reads this
    /// BEFORE running `id`'s own teardown/dispose hooks (`close_presentation_
    /// entered`'s step 2-3), not after: `RealmSlot::address.presentation_id`
    /// must already name the SURVIVING primary by the time a dispose hook
    /// could re-enter `dispatch_platform_realm` with `id`'s own dispatcher,
    /// or that reentrant dispatch would still compare EQUAL against the
    /// not-yet-updated address, pass the `StalePresentation` check, and get
    /// enqueued (same-realm reentrancy) instead of refused — running later
    /// in the same drain loop against whatever survives the close, silently
    /// misaddressed rather than refused outright.
    ///
    /// Mirrors exactly what [`PresentationForest::remove`]'s `Vec::remove`
    /// shift produces: if `id` is the CURRENT primary, the new primary is
    /// whichever presentation is next in mount order; otherwise removing
    /// `id` cannot move index 0 at all, so the primary is unchanged.
    /// `None` only if `id` names the realm's only presentation — unreachable
    /// from that same dispatch handling, which checks
    /// [`Self::is_sole_presentation`] first and never reaches this call in
    /// that case.
    #[must_use]
    pub(crate) fn primary_id_excluding(&self, id: PresentationId) -> Option<PresentationId> {
        if self.presentations.primary().id() != id {
            return Some(self.presentations.primary().id());
        }
        self.presentations
            .iter()
            .find(|presentation| presentation.id() != id)
            .map(PresentationState::id)
    }

    /// Number of presentations this realm currently hosts — for the
    /// isolation test suite (production topology allows any number since
    /// this slice lifted `PresentationForest`'s former ratchet).
    #[cfg(test)]
    pub(crate) fn presentation_count(&self) -> usize {
        self.presentations.len()
    }

    /// This realm's `WidgetsBinding` for the presentation named `id` — for
    /// the isolation test suite, which uses it to register a `GlobalKey`
    /// directly into a NON-primary presentation without going through
    /// [`Self::enter`] (registration itself needs no active composite; only
    /// resolution via [`flui_view::GlobalKey::current_element`] does).
    /// `pub(crate)` for the same cross-module reason as
    /// [`Self::install_second_presentation_for_test`] above.
    #[cfg(test)]
    pub(crate) fn presentation_widgets_for_test(&self, id: PresentationId) -> &WidgetsBinding {
        self.presentations
            .get(id)
            .expect("BUG: presentation_widgets_for_test called with an unknown id")
            .widgets()
    }

    /// This realm's `GestureBinding` for the presentation named `id` — the
    /// addressed counterpart to [`Self::gestures`] (primary-only), for the
    /// tests proving focus-loss cancellation reaches exactly the binding
    /// the addressed presentation's pointer input lands in.
    #[cfg(test)]
    pub(crate) fn presentation_gestures_for_test(&self, id: PresentationId) -> &GestureBinding {
        self.presentations
            .get(id)
            .expect("BUG: presentation_gestures_for_test called with an unknown id")
            .gestures()
    }

    /// This realm's `TextInputHandle` for the presentation named `id` — the
    /// addressed counterpart to [`Self::text_input_handle`] (primary-only),
    /// for the isolation suite proving IME sessions stay exclusive to the
    /// exact presentation they were attached on
    /// (`ime_event_addressed_to_b_does_not_reach_as_session`).
    #[must_use]
    #[cfg(test)]
    pub(crate) fn presentation_text_input_handle_for_test(
        &self,
        id: PresentationId,
    ) -> flui_interaction::TextInputHandle {
        self.presentations
            .get(id)
            .expect("BUG: presentation_text_input_handle_for_test called with an unknown id")
            .text_input_handle()
    }

    /// Assemble another presentation for this realm, sharing its exact
    /// `GlobalKeyScope` and realm-level dispatch handles — WITHOUT
    /// installing it into the forest yet. Deliberately split from
    /// [`Self::install_presentation`] below: a caller that must register a
    /// `WindowRegistry` mapping before this presentation is safely
    /// dispatchable (`runner.rs::install_presentation_alongside`) can do so
    /// BETWEEN assembly and installation — if registration fails, the
    /// assembled-but-never-installed `PresentationState` is simply dropped
    /// (no forest membership to roll back; its construction has no
    /// observable side effect on anything this realm shares, since nothing
    /// has attached/mounted into it yet).
    ///
    /// `window` becomes this presentation's own native window (cursor,
    /// haptics, redraw-poke — see `PresentationState::new`'s wiring).
    /// Its pipeline is seeded with `window.scale_factor()` before
    /// construction — the same DPR-before-first-frame ordering
    /// [`Self::construct`] uses for this realm's own primary presentation
    /// (there, the caller reads the window's scale factor and passes it in
    /// as `device_pixel_ratio`; here, `window` is already in hand, so this
    /// method reads it directly instead of requiring every caller to
    /// remember to). Without this, a non-primary presentation's
    /// `RenderView` and first frame would disagree with its OWN window's
    /// actual scale — silently defaulting to `1.0` regardless of what
    /// `window.scale_factor()` actually reports.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "reachable only through runner.rs::install_presentation_alongside, itself \
                      desktop-only"
        )
    )]
    pub(crate) fn assemble_presentation(
        &self,
        window: Arc<dyn PlatformWindow>,
    ) -> PresentationState {
        let (_, presentation_id) = super::runtime::next_identity();
        let pipeline = PipelineCell::new(PipelineOwner::new());
        pipeline.with_mut(|owner| owner.set_device_pixel_ratio(window.scale_factor() as f32));
        // The prototype is stamped for the PRIMARY presentation; this
        // presentation's accessibility actions must address ITSELF, or the
        // drain would resolve them against a sibling's semantics tree.
        let command_sender = UiCommandSender {
            presentation_id,
            ..self.sender_prototype.clone()
        };
        PresentationState::new(
            presentation_id,
            pipeline,
            window,
            RealmCapabilities {
                global_key_scope: self.global_key_scope.clone(),
                async_driver: self.scheduler.async_driver().clone(),
                local_post_frame_handle: self.local_post_frame.local_handle(),
                interaction_dispatch_handle: self.interaction_lane.dispatch_handle(),
                scheduler: &self.scheduler,
                wake: Arc::clone(&self.wake),
                command_sender,
            },
        )
    }

    /// Install an already-[`assembled`](Self::assemble_presentation)
    /// presentation into this realm's forest — the production entry point
    /// (this slice lifted `PresentationForest::install`'s former
    /// `len()<=1` ratchet).
    ///
    /// The caller is responsible for minting this presentation's
    /// `WindowRegistry` mapping BEFORE calling this (typically between
    /// `assemble_presentation` and this call) — `UiRealm` has no access to
    /// that registry (ADR-0037 §2), exactly the same split
    /// [`Self::close_presentation_entered`]'s own doc states for teardown.
    /// `runner.rs::install_presentation_alongside` is that caller in
    /// production; `Self::install_second_presentation_for_test`
    /// (`cfg(test)`-only, no stable link target in a non-test doc build) is
    /// the test-only counterpart for `UiRealm`-only tests that never touch
    /// `AppRuntime`/`WindowRegistry` at all.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "reachable only through runner.rs::install_presentation_alongside, itself \
                      desktop-only"
        )
    )]
    pub(crate) fn install_presentation(
        &mut self,
        presentation: PresentationState,
    ) -> PresentationId {
        let presentation_id = presentation.id();
        self.presentations.install(presentation);
        presentation_id
    }

    /// [`Self::install_presentation`], with a throwaway test window — for
    /// `UiRealm`-only tests (this module's own `mod tests`, plus
    /// `runner.rs`'s dispatch-seam tests) that exercise a genuine
    /// multi-presentation realm WITHOUT ever touching `AppRuntime`'s
    /// `WindowRegistry` — that layer is a different module's own test
    /// concern (`runner.rs`'s `install_presentation_alongside`, exercised by
    /// the three tests that need a REAL window mapping for the second
    /// presentation). `pub(crate)` so `runner.rs`'s own tests that only need
    /// forest membership, never registry routing, can still use it.
    #[cfg(test)]
    pub(crate) fn install_second_presentation_for_test(&mut self) -> PresentationId {
        let window = super::presentation::test_platform_window(None);
        let presentation = self.assemble_presentation(window);
        self.install_presentation(presentation)
    }

    /// The presentation keyboard input currently routes to
    /// ([`FocusCoordinator`]) — for the isolation suite proving
    /// `WindowFocus` events move it.
    #[cfg(test)]
    pub(crate) fn active_presentation_for_test(&self) -> PresentationId {
        self.focus_coordinator.active()
    }

    /// Whether `id`'s own [`FrameClock`](flui_scheduler::FrameClock) is
    /// currently marked hidden — for
    /// the isolation suite (`runner.rs`) proving `WindowVisibility` events
    /// gate exactly the presentation they were addressed to, never a
    /// sibling's. `None` if `id` is not resident (already closed, or a
    /// forged/mixed address).
    #[cfg(test)]
    pub(crate) fn presentation_hidden_for_test(&self, id: PresentationId) -> Option<bool> {
        self.presentations.get(id).map(|p| p.clock().is_hidden())
    }

    /// The primary presentation's own `FrameClock::produced_count` — for
    /// tests in a sibling module (`runner.rs`) that need to observe the
    /// clock's produce count without reaching into the private
    /// `presentations` field directly.
    #[cfg(test)]
    pub(crate) fn primary_produced_count_for_test(&self) -> u64 {
        self.presentations.primary().clock().produced_count()
    }

    /// Whether `id` is this realm's currently ACTIVE presentation
    /// ([`FocusCoordinator`]). Production reader:
    /// `runner.rs`'s `PlatformToUi::WindowFocus` handling uses this to tell
    /// an authoritative focus-loss (the active presentation's own window
    /// losing OS focus) apart from a stale one (a DIFFERENT, non-active
    /// presentation of this same realm reporting focus loss, a normal
    /// consequence of focus having already moved elsewhere within this
    /// realm) — see that call site's own doc for why the distinction
    /// matters for the realm-wide lifecycle aggregate.
    #[must_use]
    pub(crate) fn is_active_presentation(&self, id: PresentationId) -> bool {
        self.focus_coordinator.active() == id
    }

    /// This realm's own scheduler — the strong root. `runner.rs`'s
    /// realm-scoped lifecycle sites (`emit_lifecycle_transition`, the
    /// per-backend frame pumps) read through here instead of a process-global
    /// singleton; see [`Self::scheduler`]'s field doc for the ownership
    /// story.
    #[must_use]
    pub(crate) fn scheduler(&self) -> &flui_scheduler::UpdateScheduler {
        &self.scheduler
    }

    /// This realm's owner-local post-frame lane. `runner.rs`'s per-backend
    /// frame pumps pass this to `UpdateScheduler::drive_frame_with_lane` so
    /// the frame drive drains it in the same total order as the shared
    /// queue — drain-by-parameter, the same reason [`Self::scheduler`]
    /// exists rather than a process-global lookup.
    #[must_use]
    pub(crate) fn local_post_frame_lane(&self) -> &flui_scheduler::LocalPostFrameLane {
        &self.local_post_frame
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
    /// A composite `GlobalKey` registry spanning every presentation this
    /// realm hosts (ADR-0043 §1) is active for the entire dynamic
    /// extent of `f`, including lifecycle/build callbacks — whole-frame,
    /// exactly as a single presentation's own registry always was: command
    /// drains, deferred arena resolutions, and hot-reload reassemble all
    /// resolve keys realm-wide regardless of which presentation's own
    /// segment is running. Nested entry is stack-shaped and panic unwinding
    /// restores the previously active realm.
    ///
    /// Does NOT activate the local post-frame lane: `LocalPostFrameHandle`
    /// addresses its lane directly (a `Weak` pointer minted once per
    /// presentation), so `schedule_local` needs no ambient "active lane"
    /// scope to succeed. The frame drive drains that lane by passing it
    /// explicitly to `UpdateScheduler::drive_frame_with_lane`/
    /// `end_frame_with_lane`, not by anything entered here.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        self.interaction_lane.enter(|| {
            let composite = GlobalKeyRegistryComposite::assemble(
                self.presentations.iter().map(PresentationState::widgets),
            );
            composite.enter(|| f(self))
        })
    }

    /// [`Self::enter`], but the composite excludes presentation `closing`.
    ///
    /// Used ONLY by [`Self::close_presentation_entered`]'s step 2–3 phase.
    /// `PresentationState::close`'s final step (`detach_root_widget`) holds
    /// `closing`'s own `WidgetsBindingInner` write lock for the entire
    /// recursive subtree removal — the exclusive access that walk needs to
    /// unmount every descendant. `GlobalKeyRegistryComposite`'s lookup tries
    /// each member in insertion order regardless of which key is being
    /// resolved (`build_composite`'s `for` loop runs until one member's
    /// `lookup_element` returns `Some`, trying every earlier member first),
    /// so if `closing` were composed in, a dispose hook's `GlobalKey::
    /// current_element()` call — for ANY key, even one belonging to a
    /// different presentation entirely — would have `closing`'s own lookup
    /// closure try to re-acquire a read lock on the exact `RwLock` this call
    /// already holds for writing, on the same thread: an unconditional
    /// self-deadlock (`parking_lot::RwLock` is not reentrant), not a race.
    /// Caught by this issue's own red-exploit test
    /// (`dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter`
    /// in `runner.rs`, which hung before this exclusion existed).
    ///
    /// Excluding `closing` costs nothing real: every OTHER presentation
    /// still resolves normally, and querying a presentation's OWN GlobalKey
    /// while that exact presentation's registry is mid-teardown has no
    /// principled answer anyway — the key is about to be unregistered by
    /// the same call regardless of what a lookup returns for it right now.
    fn enter_for_close<R>(&self, closing: PresentationId, f: impl FnOnce(&Self) -> R) -> R {
        self.interaction_lane.enter(|| {
            let composite = GlobalKeyRegistryComposite::assemble(
                self.presentations
                    .iter()
                    .filter(|presentation| presentation.id() != closing)
                    .map(PresentationState::widgets),
            );
            composite.enter(|| f(self))
        })
    }

    /// Owner-local widgets binding. Crate-private so callers cannot bypass the
    /// guarded realm entry boundary.
    pub(crate) fn widgets(&self) -> &WidgetsBinding {
        self.presentations.primary().widgets()
    }

    /// Gesture state for the realm's current single presentation.
    ///
    /// Crate-private so platform input can only reach it through the entered
    /// realm dispatch path rather than exposing a second public owner seam.
    /// The live root media-query source (see the field doc).
    pub(crate) fn media_query(&self) -> &crate::app::media_query_root::MediaQuerySource {
        &self.media_query
    }

    pub(crate) fn gestures(&self) -> &GestureBinding {
        self.presentations.primary().gestures()
    }

    /// Cancel in-flight pointer sequences on the ADDRESSED presentation's
    /// own gesture binding.
    ///
    /// Pointer input routes per presentation
    /// ([`Self::handle_input_addressed`] dispatches to
    /// `presentation.gestures()`), so a focus-loss cancellation must reach
    /// the same binding that presentation's Downs landed in — the
    /// realm-level [`Self::gestures`] wrapper is primary-only, and under a
    /// shared-realm window policy it would cancel a sibling presentation's
    /// sequences while leaving the defocused window's own drag stranded. A
    /// presentation this realm no longer hosts is a traced no-op, matching
    /// the addressed-input path's posture for the same race.
    pub(crate) fn cancel_pointer_sequences_for(&self, presentation_id: PresentationId) {
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a pointer-sequence cancellation addressed to a presentation this \
                 realm no longer hosts"
            );
            return;
        };
        presentation.gestures().cancel_active_pointers();
    }

    /// Focus state for the realm's current primary presentation. Since
    /// issue #555's addressed-routing slice, [`Self::handle_input_addressed`] reads the ADDRESSED
    /// presentation's own `focus_manager()` directly instead of through this
    /// primary-only wrapper; kept for the tests that still exercise a
    /// single-presentation realm's focus tree without addressing.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production input dispatch reads the addressed presentation's own \
                      focus_manager() directly (handle_input_addressed); this primary-only \
                      wrapper is exercised only by tests"
        )
    )]
    pub(crate) fn focus_manager(&self) -> Rc<FocusManager> {
        self.presentations.primary().focus_manager()
    }

    /// Weak text-input capability for this exact presentation.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn text_input_handle(&self) -> flui_interaction::TextInputHandle {
        self.presentations.primary().text_input_handle()
    }

    /// Keep presentation-owned resources aligned with the synthesized
    /// application lifecycle delivered by the platform runner.
    pub(crate) fn handle_presentation_lifecycle(&self, state: AppLifecycleState) {
        match state {
            AppLifecycleState::Resumed | AppLifecycleState::Inactive => {
                self.presentations.primary().resume();
            }
            AppLifecycleState::Hidden | AppLifecycleState::Paused => {
                self.presentations.primary().suspend();
            }
            AppLifecycleState::Detached => {
                self.presentations.primary().close();
            }
        }
    }

    /// Reassemble EVERY presentation this realm hosts, in mount order —
    /// under the whole-frame composite `enter()` already activates, so a
    /// key resolved mid-fan-out still finds any presentation's tree, not
    /// just the one currently reassembling.
    ///
    /// Deliberately does not short-circuit on the first presentation that
    /// reports a change: every presentation must reassemble regardless of
    /// what its predecessors reported, or a hot reload would silently skip
    /// later-mounted presentations whenever an earlier one happened to
    /// report no change. Returns whether ANY presentation requires a
    /// redraw.
    #[cfg(feature = "hot-reload")]
    #[must_use]
    pub(crate) fn apply_hot_reload(&self, tier: flui_hot_reload::HotReloadTier) -> bool {
        let mut needs_redraw = false;
        for presentation in self.presentations.iter() {
            if presentation.apply_hot_reload(tier) {
                needs_redraw = true;
            }
        }
        needs_redraw
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
        self.presentations.primary().renderer()
    }

    /// Re-dirty this realm's root so the next frame actually produces
    /// content instead of finding nothing to do — called directly from
    /// `runner.rs`'s `emit_lifecycle_transition` on the frames-disabled->
    /// enabled edge (that call site already has this realm in scope; see
    /// its own doc for why the redirty lives there and not in an `UpdateScheduler`
    /// lifecycle listener). FLUI has no retained-scene layer to fall back
    /// on, so a `Hidden`/`Paused` -> `Resumed`/`Inactive` transition needs
    /// the same explicit re-dirty `allow_first_frame` needs after a
    /// deferral lifts.
    pub(crate) fn redirty_root_for_frames_reenable(&self) {
        crate::bindings::redirty_pipeline_root(
            self.presentations
                .primary()
                .renderer()
                .root_pipeline_owner(),
        );
    }

    /// A clone of the PRIMARY presentation's own controller registry for
    /// implicit animations.
    ///
    /// `Vsync` is `Arc`-backed; cloning is two atomic increments — cheap. App
    /// code constructs a `VsyncScope` from this clone so every
    /// implicitly-animated widget below registers its controller here. The
    /// production frame driver ([`Self::draw_frame_entered`]) ticks EVERY
    /// presentation's own registry once per frame (before that
    /// presentation's build phase) and keeps the frame loop alive until the
    /// last running controller, on any presentation, completes.
    ///
    /// Moved from this realm's own `vsync_slot` to per-presentation storage
    /// (each surface paces its own animations independently)
    /// — this forwarder to `self.presentations.primary()` keeps every
    /// existing single-presentation caller and test unchanged; a genuine
    /// multi-presentation caller reads
    /// `self.presentations.get(id).vsync()` directly instead.
    pub(crate) fn vsync(&self) -> Vsync {
        self.presentations.primary().vsync()
    }

    /// Replace the PRIMARY presentation's registry with a pre-existing
    /// shared `Vsync`.
    ///
    /// Use when a `VsyncScope` was built before this presentation's registry
    /// was acquired (the scope needs the handle to pass to descendants, and
    /// this presentation must drive that same registry). Call before any
    /// controller is registered so no registration is stranded on the
    /// discarded registry. Never mount a second `VsyncScope` at the root
    /// with a *different* registry — this presentation ticks its own while
    /// descendants register into the other, leaving them frozen.
    #[expect(
        dead_code,
        reason = "no production caller yet, and no test exercises the \
                  custom-registry substitution path -- an app-author escape \
                  hatch that has no wiring point since UiRealm is pub(crate)-only"
    )]
    pub(crate) fn set_vsync(&self, vsync: Vsync) {
        self.presentations.primary().set_vsync(vsync);
    }

    /// Whether at least one registered implicit-animation controller is
    /// currently running, on ANY presentation this realm hosts.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "draw_frame_entered reads has_running() on each presentation's own \
                      Vsync clone directly, not through this wrapper; kept for \
                      tests and future external callers"
        )
    )]
    pub(crate) fn has_vsync_running(&self) -> bool {
        self.presentations
            .iter()
            .any(|presentation| presentation.vsync().has_running())
    }

    /// Current virtual seconds for the Vsync tick.
    ///
    /// Production: `self.start.elapsed().as_secs_f64()` — one monotonic
    /// origin shared across the Vsync tick and all frame accounting, so
    /// there is no clock drift between the two. Tests: the injected
    /// override (`set_now_secs_for_test`, `#[cfg(test)]`-only so it cannot be
    /// linked from a normal doc build) takes precedence, allowing
    /// deterministic animation stepping with no wall-clock reads.
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

    /// Inject a deterministic virtual `now_secs` for test frames: overrides
    /// the wall-clock read `now_secs` otherwise takes, so a test can drive
    /// the Vsync tick and frame accounting from values it controls instead
    /// of racing real elapsed time. `0.0` is stored as a sentinel-adjusted
    /// nonzero bit pattern so `now_secs`'s `bits != 0` check (its "no
    /// override installed" test) cannot mistake an explicit zero override
    /// for an absent one.
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
                  wall-clock mid-test; kept as the counterpart to \
                  set_now_secs_for_test for whichever future test needs to \
                  revert mid-run instead of dropping the whole realm"
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
    ///
    /// Every current caller (`attach_root_widget*`, and every
    /// [`Self::handle_input_addressed`] arm not yet ported to it) is still a
    /// primary-only operation, so this marks `self.presentations.primary()`'s
    /// own pump wake bit (ADR-0043 §3) specifically — see
    /// [`Self::request_redraw_for`] for the addressed counterpart
    /// [`Self::handle_input_addressed`] actually uses.
    pub(crate) fn request_redraw(&self) {
        self.request_redraw_for(self.presentations.primary());
    }

    /// [`Self::request_redraw`], addressed to exactly `presentation` rather
    /// than always the primary (issue #555's addressed-routing slice): still
    /// sets the realm-wide coalesced flag (needed either way — it is what
    /// wakes an idle event loop at all) but marks the ADDRESSED
    /// presentation's own pump wake bit, never a sibling's, so an
    /// idle-and-quiet sibling presentation is not woken by a redraw request
    /// that was never its own (`redraw_request_from_a_does_not_wake_bs_window`
    /// covers the platform-window half of this same requirement via each
    /// presentation's own `on_need_visual_update` wiring, `presentation.rs`).
    fn request_redraw_for(&self, presentation: &PresentationState) {
        self.needs_redraw.store(true, Ordering::Relaxed);
        presentation.mark_redraw_pending();
        // The flags alone are inert while the event loop is parked: nothing
        // converts them into a frame until SOMETHING pokes the platform.
        // An input-driven redraw request (a drag's scroll write) was
        // exactly that case — the loop went straight back to waiting after
        // the pointer event, the flags sat unread, and a live drag froze
        // the screen while the position advanced invisibly. The wake sets
        // `needs_redraw` (idempotent with the store above) AND requests a
        // platform redraw, which winit coalesces per frame.
        (self.wake)();
    }

    /// [`Self::request_redraw_for`]'s pipeline-dirtying counterpart,
    /// addressed to exactly `presentation` rather than always the primary.
    /// Forces `presentation` to repaint on its next pump segment even
    /// though nothing in its widget tree itself changed — for a
    /// renderer-side reason the build/layout/paint pipeline has no way to
    /// observe on its own. Two production callers today, each resolving
    /// `presentation` from what only THAT caller knows:
    ///
    /// - [`Self::mark_primary_needs_full_repaint`] (below), for the
    ///   pre-frame GPU device-recovery path (`runner.rs`'s
    ///   `render_frame_with_device_recovery`), where the recovered device's
    ///   backing store was invalidated by the loss — `Renderer::recover`
    ///   already primes a full repaint (`damage_tracker.mark_full_repaint`)
    ///   for whatever scene reaches it next, but nothing reaches it at all
    ///   unless something forces one. That caller is still single-renderer/
    ///   single-presentation shaped (see its own TODO(#559)), so `primary()`
    ///   is the only presentation it has to address.
    /// - [`Self::render_frame_entered`]'s own `retry_needed` arm (issue
    ///   #637), for a submit that failed with `SurfaceLost`, `DeviceLost`,
    ///   or `SurfaceValidation` mid-frame: the frame that just failed had
    ///   already consumed the pipeline's dirty state producing the scene it
    ///   tried to send, so the eventual retry needs its own fresh reason to
    ///   redo the work, same as the pre-frame case above — addressed to
    ///   that call's own `producer`, NEVER `primary()` unconditionally
    ///   (that method's own doc, at the `producer` binding, explains why
    ///   the two can differ).
    ///
    /// [`Self::request_redraw_for`] ALONE is necessary but not sufficient
    /// for either: it opens `draw_frame_entered`'s per-presentation segment
    /// gate (`PresentationState::mark_redraw_pending`, read by
    /// `take_redraw_pending`), which is REQUIRED for the segment to run at
    /// all — but an otherwise-unchanged tree, even with that gate open,
    /// still produces `FramePaintOutcome::Idle` and never reaches
    /// `render_scene`, because `PipelineOwner` tracks its own dirty state
    /// independently of the clock's demand mechanism (confirmed by a
    /// probe, not assumed: `should_run_segment()` correctly read `true`
    /// with `request_redraw_for` alone, and the pipeline still produced
    /// `Idle`). Marking the root render object dirty is what gives the
    /// pipeline actual work to redo.
    ///
    /// [`PipelineOwner::mark_needs_paint`], deliberately NOT `mark_needs_
    /// layout`: layout has not changed across either caller's failure —
    /// a device loss invalidates only the renderer's backing store, and a
    /// submit failure (`SurfaceLost`/`DeviceLost`/`SurfaceValidation`) is
    /// likewise a renderer/surface-side rejection of an already-correct
    /// scene, never a report that the widget tree's geometry is wrong — so
    /// only the PAINTED OUTPUT needs to be resubmitted in either case.
    /// `mark_needs_paint` is sufficient because paint is already a
    /// full-tree descent every frame in this codebase (see `docs/` notes on
    /// that shape) — marking just the root non-Idle is enough for the
    /// descent to cover everything beneath it; forcing a full RELAYOUT of
    /// the whole tree for a purely renderer-side rejection would be
    /// strictly heavier than either fix needs. Verified, not assumed:
    /// swapping this one call site is what the SurfaceLost retry test's
    /// by-hand stand-in (`ui_realm.rs`'s
    /// `surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame`)
    /// also uses `mark_needs_layout` for, but that stand-in predates this
    /// method and was never revisited against the lighter mark — the
    /// device-recovery caller was the first real (non-test) one, and it
    /// takes the lighter one.
    ///
    /// Deliberately NOT extended to [`Self::render_frame_entered`]'s
    /// `FramePaintOutcome::Errored` outcome (a build/layout/paint failure
    /// inside the pipeline itself, distinct from the three submit failures
    /// above) — see that method's own comment at its `retry_needs_repaint`
    /// binding for why the reasoning above does not transfer to that case.
    ///
    /// No longer wasm-dead-code (this method used to carry a
    /// `#[cfg_attr(target_arch = "wasm32", expect(dead_code, ...))]` when it
    /// had only the pre-frame device-recovery caller): `render_frame_entered`
    /// runs on every target, `wasm32` included (`runner.rs`'s
    /// `bootstrap_web` calls it directly), so its own `retry_needed` arm
    /// reaches this method there too.
    fn mark_needs_full_repaint_for(&self, presentation: &PresentationState) {
        self.request_redraw_for(presentation);
        presentation
            .renderer()
            .root_pipeline_owner()
            .with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_paint(root_id);
                }
            });
    }

    /// [`Self::mark_needs_full_repaint_for`], addressed to
    /// `self.presentations.primary()` — the shape its one caller
    /// (`runner.rs`'s `render_frame_with_device_recovery`) needs: that
    /// function runs BEFORE `render_frame_entered`, driven purely by
    /// `renderer.is_device_lost()`, with no per-pump `producer` of its own
    /// to resolve which presentation actually owns the lost device.
    // TODO(#559): marks `self.presentations.primary()` unconditionally --
    // a realm with a non-primary presentation hosting the device that just
    // recovered would mark the WRONG presentation's tree. Latent today
    // only because a secondary window carries no widget content yet
    // (`open_secondary_window`'s own doc); #559's addressing slice is
    // where this needs to become presentation-addressed, matching how
    // `render_frame_with_device_recovery` itself is still single-renderer/
    // single-presentation shaped.
    //
    // wasm-dead-code (unlike `mark_needs_full_repaint_for` above, which
    // `render_frame_entered`'s own `retry_needed` arm also calls on every
    // target): this wrapper's only caller, `render_frame_with_device_
    // recovery`, is itself `#[cfg(all(not(target_os = "ios"),
    // not(target_arch = "wasm32")))]` (desktop/Android only -- web's own
    // recovery stays un-unified and pokes `wake_handle()` directly
    // instead), so on `wasm32` this wrapper genuinely has no caller at all.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the only caller, render_frame_with_device_recovery, is itself \
                      desktop/Android-only -- see this method's own comment"
        )
    )]
    pub(crate) fn mark_primary_needs_full_repaint(&self) {
        self.mark_needs_full_repaint_for(self.presentations.primary());
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
    /// `needs_redraw` AND pokes the installed window.
    ///
    /// Deadlock-safety: this only ever touches the `Send + Sync` state
    /// captured in [`Self::wake`] (an `Arc<AtomicBool>` plus an `Arc<Mutex<Option<Arc<dyn
    /// PlatformWindow>>>>` on `AppRuntime` — see `FrameWakeHandle` in
    /// `runtime.rs`), never this realm's own `widgets`/`renderer`/gesture
    /// locks, so it is safe to call from inside a build/layout/paint
    /// callback without risking a lock-ordering cycle against those.
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
    // `AppBinding`; the deferral gate itself is re-homed from
    // `RenderingFlutterBinding`'s own counter onto the primary presentation's
    // `FrameClock` — see `render_frame_entered`'s own doc for the submit-gate
    // check that is the actual production consumer now)
    // ========================================================================

    /// Defer this realm's primary presentation from sending a produced
    /// frame to the engine until a matching [`Self::allow_first_frame`].
    /// Does NOT stop that presentation's segment from running — see
    /// [`FrameClock::defer`](flui_scheduler::FrameClock::defer)'s own doc.
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
        self.presentations.primary().clock().defer();
    }

    /// Undo one [`Self::defer_first_frame`]. See
    /// [`FrameClock::lift`](flui_scheduler::FrameClock::lift).
    ///
    /// A deferred pump's segment already ran and produced a real (withheld)
    /// scene — see `FrameClock`'s module doc's `.flutter/` citation — so
    /// `poll` already cleared that demand; there is nothing retained for a
    /// bare `lift()` alone to produce from. FLUI has no retained-scene
    /// layer to fall back on the way Flutter's `scheduleWarmUpFrame`
    /// re-composites the retained layer tree (see
    /// `RenderingFlutterBinding::redirty_root_for_represent`'s own doc for
    /// the identical problem that method exists to solve), so this method
    /// re-dirties the root itself whenever it actually lifts an active
    /// deferral — the withheld work does not sit blank until some UNRELATED
    /// input dirties the tree.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- see defer_first_frame's doc"
        )
    )]
    pub(crate) fn allow_first_frame(&self) {
        let clock = self.presentations.primary().clock();
        let was_deferred = clock.is_deferred();
        clock.lift();
        if was_deferred {
            crate::bindings::redirty_pipeline_root(
                self.presentations
                    .primary()
                    .renderer()
                    .root_pipeline_owner(),
            );
        }
    }

    /// Whether the primary presentation should hand a produced frame to the
    /// engine right now — exactly `!is_deferred()`. See
    /// [`FrameClock::is_deferred`](flui_scheduler::FrameClock::is_deferred).
    ///
    /// `render_frame_entered` consults the SAME query directly at its own
    /// submit point (see that method's own doc); this forwarder exists for
    /// tests exercising the deferral contract end to end through `UiRealm`'s
    /// own API rather than reaching into the presentation's clock directly.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "render_frame_entered reads presentation.clock().is_deferred() directly, \
                      not through this wrapper; kept for tests and future external callers"
        )
    )]
    pub(crate) fn send_frames_to_engine(&self) -> bool {
        !self.presentations.primary().clock().is_deferred()
    }

    /// Records a compositor/platform-delivered frame-request signal — a
    /// native `RedrawRequested` the platform actually paces (Wayland's
    /// per-surface frame callbacks; see `docs/adr/ADR-0044-driver-loop-hybrid.md`'s
    /// per-platform table for which backends genuinely deliver one) —
    /// against the primary presentation's
    /// [`FrameClock`](flui_scheduler::FrameClock). See
    /// [`FrameClock::record_compositor_tick`](flui_scheduler::FrameClock::record_compositor_tick)
    /// for what this feeds — pacing-feedback bookkeeping ONLY; this marks
    /// no demand of its own (see that method's own doc for why that was
    /// tried and reverted). Single-presentation-addressed for the same
    /// reason [`Self::vsync`] is — production topology's current ratchet
    /// is exactly one presentation per realm.
    #[cfg_attr(
        any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
        expect(
            dead_code,
            reason = "this method's only caller, bootstrap_desktop's on_request_frame \
                      closure (runner.rs), is cfg'd to exactly the desktop targets this \
                      predicate excludes -- android has no build here (no NDK toolchain: \
                      `cargo check -p flui-app --target aarch64-linux-android` fails at the \
                      cc-detection step before reaching this file), ios likewise (no Xcode \
                      SDK: fails in the psm build script before reaching this file), and \
                      wasm32 (run_web's own RAF-driven frame-pump closure does not call this \
                      yet) DOES build clean here and was verified dead_code-free with this \
                      predicate. Wiring each backend's own compositor-tick feedback is \
                      deferred, stated honestly in docs/adr/ADR-0044-driver-loop-hybrid.md's \
                      per-platform table (none of android/ios/web get compositor pacing in \
                      this slice; all three keep the wake-channel driver)."
        )
    )]
    pub(crate) fn record_compositor_tick(&self, now: web_time::Instant) {
        self.presentations
            .primary()
            .clock()
            .record_compositor_tick(now);
    }

    /// Total frames rendered successfully by this presentation.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "draw_frame_entered reads self.presentations.primary().frames_rendered() \
                      directly for the frame-number computation, not through \
                      this wrapper; kept for tests and future external callers"
        )
    )]
    pub(crate) fn frames_rendered(&self) -> u64 {
        self.presentations.primary().frames_rendered()
    }

    /// Frames dropped due to surface errors on this presentation. See
    /// `PresentationState::frames_dropped`'s own doc for how this stays
    /// structurally distinct from `FrameClock::produces_deferred` (a
    /// backpressure/hidden deferral is never counted here).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- exercised by \
                      frame_clock_segment_gate's frames-dropped-vs-deferred pin"
        )
    )]
    pub(crate) fn frames_dropped(&self) -> u64 {
        self.presentations.primary().frames_dropped()
    }

    /// Turn this presentation's performance overlay on or off. See the
    /// retired `AppBinding::set_performance_overlay`'s doc.
    pub(crate) fn set_performance_overlay(&self, enabled: bool) {
        self.presentations
            .primary()
            .set_performance_overlay(enabled);
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
        self.presentations
            .primary()
            .perform_haptic_feedback(feedback);
    }

    /// Apply a new device pixel ratio to this realm's render pipeline (the
    /// resize path; construction applies the initial ratio directly).
    pub(crate) fn set_device_pixel_ratio(&self, device_pixel_ratio: f32) {
        self.presentations
            .primary()
            .renderer()
            .root_pipeline_owner()
            .with_mut(|owner| owner.set_device_pixel_ratio(device_pixel_ratio));
    }

    /// Check if there is pending work in ANY presentation this realm hosts:
    /// a pending build, pending gesture motion/deadlines, or a dirty render
    /// node. The runner's wake gate (`needs_redraw() || has_pending_work()`)
    /// reads this every frame. Production topology is exactly one
    /// presentation, so this union is behaviorally identical to reading the
    /// primary's own state directly; it generalizes to the isolation suite's
    /// N>1 forests without needing a second code path.
    pub(crate) fn has_pending_work(&self) -> bool {
        self.presentations.iter().any(|presentation| {
            presentation.has_pending_work()
                || presentation.gestures().has_pending_motion()
                || presentation.gestures().has_pending_deadlines()
        })
    }

    /// The earliest wall-clock instant ANY presentation this realm hosts
    /// needs the platform loop to wake at, if any — the min over every
    /// presentation's own armed gesture-arena
    /// deadline (`GestureBinding::next_deadline`, e.g. a long-press hold or
    /// a double-tap give-up). This is genuinely INDEPENDENT of every
    /// presentation's `FrameClock` state: a gated presentation's pending
    /// input-correctness deadline (per this issue's own rule — arena
    /// deadlines are input correctness, not frame production) still needs
    /// the loop to wake and re-drive a pump so `tick_deadlines` can resolve
    /// it, even though that pump will render nothing for the gated
    /// presentation itself.
    #[must_use]
    pub(crate) fn next_wake(&self) -> Option<web_time::Instant> {
        self.presentations
            .iter()
            .filter_map(|presentation| presentation.gestures().next_deadline())
            .min()
    }

    // ========================================================================
    // Root attach (moved from the retired `AppBinding`)
    // ========================================================================

    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build. Forwards
    /// to [`flui_view::WidgetsBinding::attach_root_widget`] — the single
    /// root-bootstrap path; every runner entry point calls
    /// [`Self::attach_root_widget_with_size`] (this method's sized sibling)
    /// instead, not a separate hand-rolled wiring.
    ///
    /// # Implicit-animation and gesture-arena auto-wrap
    ///
    /// The realm automatically wraps `view` in a [`VsyncScope`] backed by
    /// [`Self::vsync`] and a [`GestureArenaScope`] backed by this
    /// presentation's gesture arena before handing it to the element tree
    /// (see [`Self::attach_root_widget_entered`]'s body) — every implicitly-
    /// animated widget and every `GestureDetector` below the root joins this
    /// realm's own registry/arena with no app-author boilerplate. Never
    /// mount a second `VsyncScope`/`GestureArenaScope` at the root with a
    /// *different* registry/arena — this realm ticks/arbitrates its own
    /// while descendants register into the other, leaving them frozen or
    /// competing in an arena nothing closes.
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
        // The root MediaQuery publishes the realm's live platform data
        // (size, device pixel ratio, brightness) to the whole user subtree;
        // the realm's resize/appearance arms write the shared source and the
        // wrapper republishes.
        let with_media_query = crate::app::media_query_root::MediaQueryRoot::new(
            std::rc::Rc::clone(&self.media_query),
            flui_view::view::ViewExt::boxed(view.clone()),
        );
        let focused = FocusRoot::new(with_media_query);
        let animated = VsyncScope::new(self.vsync(), focused);
        let wrapped = GestureArenaScope::new(self.gestures().arena().clone(), animated);
        self.presentations
            .primary()
            .widgets()
            .attach_root_widget(&wrapped)?;
        self.request_redraw();
        tracing::debug!("Root widget attached");
        Ok(())
    }

    /// [`Self::attach_root_widget_entered`], but targets an arbitrary
    /// RESIDENT presentation instead of always `primary()` — for tests that
    /// need a SECOND presentation to carry real, paintable content.
    /// `draw_frame_for_presentation` resolves to `FramePaintOutcome::Idle`
    /// for a presentation with nothing attached (no layer tree to paint),
    /// so a multi-presentation test proving telemetry attribution follows
    /// the presentation that actually produced (not `primary()`
    /// unconditionally) needs the non-primary presentation to genuinely
    /// paint, not merely flip a dirty bit.
    ///
    /// # Panics
    ///
    /// Panics if `id` does not name a presentation this realm currently
    /// hosts.
    #[cfg(test)]
    pub(crate) fn attach_root_widget_to_for_test<V>(
        &self,
        id: PresentationId,
        view: &V,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.enter(|realm| {
            let presentation = realm.presentations.get(id).expect(
                "BUG: attach_root_widget_to_for_test given a presentation id this realm does \
                 not host",
            );
            let focused = FocusRoot::new(view.clone());
            let animated = VsyncScope::new(presentation.vsync(), focused);
            let wrapped = GestureArenaScope::new(presentation.gestures().arena().clone(), animated);
            presentation.widgets().attach_root_widget(&wrapped)?;
            realm.request_redraw_for(presentation);
            tracing::debug!(?id, "Root widget attached (non-primary, test-only)");
            Ok(())
        })
    }

    /// Attach a root widget sizing the root view to an explicit logical
    /// `width` × `height` — the platform window's surface size.
    ///
    /// Identical to [`Self::attach_root_widget`] except the root
    /// `RenderView` is born at the real window size instead of the
    /// framework's fallback default. This is the runner's bootstrap entry
    /// point. See [`Self::attach_root_widget`] for the auto-wrap invariants.
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
        // Same root MediaQuery as the production attach path — the sized
        // variant must not present a different ambient environment.
        let with_media_query = crate::app::media_query_root::MediaQueryRoot::new(
            std::rc::Rc::clone(&self.media_query),
            flui_view::view::ViewExt::boxed(view.clone()),
        );
        let focused = FocusRoot::new(with_media_query);
        let animated = VsyncScope::new(self.vsync(), focused);
        let wrapped = GestureArenaScope::new(self.gestures().arena().clone(), animated);
        self.presentations
            .primary()
            .widgets()
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
    pub(crate) fn draw_frame(&self, constraints: BoxConstraints) -> Option<Scene> {
        match self.enter(|realm| realm.draw_frame_entered(constraints)).1 {
            FramePaintOutcome::Painted(scene) => Some(scene),
            FramePaintOutcome::Idle | FramePaintOutcome::Errored => None,
        }
    }

    /// The complete build+layout+paint pipeline for one frame.
    ///
    /// **Frame-phase parity (critical):** the realm-level pre-phase (vsync
    /// tick, then gesture-deadline tick) MUST precede every presentation's
    /// own segment, for the identical ordering argument the retired
    /// `AppBinding::draw_frame_entered` depended on: both tick calls can
    /// dirty render/build state a segment's dirty sample needs to observe.
    /// Reordering the pre-phase is out of scope for the change that
    /// introduced the per-presentation loop below; the frame-loop tests are
    /// the parity oracle.
    ///
    /// **Per-presentation segment loop (ADR-0043 §3; issue #556):**
    /// presentations are processed once each, in mount order. Each
    /// presentation's own [`FrameClock`](flui_scheduler::FrameClock) —
    /// replacing the old `take_redraw_pending() || has_pending_work()`
    /// predicate read directly — decides whether its segment runs this pump.
    /// The clock never inspects the tree itself (ownership rule: it never
    /// knows phases, callbacks, or element trees), so the exact union the
    /// old predicate read is marked as `Dirty` demand and the clock's
    /// `poll` makes the call: `PollDecision::should_run_segment` is
    /// EXACTLY that same `woken || has_pending_work()` union (hidden/
    /// backpressure aside — unwired in production today), independent of
    /// first-frame deferral, which never gates the segment (see
    /// `FrameClock`'s own module doc's `.flutter/` citation — deferral
    /// withholds only the submit). Production topology is exactly one
    /// presentation (`PresentationForest`'s ratchet), so this reduces to
    /// exactly today's single unconditional segment; see
    /// [`Self::draw_frame_for_presentation`]'s doc for the proof that the
    /// gate cannot skip a segment the old, ungated code would have run.
    /// Returns the LAST presentation whose segment actually ran this pump,
    /// paired with its outcome (matches today's single-presentation return
    /// value exactly when only one presentation is mounted; with more than
    /// one, `render_frame_entered`, this method's own caller, submits and
    /// records telemetry against the RETURNED id, never an assumed
    /// `primary()` — see that method's own doc for the misattribution bug
    /// this addressing closes). A caller reading a SPECIFIC non-last
    /// presentation's own render state still does so directly rather than
    /// through this aggregate.
    fn draw_frame_entered(
        &self,
        constraints: BoxConstraints,
    ) -> (PresentationId, FramePaintOutcome) {
        // Vsync tick + gesture-deadline tick — MUST precede every
        // presentation's build phase. See the retired `AppBinding::
        // draw_frame_entered`'s doc for the full disjoint-controller-set
        // argument this ordering depends on. Each presentation ticks its OWN
        // registry now (the registry moved off this realm's former
        // `vsync_slot`, one per surface), off the SAME realm-relative `now`
        // every presentation observes, so there is no clock drift between
        // siblings sharing this one pump.
        let now = self.now_secs();
        for presentation in self.presentations.iter() {
            let vsync = presentation.vsync();
            // Sampled BEFORE `tick_all`, not after: the tick that completes
            // a controller still delivers that controller's final value and
            // status change (`.flutter/packages/flutter/lib/src/scheduler/
            // ticker.dart:272-285`'s `_tick` calls `_onTick` unconditionally,
            // THEN checks whether to schedule another one), so a controller
            // that WAS running when this tick started must still mark
            // demand for THIS exact pump even though it may have just
            // settled -- the identical before/after-tick question
            // `flui-testing`'s own `pump_presentation` had to fix (see its
            // doc), just never wired at this layer before this slice.
            //
            // The "does the NEXT pump need to be scheduled" question
            // (Flutter's own `shouldScheduleTick`, sampled AFTER the tick)
            // is answered separately, in `render_frame_entered`, AFTER the
            // segment this demand mark feeds has actually rendered and
            // `mark_rendered()`/the retry check has run -- NOT here. See
            // that method's own comment for why: raising the continuation
            // wake here, before this same pump's `mark_rendered()` clears
            // `needs_redraw`, silently clobbers it within the SAME
            // callback -- a real, probe-confirmed stall (not hypothetical)
            // for a controller with no other tree-visible effect, since
            // nothing else keeps `needs_redraw`/`has_pending_work()` true
            // once that clobber happens.
            let was_running = vsync.has_running();
            vsync.tick_all(now);

            if was_running {
                // A running controller with no OTHER tree-visible effect
                // now genuinely marks demand -- and therefore flushes --
                // every tick, matching Flutter's own `Ticker`-driven
                // `scheduleFrame` (the running ticker alone is sufficient),
                // rather than silently relying on some unrelated dirty
                // state to also be present. This closes a gap the previous
                // slice on this issue explicitly named and deferred.
                presentation.clock().mark_demand(DemandKind::Animation);
            }

            presentation.gestures().tick_deadlines();
            if presentation.gestures().has_pending_deadlines() {
                self.wake_frame();
            }
        }

        // The async-driver step lives in `UpdateScheduler::handle_begin_frame`'s
        // mid-frame slot, not here: this pipeline runs in
        // `PersistentCallbacks`, where `drive_async_tasks` debug-asserts it
        // must never poll (polling here could re-enter a frame-phase-only
        // capability from inside a woken future). One mid-frame poll per
        // frame, on the right `UpdateScheduler` instance, is enforced by the
        // scheduler itself.

        let mut last_outcome = FramePaintOutcome::Idle;
        let mut producer = self.presentations.primary().id();
        for presentation in self.presentations.iter() {
            // Platform-driven semantics enablement lands here, BEFORE dirty
            // sampling: the activation listener could only flip the host
            // flag and wake the loop (it runs on the adapter's thread), and
            // enabling seeds the root as needing semantics — exactly the
            // pending work the sampling below should then observe.
            presentation.reconcile_semantics_enablement();
            // Segment start: clear this presentation's wake bit BEFORE
            // sampling dirty, so a mark arriving WHILE this segment runs
            // sets the bit again and lands next pump instead of being lost.
            // The union is fed into the clock as `Dirty` demand rather than
            // gating directly — the clock decides, the realm only reports.
            let woken = presentation.take_redraw_pending();
            if woken || presentation.has_pending_work() {
                presentation.clock().mark_demand(DemandKind::Dirty);
            }

            let segment_start = presentation.clock().now();
            let decision = presentation.clock().poll(segment_start);
            if !decision.should_run_segment() {
                // Nothing to flush and nobody asked, or a gate (hidden /
                // backpressure) is closed: skip this presentation's segment
                // entirely. Bound: each presentation builds and flushes at
                // most once per pump; this `continue` is what makes that
                // true rather than merely documented. First-frame deferral
                // is NOT one of these reasons — see `FrameClock`'s module
                // doc: deferral withholds only the submit, never the
                // segment (`.flutter/packages/flutter/lib/src/rendering/
                // binding.dart:582-599`), so a deferred-but-demanded
                // presentation always reaches `should_run_segment() ==
                // true` (`PollDecision::ProduceWithheld`) below.
                continue;
            }

            let result = Self::draw_frame_for_presentation(presentation, constraints);
            // Telemetry: remember this segment's span so `render_frame_entered`
            // (this method's own caller, which decides whether/how to submit)
            // can attach it to a `FrameSnapshot` at its own submit point --
            // see `PresentationState::last_segment_span`'s own doc for why
            // this is a side channel rather than a local variable, and why
            // it lives on the presentation rather than the shared
            // `FrameClock`. `take_last_segment_span` (never a plain `get`)
            // is what makes this per-pump: a presentation whose segment does
            // NOT run some later pump sees `None` here, not this pump's
            // stale value.
            presentation.set_last_segment_span(segment_start, presentation.clock().now());
            // Latch "first frame confirmed sent" only for an unconditional
            // `Produce` that didn't error -- mirrors the old
            // `RenderingFlutterBinding::mark_first_frame_sent`'s `!errored`
            // guard, now also excluding `ProduceWithheld`: a withheld
            // result was never sent, by construction, so confirming it as
            // sent would be self-contradictory and would wrongly disarm a
            // later `defer_first_frame` call. `render_frame_entered` below
            // separately re-checks `is_deferred()` at its own submit point
            // before honoring whatever this segment produced.
            if decision.is_produce() && !matches!(result, FramePaintOutcome::Errored) {
                presentation.clock().mark_first_frame_sent();
            }
            last_outcome = result;
            producer = presentation.id();
        }
        (producer, last_outcome)
    }

    /// One presentation's build+layout+paint segment — moves VERBATIM from
    /// the retired `AppBinding::draw_frame_entered`'s Phase 1–4, now
    /// parameterized by `presentation` instead of hard-wired to
    /// `self.presentations.primary()`.
    ///
    /// **Why the caller's dirty gate cannot skip a segment this body would
    /// have produced `Painted` for:** `run_frame_with_layout_builders`
    /// itself returns `None` (this function's `Idle` outcome) whenever
    /// nothing is dirty — that decision already lived inside the pipeline
    /// before this loop existed. `Self::has_pending_work` (build pending OR
    /// a dirty render node) is exactly the union of conditions that
    /// decision depends on, so gating the CALL on the same union, one level
    /// up, changes nothing observable: either both agree nothing is dirty
    /// (skip here, `Idle` there — same outcome, cheaper), or either finds
    /// real work and the segment runs exactly as it always did.
    fn draw_frame_for_presentation(
        presentation: &PresentationState,
        constraints: BoxConstraints,
    ) -> FramePaintOutcome {
        #[cfg(test)]
        presentation.record_flush();

        // Phase 1: Build (WidgetsBinding)
        {
            let w = presentation.widgets();
            if w.has_pending_builds() {
                w.draw_frame();
            }
        }

        // Phase 2 & 3: Layout, Compositing, Paint, Semantics through the
        // typestate-driven orchestrator.
        let mut pipeline_errored = false;
        let (layer_tree, link_registry) = {
            presentation
                .renderer()
                .root_pipeline_owner()
                .with_mut(|owner| owner.set_root_constraints(Some(constraints)));
            let result = presentation
                .widgets()
                .run_frame_with_layout_builders(presentation.pipeline());
            let link_registry = presentation
                .renderer()
                .root_pipeline_owner()
                .with_mut(PipelineOwner::take_link_registry);
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
            let w = presentation.widgets();
            w.service_child_requests(presentation.pipeline());
        }

        // Phase 4: Create Scene from LayerTree
        let size = constraints.constrain(Size::ZERO);
        let frame_number = presentation.frames_rendered() + 1;

        if let Some(mut layer_tree) = layer_tree {
            presentation.attach_performance_overlay(&mut layer_tree);

            let root = layer_tree.root();
            let scene = Scene::with_links(
                size,
                layer_tree,
                root,
                link_registry.unwrap_or_default(),
                frame_number,
            );
            // By value, not `Arc<Scene>` — see `FramePaintOutcome::Painted`'s
            // own doc for why.
            FramePaintOutcome::Painted(scene)
        } else if pipeline_errored {
            FramePaintOutcome::Errored
        } else {
            FramePaintOutcome::Idle
        }
    }

    /// Render while the platform dispatcher already owns the realm entry.
    /// This keeps scheduler callbacks and the full build/layout/paint/raster
    /// transaction under one activation instead of creating a nested scope.
    ///
    /// Returns whether the frame reached `present()` — needed for the
    /// runner's no-present fallback throttle: `Fifo` present blocks every
    /// PRESENTED frame at display cadence, but a frame that never presents
    /// (nothing dirty, no damage, occluded surface, surface lost) carries no
    /// such pacing signal.
    ///
    /// Step by step: settle any lone arena member queued by an earlier event
    /// whose owner boundary could not finish (e.g. after a panic); flush
    /// coalesced pointer moves; draw the frame ([`Self::draw_frame_entered`]);
    /// re-hit-test stationary pointing devices against the freshly laid-out
    /// tree; then, gated by the primary presentation's own
    /// `FrameClock::is_deferred`, mark full-repaint damage and hand the
    /// scene to `renderer.render_scene`. This submit gate is DELIBERATELY
    /// separate from `draw_frame_entered`'s own segment gate: first-frame
    /// deferral withholds only the submit, never the build/layout/paint
    /// work (`.flutter/packages/flutter/lib/src/rendering/binding.dart:582-599`
    /// — `RendererBinding.deferFirstFrame`'s own doc: "the framework will
    /// still do all the work to produce frames, but those frames are never
    /// sent to the engine"), so a deferred presentation's segment can very
    /// much have produced a real `Painted` outcome here, and this check is
    /// what keeps it off the engine. `SurfaceLost`/`DeviceLost`/
    /// `SurfaceValidation`, a pipeline `Errored` outcome, and any other
    /// render error all count as a dropped (not settled) frame, arming a
    /// retry via [`Self::wake_frame`] instead of [`Self::mark_rendered`]'s
    /// idle-clear — but only the three named submit failures additionally
    /// re-dirty the pipeline via [`Self::mark_needs_full_repaint_for`]; see
    /// the `retry_needs_repaint` binding below for why a pipeline `Errored`
    /// outcome does not get the same treatment.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn render_frame_entered<R: RasterBackend>(&self, renderer: &mut R) -> bool {
        self.gestures().drain_deferred_arena_resolutions();
        self.gestures().flush_pending_moves();

        let (width, height) = renderer.size();
        let dpr = self
            .presentations
            .primary()
            .renderer()
            .root_pipeline_owner()
            .with(PipelineOwner::device_pixel_ratio);
        let constraints =
            BoxConstraints::tight(Size::new(px(width as f32 / dpr), px(height as f32 / dpr)));
        let (producer_id, outcome) = self.draw_frame_entered(constraints);
        // The presentation whose segment actually produced `outcome` above —
        // NEVER assumed to be `primary()`. On a pump where the primary
        // skips (nothing dirty) and a secondary presentation produces (both
        // reachable in production: `install_presentation_alongside` <-
        // `open_secondary_window`), `primary()` and the real producer
        // differ, and every decision below (the deferred-submit gate, the
        // submit timestamp, frame accounting, telemetry) must be read from
        // and recorded against the ACTUAL producer's own clock, not a
        // sibling's — see `Self::record_submit_telemetry`'s own doc for the
        // misattribution this addressing closes.
        let producer = self
            .presentations
            .get(producer_id)
            .unwrap_or_else(|| self.presentations.primary());

        self.gestures()
            .mouse_tracker()
            .update_all_devices(|position| {
                let mut result = flui_interaction::routing::HitTestResult::new();
                self.presentations
                    .primary()
                    .renderer()
                    .hit_test_in_view(&mut result, position, 0);
                result
            });

        let errored = matches!(outcome, FramePaintOutcome::Errored);
        // The submit gate: withheld while the PRODUCER's own first frame is
        // deferred -- see this method's own doc for why this is a SEPARATE
        // check from `draw_frame_entered`'s segment gate, not a redundant
        // re-check of the same decision.
        let should_send = !producer.clock().is_deferred();

        let mut presented = false;
        let mut retry_needed = errored;
        // Tracks a NARROWER condition than `retry_needed`: whether the
        // eventual retry also needs [`Self::mark_needs_full_repaint_for`]
        // to have something to redo. Only the three submit-failure arms
        // below (`SurfaceLost`/`DeviceLost`/`SurfaceValidation`) set this —
        // deliberately NOT `errored` (a `FramePaintOutcome::Errored`
        // outcome, i.e. `run_frame_with_layout_builders` itself returned
        // `Err`): on that path `render_scene` is never called at all, so
        // the repaint-mark's own justification ("the frame had already
        // consumed the pipeline's dirty state producing the scene it tried
        // to submit") is simply false there — nothing was produced to
        // submit. Marking `mark_needs_paint` also could not even address a
        // BUILD/LAYOUT failure (paint alone does not redo either), so it
        // would add a repaint with no plausible fix-the-retry effect while
        // still paying its cost every wake. That arm keeps its PRE-#637
        // behavior unchanged: `wake_frame()` only, same as `main`.
        let mut retry_needs_repaint = false;
        if should_send
            && let FramePaintOutcome::Painted(ref scene) = outcome
            && scene.has_content()
        {
            renderer.mark_full_repaint();
            let render_result = renderer.render_scene(scene);
            // Telemetry: sampled AFTER `render_scene` returns, never before
            // it -- on the production wgpu backend, a `Ok(true)` result
            // means `render_scene` itself called `output.present()`, which
            // under the default `Fifo` presentation mode BLOCKS until the
            // next vsync before returning. Sampling before the call (as an
            // earlier version of this method did) understates every
            // "input-to-present"/"produce-to-present" latency by up to a
            // full frame interval -- it would measure submit, not present,
            // while `flui_scheduler::frame_histogram`'s
            // `input_to_present_histogram`/`produce_to_present_histogram`
            // names (and `FrameSnapshot::submit_at`'s own doc) already
            // claim the present-inclusive meaning. Sampling once, here,
            // after the call returns -- for EVERY outcome, not just
            // `Ok(true)` -- keeps that claim true uniformly: a failure
            // arm's `submit_at` is likewise "whenever `render_scene`
            // finished attempting this submit", including whatever work
            // (partial blocking, driver validation) it did before failing.
            // Never recorded for `Ok(false)` (no damage/occluded, nothing
            // actually reached the backend): that branch's pending input
            // epochs stay buffered on the clock for whichever later pump
            // does submit, per `FrameClock::record_frame`'s own doc.
            let submit_at = producer.clock().now();
            match render_result {
                Ok(did_present) => {
                    presented = did_present;
                    if did_present {
                        producer.record_frame_rendered();
                        Self::record_submit_telemetry(
                            producer,
                            submit_at,
                            PresentOutcome::Presented,
                            EpochDisposition::Drain,
                        );
                        tracing::trace!(
                            frame = scene.frame_number(),
                            total = producer.frames_rendered(),
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
                    producer.record_frame_dropped();
                    // `Retain`, not `Drain`: this arm arms a retry
                    // (`retry_needed = true` below) via `wake_frame()`, and
                    // the eventual real submit that retry produces must
                    // still find the epochs that arrived before this
                    // failure -- see `record_submit_telemetry`'s own doc.
                    Self::record_submit_telemetry(
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    retry_needed = true;
                    // See `retry_needs_repaint`'s own binding above for why
                    // this arm (a genuine submit failure, not a pipeline
                    // error) is one of the three that also re-dirties
                    // `producer`'s pipeline via
                    // `mark_needs_full_repaint_for` below.
                    retry_needs_repaint = true;
                    tracing::debug!("Surface lost; frame dropped — retry armed via wake_frame()");
                }
                Err(EngineError::DeviceLost) => {
                    producer.record_frame_dropped();
                    // `Retain`, not `Drain`: this arm now arms a retry
                    // (`retry_needed = true` below), and the eventual real
                    // submit that retry produces must still find the epochs
                    // that arrived before this failure — see
                    // `record_submit_telemetry`'s own doc.
                    Self::record_submit_telemetry(
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    // Division of labor with the platform runner's own
                    // recovery wake (`render_frame_with_device_recovery` in
                    // `runner.rs`): this arm only ever runs once
                    // `render_scene` was actually called, i.e. there was
                    // dirty content to submit — it owns the retry for a
                    // loss OBSERVED MID-FRAME, for every `RasterBackend`
                    // consumer (headless included), not just the desktop/
                    // Android runners. A loss discovered on a quiescent
                    // loop, before anything is dirty enough to reach
                    // `render_scene` at all, never reaches this arm
                    // (`draw_frame_entered` produces `Idle`, nothing to
                    // submit) — that gap is what the runner's own wake on a
                    // FAILED pre-frame recovery attempt covers instead. The
                    // two are non-overlapping, not redundant.
                    retry_needed = true;
                    // See `retry_needs_repaint`'s own binding above.
                    retry_needs_repaint = true;
                    tracing::warn!(
                        "GPU device lost; frame dropped — retry armed, recovery is \
                         attempted by the renderer owner"
                    );
                }
                Err(EngineError::SurfaceValidation) => {
                    producer.record_frame_dropped();
                    // `Retain`, not `Drain`: this arm now arms a retry, and
                    // the eventual real submit that retry produces must
                    // still find the epochs that arrived before this
                    // failure — see `record_submit_telemetry`'s own doc.
                    Self::record_submit_telemetry(
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    // Retry armed for the same reason as `DeviceLost`
                    // above. NOTE: as of this change the wgpu backend does
                    // NOT yet reconfigure the surface before the retry's
                    // own acquire attempt — `Renderer::acquire_surface_
                    // texture`'s `Validation` arm gaining its own
                    // reconfigure-and-retry-once (with its own test) ships
                    // separately. Until it lands, an armed retry here
                    // re-attempts the identical acquire and may keep
                    // failing; arming it anyway is still strictly better
                    // than the alternative (a permanently dropped frame
                    // with nothing ever retrying it), and gives a
                    // transient validation error the chance to clear on a
                    // later wake.
                    //
                    // Updated by issue #637: this NOTE was written when the
                    // armed retry silently parked after one no-op frame (see
                    // that issue) — "may keep failing" understated the old
                    // behavior, since a parked retry could not keep doing
                    // anything past its first no-op. Now that the retry
                    // actually re-submits (`retry_needs_repaint` below), a
                    // PERMANENTLY misconfigured surface genuinely repeats
                    // this failure indefinitely, at `NO_PRESENT_FALLBACK_
                    // PACE`'s ~62 Hz throttle (`runner.rs`) rather than a
                    // busy spin — a real, continuous full-tree paint cost
                    // that did not exist before this fix. Whether that
                    // steady state should sit behind its own backoff (like
                    // `DeviceRecoveryBackoff` already gates the pre-frame
                    // device-recovery path) is a real question this change
                    // does not answer — a working retry that costs CPU
                    // indefinitely on a permanently broken surface is still
                    // strictly better than one that silently gives up, but
                    // is not obviously the final shape either.
                    retry_needed = true;
                    retry_needs_repaint = true;
                    tracing::error!(
                        "Surface validation error — surface misconfig; retry armed for \
                         the next wake"
                    );
                }
                Err(e) => {
                    producer.record_frame_dropped();
                    Self::record_submit_telemetry(
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Drain,
                    );
                    tracing::error!(error = ?e, "Render error (non-recoverable this frame)");
                }
            }
        }

        if retry_needed {
            // Issue #637: `wake_frame()` alone re-opens `draw_frame_entered`'s
            // per-presentation segment gate but never touches `PipelineOwner`'s
            // own independent dirty tracking — the frame that just failed to
            // submit had already consumed whatever was dirty producing the
            // scene it tried to send, so an otherwise-unchanged tree finds
            // nothing dirty on the retry pump, produces `Idle`, and
            // `mark_rendered()` clears the flag having never reached
            // `render_scene`: one no-op frame, then the retry silently parks.
            //
            // `retry_needs_repaint` (set only by the three submit-failure
            // arms above, never by a pipeline `Errored` outcome — see that
            // binding's own comment) gates
            // [`Self::mark_needs_full_repaint_for`], the same fix #630
            // already established for the pre-frame device-recovery-success
            // arm (`runner.rs`'s `render_frame_with_device_recovery`) —
            // shared rather than duplicated via that one function, since
            // both are "a retry needs the pipeline to actually have work to
            // redo, not just a wake"; see that function's own doc for why
            // `mark_needs_paint` (not `mark_needs_layout`) is the right
            // weight. Addressed to `producer` — the presentation whose
            // segment actually produced the scene that just failed to
            // submit, resolved above and NEVER assumed to be `primary()`
            // (this method's own doc, at the `producer` binding, explains
            // why the two can differ) — not `self.presentations.primary()`:
            // an earlier version of this fix wrongly hard-coded `primary()`
            // here, which left the fix inert on any pump where a secondary
            // presentation was the actual producer.
            //
            // Steady-state cost of a PERMANENTLY failing submit: unlike the
            // pre-frame device-recovery path, this arm is not gated by
            // `DeviceRecoveryBackoff` — a surface that keeps failing every
            // submit now retries every wake, throttled only by
            // `runner.rs`'s `NO_PRESENT_FALLBACK_PACE` (~62 Hz), running a
            // full-tree paint each time, indefinitely. Before this fix that
            // same surface went quiet after one no-op frame. A working
            // retry that costs CPU forever on a permanently broken surface
            // is still strictly better than one that silently gives up, but
            // whether this steady state deserves its own backoff is a
            // separate question this change does not answer.
            if retry_needs_repaint {
                self.mark_needs_full_repaint_for(producer);
            }
            // Still called unconditionally for every `retry_needed` cause,
            // pipeline `Errored` included: unlike the pre-frame success arm
            // (which runs synchronously right before this same call's own
            // `render_frame_entered`, so nothing external needs poking),
            // every cause here fails INSIDE this call — the retry can only
            // happen on a LATER wake, so the platform still needs the poke
            // `wake_frame()` provides regardless of whether a repaint was
            // also marked above.
            self.wake_frame();
        } else {
            self.mark_rendered();
        }

        // Animation continuation wake — deliberately placed AFTER
        // `mark_rendered()`/the retry check above, not inside
        // `draw_frame_entered`'s vsync loop where an earlier version of
        // this method raised it. Reason, found by a production-sequence
        // probe rather than assumed:
        // `draw_frame_entered`'s own tick already ran (this method called
        // it above), so `presentation.vsync().has_running()` here reads
        // the SAME post-tick state Flutter's own `shouldScheduleTick`
        // reads — but `mark_rendered()` unconditionally clears
        // `needs_redraw` to `false`, and a `wake_frame()` call made BEFORE
        // that point (inside `draw_frame_entered`) gets silently clobbered
        // within this SAME callback. On the real desktop path, where the
        // next platform callback's own dirty gate (`wake_action`, in
        // `runner.rs`) reads `needs_redraw()`/`has_pending_work()` fresh —
        // neither of which a bare running controller with no other
        // tree-visible effect keeps true — that clobber is a genuine,
        // silent stall: the controller never receives another tick after
        // its anchor frame. Confirmed red before this fix, green after, by
        // `runner.rs`'s
        // `a_running_controller_with_no_other_dirty_state_keeps_producing_across_the_real_wake_action_gate`,
        // which drives the exact `record_compositor_tick` -> dirty-gate ->
        // `render_frame_entered` sequence `bootstrap_desktop`'s closure
        // uses, not `draw_frame_entered` called directly (which never
        // exercises `wake_action` at all and could not have caught this).
        //
        // Marks a fresh `Animation` demand here (this pump's own mask was
        // already cleared by the produce above) specifically so
        // `try_arm_redraw_request` has something nonempty to arm — this is
        // demand for the NEXT pump, a different concern from the
        // before-tick mark in `draw_frame_entered` that feeds THIS pump's
        // own segment. Still edge-triggered (coalesces repeated callbacks
        // under backpressure into one wake, same as before this move) —
        // only the ordering relative to `mark_rendered()` changed.
        for presentation in self.presentations.iter() {
            if presentation.vsync().has_running() {
                presentation.clock().mark_demand(DemandKind::Animation);
                if presentation.clock().try_arm_redraw_request() {
                    self.wake_frame();
                }
            }
        }

        presented
    }

    /// Finalize and record this pump's [`flui_scheduler::FrameSnapshot`] for
    /// `presentation`, IF a segment actually ran THIS PUMP for
    /// `presentation` specifically (`PresentationState::
    /// take_last_segment_span` returns `Some`) — a presentation whose
    /// segment was skipped entirely this pump (`draw_frame_entered`'s own
    /// gate) has nothing to record.
    ///
    /// `presentation` must be the exact presentation whose segment produced
    /// the outcome being submitted this call — [`Self::render_frame_entered`]
    /// resolves this from `draw_frame_entered`'s own returned producer id,
    /// never `self.presentations.primary()` unconditionally. The bug this
    /// guards against: with more than one presentation mounted, a pump where
    /// the primary's segment is skipped (nothing dirty) while a secondary's
    /// produces used to still record against `primary()`; because the old
    /// per-clock segment-span latch was never cleared once set, a primary
    /// that HAD produced on some EARLIER pump still read `Some` here (stale,
    /// from that earlier pump, not this one), so the guard below wrongly
    /// passed — recording a `FrameSnapshot` with the primary's stale span, a
    /// duplicate `frame_id` (the primary's own `produced_count` unchanged
    /// since it didn't poll-produce this pump), and the primary's own
    /// pending input epochs drained into a frame it never produced. Reading
    /// and clearing PER-PRESENTATION via `take_last_segment_span` (rather
    /// than a shared, never-cleared `get`) and resolving the correct
    /// producer together close this: a presentation not addressed here never
    /// has its span read, and the addressed one's stale cross-pump reads are
    /// impossible because the span is cleared the moment it is used.
    ///
    /// Called only from the branches of [`Self::render_frame_entered`] that
    /// reached a real submit attempt (never for `Ok(false)`/no-damage, whose
    /// pending input epochs must stay buffered for a later, real submit).
    ///
    /// `epochs` decides whether this presentation's pending input epochs are
    /// drained into the recorded snapshot ([`EpochDisposition::Drain`], the
    /// common case) or merely read from without consuming them
    /// ([`EpochDisposition::Retain`]) — see that type's own doc for why a
    /// submit failure that arms a retry must use `Retain`: draining on the
    /// failed attempt would leave the eventual retry's own real submit with
    /// nothing pending to attribute to the frame that actually reaches the
    /// screen.
    fn record_submit_telemetry(
        presentation: &PresentationState,
        submit_at: Instant,
        present_outcome: PresentOutcome,
        epochs: EpochDisposition,
    ) {
        if let Some((segment_start, segment_end)) = presentation.take_last_segment_span() {
            let snapshot = match epochs {
                EpochDisposition::Drain => presentation.clock().record_frame(
                    presentation.id(),
                    segment_start,
                    segment_start,
                    segment_end,
                    submit_at,
                    present_outcome,
                ),
                EpochDisposition::Retain => presentation.clock().record_frame_retaining_epochs(
                    presentation.id(),
                    segment_start,
                    segment_start,
                    segment_end,
                    submit_at,
                    present_outcome,
                ),
            };
            Self::emit_frame_telemetry(presentation, &snapshot);
        }
    }

    fn emit_frame_telemetry(presentation: &PresentationState, snapshot: &FrameSnapshot) {
        if !tracing::enabled!(target: "flui.frame", tracing::Level::TRACE) {
            return;
        }

        let outcome = match snapshot.present_outcome {
            PresentOutcome::Presented => "presented",
            PresentOutcome::Errored => "errored",
            _ => "unknown",
        };
        let input_latency_max_us = snapshot
            .latencies()
            .map(|(_, latency)| Self::duration_micros_saturated(latency))
            .max()
            .unwrap_or(0);

        tracing::trace!(
            target: "flui.frame",
            event = "frame_telemetry",
            presentation = %snapshot.presentation,
            frame_id = %snapshot.frame_id,
            present_outcome = outcome,
            segment_us = Self::duration_micros_saturated(snapshot.segment_span()),
            produce_to_present_us = Self::duration_micros_saturated(
                snapshot.submit_at.saturating_duration_since(snapshot.clock_timestamp)
            ),
            input_count = snapshot.input_epochs.len(),
            input_latency_max_us,
            input_epochs_overflowed = snapshot.input_epochs.overflowed(),
            produces_deferred = presentation.clock().produces_deferred(),
            frames_dropped = presentation.frames_dropped(),
            "frame telemetry"
        );
    }

    /// Takes `web_time::Duration` to match the telemetry API it consumes.
    /// That is the same type as `std::time::Duration` on every target —
    /// `web_time` re-exports `std::time::*` and overrides only `Instant` and
    /// `SystemTime` — so this is a readability choice, not a portability one.
    fn duration_micros_saturated(duration: web_time::Duration) -> u64 {
        u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
    }

    // ========================================================================
    // Input dispatch (moved from the retired `AppBinding`)
    // ========================================================================

    /// [`Self::handle_input_addressed`], addressed to this realm's primary —
    /// the pre-addressed-routing shape, kept for every existing single-presentation
    /// test (production dispatch, `runner.rs`'s `PlatformToUi::run`, calls
    /// `handle_input_addressed` directly with the real stamped id).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production input dispatch calls handle_input_addressed directly with the \
                      real stamped presentation id (PlatformToUi::run); this primary-only \
                      convenience is exercised only by tests that never address a specific \
                      presentation"
        )
    )]
    pub(crate) fn handle_input_entered(&self, input: PlatformInput) {
        self.handle_input_addressed(self.presentations.primary().id(), input);
    }

    /// Handle a platform input event while this realm is already entered,
    /// addressed to exactly one presentation this realm hosts (issue #555
    /// the addressed-routing slice).
    ///
    /// `presentation_id` is the exact presentation `dispatch_platform_realm`
    /// (`runner.rs`) stamped this event with at enqueue time — the window
    /// that produced it, resolved through `WindowRegistry` at hop-1 — never
    /// assumed to be this realm's primary. A `presentation_id` this realm no
    /// longer hosts (closed between enqueue and drain) is a traced drop,
    /// never a panic or a silent fall-through to [`PresentationForest::
    /// primary`](super::presentation_forest::PresentationForest::primary).
    ///
    /// Pointer/IME/drag-drop events go to the ADDRESSED presentation's own
    /// gesture/text-input state — never a sibling's, and never falling
    /// through to the primary when the addressed id is missing
    /// (`input_stamped_for_b_never_reaches_as_arena`,
    /// `ime_event_addressed_to_b_does_not_reach_as_session`,
    /// `input_addressed_to_a_closed_or_unknown_presentation_drops_traced_
    /// never_falls_through`). Keyboard is the one exception: it always
    /// goes to [`Self::focus_coordinator`]'s currently ACTIVE presentation
    /// instead of the stamped one — see [`FocusCoordinator`]'s own doc for
    /// why (`keyboard_routes_to_active_presentation_only`).
    ///
    /// [`input_dropped_by_lifecycle`] gates every kind next, against the
    /// RESOLVED target's own lifecycle (not necessarily `presentation_id`'s,
    /// for `Keyboard`): `Closing`/`Closed` refuses all input; `Suspended`
    /// drops pointer/drag-drop only (keyboard/IME keep flowing, so a flaky
    /// or absent occlusion signal never becomes a keystroke blackout).
    ///
    /// Pointer events are coalesced by the target presentation's own
    /// `GestureBinding` — high-frequency move events are stored and flushed
    /// once per frame via [`Self::render_frame_entered`].
    pub(crate) fn handle_input_addressed(
        &self,
        presentation_id: PresentationId,
        input: PlatformInput,
    ) {
        let target_id = match &input {
            PlatformInput::Keyboard(_) => self.focus_coordinator.active(),
            PlatformInput::Pointer(_) | PlatformInput::Ime(_) | PlatformInput::DragDrop(_) => {
                presentation_id
            }
        };
        let Some(presentation) = self.presentations.get(target_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = target_id.as_u64(),
                stamped_presentation_id = presentation_id.as_u64(),
                input_kind = input_kind(&input),
                "dropping input addressed to a presentation this realm no longer hosts"
            );
            return;
        };
        if input_dropped_by_lifecycle(presentation.lifecycle(), &input) {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation.id().as_u64(),
                lifecycle = ?presentation.lifecycle(),
                input_kind = input_kind(&input),
                "dropping input due to presentation lifecycle"
            );
            return;
        }
        // Telemetry: stamp this event's arrival on the RESOLVED target's own
        // clock (never a wall-clock read) before dispatch, so a consumer can
        // later attribute a produced frame's latency back to this specific
        // input — see `FrameClock::stamp_input_epoch`'s own doc. Stamped
        // INSIDE each arm below, only for a kind this method actually
        // routes (every arm that stamps also calls `request_redraw_for`,
        // its own contribution to demand) — never for `DragDrop`, whose own
        // arm traces the event as dropped without routing it anywhere or
        // requesting a frame. Stamping an event this method is about to
        // drop would leave a phantom epoch sitting in the ring until
        // eviction, attributed to whichever unrelated frame happens to
        // produce next.
        match input {
            PlatformInput::Ime(ime_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                presentation.text_input().dispatch(&ime_event);
                self.request_redraw_for(presentation);
            }
            PlatformInput::Pointer(pointer_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                let routing_panic = catch_unwind(AssertUnwindSafe(|| {
                    presentation
                        .gestures()
                        .handle_pointer_event(&pointer_event, |position| {
                            let mut result = flui_interaction::routing::HitTestResult::new();
                            let offset = flui_types::Offset::new(position.dx, position.dy);
                            presentation
                                .renderer()
                                .hit_test_in_view(&mut result, offset, 0);
                            if !result.is_empty() {
                                tracing::debug!(hits = result.len(), "Hit test found targets");
                            }
                            result
                        });
                }))
                .err();
                let deferred_panic = catch_unwind(AssertUnwindSafe(|| {
                    presentation.gestures().drain_deferred_arena_resolutions();
                }))
                .err();

                let mut first_panic = None;
                preserve_first_input_panic(&mut first_panic, routing_panic, "pointer routing");
                preserve_first_input_panic(
                    &mut first_panic,
                    deferred_panic,
                    "deferred arena resolution",
                );
                self.request_redraw_for(presentation);
                if let Some(payload) = first_panic {
                    resume_unwind(payload);
                }
            }
            PlatformInput::Keyboard(keyboard_event) => {
                let clock = presentation.clock();
                clock.stamp_input_epoch(clock.now());
                presentation
                    .focus_manager()
                    .dispatch_key_event(&keyboard_event);
                self.request_redraw_for(presentation);
            }
            PlatformInput::DragDrop(drag_drop_event) => {
                // Not stamped (see this method's own doc): this event is
                // never routed and never requests a frame, so there is no
                // future produce to attribute it to.
                tracing::debug!(
                    drag_drop_kind = drag_drop_kind(&drag_drop_event),
                    "drag-and-drop input received; realm routing not implemented yet, dropping"
                );
            }
        }
    }

    /// The pointer entered (`true`) or left (`false`) `presentation_id`'s
    /// native window. Leave sweeps that presentation's hover state —
    /// `MouseRegion::on_exit` fires for every hovered region and the cursor
    /// resets — then pumps a frame so the un-hovered visuals actually paint.
    /// Enter is a no-op: the next pointer move re-primes hover from a fresh
    /// hit test on its own. A stale or unknown `presentation_id` is a traced
    /// no-op, the same posture as every other addressed signal.
    pub(crate) fn handle_window_hover_addressed(
        &self,
        presentation_id: PresentationId,
        inside: bool,
    ) {
        if inside {
            return;
        }
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::trace!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a window-hover signal for a presentation this realm no longer hosts"
            );
            return;
        };
        presentation.gestures().handle_pointer_left_window();
        self.request_redraw_for(presentation);
    }

    /// Record that `presentation_id`'s native window just gained OS focus —
    /// [`FocusCoordinator::note_focus_gained`]'s sole write side. A stale or
    /// unknown `presentation_id` (already closed, or a forged/mixed address)
    /// is a traced no-op: focus arbitration never points at a presentation
    /// this realm does not currently host.
    pub(crate) fn notify_presentation_focus_gained(&self, presentation_id: PresentationId) {
        if self.presentations.get(presentation_id).is_none() {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a focus-gained notification for a presentation this realm no longer hosts"
            );
            return;
        }
        self.focus_coordinator.note_focus_gained(presentation_id);
    }

    /// Gate `presentation_id`'s own
    /// [`FrameClock`](flui_scheduler::FrameClock) on a real visibility
    /// signal — the presentation-level counterpart of the
    /// realm-wide `AppLifecycleState` derivation
    /// `PlatformToUi::WindowVisibility` (`runner.rs`) also drives. A stale
    /// or unknown `presentation_id` (already closed, or a forged/mixed
    /// address) is a
    /// traced no-op, matching [`Self::notify_presentation_focus_gained`]'s
    /// own discipline.
    ///
    /// While hidden,
    /// [`FrameClock::poll`](flui_scheduler::FrameClock::poll) returns
    /// `Skip(Hidden)` for this
    /// presentation: `draw_frame_entered`'s segment loop skips its
    /// build/layout/paint/submit entirely, demand retained. Becoming visible
    /// again does NOT self-produce — nothing wakes an idle platform loop on
    /// its own — so this is also the ungate→wake edge: with a nonzero
    /// retained mask, this call wakes the realm unconditionally rather than
    /// consulting
    /// [`FrameClock::try_arm_redraw_request`](flui_scheduler::FrameClock::try_arm_redraw_request).
    /// That latch cannot
    /// be trusted here: it may already be armed from a demand mark that
    /// occurred BEFORE this presentation went hidden (a poke the platform
    /// loop already delivered and wasted, since `poll` returned `Skip(Hidden)`
    /// for it) — reading `try_arm_redraw_request()`'s return value would
    /// incorrectly stay silent in that sequence and strand the presentation
    /// gated forever with demand nobody ever wakes for.
    pub(crate) fn set_presentation_hidden(&self, presentation_id: PresentationId, hidden: bool) {
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a visibility change for a presentation this realm no longer hosts"
            );
            return;
        };
        presentation.clock().set_hidden(hidden);
        if !hidden && !presentation.clock().demand_mask().is_empty() {
            self.wake_frame();
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
                    // Reuse the SAME fan-out `Self::apply_hot_reload` the
                    // direct `perform_hot_reload_entered` path calls --
                    // calling `self.presentations.primary().apply_hot_reload`
                    // here instead (as this arm once did) reassembles only
                    // the primary and silently skips every other
                    // presentation, exactly the class of bug
                    // `reassemble_fans_out_to_all_presentations_in_mount_
                    // order` exists to catch on the direct path; this
                    // inbox arm is the untested twin of that same call, and
                    // the desktop worker's own hot-reload trigger goes
                    // through THIS arm, not the direct one.
                    if self.apply_hot_reload(tier) {
                        self.redraw_pending.store(true, Ordering::Release);
                    }
                    report.invoked += 1;
                }
                UiCommand::SemanticsAction {
                    presentation_id,
                    request,
                } => {
                    // Forest-membership check (issue #555's addressed-routing slice), not
                    // primary-only equality: a presentation generation this
                    // realm no longer hosts -- closed since the action was
                    // stamped, or a genuinely different (non-primary)
                    // presentation among several -- is a traced drop either
                    // way, never delivered to a sibling.
                    let Some(presentation) = self.presentations.get(presentation_id) else {
                        tracing::trace!(
                            stamped_presentation_id = presentation_id.as_u64(),
                            "dropping semantics action stamped for a presentation this realm no \
                             longer hosts"
                        );
                        report.dropped_stale += 1;
                        continue;
                    };
                    match presentation.dispatch_semantics_action(request) {
                        Ok(()) => {
                            report.invoked += 1;
                        }
                        Err(error) => {
                            // Flutter deliberately ignores actions for stale
                            // views/nodes because screen readers may lag behind
                            // the latest semantics update.
                            tracing::trace!(
                                { flui_foundation::diagnostics::PRESENTATION_ID } =
                                    presentation_id.as_u64(),
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

    /// Close and remove exactly one presentation from this realm's forest.
    ///
    /// Called from `runner.rs`'s `dispatch_platform_realm` loop, which
    /// special-cases `RealmTask::ClosePresentation` to call this with
    /// `&mut self` directly (the realm sits checked out of `AppRuntime`'s
    /// registry as an owned local at that point, so `&mut` is naturally
    /// available — no other caller reaches this method). That same
    /// dispatch has ALREADY set `dispatched_realm_id` for the whole
    /// checkout, so a dispose callback this method's own step 3 runs is
    /// covered by the identical TLS deferral guard every other realm-map
    /// mutation already respects: one deferral authority, not a second one
    /// to keep in sync with it. See `runner.rs::close_presentation` for the
    /// request-shaped public entry point (`enqueue + wake`) that gets here.
    ///
    /// A no-op (returns `false`) if `id` does not name a presentation this
    /// realm currently hosts.
    ///
    /// # Contract — six steps, in order
    ///
    /// 1. **Unregistering this presentation's window mapping from the
    ///    process-wide `WindowRegistry` is NOT this method's job.**
    ///    `UiRealm` has no access to that registry (`AppRuntime`-owned,
    ///    ADR-0037 §2's single authority) — the caller must have already
    ///    removed the registry entry before calling this. The real (and
    ///    today's only) caller is `dispatch_platform_realm`'s
    ///    `RealmTask::ClosePresentation` handling in `runner.rs`, NOT
    ///    `AppRuntime`'s realm-teardown path (`apply_uninstall`/
    ///    `teardown_platform_realm` close a WHOLE realm and never call this
    ///    method at all — a realm's own `Drop` closes every remaining
    ///    presentation directly). That dispatch handling unregisters this
    ///    exact presentation's own window mapping first when a SIBLING
    ///    presentation is staying open, and routes to a full realm
    ///    uninstall instead of calling this method at all when `id` names
    ///    the realm's only presentation (seeing [`Self::is_sole_presentation`]
    ///    return `true`) — this method's own contract never has to reason
    ///    about leaving the forest empty.
    /// 2. IME detach + focus deactivate — [`PresentationState::close`]'s
    ///    first half.
    /// 3. `detach_root_widget` through this exact presentation's own
    ///    `WidgetsBinding` — [`PresentationState::close`]'s second half —
    ///    run inside [`Self::enter_for_close`], so a `State::dispose()` a
    ///    descendant runs here gets the SAME realm-shared capabilities
    ///    (post-frame/interaction handles, TLS deferral) any other
    ///    frame/lifecycle callback gets, and can resolve a `GlobalKey`
    ///    living in any OTHER presentation this realm hosts. It cannot
    ///    resolve one of its OWN keys through the registry mid-teardown —
    ///    `enter_for_close` deliberately excludes `id` itself from the
    ///    composite it assembles; see that method's own doc for the
    ///    self-deadlock excluding it avoids. An install/uninstall request a
    ///    dispose hook makes here defers through the dispatched-path TLS
    ///    guard described above. Any BUILD SCHEDULING it does still routes
    ///    only within THIS presentation's own tree:
    ///    `RebuildHandle`/`ExternalBuildScheduler` are minted one-per-owner
    ///    and never shared, so a dispose hook has no path to a sibling
    ///    presentation's build inbox even if it tries (see
    ///    `dispose_during_teardown_cannot_reach_sibling_or_dead_services`).
    /// 4. **Async-task disposition:** the realm-level `AsyncDriver` is not
    ///    told about this closure — an in-flight task this presentation
    ///    spawned is NOT cancelled here, matching `PresentationState`'s own
    ///    `Drop` (which also does not reach into the driver). Its eventual
    ///    completion fails closed against the now-dropped tree for the same
    ///    reason step 3's dispose hooks do:
    ///    `async_completion_after_presentation_teardown_fails_closed_no_sibling_reach`
    ///    is the proof.
    /// 5. `GlobalKeyScope` claims still tagged to this presentation's
    ///    `BuildOwner` are reclaimed, traced — automatic, via `BuildOwner`'s
    ///    own `Drop`, once step 6 drops the last reference to it.
    /// 6. The removed `PresentationState` — and with it its `WidgetsBinding`
    ///    (whose drop triggers step 5), `RenderingFlutterBinding`, and every
    ///    other owned resource — drops after [`Self::enter_for_close`]
    ///    returns.
    ///
    /// # Why this is two calls, not one `&mut`-threaded closure
    ///
    /// [`Self::enter_for_close`] hands its closure `&Self` (shared) — every
    /// existing caller only ever needed read access to the realm for the
    /// duration of a frame/lifecycle callback, so it was never shaped to
    /// also hand out `&mut`. Steps 2–3 only need `&PresentationState`
    /// (`PresentationState::close` takes `&self`), so they run inside one
    /// `self.enter_for_close(...)` call. Steps 4–6 (the actual `Vec`
    /// removal + drop) need `&mut self.presentations`, which happens in a
    /// SEPARATE, sequential statement after that call returns — not nested
    /// inside it, so there is no borrow conflict and no need to reshape
    /// `enter_for_close` itself or give `PresentationForest` interior
    /// mutability.
    pub(crate) fn close_presentation_entered(&mut self, id: PresentationId) -> bool {
        let existed = self.enter_for_close(id, |realm| {
            let Some(presentation) = realm.presentations.get(id) else {
                return false;
            };
            // Re-stamp the active presentation to the survivor BEFORE
            // dispose hooks run, if `id` is the realm's currently ACTIVE
            // one (`FocusCoordinator`) -- mirrors `runner.rs`'s
            // `RealmSlot::address` re-stamp ordering (dispose-time
            // reentrancy safety: re-stamp before disposal, never after) and
            // closes the keyboard-blackhole gap a naive close would leave:
            // without this, `FocusCoordinator` keeps pointing at `id` after
            // steps 4-6 remove it from the forest, so `handle_input_
            // addressed`'s `Keyboard` arm resolves a presentation the
            // forest no longer has and every keyboard event drops traced
            // until a fresh `WindowFocus(true)` names a live presentation --
            // `primary_id_excluding` always returns `Some` here since
            // `close_presentation_entered` is never called for a realm's
            // sole presentation (that case routes through a full realm
            // uninstall instead, in `runner.rs`).
            if realm.focus_coordinator.active() == id
                && let Some(surviving) = realm.primary_id_excluding(id)
            {
                realm.focus_coordinator.note_focus_gained(surviving);
            }
            // Steps 2 + 3, composite (minus `id` itself) + capabilities active.
            presentation.close();
            true
        });
        if !existed {
            return false;
        }
        // Steps 4-6: `self.enter_for_close(...)` above has already returned,
        // so this is a plain, non-nested `&mut self.presentations` access.
        // Dropping `removed` here (end of statement) is both the
        // `GlobalKeyScope` reclaim trigger (step 5, via `BuildOwner::Drop`)
        // and the disposal itself (step 6) — no active registry needed for
        // either, since `GlobalKeyScope::reclaim_owner` is a plain
        // data-structure operation.
        let removed = self.presentations.remove(id);
        debug_assert!(
            removed.is_some(),
            "BUG: presentation existed a moment ago (checked via self.enter_for_close above) \
             and nothing between here and there could have removed it -- \
             enter_for_close's closure only reads the forest"
        );
        true
    }
}

impl Drop for UiRealm {
    fn drop(&mut self) {
        // Every presentation this realm hosts closes when the realm drops —
        // production topology is exactly one until the forest's ratchet
        // lifts, but this loop is already correct for N: nothing here
        // assumes `len() == 1`.
        //
        // Deliberately NOT wrapped in `enter()`: this runs during `Drop`,
        // which can itself run during thread-local destruction (e.g. a
        // realm still alive when its owning thread exits) where touching
        // ANOTHER thread-local (the registry activation stack `enter()`
        // pushes onto) aborts the process
        // ("cannot access a Thread Local Storage value during or after
        // destruction"). A `State::dispose()` hook that needs a `GlobalKey`
        // lookup during realm teardown is out of scope for this fix; the
        // registries this realm's `WidgetsBinding`s expose are simply
        // inactive here, matching every other un-entered context.
        for presentation in self.presentations.iter() {
            presentation.close();
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
    fn recreated_runtime_gets_fresh_realm_id() {
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

    /// Dropping a realm must free its `PipelineOwner` -- no stray strong
    /// `PipelineCell` clone survives in a listener closure, a cached
    /// binding, or (the hazard the type's own docs call out) a render
    /// object that captured its own owning cell and closed a
    /// `cell -> owner -> tree -> object -> cell` `Rc` cycle nothing frees.
    ///
    /// Red-check: have any long-lived piece of `UiRealm` (a semantics
    /// listener, the renderer binding, `WidgetsBinding`) hold an *extra*
    /// `PipelineCell` clone past the realm's own lifetime and this weak
    /// upgrade stops returning `None`.
    #[test]
    fn dropping_the_realm_frees_its_pipeline_owner() {
        let realm = UiRealm::for_test();
        let weak = realm.pipeline_for_test().downgrade_for_test();
        assert!(
            weak.upgrade().is_some(),
            "sanity: the owner is alive while the realm holds it"
        );

        drop(realm);

        assert!(
            weak.upgrade().is_none(),
            "dropping the realm must drop every strong PipelineCell clone it \
             held (presentation, widgets binding, renderer binding) -- a \
             surviving upgrade means one of them outlives the realm, or a \
             cycle is keeping the owner alive"
        );
    }

    #[test]
    fn cross_thread_navigation_command_drains_on_owner_thread() {
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

    /// The lock-free-at-dispatch invariant this test used to probe from
    /// *inside* the action handler (`Weak<RwLock<PipelineOwner>>::try_write`)
    /// is unrepresentable now: `SemanticsActionHandler` is
    /// `Arc<dyn Fn(..) + Send + Sync>` (a retained cross-thread seam — see
    /// `flui-semantics/src/action.rs`), and `PipelineCell` is `!Send`, so no
    /// handle derived from it can be captured in that closure any more. The
    /// invariant itself did not disappear — it moved into production as the
    /// `debug_assert!(is_free())` in `PresentationState::dispatch_semantics_
    /// action` (registry: `runtime-contract.toml`'s `semantics-two-phase-borrow`
    /// contract), which this test's
    /// normal pass/fail already exercises (the assert would panic the test
    /// if it ever fired). What remains directly assertable here — and what
    /// this test still proves — is the *observable* contract: dispatch
    /// defers to the owner's Idle commit point, and the counts are right.
    #[test]
    fn semantics_action_commits_on_the_owner_after_releasing_the_pipeline_lock() {
        let realm = UiRealm::for_test();
        let pipeline = realm.pipeline_for_test();
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_in_handler = Arc::clone(&invoked);
        let render_id = RenderId::new(7);
        let target = AccessibilityNodeId::from(render_id);

        let mut node = SemanticsNode::new().with_source_render_id(render_id);
        node.config_mut().add_action(
            SemanticsAction::Tap,
            Arc::new(move |action, arguments| {
                assert_eq!(action, SemanticsAction::Tap);
                assert!(arguments.is_none());
                invoked_in_handler.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root = semantics_owner.insert(node);
        semantics_owner.set_root(Some(root));
        pipeline.with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

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
            .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

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

    /// The full inbound platform wire (issue #684): an action request
    /// arriving on the platform adapter's thread — Click, addressed by the
    /// stable exported node id — crosses the AccessKit→FLUI translation,
    /// marshals through the realm inbox, and invokes the node's Tap handler
    /// at the owner's Idle drain. Driven through the window's OWN
    /// `FakeAccessibility` and the listener `wire_platform_accessibility`
    /// registered on it — never a hand-rolled sender, which would pass with
    /// the production wire unplugged.
    #[test]
    fn platform_action_request_routes_through_the_wire_to_the_handler() {
        let fake = Arc::new(flui_platform::FakeAccessibility::new());
        let window =
            crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
        let realm = UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false)))
            .expect("realm");

        let render_id = RenderId::new(7);
        let target = AccessibilityNodeId::from(render_id);
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_in_handler = Arc::clone(&invoked);
        let mut node = SemanticsNode::new().with_source_render_id(render_id);
        node.config_mut().add_action(
            SemanticsAction::Tap,
            Arc::new(move |action, _arguments| {
                assert_eq!(action, SemanticsAction::Tap, "Click must arrive as Tap");
                invoked_in_handler.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root = semantics_owner.insert(node);
        semantics_owner.set_root(Some(root));
        realm
            .pipeline_for_test()
            .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

        // An adapter only delivers actions while an AT session is active.
        fake.set_active(true);
        fake.request_action(accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(target.as_u64()),
            data: None,
        });

        assert_eq!(
            invoked.load(Ordering::SeqCst),
            0,
            "platform input must wait for the owner's Idle commit point"
        );
        let report = realm.drain_commands();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.dropped_stale, 0);
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
    }

    /// A typed payload crosses the whole wire: a SetValue request carrying
    /// its text arrives at the node's SetText handler WITH that text. A
    /// payload lost anywhere along the seam turns a screen-reader edit into
    /// an argument-free no-op, which no other test would notice.
    #[test]
    fn platform_action_payload_reaches_the_handler_with_its_arguments() {
        let fake = Arc::new(flui_platform::FakeAccessibility::new());
        let window =
            crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
        let realm = UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false)))
            .expect("realm");

        let render_id = RenderId::new(7);
        let target = AccessibilityNodeId::from(render_id);
        let received = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let mut node = SemanticsNode::new().with_source_render_id(render_id);
        node.config_mut().add_action(
            SemanticsAction::SetText,
            Arc::new(move |_action, arguments| {
                sink.lock().push(arguments);
            }),
        );
        let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
        let root = semantics_owner.insert(node);
        semantics_owner.set_root(Some(root));
        realm
            .pipeline_for_test()
            .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

        fake.set_active(true);
        fake.request_action(accesskit::ActionRequest {
            action: accesskit::Action::SetValue,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(target.as_u64()),
            data: Some(accesskit::ActionData::Value("hello".into())),
        });

        let report = realm.drain_commands();
        assert_eq!(report.invoked, 1);
        assert_eq!(
            received.lock().as_slice(),
            &[Some(flui_semantics::ActionArgs::SetText {
                text: "hello".to_string()
            })],
            "the handler must receive the platform's payload, translated"
        );
    }

    /// Requests the listener cannot route never reach the inbox at all: an
    /// action FLUI has no counterpart for, and the zero node id no
    /// published tree ever exports. Both are traced drops at the platform
    /// seam — a screen reader acting on a stale snapshot is tolerated,
    /// never a panic (issue #684's graceful-drop acceptance).
    #[test]
    fn unroutable_platform_action_requests_are_dropped_at_the_listener() {
        let fake = Arc::new(flui_platform::FakeAccessibility::new());
        let window =
            crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
        let realm = UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false)))
            .expect("realm");

        let render_id = RenderId::new(7);
        let target = AccessibilityNodeId::from(render_id);
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_in_handler = Arc::clone(&invoked);
        let mut node = SemanticsNode::new().with_source_render_id(render_id);
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
            .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

        fake.set_active(true);
        // An action with no FLUI counterpart, addressed to a live node.
        fake.request_action(accesskit::ActionRequest {
            action: accesskit::Action::Expand,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(target.as_u64()),
            data: None,
        });
        // A routable action addressed to the out-of-contract zero id.
        fake.request_action(accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(0),
            data: None,
        });

        let report = realm.drain_commands();
        assert_eq!(
            (report.invoked, report.dropped_stale),
            (0, 0),
            "neither request may even reach the inbox"
        );
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
    }

    /// The activation seam end-to-end (issue #684): assistive technology
    /// attaching may only flip the presentation's host flag (the listener
    /// runs on the adapter's thread) — the next frame's reconcile is what
    /// turns that into pipeline state, creating the semantics owner; and
    /// detaching disposes it the same way.
    #[test]
    fn at_activation_drives_semantics_assembly_through_the_frame_reconcile() {
        let fake = Arc::new(flui_platform::FakeAccessibility::new());
        let window =
            crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
        let realm = UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false)))
            .expect("realm");
        let pipeline = realm.pipeline_for_test();
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));

        assert!(
            !pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
            "assembly is off until assistive technology attaches"
        );

        fake.set_active(true);
        assert!(
            !pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
            "the listener alone must not mutate pipeline state — that is owner-thread work"
        );
        realm.enter(|_| {
            let _ = realm.draw_frame_entered(constraints);
        });
        assert!(
            pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
            "the frame reconcile turns the activation flag into pipeline state"
        );
        assert!(
            pipeline.with(|owner| owner.semantics_owner().is_some()),
            "enabling creates the semantics owner"
        );

        fake.set_active(false);
        realm.enter(|_| {
            let _ = realm.draw_frame_entered(constraints);
        });
        assert!(
            !pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
            "detaching stops assembly on the next frame"
        );
        assert!(
            pipeline.with(|owner| owner.semantics_owner().is_none()),
            "deactivation disposes the owner"
        );
    }

    /// Assistive technology attaching must also request a self-contained
    /// full republish (the adapter's state is unknown, and flushes publish
    /// incrementally — the next diff would answer a fresh screen reader
    /// with a fragment). The listener may only set the flag; the frame
    /// reconcile consumes it on the owner thread and routes it to
    /// `PipelineOwner::request_semantics_full_publish`.
    #[test]
    fn at_activation_requests_a_full_republish_and_the_reconcile_consumes_it() {
        let fake = Arc::new(flui_platform::FakeAccessibility::new());
        let window =
            crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
        let realm = UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false)))
            .expect("realm");
        let host_flag = realm
            .presentations
            .primary()
            .semantics_host()
            .full_republish_handle();
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));

        fake.set_active(true);
        assert!(
            host_flag.load(Ordering::Relaxed),
            "activation must record that the adapter needs a full tree"
        );

        realm.enter(|_| {
            let _ = realm.draw_frame_entered(constraints);
        });
        assert!(
            !host_flag.load(Ordering::Relaxed),
            "the frame reconcile consumes the request exactly once"
        );

        fake.set_active(false);
        realm.enter(|_| {
            let _ = realm.draw_frame_entered(constraints);
        });
        assert!(
            !host_flag.load(Ordering::Relaxed),
            "deactivation must not request a republish nobody will hear"
        );
    }

    /// Teardown withdraws from the platform bridge: after the presentation
    /// closes, an activation flip delivered by the (longer-lived) adapter
    /// must not reach the dead presentation's enablement flag — `close()`
    /// replaced the listener with a no-op. If the withdrawal is removed,
    /// the original listener (still held by the fake) stores `true` into
    /// the flag this test kept a handle on, and the assertion fails.
    #[test]
    fn closing_the_presentation_withdraws_from_the_platform_bridge() {
        let fake = Arc::new(flui_platform::FakeAccessibility::new());
        let window =
            crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
        // Hold the window strong across the drop: `close()`'s withdrawal
        // reaches the bridge through a `Weak` window upgrade, exactly as a
        // production runner (which owns the window) would still succeed.
        let realm = UiRealm::new(
            noop_wake(),
            Arc::clone(&window),
            1.0,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("realm");
        let flag = realm
            .presentations
            .primary()
            .semantics_host()
            .platform_semantics_enabled_handle();

        drop(realm);

        fake.set_active(true);
        assert!(
            !flag.load(Ordering::Relaxed),
            "a closed presentation's enablement flag must never flip again"
        );
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
            .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

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

    /// Issue #555's addressed-routing slice: `drain_commands`'s `SemanticsAction` arm checks
    /// FOREST MEMBERSHIP (`self.presentations.get(presentation_id)`), not
    /// equality against `self.presentations.primary().id()` — a live
    /// NON-primary presentation's own stamped action must actually invoke,
    /// not be dropped as stale just because it is not the primary.
    ///
    /// If reverted (the pre-addressed-routing primary-equality check restored):
    /// this fails — `report.invoked` would be `0` and `dropped_stale` would
    /// be `1`, because B is never the primary in this fixture.
    #[test]
    fn semantics_action_addressed_to_a_live_non_primary_presentation_is_invoked() {
        let mut realm = UiRealm::for_test();
        let b_id = realm.install_second_presentation_for_test();

        let render_id = RenderId::new(1);
        let invoked = Arc::new(AtomicUsize::new(0));
        let invoked_in_handler = Arc::clone(&invoked);
        let mut node = SemanticsNode::new().with_source_render_id(render_id);
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
            .presentations
            .get(b_id)
            .expect("B installed")
            .pipeline()
            .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

        realm
            .command_sender()
            .send(UiCommand::SemanticsAction {
                presentation_id: b_id,
                request: SemanticsActionRequest::new(
                    AccessibilityNodeId::from(render_id),
                    SemanticsAction::Tap,
                ),
            })
            .expect("realm inbox has room");

        let report = realm.drain_commands();
        assert_eq!(
            report.invoked, 1,
            "B's own stamped action must be invoked, not dropped"
        );
        assert_eq!(report.dropped_stale, 0);
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
    }

    /// Issue #555's addressed-routing slice: an action stamped for a presentation that this
    /// realm no longer hosts — closed since the request was enqueued —
    /// drops traced, exactly like a forged/never-existed generation does.
    ///
    /// If reverted the same way as the test above (primary-equality
    /// restored, or the forest-membership check deleted outright): this
    /// still passes vacuously for the WRONG reason (A, the closed
    /// presentation, was never the primary either) — the companion test
    /// above is what actually pins the membership check; this one pins the
    /// closed-presentation half of the same contract.
    #[test]
    fn semantics_action_with_stale_presentation_generation_drops_traced() {
        let mut realm = UiRealm::for_test();
        let a_id = realm.presentation_id();
        let _b_id = realm.install_second_presentation_for_test();

        realm
            .command_sender()
            .send(UiCommand::SemanticsAction {
                presentation_id: a_id,
                request: SemanticsActionRequest::new(
                    AccessibilityNodeId::from(RenderId::new(1)),
                    SemanticsAction::Tap,
                ),
            })
            .expect("realm inbox has room");

        // Close A (the sender's stamped target) while B survives -- A's
        // generation is now gone from the forest entirely.
        realm.close_presentation_entered(a_id);

        let report = realm.drain_commands();
        assert_eq!(
            report.invoked, 0,
            "a request stamped for a presentation this realm no longer hosts must never invoke"
        );
        assert_eq!(report.dropped_stale, 1);
    }

    #[test]
    fn dead_navigation_target_is_dropped_at_commit() {
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

    // ========================================================================
    // Realm coexistence — the criterion-1/2 evidence for singleton retirement.
    // Every process-global graph `UiRealm` used to front (the transitional
    // at-most-one-instance construction guard, `AppBinding`, the `UpdateScheduler`
    // singleton) is gone: these tests prove what that actually buys, rather
    // than asserting the absence of code that no longer exists.
    // ========================================================================

    fn coexistence_constraints() -> BoxConstraints {
        BoxConstraints::tight(Size::new(px(200.0), px(200.0)))
    }

    /// Two realms constructed through the PRODUCTION path (`UiRealm::new`,
    /// via `new_runtime` — not the `for_test` bypass), on ONE thread,
    /// simultaneously live.
    ///
    /// Red at the old shape: resurrecting the deleted at-most-one claim
    /// flag's swap-and-check inside `with_capacity` (an atomic swap
    /// returning early with a now-deleted typed "already exists" error
    /// variant) turns the second `new_runtime` call below into a typed
    /// error instead of a live realm — this test would fail immediately.
    /// What made that guard load-bearing at the old HEAD is also gone:
    /// `UiRealm::construct` resolves its own
    /// `RealmServices` (a fresh `UpdateScheduler` strong root) instead of reaching
    /// a process-global one, so two realms on one thread no longer alias
    /// anything to guard against.
    /// An input-driven redraw request must WAKE the platform loop, not just
    /// set flags: the loop parks between events, and flags nobody pokes it
    /// about are inert — a live drag froze the screen for exactly this
    /// reason (125 pointer moves produced 2 frames; the pointer-routing
    /// path requested a redraw every time, flag-only). The wake is what
    /// converts the request into a `RedrawRequested` and therefore a frame.
    #[test]
    fn a_redraw_request_fires_the_platform_wake() {
        let wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wake_count_in_closure = Arc::clone(&wake_count);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            wake_count_in_closure.fetch_add(1, Ordering::Relaxed);
        });
        let realm = UiRealm::new(
            wake,
            crate::app::presentation::test_platform_window(None),
            1.0,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("test realm construction");

        realm.request_redraw();

        assert!(
            wake_count.load(Ordering::Relaxed) >= 1,
            "request_redraw must fire the platform wake — flag-only requests \
             leave a parked event loop asleep and the screen frozen"
        );
    }

    /// The realm installs a live root `MediaQuery`: the user's subtree can
    /// read it from the very first build, and a platform appearance change
    /// republishes the new brightness through the same root on the next
    /// frame — the wire nothing used to consume.
    #[test]
    fn the_root_media_query_republishes_a_brightness_change() {
        use std::cell::Cell;

        #[derive(Clone)]
        struct BrightnessProbe {
            seen: std::rc::Rc<Cell<Option<flui_types::platform::Brightness>>>,
        }
        impl flui_view::View for BrightnessProbe {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateless(self)
            }
        }
        impl flui_view::StatelessView for BrightnessProbe {
            fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
                self.seen
                    .set(Some(flui_widgets::MediaQuery::of(ctx).platform_brightness));
                SizedBox::new(10.0, 10.0)
            }
        }

        let seen = std::rc::Rc::new(Cell::new(None));
        let realm = new_runtime(noop_wake()).expect("realm claims cleanly");
        realm
            .attach_root_widget_with_size(
                &BrightnessProbe {
                    seen: std::rc::Rc::clone(&seen),
                },
                10.0,
                10.0,
            )
            .expect("mounts under the root MediaQuery");
        let _ = realm.draw_frame(coexistence_constraints());
        assert_eq!(
            seen.get(),
            Some(flui_types::platform::Brightness::Light),
            "the first build reads the installed root MediaQuery (default light)"
        );

        // The appearance arm's write side: mutate the shared source and pump.
        seen.set(None);
        realm.media_query().update(|data| {
            data.platform_brightness = flui_types::platform::Brightness::Dark;
        });
        assert!(
            realm.presentations.primary().widgets().has_pending_builds(),
            "the appearance write's externally-scheduled rebuild must count as \
             dirty work — a frame gate blind to the external inbox skips the \
             very frame the schedule woke"
        );
        let _ = realm.draw_frame(coexistence_constraints());
        assert_eq!(
            seen.get(),
            Some(flui_types::platform::Brightness::Dark),
            "an appearance change republishes through the root on the next frame"
        );
    }

    #[test]
    fn two_realms_coexist_same_thread() {
        use std::cell::Cell;

        let realm_a = new_runtime(noop_wake()).expect("realm A claims cleanly");
        let realm_b = new_runtime(noop_wake())
            .expect("realm B claims cleanly ALONGSIDE realm A, same thread");

        // Disjoint identities.
        assert_ne!(
            realm_a.realm_id(),
            realm_b.realm_id(),
            "two live realms must never compare equal"
        );

        // Disjoint focus tree and disjoint PipelineOwner (the container a
        // SemanticsOwner is set on, `pipeline.write().set_semantics_owner`)
        // -- neither realm's focus dispatch nor its semantics state can be
        // the other's Rc/Arc.
        assert!(
            !Rc::ptr_eq(&realm_a.focus_manager(), &realm_b.focus_manager()),
            "two realms must never share one focus tree"
        );
        assert!(
            !realm_a
                .pipeline_for_test()
                .ptr_eq(&realm_b.pipeline_for_test()),
            "two realms must never share one PipelineOwner (and therefore never one SemanticsOwner)"
        );

        // Independent widget mounts: A gets a root; B stays unattached, so
        // A's mount must not leak into B's own WidgetsBinding.
        realm_a
            .attach_root_widget_with_size(&SizedBox::new(50.0, 50.0), 50.0, 50.0)
            .expect("realm A mounts its own root");
        assert!(
            realm_a.draw_frame(coexistence_constraints()).is_some(),
            "realm A produced a scene from its own mounted root"
        );
        assert!(
            realm_b.draw_frame(coexistence_constraints()).is_none(),
            "realm B has no root attached; realm A's mount must not leak into it"
        );

        // Independent scheduler phases: drive realm A's frame transaction
        // and observe realm B's scheduler from inside it, mid-frame.
        let phase_a_mid_frame = Cell::new(None);
        let phase_b_mid_frame = Cell::new(None);
        realm_a.scheduler().drive_frame_with_lane(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {
                phase_a_mid_frame.set(Some(realm_a.scheduler().phase()));
                phase_b_mid_frame.set(Some(realm_b.scheduler().phase()));
                let _ = realm_a.draw_frame(coexistence_constraints());
            },
            realm_a.local_post_frame_lane(),
        );
        assert_ne!(
            phase_a_mid_frame.get(),
            Some(SchedulerPhase::Idle),
            "realm A's own scheduler must be mid-frame while its pipeline runs"
        );
        assert_eq!(
            phase_b_mid_frame.get(),
            Some(SchedulerPhase::Idle),
            "realm B's scheduler must stay Idle while realm A alone is driving a frame"
        );

        // Independent gesture arenas: a route registered on A must never
        // fire for input dispatched into B.
        use flui_interaction::PointerId;
        use flui_interaction::events::{PointerType, make_down_event_for_id};
        use flui_interaction::routing::PointerRouteHandler;
        use flui_types::geometry::Pixels;

        let pointer = PointerId::new(9002).expect("nonzero pointer id");
        let position = flui_types::Offset::new(Pixels(10.0), Pixels(10.0));
        let fired = Rc::new(Cell::new(0));
        let fired_by_route = Rc::clone(&fired);
        let handler: PointerRouteHandler = Rc::new(move |_| {
            fired_by_route.set(fired_by_route.get() + 1);
        });
        realm_a
            .gestures()
            .pointer_router()
            .add_route(pointer, handler);

        let down_b = make_down_event_for_id(pointer, position, PointerType::Touch);
        realm_b.enter(|realm| {
            realm.handle_input_entered(PlatformInput::Pointer(down_b));
        });
        assert_eq!(
            fired.get(),
            0,
            "a route registered on realm A must not fire for realm B's input"
        );
        assert_eq!(realm_b.gestures().active_pointer_count(), 1);
        assert_eq!(realm_a.gestures().active_pointer_count(), 0);
    }

    /// Realm A on the test's own thread, realm B constructed, driven, and
    /// dropped entirely on a SECOND thread — proving there is no shared
    /// mutable state to race on, not merely that construction succeeds.
    /// `UiRealm` stays `!Send` throughout: `realm_b` never crosses the
    /// thread boundary, only its `Send + Sync` wake counter and plain
    /// `RealmId` do, via the join return value.
    ///
    /// A barrier released BEFORE either side's frame work only proves both
    /// threads STARTED around the same time — the OS scheduler is free to
    /// run one side's whole `draw_frame` to completion before the other
    /// even resumes, so the frame TRANSACTIONS themselves might never
    /// actually overlap. The rendezvous below fixes that by sitting INSIDE
    /// each realm's own `drive_frame` pipeline closure — which
    /// `UpdateScheduler::handle_draw_frame` guarantees runs during
    /// `SchedulerPhase::PersistentCallbacks` (see
    /// `the_production_frame_polls_the_realms_async_driver_once_before_the_pipeline`)
    /// — so neither closure can proceed past the rendezvous until BOTH
    /// realms are provably mid-transaction at the same instant.
    /// `std::sync::Barrier` has no timeout, so a regression that stops one
    /// side from ever reaching its frame closure would hang the test
    /// forever instead of failing it; `rendezvous_or_timeout` below fails
    /// loudly on a bounded deadline instead of deadlocking.
    #[test]
    fn two_realms_two_threads_no_shared_state() {
        let parties_arrived = std::sync::atomic::AtomicUsize::new(0);
        let rendezvous_or_timeout = |label: &'static str| {
            parties_arrived.fetch_add(1, Ordering::SeqCst);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while parties_arrived.load(Ordering::SeqCst) < 2 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "{label}: rendezvous timed out -- the other realm's frame \
                     transaction never became concurrently mid-flight"
                );
                std::thread::yield_now();
            }
        };

        let (wake_a, wakes_a) = counting_wake();
        let realm_a = new_runtime(wake_a).expect("realm A claims cleanly on this thread");
        let sender_a = realm_a.command_sender();

        let (wakes_b, realm_b_id) = std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let (wake_b, wakes_b) = counting_wake();
                let realm_b = new_runtime(wake_b)
                    .expect("realm B claims cleanly on its OWN thread, concurrently with A");
                let sender_b = realm_b.command_sender();

                sender_b.request_redraw();
                let _ = realm_b.drain_commands();
                realm_b.scheduler().drive_frame_with_lane(
                    flui_scheduler::Instant::now(),
                    flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                    || {
                        // Mid-PersistentCallbacks rendezvous: cannot return
                        // until realm A's own closure below has ALSO
                        // reached this point.
                        rendezvous_or_timeout("realm B");
                        let _ = realm_b.draw_frame(coexistence_constraints());
                    },
                    realm_b.local_post_frame_lane(),
                );
                (wakes_b.load(Ordering::Relaxed), realm_b.realm_id())
            });

            sender_a.request_redraw();
            let _ = realm_a.drain_commands();
            realm_a.scheduler().drive_frame_with_lane(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {
                    rendezvous_or_timeout("realm A");
                    let _ = realm_a.draw_frame(coexistence_constraints());
                },
                realm_a.local_post_frame_lane(),
            );

            handle.join().expect("realm B's thread did not panic")
        });

        assert_eq!(
            wakes_a.load(Ordering::Relaxed),
            1,
            "realm A's wake counter reflects only its own request"
        );
        assert_eq!(
            wakes_b, 1,
            "realm B's wake counter reflects only its own request, made on its own thread"
        );
        assert_ne!(realm_a.realm_id(), realm_b_id);
    }

    /// Extends `dropped_runtime_yields_owner_gone`'s single-realm shape
    /// (`UiRealmError`/`CommandSendError::OwnerGone`) across two coexisting
    /// realms: dropping realm A must leave realm B's wake counter and inbox
    /// completely untouched, and A's own senders must turn `OwnerGone`
    /// rather than silently reaching B.
    #[test]
    fn dropping_realm_a_cannot_wake_realm_b() {
        // Realm A's own wake counter has nothing left to assert once A is
        // dropped below (its `wake` closure can never fire again); only
        // realm B's counter is the interesting observable here.
        let (wake_a, _wakes_a) = counting_wake();
        let (wake_b, wakes_b) = counting_wake();
        let realm_a = new_runtime(wake_a).expect("realm A");
        let realm_b = new_runtime(wake_b).expect("realm B, alongside realm A");

        let sender_a = realm_a.command_sender();
        let realm_b_id_before = realm_b.realm_id();

        drop(realm_a);

        let navigator = NavigatorHandle::new();
        navigator.seed_initial(test_route("/"));
        assert!(
            matches!(
                sender_a.send_navigation(NavigatorCommand::maybe_pop(navigator.command_target())),
                Err(CommandSendError::OwnerGone { .. })
            ),
            "a sender into the dropped realm A must turn OwnerGone"
        );
        assert_eq!(
            wakes_b.load(Ordering::Relaxed),
            0,
            "dropping realm A must not wake realm B"
        );
        assert_eq!(
            realm_b.realm_id(),
            realm_b_id_before,
            "realm B is unaffected by realm A's drop"
        );

        // realm B's own inbox still drains normally — dropping a SIBLING
        // realm leaves it fully live, not merely non-crashed.
        let sender_b = realm_b.command_sender();
        let navigator_b = NavigatorHandle::new();
        navigator_b.seed_initial(test_route("/"));
        sender_b
            .send_navigation(NavigatorCommand::maybe_pop(navigator_b.command_target()))
            .expect("realm B's inbox has room");
        let report = realm_b.drain_commands();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.dropped_stale, 0);

        assert_eq!(
            wakes_b.load(Ordering::Relaxed),
            1,
            "only realm B's own command may wake realm B"
        );
        assert_eq!(
            realm_b.gestures().active_pointer_count(),
            0,
            "...nor its gesture arena"
        );
    }

    /// The sharpest widget-state isolation probe (per ADR-0027 §8: GlobalKey
    /// is realm-scoped, not process-global). Within ONE realm, mounting a
    /// second element under an already-registered `GlobalKey` value panics
    /// eagerly (`register_global_key_with_collision_check`,
    /// `element_tree.rs`) — that is FLUI's documented divergence from
    /// Flutter's end-of-frame duplicate detection. Mounting the SAME key
    /// VALUE as a root in two DIFFERENT realms must succeed in both: each
    /// realm's `WidgetsBinding` owns its own `ElementOwner`/registry, and the
    /// collision check is scoped to the currently-active one, never to the
    /// key's numeric id process-wide.
    ///
    /// Both realms go through the PRODUCTION constructor (`UiRealm::new`,
    /// via `new_runtime`), matching every other coexistence proof in this
    /// module — not the `for_test` bypass, so this test exercises the exact
    /// path a real embedder would.
    #[test]
    fn cross_realm_duplicate_global_key_mounts_succeed_in_both() {
        #[derive(Clone)]
        struct GlobalKeyedRootView {
            key: flui_view::GlobalKey<Self>,
        }

        impl flui_view::RenderView for GlobalKeyedRootView {
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
            ) -> flui_rendering::RenderUpdateImpact {
                render_object.set_size(
                    Some(flui_types::Pixels::ZERO),
                    Some(flui_types::Pixels::ZERO),
                )
            }
        }

        impl flui_view::View for GlobalKeyedRootView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::render_variable(self)
            }

            fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
                Some(&self.key)
            }
        }

        let shared_key = flui_view::GlobalKey::<GlobalKeyedRootView>::new();
        let realm_a = new_runtime(noop_wake()).expect("realm A claims cleanly");
        let realm_b = new_runtime(noop_wake()).expect("realm B claims cleanly ALONGSIDE realm A");

        realm_a
            .attach_root_widget_with_size(
                &GlobalKeyedRootView {
                    key: shared_key.clone(),
                },
                10.0,
                10.0,
            )
            .expect("realm A mounts the keyed root");
        let _ = realm_a.draw_frame(coexistence_constraints());

        // Realm B wraps the SAME keyed view one layer deeper (a `SizedBox`
        // ancestor realm A's mount doesn't have) -- deliberately, so the two
        // trees are NOT structurally identical. A bare identical-shape mount
        // in both realms would legitimately allocate the same local slab
        // index in each realm's independent `ElementTree`, which would make
        // an `ElementId` equality/inequality assertion below pure
        // coincidence either way, discriminating nothing. With the shapes
        // different, a numeric match WOULD be suspicious.
        realm_b
            .attach_root_widget_with_size(
                &SizedBox::new(10.0, 10.0).child(GlobalKeyedRootView {
                    key: shared_key.clone(),
                }),
                10.0,
                10.0,
            )
            .expect(
                "realm B mounts the SAME GlobalKey value without a duplicate-attachment \
                 panic -- the eager collision check is scoped to one realm's own \
                 ElementOwner, never process-wide",
            );
        let _ = realm_b.draw_frame(coexistence_constraints());

        let element_in_a = realm_a.enter(|_| shared_key.current_element());
        let element_in_b = realm_b.enter(|_| shared_key.current_element());

        assert!(element_in_a.is_some(), "realm A resolves its own mount");
        assert!(element_in_b.is_some(), "realm B resolves its own mount");
        assert_ne!(
            element_in_a, element_in_b,
            "each realm mounted its OWN distinct element under the same key value -- \
             structurally guaranteed distinct here (B's tree has one extra ancestor \
             element A's doesn't), not a coincidence of matching slab layouts"
        );

        // The distinctness above is a snapshot; the deeper isolation proof
        // is behavioral: tear realm A down entirely and confirm realm B's
        // registration is completely unaffected -- if the registries were
        // secretly shared, dropping A's `ElementOwner` would clear or
        // corrupt B's entry too.
        drop(realm_a);
        let element_in_b_after_a_dropped = realm_b.enter(|_| shared_key.current_element());
        assert_eq!(
            element_in_b_after_a_dropped, element_in_b,
            "realm B's registration under the shared key value must survive \
             realm A's teardown completely untouched"
        );
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
        use std::cell::Cell;
        use std::sync::atomic::{AtomicBool as StdAtomicBool, AtomicUsize};

        use flui_engine::EngineError;
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
            ) -> flui_rendering::RenderUpdateImpact {
                render_object.set_size(
                    Some(flui_types::Pixels::ZERO),
                    Some(flui_types::Pixels::ZERO),
                )
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

            let id = pipeline.with_mut(|owner| {
                owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >)
            });
            pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
            realm.mark_rendered();

            pipeline.with_mut(|owner| owner.mark_needs_layout(id));
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

            let handle = realm.pipeline_for_test().with(PipelineOwner::handle);
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
                realm
                    .pipeline_for_test()
                    .with(|owner| owner.root_id().is_some()),
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

            realm.pipeline_for_test().with(|owner| {
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
            });
        }

        /// Wiring test: `draw_frame` must invoke
        /// `WidgetsBinding::service_child_requests`, which drains the
        /// pipeline's `pending_child_requests` buffer.
        #[test]
        fn draw_frame_invokes_service_child_requests() {
            let realm = UiRealm::for_test();
            let pipeline = realm.pipeline_for_test();

            let sliver_id = pipeline.with_mut(|owner| {
                owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >)
            });
            pipeline.with_mut(|owner| owner.push_pending_child_request_for_test(sliver_id, 0));
            pipeline.with_mut(|owner| {
                let drained = owner.take_pending_child_requests();
                assert_eq!(drained.len(), 1, "seed must be present before draw_frame");
                owner.push_pending_child_request_for_test(sliver_id, 0);
            });

            let _ = realm.draw_frame(test_constraints());

            let remaining = pipeline.with_mut(PipelineOwner::take_pending_child_requests);
            assert!(
                remaining.is_empty(),
                "draw_frame must drain pending_child_requests via service_child_requests; \
                 {} request(s) remained undrained — wiring is absent",
                remaining.len(),
            );
        }

        /// The production frame path polls the async driver **exactly once**,
        /// on the realm's own scheduler, in the mid-frame slot — and the
        /// pipeline runs afterwards, in the persistent slot.
        #[test]
        fn the_production_frame_polls_the_realms_async_driver_once_before_the_pipeline() {
            let realm = UiRealm::for_test();
            let scheduler = realm.scheduler();

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

            scheduler.drive_frame_with_lane(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {
                    flag.store(polls_probe.load(Ordering::Acquire) == 1, Ordering::Release);
                    let _ = realm.draw_frame(test_constraints());
                },
                realm.local_post_frame_lane(),
            );

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
            let realm = UiRealm::for_test();
            let ran = Arc::new(StdAtomicBool::new(false));
            let ran_for_task = Arc::clone(&ran);
            let _token = realm.scheduler().spawn_local(Box::pin(async move {
                ran_for_task.store(true, Ordering::Release);
            }));

            let _ = realm.draw_frame(test_constraints());

            assert!(
                !ran.load(Ordering::Acquire),
                "the driver step belongs to UpdateScheduler::handle_begin_frame, not to the pipeline"
            );
        }

        /// **The production-path acceptance test.** A post-frame callback on
        /// the realm's own scheduler observes the geometry this realm's real
        /// pipeline committed **in the same frame**.
        #[test]
        fn production_post_frame_callback_observes_this_frames_committed_layout() {
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

            let root = pipeline.with_mut(|owner| {
                let root =
                    owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(FixedBox));
                owner.set_root_id(Some(root));
                root
            });

            assert_eq!(
                pipeline.with(|owner| owner.box_size(root)),
                None,
                "nothing is laid out before the first frame"
            );

            let observed = Arc::new(RwLock::new(None));
            let calls = Arc::new(AtomicUsize::new(0));
            let observed_cb = Arc::clone(&observed);
            let calls_cb = Arc::clone(&calls);
            let pipeline_cb = pipeline.clone();

            let scheduler = realm.scheduler();
            // `PipelineCell` is `!Send`, so this callback cannot go through
            // `add_post_frame_callback` (its `Send` bound is for cross-thread
            // wake). `LocalPostFrameHandle::schedule_local` accepts it
            // instead, matching the `editable_text.rs` IME cursor-loop
            // pattern.
            let post_frame_handle = realm
                .widgets()
                .with_build_owner(|owner| owner.local_post_frame_handle().cloned())
                .expect("owner-local post-frame handle installed by UiRealm::construct");
            // The handle addresses its lane directly (a `Weak` pointer), so
            // registration needs no `enter()` at all — only the drive itself
            // needs the lane, passed explicitly to `drive_frame_with_lane`
            // (drain-by-parameter, not an ambient "active lane" lookup).
            post_frame_handle
                .schedule_local(move |_timing| {
                    calls_cb.fetch_add(1, Ordering::SeqCst);
                    *observed_cb.write() = pipeline_cb.with(|owner| owner.box_size(root));
                })
                .expect("the realm's local post-frame lane outlives this call");

            realm.enter(|realm| {
                scheduler.drive_frame_with_lane(
                    flui_scheduler::Instant::now(),
                    flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                    || {
                        let _ = realm.draw_frame(BoxConstraints::new(
                            px(0.0),
                            px(200.0),
                            px(0.0),
                            px(200.0),
                        ));
                    },
                    &realm.local_post_frame,
                );
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

            let node_id = pipeline.with_mut(|owner| {
                owner.insert(Box::new(flui_objects::RenderColoredBox::red(10.0, 10.0))
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >)
            });
            pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
            assert!(
                !realm.has_pending_work(),
                "baseline: no pending work after clearing dirty nodes",
            );

            pipeline.with_mut(|owner| owner.mark_needs_layout(node_id));
            assert!(
                realm.has_pending_work(),
                "a dirty layout node must make has_pending_work() true so the runner \
                 schedules the settling frame",
            );

            pipeline.with_mut(PipelineOwner::clear_all_dirty_nodes);
            assert!(
                !realm.has_pending_work(),
                "after clearing dirty nodes has_pending_work() must be false so a \
                 settled lazy-list app does not loop forever",
            );
        }

        // ---- Input lifecycle gate --------------------------------------------

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

        /// Exhaustively pins [`input_dropped_by_lifecycle`]'s policy over
        /// every `(lifecycle, input kind)` pair, including `DragDrop`.
        ///
        /// If reverted: restore a `matches!(input, PlatformInput::Pointer(_))`
        /// check in place of the exhaustive match and the `Suspended` +
        /// `DragDrop` case fails (`assert!` sees `false` instead of `true`).
        #[test]
        fn input_lifecycle_gate_is_exhaustive_and_explicit() {
            use flui_interaction::events::{PointerType, make_down_event};
            use flui_interaction::testing::input::KeyEventBuilder;
            use flui_platform::traits::DragDropEvent;

            use super::super::super::presentation::PresentationLifecycle;

            let pointer = PlatformInput::Pointer(make_down_event(
                flui_types::Offset::new(px(0.0), px(0.0)),
                PointerType::Mouse,
            ));
            let keyboard = PlatformInput::Keyboard(
                KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build(),
            );
            let ime = PlatformInput::Ime(flui_types::ImeEvent::Commit("x".to_string()));
            let drag_drop = PlatformInput::DragDrop(DragDropEvent::Exited {
                id: flui_foundation::DataTransferId::new(1),
            });
            let every_kind = [&pointer, &keyboard, &ime, &drag_drop];

            for lifecycle in [
                PresentationLifecycle::Created,
                PresentationLifecycle::Closing,
                PresentationLifecycle::Closed,
            ] {
                for input in every_kind {
                    assert!(
                        input_dropped_by_lifecycle(lifecycle, input),
                        "{lifecycle:?} must reject every input kind, including {input:?}"
                    );
                }
            }

            assert!(input_dropped_by_lifecycle(
                PresentationLifecycle::Suspended,
                &pointer
            ));
            assert!(
                input_dropped_by_lifecycle(PresentationLifecycle::Suspended, &drag_drop),
                "DragDrop must be dropped while suspended, the same as Pointer"
            );
            assert!(!input_dropped_by_lifecycle(
                PresentationLifecycle::Suspended,
                &keyboard
            ));
            assert!(!input_dropped_by_lifecycle(
                PresentationLifecycle::Suspended,
                &ime
            ));

            for input in every_kind {
                assert!(!input_dropped_by_lifecycle(
                    PresentationLifecycle::SurfaceAttached,
                    input
                ));
            }
        }

        // ---- Gesture-arena / pointer dispatch --------------------------------

        /// Shell auto-wrap regression: the `GestureArenaScope` `attach_root_widget*`
        /// installs around the root must put every `GestureDetector` in the
        /// gesture binding's ONE shared arena, so two nested detectors on the
        /// same hit-test path resolve to exactly one winner (Flutter parity:
        /// front member wins, loser is rejected). Without the shell wrap each
        /// detector falls back to a private arena it closes itself, and the
        /// same tap fires BOTH callbacks.
        ///
        /// Drives the exact production path, with no manually mounted scope:
        /// `attach_root_widget_with_size` (what every runner's bootstrap
        /// calls) for the mount, `draw_frame` for layout, and
        /// `handle_input_entered` (what every platform input callback calls
        /// after entering a realm) for the pointer stream.
        #[test]
        fn shell_installed_arena_resolves_nested_tap_detectors_to_one_winner() {
            use flui_interaction::events::{PointerType, make_down_event, make_up_event};
            use flui_types::Color;
            use flui_widgets::{ColoredBox, GestureDetector};

            let realm = UiRealm::for_test();

            let inner_taps = Arc::new(AtomicUsize::new(0));
            let outer_taps = Arc::new(AtomicUsize::new(0));
            let inner = Arc::clone(&inner_taps);
            let outer = Arc::clone(&outer_taps);

            let root = GestureDetector::new()
                .on_tap(move || {
                    outer.fetch_add(1, Ordering::SeqCst);
                })
                .child(
                    GestureDetector::new()
                        .on_tap(move || {
                            inner.fetch_add(1, Ordering::SeqCst);
                        })
                        .child(ColoredBox::new(Color::rgb(10, 20, 30))),
                );

            realm
                .enter(|realm| realm.attach_root_widget_with_size(&root, 100.0, 100.0))
                .expect("attach succeeds");
            let _ = realm.draw_frame(test_constraints());

            // Production input arrives inside the realm (runner.rs's
            // PlatformToUi dispatch enters it before calling handle_input),
            // so the synthetic tap does the same.
            let position = flui_types::Offset::new(px(50.0), px(50.0));
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
                    position,
                    PointerType::Mouse,
                )));
                realm.handle_input_entered(PlatformInput::Pointer(make_up_event(
                    position,
                    PointerType::Mouse,
                )));
            });

            assert_eq!(
                inner_taps.load(Ordering::SeqCst),
                1,
                "the inner tap recognizer is the arena's front member and wins",
            );
            assert_eq!(
                outer_taps.load(Ordering::SeqCst),
                0,
                "the outer tap recognizer shares the arena and must be rejected — \
                 if both fire, each detector built its own private arena (no shell \
                 GestureArenaScope above the root)",
            );
        }

        /// Same auto-wrap invariant as
        /// `shell_installed_arena_resolves_nested_tap_detectors_to_one_winner`,
        /// through `attach_root_widget` (the unsized variant) and asserting
        /// on the arena's `SweepModel` directly.
        #[test]
        fn root_gesture_scope_arbitrates_overlapping_detectors_once() {
            use flui_interaction::arena::SweepModel;
            use flui_interaction::events::{PointerType, make_down_event, make_up_event};
            use flui_types::geometry::{Offset, Pixels};
            use flui_widgets::{GestureDetector, HitTestBehavior, SizedBox};

            let realm = UiRealm::for_test();
            let outer_taps = Rc::new(Cell::new(0));
            let inner_taps = Rc::new(Cell::new(0));

            let inner_count = Rc::clone(&inner_taps);
            let inner = GestureDetector::new()
                .on_tap(move || inner_count.set(inner_count.get() + 1))
                .behavior(HitTestBehavior::Opaque)
                .child(SizedBox::new(100.0, 100.0));
            let outer_count = Rc::clone(&outer_taps);
            let root = GestureDetector::new()
                .on_tap(move || outer_count.set(outer_count.get() + 1))
                .behavior(HitTestBehavior::Opaque)
                .child(inner);

            realm
                .attach_root_widget(&root)
                .expect("a fresh realm must attach the detector tree");
            let _ = realm.draw_frame(test_constraints());

            let position = Offset::new(Pixels(10.0), Pixels(10.0));
            let down = make_down_event(position, PointerType::Touch);
            let up = make_up_event(position, PointerType::Touch);
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(down));
                realm.handle_input_entered(PlatformInput::Pointer(up));
            });

            assert_eq!(
                outer_taps.get() + inner_taps.get(),
                1,
                "overlapping detectors must compete in one arena; private arenas let both taps fire"
            );
            assert_eq!(
                realm.gestures().arena().sweep_model(),
                SweepModel::BindingDriven,
                "the root scope must expose the production binding-owned arena"
            );
            assert_eq!(realm.gestures().active_pointer_count(), 0);
        }

        /// Two independently constructed realms must never observe each
        /// other's gesture-router registrations or arena state — the
        /// per-realm isolation `UiRealm::for_test()` (a fully independent
        /// pipeline/presentation/gesture-binding triple) is supposed to give.
        #[test]
        fn realm_input_dispatch_keeps_gesture_state_isolated() {
            use flui_interaction::PointerId;
            use flui_interaction::events::{
                PointerType, make_down_event_for_id, make_up_event_for_id,
            };
            use flui_interaction::routing::PointerRouteHandler;
            use flui_types::geometry::{Offset, Pixels};

            let realm_a = UiRealm::for_test();
            let realm_b = UiRealm::for_test();
            let pointer = PointerId::new(9001).expect("nonzero pointer id");
            let position = Offset::new(Pixels(10.0), Pixels(10.0));

            let fired = Rc::new(Cell::new(0));
            let fired_by_route = Rc::clone(&fired);
            let handler: PointerRouteHandler = Rc::new(move |_| {
                fired_by_route.set(fired_by_route.get() + 1);
            });
            realm_a
                .gestures()
                .pointer_router()
                .add_route(pointer, handler);

            let down_b = make_down_event_for_id(pointer, position, PointerType::Touch);
            realm_b.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(down_b));
            });

            assert_eq!(
                fired.get(),
                0,
                "a route registered in realm A must not observe realm B input"
            );
            assert_eq!(realm_a.gestures().active_pointer_count(), 0);
            assert_eq!(realm_b.gestures().active_pointer_count(), 1);

            let down_a = make_down_event_for_id(pointer, position, PointerType::Touch);
            realm_a.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(down_a));
            });

            assert_eq!(
                fired.get(),
                1,
                "realm A must dispatch through its own router"
            );
            assert_eq!(realm_a.gestures().active_pointer_count(), 1);
            assert_eq!(realm_b.gestures().active_pointer_count(), 1);

            let up_b = make_up_event_for_id(pointer, position, PointerType::Touch);
            realm_b.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(up_b));
            });
            let up_a = make_up_event_for_id(pointer, position, PointerType::Touch);
            realm_a.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(up_a));
            });

            realm_a
                .gestures()
                .pointer_router()
                .remove_all_routes(pointer);
            assert_eq!(realm_a.gestures().active_pointer_count(), 0);
            assert_eq!(realm_b.gestures().active_pointer_count(), 0);
        }

        struct CountingArenaAcceptance(Arc<AtomicU64>);

        impl flui_interaction::sealed::CustomGestureRecognizer for CountingArenaAcceptance {
            fn on_arena_accept(&self, _pointer: flui_interaction::PointerId) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }

            fn on_arena_reject(&self, _pointer: flui_interaction::PointerId) {}
        }

        /// A lone arena member that accepts on `Down` must be swept and
        /// drained by the SAME `handle_input_entered` call, leaving the
        /// arena empty — no deferred second pass required for the
        /// one-member case.
        #[test]
        fn pointer_input_boundary_drains_a_lone_deferred_winner() {
            use flui_interaction::events::{PointerType, make_down_event_for_id};
            use flui_interaction::routing::PointerRouteHandler;
            use flui_types::geometry::{Offset, Pixels};

            let realm = UiRealm::for_test();
            let pointer = flui_interaction::PointerId::new(9002).expect("nonzero pointer id");
            let accepted = Arc::new(AtomicU64::new(0));
            let arena = realm.gestures().arena().clone();
            let accepted_by_member = Arc::clone(&accepted);
            let handler: PointerRouteHandler = Rc::new(move |event| {
                if matches!(event, flui_interaction::PointerEvent::Down(_)) {
                    arena.add(
                        pointer,
                        Arc::new(CountingArenaAcceptance(Arc::clone(&accepted_by_member))),
                    );
                }
            });
            realm
                .gestures()
                .pointer_router()
                .add_route(pointer, Rc::clone(&handler));

            let down = make_down_event_for_id(
                pointer,
                Offset::new(Pixels(10.0), Pixels(10.0)),
                PointerType::Touch,
            );
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(down));
            });

            assert_eq!(accepted.load(Ordering::SeqCst), 1);
            assert!(realm.gestures().arena().is_empty());
            realm
                .gestures()
                .pointer_router()
                .remove_route(pointer, &handler);
        }

        /// Deadline keep-alive regression: a long-press armed by a pointer
        /// down must fire at its 500ms deadline even when NO further pointer
        /// events or redraw requests occur — the frame loop must keep
        /// producing frames while a recognizer deadline is pending (each
        /// frame's deadline tick re-requests the next), instead of going
        /// idle once the down event's own frames drain. Without the
        /// keep-alive the deadline tick never runs at the deadline and the
        /// gesture never fires.
        ///
        /// Simulates the runner's render loop against the real
        /// `SystemClock`-driven gesture arena — consume `needs_redraw`, draw
        /// a frame, repeat — after mounting through the production shell
        /// path (`attach_root_widget_with_size`) and delivering the down
        /// through `handle_input_entered`. The assertion is "fires at all",
        /// never "fires on time", so a loaded CI machine cannot flake it.
        ///
        /// If reverted: remove the `self.gestures().tick_deadlines()` call
        /// from `draw_frame_entered` and this fails — the frame loop below
        /// spins on `needs_redraw()`/`draw_frame()` forever with no deadline
        /// tick ever advancing the recognizer, hitting the 5s timeout.
        #[test]
        fn long_press_fires_at_its_deadline_with_no_further_input() {
            use std::time::{Duration, Instant as StdInstant};

            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::Color;
            use flui_widgets::{ColoredBox, GestureDetector};

            let realm = UiRealm::for_test();

            let presses = Arc::new(AtomicUsize::new(0));
            let in_cb = Arc::clone(&presses);
            let root = GestureDetector::new()
                .on_long_press(move || {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(ColoredBox::new(Color::rgb(10, 20, 30)));

            realm
                .enter(|realm| realm.attach_root_widget_with_size(&root, 100.0, 100.0))
                .expect("attach succeeds");
            let constraints = test_constraints();
            let _ = realm.draw_frame(constraints);

            // Contact down — and nothing else, ever after. Production input
            // arrives inside the realm (runner.rs's RealmEvent dispatch
            // enters it before calling handle_input), so the synthetic down
            // does the same.
            let position = flui_types::Offset::new(px(50.0), px(50.0));
            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Pointer(make_down_event(
                    position,
                    PointerType::Mouse,
                )));
            });

            // Simulated runner loop: the ONLY frame source is
            // `needs_redraw`, consumed the way the runner consumes it
            // (observe, render). The default long-press timeout is 500ms;
            // the cap is generous so the pass/fail signal is purely "did the
            // deadline ever fire".
            let deadline = StdInstant::now() + Duration::from_secs(5);
            while presses.load(Ordering::SeqCst) == 0 {
                assert!(
                    StdInstant::now() < deadline,
                    "long-press never fired: the frame loop went idle with an armed \
                     recognizer deadline (no keep-alive frame was requested)",
                );
                if realm.needs_redraw() {
                    realm.mark_rendered();
                    let _ = realm.draw_frame(constraints);
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }

            assert_eq!(
                presses.load(Ordering::SeqCst),
                1,
                "the held long-press fires exactly once",
            );
        }

        /// Resampled (manually clocked) pointer motion that has not yet been
        /// assigned a frame timestamp must keep `has_pending_work()` true —
        /// the wake-gate half of the same deadline/motion keep-alive
        /// contract `long_press_fires_at_its_deadline_with_no_further_input`
        /// exercises for deadlines.
        #[test]
        fn resampled_contact_motion_keeps_the_frame_wake_gate_open() {
            use std::time::Duration;

            use flui_interaction::{
                events::{PointerType, make_down_event, make_move_event, make_up_event},
                processing::SamplingClock,
                routing::HitTestResult,
            };
            use flui_types::geometry::{Offset, Pixels};

            let realm = UiRealm::for_test();
            realm.mark_rendered();
            realm
                .pipeline_for_test()
                .with_mut(PipelineOwner::clear_all_dirty_nodes);

            realm.enter(|realm| {
                realm
                    .gestures()
                    .set_resampling_enabled(true)
                    .expect("test configures sampling before Down");
                realm.gestures().set_sampling_clock(SamplingClock::Manual {
                    period: Duration::from_millis(8),
                });

                let position = Offset::new(Pixels(8.0), Pixels(13.0));
                realm
                    .gestures()
                    .handle_pointer_event(&make_down_event(position, PointerType::Touch), |_| {
                        HitTestResult::new()
                    });
                realm
                    .gestures()
                    .handle_pointer_event(&make_move_event(position, PointerType::Touch), |_| {
                        HitTestResult::new()
                    });

                assert_eq!(
                    realm.gestures().flush_pending_moves(),
                    0,
                    "manual sampling has no implicit frame timestamp"
                );
                assert!(realm.gestures().has_pending_motion());
                assert!(
                    realm.has_pending_work(),
                    "a sequence-owned sample waiting for frame time must keep the runner awake"
                );

                realm
                    .gestures()
                    .handle_pointer_event(&make_up_event(position, PointerType::Touch), |_| {
                        HitTestResult::new()
                    });
                assert!(!realm.gestures().has_pending_motion());
            });
        }

        // ---- Vsync wiring (production frame continuation) -------------------

        fn make_controller(duration_ms: u64) -> flui_animation::AnimationController {
            use std::time::Duration;
            flui_animation::AnimationController::new(
                Duration::from_millis(duration_ms),
                &flui_scheduler::UpdateScheduler::new(),
            )
        }

        /// V1 — Frame continuation (the key test): a running controller
        /// registered in the realm's Vsync must keep the runner gate
        /// schedulable across every mid-animation frame, and the gate must
        /// go idle once the controller completes.
        ///
        /// Drives `render_frame_entered` (a fresh `ScriptedRasterBackend`
        /// per call, since it is single-shot), not the bare `draw_frame`
        /// this test used before — the continuation wake this test pins is
        /// now raised in `render_frame_entered`, AFTER `mark_rendered()`
        /// runs, not inside `draw_frame_entered` (a reviewer
        /// probe found that raising it before `mark_rendered()` let the
        /// SAME callback's own `mark_rendered()` clobber it, silently
        /// stalling a controller with no other tree-visible effect on the
        /// real desktop path — see `render_frame_entered`'s own comment).
        /// `draw_frame` alone no longer implies a continuation wake by
        /// design; only `render_frame_entered` does, matching what
        /// production always actually calls.
        #[test]
        fn vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle() {
            use flui_animation::{Animation, AnimationStatus};

            let realm = UiRealm::for_test();
            let vsync = realm.vsync();

            let controller = make_controller(100);
            vsync.register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            realm.set_now_secs_for_test(0.0);
            realm.mark_rendered();
            let mut backend = ScriptedRasterBackend::new(Ok(true));
            let _ = realm.render_frame_entered(&mut backend);
            // `needs_redraw()` is what actually carries this assertion in
            // this test, not `has_pending_work()`: no widget is attached
            // (a pure-animation scenario, matching this test's own point —
            // the controller alone keeps the gate open), so nothing ever
            // dirties the pipeline and `has_pending_work()` reads `false`
            // for the entire test (checked directly, not assumed).
            // `needs_redraw()` is `true` here because `render_frame_entered`'s
            // post-`mark_rendered()` continuation-wake step (see that
            // method's own comment) called `wake_frame()` for this still-
            // running controller. The `||` stays as the real production
            // predicate this test also exercises through the FrameClock
            // segment gate (`has_pending_work()` DOES carry other tests,
            // e.g. any widget-driven dirty state) — kept here for parity
            // with that predicate, not because this specific scenario
            // needs it.
            assert!(
                realm.needs_redraw() || realm.has_pending_work(),
                "V1: the runner gate must be open after an anchor frame",
            );

            realm.set_now_secs_for_test(0.05);
            realm.mark_rendered();
            let mut backend = ScriptedRasterBackend::new(Ok(true));
            let _ = realm.render_frame_entered(&mut backend);
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
            let mut backend = ScriptedRasterBackend::new(Ok(true));
            let _ = realm.render_frame_entered(&mut backend);
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
                &flui_scheduler::UpdateScheduler::new(),
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
        fn device_lost_keeps_needs_redraw_armed_for_a_retry() {
            let realm = mount_root();
            let mut backend = ScriptedRasterBackend::new(Err(EngineError::DeviceLost));

            realm.mark_rendered();
            let presented = realm.render_frame_entered(&mut backend);

            assert!(!presented, "a DeviceLost frame never reaches present()");
            assert_eq!(
                backend.render_scene_calls, 1,
                "precondition: the mounted scene actually reached render_scene"
            );
            assert!(
                realm.needs_redraw(),
                "a dropped DeviceLost frame must re-arm needs_redraw: rebuilding the \
                 device is the renderer owner's job, but on a quiescent loop nothing \
                 else would ever wake the owner to retry the recovery"
            );
        }

        #[test]
        fn surface_validation_keeps_needs_redraw_armed_for_a_retry() {
            let realm = mount_root();
            let mut backend = ScriptedRasterBackend::new(Err(EngineError::SurfaceValidation));

            realm.mark_rendered();
            let presented = realm.render_frame_entered(&mut backend);

            assert!(
                !presented,
                "a SurfaceValidation frame never reaches present()"
            );
            assert_eq!(
                backend.render_scene_calls, 1,
                "precondition: the mounted scene actually reached render_scene"
            );
            assert!(
                realm.needs_redraw(),
                "a dropped SurfaceValidation frame must re-arm needs_redraw so a later \
                 wake gets another acquire attempt instead of parking the misconfigured \
                 surface forever"
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
            // The property this test's own NAME claims, actually asserted:
            // the pipeline (build/layout/paint) really did run -- Flutter
            // parity, `.flutter/packages/flutter/lib/src/rendering/binding.dart:582-599`
            // (`RendererBinding.deferFirstFrame`'s own doc: "the framework
            // will still do all the work to produce frames"). Without this
            // assertion a regression that skips the WHOLE segment while
            // deferred (not just the submit) would still pass every check
            // above -- `render_scene_calls == 0` and `frames_rendered ==
            // 0` are also exactly what a fully-skipped segment produces.
            assert_eq!(
                realm.presentations.primary().flush_count(),
                1,
                "the segment must have actually run while deferred, not been skipped outright"
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
        // The panic message changed with the migration from
        // `RenderingFlutterBinding`'s own counter to `FrameClock::lift` --
        // same caller-contract panic, new mechanism's own wording.
        #[should_panic(expected = "FrameClock::lift called without a matching defer")]
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
            realm.pipeline_for_test().with_mut(|owner| {
                let root_id = owner.insert(Box::new(PanicOnLayoutBox)
                    as Box<
                        dyn flui_rendering::traits::RenderObject<
                                flui_rendering::protocol::BoxProtocol,
                            >,
                    >);
                owner.set_root_id(Some(root_id));
            });
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

    // ========================================================================
    // Presentation-owned text input — migrated from the retired
    // `AppBinding`'s own test module (`binding.rs`, deleted alongside it).
    // End-to-end against a headless `FakeTextInput`, including realm-routed
    // IME dispatch through `handle_input_entered`.
    // ========================================================================
    mod presentation_text_input {
        use std::cell::RefCell;

        use flui_types::ImeEvent;
        use flui_types::geometry::Bounds;

        use super::*;

        /// A concrete headless recorder and the same value viewed through the
        /// platform capability supplied to the presentation.
        fn headless_text_input() -> (
            Arc<flui_platform::FakeTextInput>,
            Arc<dyn flui_platform::traits::PlatformTextInput>,
        ) {
            let fake = Arc::new(flui_platform::FakeTextInput::new());
            let capability: Arc<dyn flui_platform::traits::PlatformTextInput> = fake.clone();
            (fake, capability)
        }

        fn test_constraints() -> BoxConstraints {
            BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
        }

        /// Attach records `set_ime_allowed(true)`; preedit/commit events
        /// routed through `handle_input_entered` reach the attached client
        /// with the exact delivered strings; detach from the still-active
        /// token records `set_ime_allowed(false)`.
        #[test]
        fn attach_dispatch_and_active_detach_round_trip_through_the_platform() {
            let (fake, text_input) = headless_text_input();

            let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));
            let handle = realm.text_input_handle();

            let received = Rc::new(RefCell::new(Vec::new()));
            let sink = Rc::clone(&received);
            let token = handle
                .attach(Rc::new(move |event: &ImeEvent| {
                    sink.borrow_mut().push(event.clone());
                }))
                .expect("headless presentation supports text input");

            assert_eq!(
                fake.last_ime_allowed(),
                Some(true),
                "attach must enable platform IME composition"
            );

            realm.enter(|realm| {
                realm.handle_input_entered(PlatformInput::Ime(ImeEvent::Preedit {
                    text: "ni".to_string(),
                    cursor: Some((0, 2)),
                }));
                realm
                    .handle_input_entered(PlatformInput::Ime(ImeEvent::Commit("你好".to_string())));
            });

            assert_eq!(
                received.borrow().as_slice(),
                [
                    ImeEvent::Preedit {
                        text: "ni".to_string(),
                        cursor: Some((0, 2)),
                    },
                    ImeEvent::Commit("你好".to_string()),
                ],
                "handle_input_entered must deliver the exact ImeEvent payload to the attached client"
            );

            assert_eq!(
                handle.detach(token).expect("presentation remains open"),
                flui_interaction::DetachOutcome::Detached
            );
            assert_eq!(
                fake.last_ime_allowed(),
                Some(false),
                "detaching the active token must disable platform IME composition"
            );
        }

        /// The stale-detach race named in `TextInputOwner`'s module doc:
        /// field A attaches, field B attaches (replacing A), and A's
        /// now-stale detach must record NOTHING on the platform side — only
        /// B's later, active-token detach may disable IME.
        #[test]
        fn a_stale_detach_records_nothing_on_the_platform() {
            let (fake, text_input) = headless_text_input();

            let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));
            let handle = realm.text_input_handle();

            let token_a = handle
                .attach(Rc::new(|_event: &ImeEvent| {}))
                .expect("supported presentation");
            assert_eq!(fake.ime_allowed_calls(), vec![true]);

            let token_b = handle
                .attach(Rc::new(|_event: &ImeEvent| {}))
                .expect("supported presentation");
            assert_eq!(
                fake.ime_allowed_calls(),
                vec![true],
                "replacement on one presentation keeps the already-enabled IME session"
            );

            assert_eq!(
                handle.detach(token_a).expect("presentation remains open"),
                flui_interaction::DetachOutcome::Stale
            );
            assert_eq!(
                fake.ime_allowed_calls(),
                vec![true],
                "a stale detach (token_a, already replaced by token_b) records nothing"
            );

            assert_eq!(
                handle.detach(token_b).expect("presentation remains open"),
                flui_interaction::DetachOutcome::Detached
            );
            assert_eq!(
                fake.ime_allowed_calls(),
                vec![true, false],
                "the active token's detach still disables IME"
            );
        }

        /// End-to-end proof of a claim `flui-widgets`' own
        /// `editable_text::tests` cannot make on their own: a real
        /// mounted `EditableText` receives the weak handle of this realm's
        /// directly owned platform capability, not a mock or a stand-in.
        #[test]
        fn a_mounted_editable_text_toggles_platform_ime_on_focus_and_blur() {
            let (fake, text_input) = headless_text_input();

            let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));

            let controller = flui_widgets::TextEditingController::new();
            let focus_node = flui_interaction::FocusNode::with_debug_label("app-ime-integration");
            realm
                .enter(|realm| {
                    realm.attach_root_widget(&flui_widgets::EditableText::new(
                        controller.clone(),
                        Rc::clone(&focus_node),
                    ))
                })
                .expect("attach succeeds");
            let _ = realm.draw_frame(test_constraints());

            assert!(focus_node.is_attached());
            focus_node.request_focus();
            assert!(focus_node.has_primary_focus());
            assert_eq!(
                fake.last_ime_allowed(),
                Some(true),
                "focusing a mounted EditableText must attach through its presentation \
                 and enable platform IME composition"
            );

            focus_node.unfocus();
            assert_eq!(
                fake.last_ime_allowed(),
                Some(false),
                "blurring the field must detach and disable platform IME composition"
            );
        }

        /// `TextInputHandle::set_cursor_area` reaches this realm's exact
        /// `PlatformTextInput` capability through the same `PresentationState`
        /// ownership path used in production. Deliberately does not mount a
        /// widget tree: this test proves the owner forwards an
        /// already-computed `Bounds` to the platform, not that a real
        /// `EditableText` computes the right one.
        #[test]
        fn set_ime_cursor_area_reaches_the_presentations_platform_capability() {
            let (fake, text_input) = headless_text_input();

            let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));

            let area = Bounds::new(
                flui_types::Point::new(px(10.0), px(20.0)),
                flui_types::Size::new(px(2.0), px(18.0)),
            );
            realm
                .text_input_handle()
                .set_cursor_area(area)
                .expect("headless presentation supports text input");

            assert_eq!(
                fake.cursor_area_calls(),
                vec![area],
                "set_ime_cursor_area must call through to the presentation-owned \
                 PlatformTextInput capability with the exact area"
            );
        }

        /// A presentation without IME support reports a typed error.
        #[test]
        fn set_ime_cursor_area_without_platform_support_is_typed() {
            let realm = UiRealm::for_test();
            assert_eq!(
                realm.text_input_handle().set_cursor_area(Bounds::new(
                    flui_types::Point::new(px(0.0), px(0.0)),
                    flui_types::Size::new(px(1.0), px(1.0)),
                )),
                Err(flui_interaction::TextInputError::Unsupported)
            );
        }
    }

    // ========================================================================
    // Presentation forest — isolation suite (ADR-0043 §1)
    //
    // Production topology is exactly one presentation per realm
    // (`PresentationForest`'s `install` ratchet); these tests bypass it via
    // `push_for_test` to exercise the composite `GlobalKey` registry and
    // hot-reload fan-out against a genuine N=2 forest.
    // ========================================================================
    mod presentation_forest_isolation {
        use super::*;

        /// This slice lifted `PresentationForest::install`'s former
        /// `len()<=1` ratchet: `install_second_presentation_for_test` (and
        /// its production counterpart, `UiRealm::install_presentation`) now
        /// go through the SAME production `install` call, not a
        /// `cfg(test)`-only bypass.
        #[test]
        fn install_presentation_grows_the_forest_past_one() {
            let mut realm = UiRealm::for_test();
            assert_eq!(realm.presentation_count(), 1);

            realm.install_second_presentation_for_test();

            assert_eq!(
                realm.presentation_count(),
                2,
                "PresentationForest::install must accept a second presentation now that the \
                 ratchet is lifted"
            );
        }

        /// The realm composite `enter()` activates (ADR-0043 §1)
        /// must resolve a `GlobalKey` registered in EITHER presentation's own
        /// tree, not just the primary one — the correctness property
        /// `GlobalKeyRegistryComposite` exists for.
        #[test]
        fn composite_registry_resolves_keys_registered_in_either_presentation() {
            let mut realm = UiRealm::for_test();
            let second_id = realm.install_second_presentation_for_test();

            let key_in_primary = flui_view::GlobalKey::<()>::new();
            let element_in_primary = flui_foundation::ElementId::new(1);
            let key_in_second = flui_view::GlobalKey::<()>::new();
            let element_in_second = flui_foundation::ElementId::new(1); // deliberately the SAME numeral

            realm.enter(|realm| {
                realm
                    .presentations
                    .primary()
                    .widgets()
                    .with_build_owner_mut(|owner| {
                        owner.register_global_key(key_in_primary.id(), element_in_primary);
                    });
                let second = realm
                    .presentations
                    .get(second_id)
                    .expect("second presentation installed");
                second.widgets().with_build_owner_mut(|owner| {
                    owner.register_global_key(key_in_second.id(), element_in_second);
                });

                assert_eq!(
                    key_in_primary.current_element(),
                    Some(element_in_primary),
                    "composite must resolve a key registered in the primary presentation"
                );
                assert_eq!(
                    key_in_second.current_element(),
                    Some(element_in_second),
                    "composite must resolve a key registered in the second \
                     presentation, even though both trees use the same raw \
                     ElementId numeral"
                );
            });
        }

        /// `PresentationState::new` wires `RealmCapabilities::
        /// global_key_scope` into `BuildOwner::set_global_key_scope` FIRST —
        /// before this presentation's own focus/IME wiring, before
        /// attach/mount (see that constructor's own doc comment). The pin:
        /// two presentations of ONE realm, sharing that exact scope,
        /// mounting the IDENTICAL `GlobalKey` must conflict eagerly —
        /// `GlobalKeyScope`'s cross-owner uniqueness domain — naming both
        /// owner tags in the panic (`global_key_scope.rs::claim_and_register`'s
        /// sole conflict branch).
        ///
        /// Unpinned before this test: deleting the `set_global_key_scope`
        /// call from `PresentationState::new` leaves both the flui-view and
        /// flui-app suites green, because each presentation's `BuildOwner`
        /// then lazily creates its OWN private, single-tenant scope on first
        /// use (`claim_and_register`'s `get_or_insert_with` fallback) — the
        /// two owners never actually share one scope, so mounting the same
        /// key in both silently succeeds in each instead of conflicting.
        /// Mutant-verified: removing that one setter call from
        /// `PresentationState::new` makes this test fail (no panic);
        /// restoring it makes the eager panic fire again.
        #[test]
        #[should_panic(expected = "is already claimed by")]
        fn duplicate_global_key_across_presentations_panics_eagerly_naming_both_owners() {
            let mut realm = UiRealm::for_test();
            let b_id = realm.install_second_presentation_for_test();

            let key = flui_view::GlobalKey::<()>::new();
            let element_in_a = flui_foundation::ElementId::new(1);
            let element_in_b = flui_foundation::ElementId::new(2);

            realm.widgets().with_build_owner_mut(|owner| {
                owner.register_global_key(key.id(), element_in_a);
            });

            // B shares A's realm's exact GlobalKeyScope
            // (install_second_presentation_for_test wires it that way, the
            // same capability-threading order PresentationState::new uses
            // in production) -- mounting the SAME key here must conflict
            // eagerly, never silently succeed in a second, private scope.
            realm
                .presentation_widgets_for_test(b_id)
                .with_build_owner_mut(|owner| {
                    owner.register_global_key(key.id(), element_in_b);
                });
        }

        /// `UiRealm::apply_hot_reload` must reassemble EVERY presentation in
        /// mount order, not just the primary one.
        ///
        /// Oracle: `has_pending_builds()` on EACH presentation's own
        /// `WidgetsBinding` after `apply_hot_reload` — `perform_reassemble`
        /// (`BuildOwner::reassemble`) marks every MOUNTED element dirty, so a
        /// presentation that was actually reassembled reports a pending
        /// build; one that fan-out skipped does not. A membership check
        /// alone (does the forest still contain both ids) is vacuous here:
        /// it stays green even if fan-out silently reverted to
        /// primary-only, since nothing removes a presentation from the
        /// forest just because reassemble skipped it. Mutant-verified:
        /// reverting `UiRealm::apply_hot_reload`'s loop to call
        /// `self.presentations.primary().apply_hot_reload(tier)` alone (no
        /// loop over the whole forest) makes this test fail on B's
        /// `has_pending_builds()` assertion; restoring the loop makes it
        /// pass again.
        #[test]
        #[cfg(feature = "hot-reload")]
        fn reassemble_fans_out_to_all_presentations_in_mount_order() {
            use flui_hot_reload::HotReloadTier;

            let mut realm = UiRealm::for_test();
            let b_id = realm.install_second_presentation_for_test();

            // Mount independent content on BOTH presentations -- reassemble
            // has nothing to mark dirty on an empty tree, so the oracle
            // below needs a real mounted root on each.
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
                .expect("A mounts");
            realm
                .enter(|realm| {
                    realm
                        .presentations
                        .get(b_id)
                        .expect("B installed")
                        .widgets()
                        .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 20.0))
                })
                .expect("B mounts");

            // A fresh mount always starts with a pending initial build --
            // drain both before reassembling, or the oracle below cannot
            // tell "still pending from mount" apart from "reassemble
            // actually re-marked it dirty".
            let constraints = BoxConstraints::tight(flui_types::Size::new(px(50.0), px(50.0)));
            let _ = realm.draw_frame(constraints);
            assert!(
                !realm.widgets().has_pending_builds(),
                "precondition: A's initial build must be drained before reassembling"
            );
            assert!(
                !realm
                    .presentations
                    .get(b_id)
                    .expect("B installed")
                    .widgets()
                    .has_pending_builds(),
                "precondition: B's initial build must be drained before reassembling"
            );

            let _ = realm.apply_hot_reload(HotReloadTier::HotReload);

            assert!(
                realm.widgets().has_pending_builds(),
                "reassemble must mark A's mounted root dirty"
            );
            assert!(
                realm
                    .presentations
                    .get(b_id)
                    .expect("B installed")
                    .widgets()
                    .has_pending_builds(),
                "reassemble must mark B's mounted root dirty too -- fan-out must not skip \
                 or stop at the primary presentation"
            );
        }

        /// The SAME fan-out property as `reassemble_fans_out_to_all_
        /// presentations_in_mount_order`, but driven through the OTHER
        /// production entry point: the closed-command inbox
        /// (`command_sender().request_hot_reload` + `drain_commands`), the
        /// path the desktop worker's own hot-reload trigger actually uses --
        /// not `apply_hot_reload`/`perform_hot_reload_entered` called
        /// directly. Before this arm reused `Self::apply_hot_reload`, it
        /// called `self.presentations.primary().apply_hot_reload(tier)`
        /// directly, reassembling only the primary and silently skipping
        /// every sibling — the registry-pinned exploit's exact regression
        /// class, on its untested twin path. Mutant-verified: reverting
        /// `drain_commands`'s `UiCommand::HotReload` arm back to
        /// `self.presentations.primary().apply_hot_reload(tier)` makes this
        /// test fail on B's `has_pending_builds()` assertion; restoring the
        /// `self.apply_hot_reload(tier)` fan-out call makes it pass again.
        #[test]
        #[cfg(feature = "hot-reload")]
        fn hot_reload_via_the_command_inbox_fans_out_to_all_presentations_in_mount_order() {
            use flui_hot_reload::HotReloadTier;

            let mut realm = UiRealm::for_test();
            let b_id = realm.install_second_presentation_for_test();

            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
                .expect("A mounts");
            realm
                .enter(|realm| {
                    realm
                        .presentations
                        .get(b_id)
                        .expect("B installed")
                        .widgets()
                        .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 20.0))
                })
                .expect("B mounts");

            let constraints = BoxConstraints::tight(flui_types::Size::new(px(50.0), px(50.0)));
            let _ = realm.draw_frame(constraints);
            assert!(
                !realm.widgets().has_pending_builds(),
                "precondition: A's initial build must be drained before reassembling"
            );
            assert!(
                !realm
                    .presentations
                    .get(b_id)
                    .expect("B installed")
                    .widgets()
                    .has_pending_builds(),
                "precondition: B's initial build must be drained before reassembling"
            );

            realm
                .command_sender()
                .request_hot_reload(HotReloadTier::HotReload)
                .expect("inbox has room");
            let report = realm.drain_commands();
            assert_eq!(
                report.invoked, 1,
                "the hot-reload command must be applied, not dropped as stale"
            );

            assert!(
                realm.widgets().has_pending_builds(),
                "the inbox path must mark A's mounted root dirty, exactly like the direct path"
            );
            assert!(
                realm
                    .presentations
                    .get(b_id)
                    .expect("B installed")
                    .widgets()
                    .has_pending_builds(),
                "the inbox path must mark B's mounted root dirty too -- it is the SAME \
                 fan-out as the direct perform_hot_reload_entered path, not a narrower one"
            );
        }

        /// Dropping the realm closes EVERY presentation it hosts, not just
        /// the primary one.
        #[test]
        fn dropping_the_realm_closes_every_presentation() {
            let mut realm = UiRealm::for_test();
            realm.install_second_presentation_for_test();

            let lifecycles_before: Vec<_> = realm
                .presentations
                .iter()
                .map(crate::app::presentation::PresentationState::lifecycle)
                .collect();
            assert!(
                lifecycles_before.iter().all(|l| {
                    *l == crate::app::presentation::PresentationLifecycle::SurfaceAttached
                }),
                "both presentations must start attached"
            );

            drop(realm);
            // Both presentations' own Drop impls ran as part of the realm's
            // Drop (each PresentationState transitions to Closed on its own
            // drop -- see PresentationState::close/Drop); nothing left to
            // assert on here beyond "this did not panic", since the values
            // themselves are gone. Proving that closing ONE presentation
            // structurally cannot disturb a SURVIVING sibling's own layer
            // tree needs an addressed per-presentation teardown path (close
            // exactly one member, keep the rest running) that this slice
            // does not add -- production topology never removes a single
            // member from a live forest today, it only ever drops the whole
            // realm, which is what this test actually exercises.
        }

        fn segment_constraints() -> BoxConstraints {
            BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
        }

        /// Oracle: FLUSH COUNTS (`PresentationState::flush_count`), never
        /// rebuild counts — a presentation with a settled, never-rebuilding
        /// tree still flushes every segment its own dirty state (or wake
        /// bit) causes to run, and a presentation that was never dirtied
        /// must NOT flush just because its sibling did.
        #[test]
        fn sibling_presentations_flush_independently() {
            let mut realm = UiRealm::for_test();
            let second_id = realm.install_second_presentation_for_test();

            // Mark ONLY the second presentation dirty — reaching its own
            // wake bit directly, the same seam a later addressed-routing
            // slice would call through (no addressed entry point exists
            // yet).
            realm
                .presentations
                .get(second_id)
                .expect("second presentation installed")
                .mark_redraw_pending();

            let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));

            assert_eq!(
                realm.presentations.primary().flush_count(),
                0,
                "the primary presentation was never marked dirty and must \
                 not flush just because its sibling did"
            );
            assert_eq!(
                realm
                    .presentations
                    .get(second_id)
                    .expect("still installed")
                    .flush_count(),
                1,
                "the second presentation's own wake bit must cause exactly \
                 one flush"
            );

            // A second pump with nothing newly dirty must flush neither —
            // both presentations are now settled.
            let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));
            assert_eq!(
                realm.presentations.primary().flush_count(),
                0,
                "still never dirtied, still zero flushes"
            );
            assert_eq!(
                realm
                    .presentations
                    .get(second_id)
                    .expect("still installed")
                    .flush_count(),
                1,
                "a settled presentation must not flush again with nothing \
                 new to do"
            );
        }

        /// A mark arriving for a presentation while ITS OWN segment is
        /// running must not be lost: the segment already cleared the bit at
        /// its own START, so a mark landing anywhere between that clear and
        /// the next pump's sample must survive to be observed there.
        ///
        /// This collapses the timing to a single thread (mark, run the
        /// segment, mark again immediately after) rather than a literal
        /// concurrent race — in a single-threaded pump there is no OTHER
        /// window where a mark could land except between one segment
        /// ending and the next beginning, or nested inside the segment's
        /// own build via a real callback; both reduce to the same
        /// observable property this test checks: the bit set after the
        /// clear is not silently dropped.
        #[test]
        fn cross_presentation_dirty_during_a_segment_sets_wake_bit_and_lands_next_pump() {
            let realm = UiRealm::for_test();
            let presentation = realm.presentations.primary();

            presentation.mark_redraw_pending();
            let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));
            assert_eq!(
                presentation.flush_count(),
                1,
                "the armed wake bit must cause exactly one flush"
            );

            // A mark arrives during (or immediately after) that segment —
            // nothing else about the presentation is dirty (no widget tree
            // was ever attached, so no pending builds; the render tree is
            // empty, so no dirty nodes either): the wake bit is the ONLY
            // dirty signal in play.
            presentation.mark_redraw_pending();

            let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));
            assert_eq!(
                presentation.flush_count(),
                2,
                "the wake bit set after the first segment cleared its own \
                 bit must not be lost -- it must cause a second flush on \
                 the next pump, even though nothing else about the \
                 presentation is dirty"
            );
        }
    }

    // ========================================================================
    // Addressed input/keyboard routing (issue #555's addressed-routing slice hop-2) — every
    // input/IME delivery lands on exactly the stamped presentation, except
    // keyboard, which routes through FocusCoordinator's active presentation
    // instead (ADR-0043 §4).
    // ========================================================================
    mod addressed_input_routing {
        use std::cell::RefCell;

        use flui_interaction::events::{PointerType, make_down_event};
        use flui_interaction::testing::input::KeyEventBuilder;
        use flui_types::ImeEvent;
        use flui_types::geometry::{Offset, Pixels};

        use super::*;

        fn headless_text_input() -> (
            Arc<flui_platform::FakeTextInput>,
            Arc<dyn flui_platform::traits::PlatformTextInput>,
        ) {
            let fake = Arc::new(flui_platform::FakeTextInput::new());
            let capability: Arc<dyn flui_platform::traits::PlatformTextInput> = fake.clone();
            (fake, capability)
        }

        /// A pointer event addressed to B (the NON-primary presentation)
        /// must reach ONLY B's own gesture arena — never A's (the primary),
        /// even though both presentations share one realm's `enter()` scope
        /// and dispatch machinery.
        ///
        /// Deliberately stamps for B, not A: A is this realm's primary, so a
        /// mutant that reroutes `handle_input_addressed` to
        /// `self.presentations.primary()` regardless of the addressed id
        /// would still (accidentally) satisfy an A-stamped version of this
        /// test — addressed-vs-primary is unobservable when the stamped
        /// target IS the primary. Stamping for B is the only fixture that
        /// actually exercises the addressed lookup: reverting the `Pointer`
        /// arm to `self.presentations.primary()` makes this fail (B's count
        /// stays `0`, A's becomes `1`).
        #[test]
        fn input_stamped_for_b_never_reaches_as_arena() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            let down = make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(b_id, PlatformInput::Pointer(down));
            });

            assert_eq!(
                realm
                    .presentations
                    .get(b_id)
                    .expect("B installed")
                    .gestures()
                    .active_pointer_count(),
                1,
                "the addressed (non-primary) presentation must receive the pointer event"
            );
            assert_eq!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .gestures()
                    .active_pointer_count(),
                0,
                "a pointer event stamped for B must never reach A's own gesture arena, even \
                 though A is this realm's primary"
            );
        }

        /// Input addressed to a presentation this realm does not (or no
        /// longer) host must drop traced -- never silently fall through to
        /// the primary, and never reach ANY live presentation's own arena.
        /// Covers both shapes of "not a live presentation": an id that
        /// never existed in this realm, and a real id that existed, was
        /// closed, and is now gone from the forest.
        ///
        /// If reverted (`handle_input_addressed` falling back to
        /// `self.presentations.primary()` when the addressed id is not
        /// found, instead of tracing and returning): this fails in BOTH
        /// cases -- A's (the primary's) pointer count would become
        /// nonzero instead of staying `0`.
        #[test]
        fn input_addressed_to_a_closed_or_unknown_presentation_drops_traced_never_falls_through() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            // Case 1: an id that never existed in this realm at all.
            let bogus = flui_foundation::PresentationId::new_gen(
                999,
                std::num::NonZeroU32::new(999).expect("nonzero"),
            );
            let down_for_bogus =
                make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(bogus, PlatformInput::Pointer(down_for_bogus));
            });
            assert_eq!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .gestures()
                    .active_pointer_count(),
                0,
                "input addressed to a never-existed presentation id must never fall through \
                 to the primary"
            );
            assert_eq!(
                realm
                    .presentations
                    .get(b_id)
                    .expect("B installed")
                    .gestures()
                    .active_pointer_count(),
                0,
                "input addressed to a never-existed presentation id must never reach ANY \
                 live presentation"
            );

            // Case 2: a real id that existed, was closed, and is now gone
            // from the forest.
            realm.close_presentation_entered(b_id);
            assert!(
                realm.presentations.get(b_id).is_none(),
                "precondition: B must actually be gone from the forest after closing"
            );
            let down_for_closed =
                make_down_event(Offset::new(Pixels(8.0), Pixels(10.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(b_id, PlatformInput::Pointer(down_for_closed));
            });
            assert_eq!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .gestures()
                    .active_pointer_count(),
                0,
                "input addressed to a CLOSED presentation id must never fall through to the \
                 surviving primary either"
            );
        }

        /// Two independently attached IME sessions, one per presentation:
        /// delivering an event addressed to B (the NON-primary presentation)
        /// must reach only B's attached client and must not disturb A's
        /// (the primary's) own platform IME session.
        ///
        /// Deliberately addresses B, not A — see `input_stamped_for_b_never_
        /// reaches_as_arena`'s doc for why an A-addressed fixture cannot
        /// distinguish "delivered to the addressed presentation" from
        /// "delivered to the primary": reverting the `Ime` arm to
        /// `self.presentations.primary()` makes this fail (B's sink stays
        /// empty, A's receives the event instead).
        #[test]
        fn ime_event_addressed_to_b_does_not_reach_as_session() {
            let (fake_a, capability_a) = headless_text_input();
            let (fake_b, capability_b) = headless_text_input();

            let mut realm = UiRealm::for_test_with_text_input(Some(capability_a));
            let a_id = realm.presentation_id();
            let window_b = crate::app::presentation::test_platform_window(Some(capability_b));
            let presentation_b = realm.assemble_presentation(window_b);
            let b_id = realm.install_presentation(presentation_b);

            let received_a = Rc::new(RefCell::new(Vec::new()));
            let sink_a = Rc::clone(&received_a);
            let handle_a = realm.presentation_text_input_handle_for_test(a_id);
            let _token_a = handle_a
                .attach(Rc::new(move |event: &ImeEvent| {
                    sink_a.borrow_mut().push(event.clone());
                }))
                .expect("A's headless window supports text input");

            let received_b = Rc::new(RefCell::new(Vec::new()));
            let sink_b = Rc::clone(&received_b);
            let handle_b = realm.presentation_text_input_handle_for_test(b_id);
            let _token_b = handle_b
                .attach(Rc::new(move |event: &ImeEvent| {
                    sink_b.borrow_mut().push(event.clone());
                }))
                .expect("B's headless window supports text input");

            assert_eq!(fake_a.last_ime_allowed(), Some(true));
            assert_eq!(fake_b.last_ime_allowed(), Some(true));

            realm.enter(|realm| {
                realm.handle_input_addressed(
                    b_id,
                    PlatformInput::Ime(ImeEvent::Commit("hello".to_string())),
                );
            });

            assert_eq!(
                received_b.borrow().as_slice(),
                [ImeEvent::Commit("hello".to_string())],
                "B's attached client must receive the event addressed to B, even though A is \
                 this realm's primary"
            );
            assert!(
                received_a.borrow().is_empty(),
                "an IME event addressed to B must never reach A's attached client"
            );
            assert_eq!(
                fake_a.last_ime_allowed(),
                Some(true),
                "delivering an event to B must not touch A's own platform IME session"
            );
        }

        /// Keyboard input always routes to the realm's ACTIVE presentation
        /// (`FocusCoordinator`), never to whichever presentation an
        /// individual event happens to be stamped for — the race/stray-event
        /// case where the stamp and the OS-focused window disagree.
        ///
        /// Oracle: each presentation's own wake-only `redraw_pending` bit
        /// (`take_redraw_pending`), set only by the presentation that
        /// actually ran `dispatch_key_event`. If reverted (the `Keyboard`
        /// arm dispatches to the STAMPED `presentation_id` instead of
        /// `self.focus_coordinator.active()`): A's bit would be set and B's
        /// would not, the opposite of what this test asserts.
        #[test]
        fn keyboard_routes_to_active_presentation_only() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();
            assert_eq!(
                realm.active_presentation_for_test(),
                a_id,
                "a freshly constructed realm starts with its initial presentation active"
            );

            realm.notify_presentation_focus_gained(b_id);
            assert_eq!(
                realm.active_presentation_for_test(),
                b_id,
                "a WindowFocus(true) naming B must move the active presentation to B"
            );

            let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
            // Stamped for A -- but B is the currently active presentation.
            realm.enter(|realm| {
                realm.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
            });

            let a_presentation = realm.presentations.get(a_id).expect("A installed");
            let b_presentation = realm.presentations.get(b_id).expect("B installed");
            assert!(
                !a_presentation.take_redraw_pending(),
                "A is only the STAMPED presentation, never the active one here -- it must not \
                 have received the keyboard event"
            );
            assert!(
                b_presentation.take_redraw_pending(),
                "keyboard input must route to B, the ACTIVE presentation, regardless of which \
                 presentation the event was stamped for"
            );
        }

        // This module can only exercise `UiRealm`'s own write side
        // (`notify_presentation_focus_gained`) directly -- the production
        // `WindowFocus(false)`-never-moves-active guard actually lives in
        // `runner.rs`'s `PlatformToUi::run` (the `if focused` check around
        // the call to `notify_presentation_focus_gained`), which a direct
        // `UiRealm`-level test cannot reach or mutate. See
        // `realm_dispatch_tests::window_focus_true_moves_active_
        // presentation_end_to_end_and_false_does_not` (`runner.rs`) for the
        // real end-to-end proof, driven through `dispatch_platform_realm`
        // with genuine `RealmTask::Event(PlatformToUi::WindowFocus(_))`
        // tasks -- a vacuous direct-call version of this test (set B active,
        // assert B active, assert B active again with no operation between)
        // used to live here and was replaced for exactly that reason.

        /// A focus-gained notification naming a presentation this realm no
        /// longer hosts (already closed, or a forged id) is a traced no-op —
        /// arbitration never points at a dead presentation.
        #[test]
        fn focus_gained_for_an_unknown_presentation_is_a_traced_no_op() {
            let realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let bogus = flui_foundation::PresentationId::new_gen(
                999,
                std::num::NonZeroU32::new(999).expect("nonzero"),
            );

            realm.notify_presentation_focus_gained(bogus);

            assert_eq!(
                realm.active_presentation_for_test(),
                a_id,
                "an unknown presentation id must never become the active one"
            );
        }

        /// Closing the realm's currently ACTIVE presentation must re-stamp
        /// `FocusCoordinator` to the surviving primary before returning --
        /// never leave it pointing at a now-dead id, which would
        /// black-hole every keyboard event until a fresh `WindowFocus(true)`
        /// happened to name a live presentation. Closes B (the non-primary)
        /// while B is active, so this also exercises "the active
        /// presentation being closed is not the primary" -- not just "close
        /// the primary while it's active".
        ///
        /// If reverted (the re-stamp removed from
        /// `close_presentation_entered`): this fails -- the post-close
        /// keyboard event resolves `self.presentations.get(dead_b_id)` to
        /// `None` and drops traced, so A's `redraw_pending` bit is never
        /// set.
        #[test]
        fn keyboard_after_closing_the_active_presentation_routes_to_the_survivor() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            realm.notify_presentation_focus_gained(b_id);
            assert_eq!(realm.active_presentation_for_test(), b_id);

            realm.close_presentation_entered(b_id);

            assert_eq!(
                realm.active_presentation_for_test(),
                a_id,
                "closing the active presentation must re-stamp active to the surviving primary"
            );

            let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
            realm.enter(|realm| {
                realm.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
            });

            assert!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .take_redraw_pending(),
                "keyboard input must reach the surviving primary after the active presentation \
                 closes, never drop as if no presentation were active"
            );
        }
    }

    // ========================================================================
    // Per-presentation redraw wake (issue #555's addressed-routing slice) — a redraw request that
    // dirties one presentation's own pipeline must poke ITS OWN native
    // window only, never a sibling's.
    // ========================================================================
    mod redraw_wake_routing {
        use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

        use flui_platform::CursorIcon;
        use flui_platform::traits::{CursorError, WindowId};
        use flui_types::geometry::{DevicePixels, Pixels};

        use super::*;

        struct RedrawCountingWindow {
            id: WindowId,
            redraw_calls: Arc<AtomicU32>,
        }

        impl PlatformWindow for RedrawCountingWindow {
            fn id(&self) -> WindowId {
                self.id
            }

            fn physical_size(&self) -> Size<DevicePixels> {
                Size::default()
            }

            fn logical_size(&self) -> Size<Pixels> {
                Size::default()
            }

            fn scale_factor(&self) -> f64 {
                1.0
            }

            fn request_redraw(&self) {
                self.redraw_calls.fetch_add(1, AtomicOrdering::Relaxed);
            }

            fn is_focused(&self) -> bool {
                false
            }

            fn is_visible(&self) -> bool {
                true
            }

            fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        fn counting_window(id: u64) -> (Arc<dyn PlatformWindow>, Arc<AtomicU32>) {
            let redraw_calls = Arc::new(AtomicU32::new(0));
            let window: Arc<dyn PlatformWindow> = Arc::new(RedrawCountingWindow {
                id: WindowId(id),
                redraw_calls: Arc::clone(&redraw_calls),
            });
            (window, redraw_calls)
        }

        /// A redraw request that dirties presentation A's own pipeline must
        /// poke ONLY A's native window — never B's — even though both
        /// presentations share this realm's platform wake capability.
        ///
        /// If reverted (`PresentationState::new`'s `on_need_visual_update`
        /// wiring calling only `capabilities.wake` with no per-window poke,
        /// the pre-addressed-routing shape): this fails by observing A's own
        /// counter stay at zero — `capabilities.wake` in this test is a
        /// no-op closure, so nothing would poke ANY window at all.
        #[test]
        fn redraw_request_from_a_does_not_wake_bs_window() {
            let (window_a, calls_a) = counting_window(1);
            let (window_b, calls_b) = counting_window(2);

            // `PresentationState` keeps only a `Weak` ref to its window (the
            // platform owns the strong `Arc` in production, e.g. via its own
            // per-window callback closures) -- these two local bindings are
            // what keep each window alive for this test, exactly like
            // `presentation.rs`'s own `perform_haptic_feedback_*` tests do;
            // pass clones into the realm, never the only strong reference.
            let mut realm = UiRealm::new(
                noop_wake(),
                Arc::clone(&window_a),
                1.0,
                Arc::new(AtomicBool::new(false)),
            )
            .expect("realm constructs");
            let a_id = realm.presentation_id();
            let presentation_b = realm.assemble_presentation(Arc::clone(&window_b));
            let b_id = realm.install_presentation(presentation_b);

            assert_eq!(calls_a.load(AtomicOrdering::Relaxed), 0);
            assert_eq!(calls_b.load(AtomicOrdering::Relaxed), 0);

            realm
                .presentations
                .get(a_id)
                .expect("A installed")
                .pipeline()
                .with(flui_rendering::pipeline::PipelineOwner::request_visual_update);

            assert_eq!(
                calls_a.load(AtomicOrdering::Relaxed),
                1,
                "dirtying A's own pipeline must poke A's own window"
            );
            assert_eq!(
                calls_b.load(AtomicOrdering::Relaxed),
                0,
                "dirtying A must never poke B's window"
            );

            // The reverse direction, for symmetry: dirtying B pokes B only.
            realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .pipeline()
                .with(flui_rendering::pipeline::PipelineOwner::request_visual_update);

            assert_eq!(
                calls_a.load(AtomicOrdering::Relaxed),
                1,
                "dirtying B must never poke A's window"
            );
            assert_eq!(
                calls_b.load(AtomicOrdering::Relaxed),
                1,
                "dirtying B's own pipeline must poke B's own window"
            );
        }

        /// A frame demanded through the realm's own `UpdateScheduler` must
        /// reach the realm's platform wake.
        ///
        /// This is the edge an async completion travels: `TaskWaker::
        /// wake_by_ref` -> `AsyncDriver`'s request-frame -> `UpdateScheduler::
        /// request_frame` -> the `frame_scheduled` false->true edge -> the
        /// `on_frame_scheduled` hook. If that hook is never installed the
        /// scheduler's demand is a bare atomic store that only a pump already
        /// in flight can observe, and an idle `ControlFlow::Wait` loop sleeps
        /// through it — the window updates on the next unrelated event, or
        /// never.
        ///
        /// If reverted (drop the `set_on_frame_scheduled` install in
        /// `UiRealm::construct`): `on_frame_scheduled` stays `None`,
        /// `request_frame_impl` returns after the swap, and this observes
        /// zero wakes.
        #[test]
        fn a_scheduler_frame_request_reaches_the_realms_platform_wake() {
            let wakes = Arc::new(AtomicU32::new(0));
            let wake_counter = Arc::clone(&wakes);
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
            });

            let (window, _calls) = counting_window(1);
            let realm = UiRealm::new(wake, window, 1.0, Arc::new(AtomicBool::new(false)))
                .expect("realm constructs");

            // Construction may legitimately have demanded a first frame;
            // measure only the transition this test causes.
            // Clear the `frame_scheduled` latch so the request below is a real
            // false->true transition — the only edge the hook fires on.
            realm.scheduler().finish_async_pump();
            let before = wakes.load(AtomicOrdering::Relaxed);

            realm.scheduler().request_frame();

            assert!(
                wakes.load(AtomicOrdering::Relaxed) > before,
                "a scheduler frame request must reach the realm's platform wake; \
                 without it an async completion cannot wake an idle event loop"
            );
        }

        /// The same edge, fired from a foreign thread — the shape an async
        /// task completing on a background executor actually takes.
        ///
        /// The wake handed to a realm is `Send + Sync` precisely so this is
        /// legal; this pins that the scheduler seam preserves it.
        #[test]
        fn a_cross_thread_frame_request_reaches_the_realms_platform_wake() {
            let wakes = Arc::new(AtomicU32::new(0));
            let wake_counter = Arc::clone(&wakes);
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
            });

            let (window, _calls) = counting_window(1);
            let realm = UiRealm::new(wake, window, 1.0, Arc::new(AtomicBool::new(false)))
                .expect("realm constructs");
            // Clear the `frame_scheduled` latch so the request below is a real
            // false->true transition — the only edge the hook fires on.
            realm.scheduler().finish_async_pump();
            let before = wakes.load(AtomicOrdering::Relaxed);

            let scheduler = realm.scheduler().clone();
            std::thread::spawn(move || scheduler.request_frame())
                .join()
                .expect("waker thread completes");

            assert!(
                wakes.load(AtomicOrdering::Relaxed) > before,
                "a frame request raised off the owner thread must still reach the wake"
            );
        }
    }

    // ========================================================================
    // Presentation DPR seeding — a non-primary presentation's pipeline must
    // be seeded with ITS OWN window's scale factor before construction, the
    // same DPR-before-first-frame ordering the primary path already gets.
    // ========================================================================
    mod presentation_dpr_seeding {
        use flui_platform::CursorIcon;
        use flui_platform::traits::{CursorError, WindowId};
        use flui_types::geometry::{DevicePixels, Pixels};

        use super::*;

        struct ScaledTestWindow {
            id: WindowId,
            scale_factor: f64,
        }

        impl PlatformWindow for ScaledTestWindow {
            fn id(&self) -> WindowId {
                self.id
            }

            fn physical_size(&self) -> Size<DevicePixels> {
                Size::default()
            }

            fn logical_size(&self) -> Size<Pixels> {
                Size::default()
            }

            fn scale_factor(&self) -> f64 {
                self.scale_factor
            }

            fn request_redraw(&self) {}

            fn is_focused(&self) -> bool {
                false
            }

            fn is_visible(&self) -> bool {
                true
            }

            fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        /// A non-primary presentation's pipeline must be seeded with ITS
        /// OWN window's `scale_factor()` before construction — the same
        /// DPR-before-first-frame ordering `UiRealm::construct` already
        /// gives this realm's primary presentation (there, the caller reads
        /// the window's scale factor and passes it in explicitly; here,
        /// `assemble_presentation` already holds the window, so it reads
        /// the value directly instead).
        ///
        /// If reverted (the `set_device_pixel_ratio` seeding call removed
        /// from `assemble_presentation`): this fails — the readback
        /// observes the `PipelineOwner::new()` default (`1.0`) instead of
        /// the window's real `2.5`.
        #[test]
        fn non_primary_presentation_pipeline_is_seeded_with_its_own_windows_scale_factor() {
            let mut realm = UiRealm::for_test();
            let window_b: Arc<dyn PlatformWindow> = Arc::new(ScaledTestWindow {
                id: WindowId(42),
                scale_factor: 2.5,
            });
            let presentation_b = realm.assemble_presentation(window_b);
            let b_id = realm.install_presentation(presentation_b);

            let dpr = realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .pipeline()
                .with(flui_rendering::pipeline::PipelineOwner::device_pixel_ratio);

            assert_eq!(
                dpr, 2.5,
                "B's pipeline must be seeded with its own window's scale factor before \
                 construction, not the PipelineOwner default"
            );
        }
    }

    // ========================================================================
    // Async-task disposition audit (ADR-0043 §5) — a dropped presentation's
    // in-flight AsyncDriver task must not reach a live sibling
    // ========================================================================
    mod async_completion_isolation {
        use std::cell::RefCell;

        use super::*;

        /// Test-local view that captures its `RebuildHandle` and the realm's
        /// `AsyncDriver` in `init_state` into a shared slot the test reads
        /// back — the exact seam `flui_view::element::future_builder::
        /// FutureBuilder` (the framework's own production async widget) uses,
        /// minus the `AsyncSnapshot` state machine this test does not need.
        #[derive(Clone)]
        struct AsyncCaptureProbeView {
            captured: Rc<RefCell<Option<(flui_view::RebuildHandle, flui_scheduler::AsyncDriver)>>>,
        }

        struct AsyncCaptureProbeState {
            captured: Rc<RefCell<Option<(flui_view::RebuildHandle, flui_scheduler::AsyncDriver)>>>,
        }

        impl StatefulView for AsyncCaptureProbeView {
            type State = AsyncCaptureProbeState;

            fn create_state(&self) -> Self::State {
                AsyncCaptureProbeState {
                    captured: Rc::clone(&self.captured),
                }
            }
        }

        impl ViewState<AsyncCaptureProbeView> for AsyncCaptureProbeState {
            fn init_state(&mut self, ctx: &dyn flui_view::BuildContext) {
                if let Some(driver) = ctx.async_driver() {
                    *self.captured.borrow_mut() = Some((ctx.rebuild_handle(), driver));
                }
            }

            fn build(
                &self,
                _view: &AsyncCaptureProbeView,
                _ctx: &dyn flui_view::BuildContext,
            ) -> impl IntoView {
                flui_widgets::SizedBox::new(0.0, 0.0)
            }
        }

        impl flui_view::View for AsyncCaptureProbeView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateful(self)
            }
        }

        /// The mandatory async-task-disposition audit: `AsyncDriver` is
        /// realm-level (shared by every presentation in a realm), so an
        /// in-flight task spawned from a presentation that later closes is
        /// NOT cancelled — it keeps getting polled until it completes. Its
        /// completion reaches back into a tree through a `RebuildHandle`,
        /// which routes through `ExternalBuildScheduler` — an
        /// `Arc<Mutex<HashMap<ElementId, _>>>` inbox minted fresh per
        /// `BuildOwner` at construction, never shared or reused across
        /// owners (`crates/flui-view/src/owner/build_owner.rs`'s
        /// `external_scheduler`). A stale completion's `schedule()` call
        /// therefore writes into ITS OWN (orphaned, since that `BuildOwner`
        /// already dropped) inbox — there is no shared table keyed only by
        /// raw `ElementId` for it to alias a live sibling's entry in, even
        /// when the two owners' trees happen to reuse the same numeral (the
        /// identical-numeral hazard `GlobalKeyRegistryComposite` exists for
        /// is a DIFFERENT registry; this one was never shared to begin
        /// with). This test proves it end to end rather than resting on
        /// that code reading alone: no fix was needed here, and this test is
        /// the evidence for that, not a description of one.
        #[test]
        fn async_completion_after_presentation_teardown_fails_closed_no_sibling_reach() {
            let mut realm = UiRealm::for_test();
            // `realm` starts with exactly one presentation; call it "A" and
            // install a second, "B", to observe.
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            let captured = Rc::new(RefCell::new(None));
            let probe = AsyncCaptureProbeView {
                captured: Rc::clone(&captured),
            };
            realm
                .enter(|realm| realm.attach_root_widget(&probe))
                .expect("A mounts the capture probe");
            // `attach_root_widget` only creates the root element and
            // schedules the first build -- `init_state` (where the probe
            // captures its handles) does not run until that build pass
            // actually happens, exactly like `a1_autowrap_causes_
            // registration_after_build_pass` above.
            let _ = realm.enter(|realm| {
                realm.draw_frame_entered(BoxConstraints::tight(flui_types::Size::new(
                    px(20.0),
                    px(20.0),
                )))
            });

            let (rebuild_handle, driver) = captured
                .borrow_mut()
                .take()
                .expect("init_state must have captured both handles");

            // Spawn on the REALM's shared driver — exactly what a real
            // async widget on presentation A would have done. Nothing
            // drives it to completion until explicitly polled below, well
            // after A has already closed. `Arc<AtomicBool>`, not
            // `Rc<Cell<bool>>`: `BoxedTask` requires `Send` (the driver is
            // shared across threads even though polling only ever happens
            // on the frame thread — see `AsyncDriver`'s own doc).
            let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let ran_marker = Arc::clone(&ran);
            let _token = driver.spawn_local(Box::pin(async move {
                rebuild_handle.schedule(flui_foundation::RebuildReason::AsyncCompletion);
                ran_marker.store(true, Ordering::Relaxed);
            }));

            // Close A through the real consolidated teardown path —
            // production never cancels this realm-level `AsyncDriver`'s
            // in-flight tasks (see this test's own doc and
            // `UiRealm::close_presentation_entered`'s).
            assert!(realm.close_presentation_entered(a_id), "A was installed");

            let b_has_pending_builds = || {
                realm
                    .presentations
                    .get(b_id)
                    .expect("B still installed")
                    .widgets()
                    .has_pending_builds()
            };
            assert!(
                !b_has_pending_builds(),
                "precondition: B has no pending build before A's stale task runs"
            );

            // Poll the driver to completion — A's task runs, its captured
            // `RebuildHandle` schedules against A's own (now-orphaned)
            // inbox.
            realm.scheduler().drive_frame_with_lane(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {},
                realm.local_post_frame_lane(),
            );
            assert!(
                ran.load(Ordering::Relaxed),
                "A's stale task must still run to completion"
            );

            assert!(
                !b_has_pending_builds(),
                "A's stale completion must not have reached B's build inbox"
            );
        }
    }

    // ========================================================================
    // Dispose-during-teardown isolation (ADR-0043 §5 step 3)
    // ========================================================================
    mod dispose_teardown_isolation {
        use std::cell::{Cell, RefCell};

        use super::*;

        /// Captures its own `RebuildHandle` in `init_state` (capabilities are
        /// installed then, per port-check trigger #22) and schedules through
        /// it from `dispose`, exactly the shape a real widget's cleanup path
        /// takes (e.g. cancelling a subscription and requesting one final
        /// rebuild to reflect that).
        #[derive(Clone)]
        struct DisposeProbeView {
            handle_slot: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
            disposed: Rc<Cell<bool>>,
        }

        struct DisposeProbeState {
            handle_slot: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
            disposed: Rc<Cell<bool>>,
            handle: Option<flui_view::RebuildHandle>,
        }

        impl StatefulView for DisposeProbeView {
            type State = DisposeProbeState;

            fn create_state(&self) -> Self::State {
                DisposeProbeState {
                    handle_slot: Rc::clone(&self.handle_slot),
                    disposed: Rc::clone(&self.disposed),
                    handle: None,
                }
            }
        }

        impl ViewState<DisposeProbeView> for DisposeProbeState {
            fn init_state(&mut self, ctx: &dyn flui_view::BuildContext) {
                let handle = ctx.rebuild_handle();
                *self.handle_slot.borrow_mut() = Some(handle.clone());
                self.handle = Some(handle);
            }

            fn build(
                &self,
                _view: &DisposeProbeView,
                _ctx: &dyn flui_view::BuildContext,
            ) -> impl IntoView {
                flui_widgets::SizedBox::new(0.0, 0.0)
            }

            fn dispose(&mut self) {
                // Capabilities installed at init_state are still valid here
                // (nothing about this presentation's realm-shared dispatch
                // handles has been touched by teardown step 3 alone) —
                // schedule through this element's OWN handle, exactly a real
                // cleanup hook would.
                if let Some(handle) = &self.handle {
                    handle.schedule(flui_foundation::RebuildReason::StateChange);
                }
                self.disposed.set(true);
            }
        }

        impl flui_view::View for DisposeProbeView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateful(self)
            }
        }

        /// A `State::dispose()` hook running as part of `UiRealm::
        /// close_presentation_entered`'s teardown (step 3) schedules through
        /// its OWN `RebuildHandle` — proving it reaches only its own (about
        /// to be dropped) tree, never a live sibling's, by the same
        /// `ExternalBuildScheduler`-is-minted-one-per-owner argument
        /// `close_presentation_entered`'s own doc makes. "Dead services" — this
        /// presentation's OWN focus/IME, already closed by teardown step 2
        /// before step 3's detach runs — failing closed rather than
        /// panicking is covered by `presentation.rs`'s
        /// `text_input_handle_is_bound_to_the_owned_text_input_state`; this
        /// test does not re-derive that, only the sibling-reach half.
        #[test]
        fn dispose_during_teardown_cannot_reach_sibling_or_dead_services() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            let handle_slot = Rc::new(RefCell::new(None));
            let disposed = Rc::new(Cell::new(false));
            let probe = DisposeProbeView {
                handle_slot: Rc::clone(&handle_slot),
                disposed: Rc::clone(&disposed),
            };
            // Through the normal `UiRealm::attach_root_widget` auto-wrap
            // (`FocusRoot`/`VsyncScope`/`GestureArenaScope`), so this probe
            // is a DESCENDANT of the mounted root, not the root itself —
            // exercising `WidgetsBinding::detach_root_widget`'s cascading
            // `remove_subtree` teardown (not just a single-node `remove`),
            // the same depth a real widget tree has.
            realm
                .enter(|realm| realm.attach_root_widget(&probe))
                .expect("A mounts the dispose probe");
            let _ = realm.enter(|realm| {
                realm.draw_frame_entered(BoxConstraints::tight(flui_types::Size::new(
                    px(20.0),
                    px(20.0),
                )))
            });
            assert!(
                handle_slot.borrow().is_some(),
                "init_state must have captured the rebuild handle"
            );

            fn b_has_pending_builds(realm: &UiRealm, b_id: PresentationId) -> bool {
                realm
                    .presentations
                    .get(b_id)
                    .expect("B still installed")
                    .widgets()
                    .has_pending_builds()
            }
            assert!(
                !b_has_pending_builds(&realm, b_id),
                "precondition: B has no pending build before A tears down"
            );

            assert!(
                realm.close_presentation_entered(a_id),
                "A must have been installed and removable"
            );

            assert!(
                disposed.get(),
                "close_presentation_entered's detach_root_widget must have run A's dispose hook"
            );
            assert!(
                !b_has_pending_builds(&realm, b_id),
                "A's dispose-time schedule() must not have reached B's build inbox"
            );
        }
    }

    // ========================================================================
    // Closing one presentation must be structurally invisible to a
    // surviving sibling (ADR-0043's end-state invariant)
    // ========================================================================
    mod closing_one_presentation_is_invisible_to_siblings {
        use super::*;

        /// Oracle: a LAYER-TREE COMPARE (`format!("{:?}", ...)` on the
        /// produced `LayerTree`), never a rebuild/flush count — B's own
        /// render output, not merely whether B ran, must be byte-for-byte
        /// unaffected by A closing.
        #[test]
        fn closing_presentation_a_leaves_sibling_layer_tree_identical() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            // Mount independent content on both presentations.
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
                .expect("A mounts");
            realm
                .enter(|realm| {
                    realm
                        .presentations
                        .get(b_id)
                        .expect("B installed")
                        .widgets()
                        .attach_root_widget(&flui_widgets::SizedBox::new(30.0, 30.0))
                })
                .expect("B mounts");

            let constraints = BoxConstraints::tight(flui_types::Size::new(px(50.0), px(50.0)));
            let b_layer_tree_before = realm.enter(|realm| {
                let b = realm.presentations.get(b_id).expect("B installed");
                match UiRealm::draw_frame_for_presentation(b, constraints) {
                    FramePaintOutcome::Painted(scene) => format!("{:?}", scene.layer_tree()),
                    FramePaintOutcome::Idle => panic!("B's first frame must paint, got Idle"),
                    FramePaintOutcome::Errored => {
                        panic!("B's first frame must paint, got Errored")
                    }
                }
            });

            assert!(
                realm.close_presentation_entered(a_id),
                "A must have been installed and removable"
            );

            let b_layer_tree_after = realm.enter(|realm| {
                let b = realm.presentations.get(b_id).expect("B still installed");
                // The first draw already settled B (nothing left dirty), so
                // an unconditional second draw would correctly report Idle
                // regardless of A -- that would prove nothing about B's
                // CONTENT. Force B's root render node dirty directly so this
                // second draw genuinely re-produces its layer tree from
                // scratch, the same content as the first draw, to compare
                // against it (a rebuild alone is not enough: reconciling an
                // unchanged `SizedBox` config is legitimately a no-op that
                // marks nothing dirty).
                b.pipeline().with_mut(|owner| {
                    if let Some(root_id) = owner.root_id() {
                        owner.mark_needs_paint(root_id);
                    }
                });
                match UiRealm::draw_frame_for_presentation(b, constraints) {
                    FramePaintOutcome::Painted(scene) => format!("{:?}", scene.layer_tree()),
                    FramePaintOutcome::Idle => {
                        panic!("B's post-A-close frame must still paint, got Idle")
                    }
                    FramePaintOutcome::Errored => {
                        panic!("B's post-A-close frame must still paint, got Errored")
                    }
                }
            });

            assert_eq!(
                b_layer_tree_before, b_layer_tree_after,
                "B's own layer tree must be byte-for-byte identical before and \
                 after A closes -- nothing about B's content changed, so \
                 nothing about its render output may either"
            );
        }

        /// A `RepaintHandle` obtained from presentation A's pipeline BEFORE
        /// A closes, and still held afterward, must fail closed
        /// (`DirtySendError::OwnerGone`) rather than panic when used — the
        /// underlying `PipelineOwner`'s dirty-request channel receiver drops
        /// along with A's `PresentationState`, so the handle's sender side
        /// simply finds nobody listening.
        #[test]
        fn dropped_presentations_surviving_pipeline_handles_fail_closed() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();

            let render_id = realm.presentations.primary().pipeline().with_mut(|owner| {
                owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(
                    flui_objects::RenderColoredBox::red(10.0, 10.0),
                ))
            });
            let repaint_handle = realm
                .presentations
                .primary()
                .pipeline()
                .with(|owner| owner.repaint_handle(render_id))
                .expect("a freshly inserted node has a live repaint handle");

            assert!(
                repaint_handle.mark_needs_layout().is_ok(),
                "precondition: the handle works while A is still alive"
            );

            assert!(realm.close_presentation_entered(a_id), "A was installed");

            assert!(
                matches!(
                    repaint_handle.mark_needs_layout(),
                    Err(flui_rendering::pipeline::DirtySendError::OwnerGone)
                ),
                "a RepaintHandle surviving its presentation's teardown must \
                 fail closed, not panic and not silently succeed"
            );
        }
    }

    /// This module's own red-exploits: the segment gate's poll-decision
    /// equivalence table, the close-mid-animation probe, and the
    /// zero-produce (demand-driven idle) invariant. Distinct from
    /// `frame_pipeline_and_vsync` (which pins the PRE-EXISTING deferral/vsync
    /// behavior unedited) — these are the NEW clock-level guarantees this
    /// slice adds.
    mod frame_clock_segment_gate {
        use std::sync::Mutex as StdMutex;

        use tracing::field::{Field, Visit};
        use tracing::{Event, Subscriber};
        use tracing_subscriber::layer::{Context, SubscriberExt as _};
        use tracing_subscriber::registry::LookupSpan;
        use tracing_subscriber::{Layer as TracingLayer, Registry};

        use super::*;

        #[derive(Clone, Default)]
        struct FrameEventCapture(Arc<StdMutex<Vec<(String, String)>>>);

        struct FrameFieldVisitor<'a>(&'a mut Vec<(String, String)>);

        impl Visit for FrameFieldVisitor<'_> {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0.push((field.name().to_owned(), format!("{value:?}")));
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.push((field.name().to_owned(), value.to_owned()));
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                self.0.push((field.name().to_owned(), value.to_string()));
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                self.0.push((field.name().to_owned(), value.to_string()));
            }
        }

        impl<S> TracingLayer<S> for FrameEventCapture
        where
            S: Subscriber + for<'lookup> LookupSpan<'lookup>,
        {
            fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
                if event.metadata().target() != "flui.frame" {
                    return;
                }
                let mut fields = Vec::new();
                event.record(&mut FrameFieldVisitor(&mut fields));
                self.0
                    .lock()
                    .expect("BUG: frame capture is locked only by this test")
                    .extend(fields);
            }
        }

        fn table_constraints() -> BoxConstraints {
            BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
        }

        /// A realm with a minimal root widget already attached -- this
        /// module's own copy: `frame_pipeline_and_vsync::mount_root` is
        /// private to that sibling module.
        fn mount_root_here() -> UiRealm {
            let realm = UiRealm::for_test();
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(1.0, 1.0)))
                .expect("attach succeeds");
            realm
        }

        /// A raster backend that always reports a successful present --
        /// this module's own minimal double (the sibling
        /// `frame_pipeline_and_vsync` module's `ScriptedRasterBackend` is
        /// private to it).
        struct AlwaysPresentBackend;

        impl RasterBackend for AlwaysPresentBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
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

        /// A raster backend that always reports `SurfaceLost` — this
        /// module's own minimal double for the "a real submit genuinely
        /// failed" half of the frames-dropped-vs-deferred pin below.
        struct AlwaysErrorsBackend;

        impl RasterBackend for AlwaysErrorsBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                Err(EngineError::SurfaceLost)
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

        /// A raster backend that always reports `DeviceLost` — this
        /// module's own minimal double for
        /// `device_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame`
        /// below, kept separate from [`AlwaysErrorsBackend`] (which is
        /// dedicated to the `SurfaceLost` pin) so each test's backend name
        /// says exactly which failure it scripts.
        struct AlwaysDeviceLostBackend;

        impl RasterBackend for AlwaysDeviceLostBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                Err(EngineError::DeviceLost)
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

        /// A raster backend that always reports `SurfaceValidation` — this
        /// module's own minimal double for
        /// `surface_validation_retry_preserves_the_original_input_epoch_for_the_presented_frame`
        /// below, the `SurfaceValidation` counterpart of
        /// [`AlwaysDeviceLostBackend`].
        struct AlwaysSurfaceValidationBackend;

        impl RasterBackend for AlwaysSurfaceValidationBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                Err(EngineError::SurfaceValidation)
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

        /// END-STATE INVARIANT: at N=1 -- a clock that is
        /// never hidden and never backpressured -- the segment gate's
        /// decision (`clock.poll().should_run_segment()`, fed `Dirty`
        /// demand from `take_redraw_pending() || has_pending_work()`) is
        /// equivalent to that same union read directly, the predicate it
        /// replaces, over every reachable combination of the two inputs.
        /// This table covers the NOT-deferred subspace; first-frame
        /// deferral is a THIRD, orthogonal axis that does not perturb it —
        /// `should_run_segment()` is `true` for both `Produce` and
        /// `ProduceWithheld`, so a deferred presentation reaches the exact
        /// same "did the segment run" answer as an undeferred one with the
        /// same demand (see `segment_still_runs_by_the_same_predicate_while_
        /// deferred` below for that companion proof; deferral only changes
        /// whether `render_frame_entered` may submit the result):
        ///
        /// | woken | pending build | segment runs |
        /// |-------|---------------|---------------|
        /// | false | false         | false         |
        /// | true  | false         | true          |
        /// | false | true          | true          |
        /// | true  | true          | true          |
        #[test]
        fn segment_runs_iff_woken_or_has_pending_work_over_the_full_table() {
            for (case, woken, attach_root, expect_segment_runs) in [
                ("neither", false, false, false),
                ("woken_only", true, false, true),
                ("pending_work_only", false, true, true),
                ("both", true, true, true),
            ] {
                let realm = UiRealm::for_test();
                if attach_root {
                    realm
                        .enter(|realm| {
                            realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
                        })
                        .expect("attach succeeds");
                }
                // `attach_root_widget` itself calls `request_redraw()`,
                // which marks the wake bit too -- normalize it to exactly
                // this case's own `woken` input so the two axes stay
                // independent, rather than whatever attaching happened to
                // leave behind.
                let _ = realm.presentations.primary().take_redraw_pending();
                if woken {
                    realm.presentations.primary().mark_redraw_pending();
                }

                let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

                assert_eq!(
                    realm.presentations.primary().flush_count() == 1,
                    expect_segment_runs,
                    "case {case}: woken={woken} attach_root={attach_root} -- whether the \
                     segment ran must equal woken || has_pending_work"
                );
            }
        }

        /// Companion: with nothing dirty a second pump must not re-run the
        /// segment either (the table's `false/false` row still holds after
        /// a settled first frame, not just from a fresh realm).
        #[test]
        fn a_settled_presentation_stays_at_zero_flushes_across_repeated_idle_pumps() {
            let realm = UiRealm::for_test();
            for _ in 0..5 {
                let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            }
            assert_eq!(realm.presentations.primary().flush_count(), 0);
        }

        /// The equivalence table above only varies `woken`/`has_pending_work`
        /// with an undeferred clock; this proves first-frame deferral does
        /// not perturb the segment-runs answer for the SAME demand -- the
        /// regression this exact companion exists to catch is a `poll`
        /// that treats deferral as a reason to skip the segment (it must
        /// only ever withhold the submit; see `.flutter/packages/flutter/
        /// lib/src/rendering/binding.dart:582-599`).
        #[test]
        fn segment_still_runs_by_the_same_predicate_while_deferred() {
            let realm = mount_root_here();
            realm.presentations.primary().clock().defer();

            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

            assert_eq!(
                realm.presentations.primary().flush_count(),
                1,
                "attach's own pending build must still run the segment even though deferred"
            );
        }

        /// Item 8 (wake-bit -> clock-latch survival across a full
        /// defer/lift round trip): a wake bit marked before deferral begins
        /// is consumed by the very next (withheld) pump -- the segment
        /// genuinely runs on it -- and neither an already-settled pump
        /// during the deferral, nor lifting with nothing newly dirty,
        /// produces a second, spurious flush.
        #[test]
        fn a_wake_bit_marked_before_deferral_is_consumed_by_exactly_one_flush() {
            let realm = UiRealm::for_test(); // no attach: the wake bit is the ONLY demand source
            let presentation = realm.presentations.primary();

            presentation.mark_redraw_pending();
            presentation.clock().defer();

            // Deferred, but the segment still runs on the marked wake bit
            // (Flutter parity) -- this pump's demand is consumed here.
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert_eq!(
                presentation.flush_count(),
                1,
                "the wake bit marked before deferral must be consumed by exactly one flush"
            );

            // Nothing newly dirty: settled, still deferred.
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert_eq!(
                presentation.flush_count(),
                1,
                "still deferred and settled -- no new flush"
            );

            presentation.clock().lift();
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert_eq!(
                presentation.flush_count(),
                1,
                "lifting with nothing newly marked must not conjure a spurious extra flush -- \
                 the earlier withheld poll already consumed the only demand there was"
            );
        }

        /// Item 9 (the `mark_first_frame_sent` timing decision, pinned): an
        /// idle pump (genuinely nothing dirty, so the segment never even
        /// runs) must NOT latch "first frame confirmed sent" -- only a
        /// real, successful `Produce` does. If an idle pump wrongly
        /// latched, a `defer_first_frame` issued only AFTER that idle pump
        /// would already be permanently inert; this proves it is not.
        #[test]
        fn an_idle_pump_while_deferred_does_not_latch_first_frame_sent() {
            let realm = UiRealm::for_test(); // nothing attached: no demand at all
            realm.defer_first_frame();

            let mut backend = AlwaysPresentBackend;
            let presented = realm.render_frame_entered(&mut backend);
            assert!(!presented, "nothing to present with no root attached");

            // If the idle pump above had (wrongly) latched first_frame_sent,
            // this deferral would already be permanently inert. Attach a
            // root now (real demand) and confirm it is STILL deferred.
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(1.0, 1.0)))
                .expect("attach succeeds");
            let presented = realm.render_frame_entered(&mut backend);
            assert!(
                !presented,
                "the earlier idle pump must not have latched first_frame_sent -- this \
                 presentation is still deferred"
            );
            // But the segment DID run (Flutter parity): the attach's own
            // pending build was serviced even though withheld.
            assert_eq!(realm.presentations.primary().flush_count(), 1);

            realm.allow_first_frame();
            let presented = realm.render_frame_entered(&mut backend);
            assert!(
                presented,
                "lifting must finally let the withheld content through"
            );
        }

        /// Close-mid-animation probe: a presentation closing
        /// while a controller registered on ITS OWN `Vsync` is still running
        /// must not panic, must stop ticking that controller immediately
        /// (its registry dies with the presentation), and must leave a
        /// SIBLING presentation's own clock producing exactly as before.
        #[test]
        fn closing_a_presentation_mid_animation_stops_its_ticks_without_panicking_or_touching_the_sibling()
         {
            use std::time::Duration;

            use flui_animation::{Animation, AnimationController};

            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            let controller = AnimationController::new(
                Duration::from_millis(200),
                &flui_scheduler::UpdateScheduler::new(),
            );
            {
                let b = realm.presentations.get(b_id).expect("B installed");
                b.vsync().register(controller.clone());
            }
            controller.forward().expect("fresh controller forwards");

            realm.set_now_secs_for_test(0.0);
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            realm.set_now_secs_for_test(0.05);
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            let value_before_close = controller.value();
            assert!(
                value_before_close > 0.0,
                "sanity: the controller must have advanced before B closes"
            );

            // A produces independently of B -- establish the baseline this
            // probe checks survives B's teardown.
            let a = realm.presentations.get(a_id).expect("A installed");
            a.mark_redraw_pending();
            let a_produced_before = a.clock().produced_count();
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .clock()
                    .produced_count()
                    > a_produced_before,
                "sanity: A's own clock must produce independently of B before the close"
            );

            assert!(
                realm.close_presentation_entered(b_id),
                "B must have been installed and removable -- no panic reaching this line \
                 is itself part of the probe"
            );

            // B is gone from the forest; nothing ticks its controller
            // further, no matter how far the virtual clock advances.
            realm.set_now_secs_for_test(0.5);
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert_eq!(
                controller.value(),
                value_before_close,
                "B's own Vsync registry died with its presentation -- the controller must \
                 not be ticked any further, not even to completion"
            );

            // A's own clock is unaffected by B's teardown.
            let a = realm.presentations.get(a_id).expect("A installed");
            a.mark_redraw_pending();
            let a_produced_before_second = a.clock().produced_count();
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .clock()
                    .produced_count()
                    > a_produced_before_second,
                "A's own clock must keep producing normally after B's presentation closes"
            );

            controller.dispose();
        }

        /// Driver-loop hybrid: a running controller with
        /// NO other tree-visible dirty state must still flush the segment
        /// on every tick it's running -- Flutter's own `Ticker`-driven
        /// `scheduleFrame` behavior, where the running ticker alone is
        /// sufficient (a listener's own `notifyListeners` is not required
        /// for the frame to be scheduled). Before this slice, `Animation`
        /// demand was never marked in production wiring (`draw_frame_
        /// entered`'s vsync-continuation loop only called `wake_frame()`,
        /// never `presentation.clock().mark_demand`), so a bare running
        /// controller with nothing else dirty would silently never flush
        /// -- this is the regression this test exists to catch.
        #[test]
        fn a_running_controller_with_no_other_dirty_state_still_flushes_every_tick() {
            use std::time::Duration;

            use flui_animation::AnimationController;

            let realm = UiRealm::for_test();
            let controller = AnimationController::new(
                Duration::from_millis(100),
                &flui_scheduler::UpdateScheduler::new(),
            );
            realm.vsync().register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            realm.set_now_secs_for_test(0.0);
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert_eq!(
                realm.presentations.primary().flush_count(),
                1,
                "a running controller alone, with no widget tree attached and no other \
                 dirty state, must still flush this pump's segment"
            );

            realm.set_now_secs_for_test(0.05);
            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            assert_eq!(
                realm.presentations.primary().flush_count(),
                2,
                "and again on the next mid-run tick"
            );

            controller.dispose();
        }

        /// Edge-trigger discipline, end to end: N ticks
        /// of a running controller that land while capacity never frees
        /// must collapse into exactly one platform-facing wake, through
        /// the REAL production call site — not just
        /// `FrameClock::try_arm_redraw_request` in isolation (see
        /// flui-scheduler's own unit test for that, at the bare-clock
        /// level). A produce (capacity freed) must re-arm it.
        ///
        /// Drives `render_frame_entered`, not the bare `draw_frame_entered`
        /// this test used before: the continuation-wake check now runs
        /// AFTER `mark_rendered()`, inside `render_frame_entered`, not
        /// inside `draw_frame_entered`'s vsync loop — see that
        /// method's own comment for why the move was necessary: raising
        /// the wake before `mark_rendered()` let the SAME callback silently
        /// clobber it). One consequence, pinned here rather than left
        /// implicit: the callback that frees capacity and produces ALSO
        /// re-arms and wakes again in that SAME callback now (the check
        /// runs after `poll()` already reset the latch), not on a
        /// SEPARATE, later callback the way it did when the check ran
        /// before `poll()`.
        #[test]
        fn n_ticks_under_backpressure_wake_the_platform_exactly_once_then_rearm() {
            use std::time::Duration;

            use flui_animation::AnimationController;

            let (wake, wake_count) = super::counting_wake();
            let realm = super::new_runtime(wake).expect("runtime");
            let controller = AnimationController::new(
                Duration::from_secs(1),
                &flui_scheduler::UpdateScheduler::new(),
            );
            realm.vsync().register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            let clock = realm.presentations.primary().clock();
            clock.set_max_in_flight(1);
            clock.record_submit(); // at capacity for the whole loop below
            wake_count.store(0, Ordering::Relaxed);

            let mut backend = AlwaysPresentBackend;

            for tick in 1..=5 {
                realm.set_now_secs_for_test(0.01 * f64::from(tick));
                let _ = realm.render_frame_entered(&mut backend);
            }
            assert_eq!(
                wake_count.load(Ordering::Relaxed),
                1,
                "five ticks landing while capacity never frees must collapse into exactly \
                 one platform-facing wake"
            );

            // Free capacity: THIS SAME callback's `render_frame_entered`
            // now both produces (`poll()` grants a produce, clearing the
            // mask and the armed latch) AND re-arms a fresh wake for the
            // controller's continuation in its own post-render step --
            // the two are no longer split across two separate callbacks.
            clock.record_retire();
            realm.set_now_secs_for_test(0.10);
            let _ = realm.render_frame_entered(&mut backend);
            assert_eq!(
                wake_count.load(Ordering::Relaxed),
                2,
                "the produce-and-continue callback re-arms in the SAME callback now that the \
                 continuation check runs after mark_rendered(), which itself runs after \
                 poll() already reset the latch"
            );

            controller.dispose();
        }

        /// END-STATE INVARIANT (the demand-driven-idle headline):
        /// with an empty demand mask and no gating, `poll` returns
        /// `Skip(NoDemand)` every pump and the segment never runs -- zero
        /// produces, zero pipeline flushes -- no matter how many times the
        /// realm is pumped for purely logical reasons (no attach, no
        /// redraw request, nothing ever dirtied).
        #[test]
        fn an_idle_presentation_produces_and_flushes_exactly_zero_over_many_pumps() {
            let realm = UiRealm::for_test();

            for pump in 0..50 {
                realm.set_now_secs_for_test(f64::from(pump) * 0.016);
                let (_, outcome) =
                    realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
                assert!(
                    matches!(outcome, FramePaintOutcome::Idle),
                    "pump {pump}: an idle presentation must never produce anything but Idle"
                );
            }

            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                0,
                "an idle presentation's clock must grant zero produces over any number of pumps"
            );
            assert_eq!(
                realm.presentations.primary().flush_count(),
                0,
                "an idle presentation's segment must never run -- zero pipeline flushes"
            );
        }

        /// The SAME invariant as the test above, but through the actual
        /// PRODUCTION call sequence rather than `draw_frame_entered` alone:
        /// `bootstrap_desktop`'s `on_request_frame` closure calls
        /// `UiRealm::record_compositor_tick` unconditionally, on every
        /// single pump, before ever reaching `draw_frame_entered`. An
        /// earlier version of `FrameClock::record_compositor_tick` marked
        /// `DemandKind::Host` unconditionally, which -- because this
        /// exact sequence runs on every desktop pump -- made `poll`
        /// permanently unable to return `Skip(NoDemand)` there: the
        /// demand-mask gate, the entire point of this clock, was inert on
        /// the one path that matters. This test is what would have caught that;
        /// the test above alone does not, because it never calls
        /// `record_compositor_tick`.
        #[test]
        fn an_idle_presentation_stays_idle_through_the_production_call_sequence_with_compositor_ticks()
         {
            let realm = UiRealm::for_test();
            let mut now = web_time::Instant::now();

            for pump in 0..50 {
                realm.set_now_secs_for_test(f64::from(pump) * 0.016);
                now += std::time::Duration::from_millis(16);
                realm.record_compositor_tick(now);
                let (_, outcome) =
                    realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
                assert!(
                    matches!(outcome, FramePaintOutcome::Idle),
                    "pump {pump}: recording a compositor tick every single pump, exactly as \
                     production does, must not by itself make an idle presentation produce \
                     anything but Idle"
                );
            }

            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                0,
                "recording a compositor tick every pump, through the real production \
                 sequence, must not grant a single produce on an otherwise-idle presentation"
            );
            assert_eq!(
                realm.presentations.primary().flush_count(),
                0,
                "and must never run the segment either"
            );
        }

        /// The instant-response half of the same invariant: after an idle
        /// stretch with a settled tree, dirtying the root render node
        /// produces exactly one frame on the very next pump -- kills
        /// "wake-to-first-frame costs an extra pump".
        #[test]
        fn a_single_dirty_mark_after_an_idle_stretch_produces_on_the_very_next_pump() {
            let realm = UiRealm::for_test();
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
                .expect("attach succeeds");
            // Settle: the attach itself is one produce: the FIRST pump below
            // paints it, and every pump after that finds the tree quiescent.
            for pump in 0..20 {
                realm.set_now_secs_for_test(f64::from(pump) * 0.016);
                let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            }
            let produced_while_settled = realm.presentations.primary().clock().produced_count();
            assert_eq!(
                produced_while_settled, 1,
                "sanity: exactly the attach's own first paint, then genuinely idle"
            );

            // Dirty the root render node directly (the same real signal
            // `has_pending_work` reads), then pump exactly once more.
            realm.presentations.primary().pipeline().with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_paint(root_id);
                }
            });
            let (_, outcome) = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

            assert!(
                matches!(outcome, FramePaintOutcome::Painted(_)),
                "the very next pump after the dirty mark must produce a painted frame, \
                 not merely go dirty and wait"
            );
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                produced_while_settled + 1,
                "exactly one additional produce -- no wake-to-first-frame delay"
            );
        }

        // ================================================================
        // Hidden-surface gating: `FrameClock::set_hidden` wired to
        // `UiRealm::set_presentation_hidden`, and the ungate->wake edge
        // that keeps a demand mark from stranding the presentation.
        // ================================================================

        /// A raster backend that always presents, counting how many times
        /// `render_scene` actually ran -- the "zero GPU submits" oracle for
        /// the tests below. Distinct from the sibling `frame_pipeline_and_
        /// vsync` module's own `CountingRasterBackend`, which is private to
        /// that module.
        struct HiddenGateCountingBackend {
            render_scene_calls: u32,
        }

        impl HiddenGateCountingBackend {
            fn new() -> Self {
                Self {
                    render_scene_calls: 0,
                }
            }
        }

        impl RasterBackend for HiddenGateCountingBackend {
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

        /// END-STATE INVARIANT: a hidden presentation's segment never runs
        /// and its backend never receives a submit, however many times it
        /// is dirtied and pumped.
        #[test]
        fn gated_presentation_produces_zero_segments_and_zero_submits_while_hidden() {
            let realm = mount_root_here();
            let presentation_id = realm.presentations.primary().id();
            let mut backend = HiddenGateCountingBackend::new();

            // Settle the initial attach paint first, so the assertions below
            // isolate exactly what happens WHILE hidden -- baselines
            // captured AFTER settling, not absolute zero (the settle itself
            // is one legitimate submit).
            let _ = realm.render_frame_entered(&mut backend);
            realm.set_presentation_hidden(presentation_id, true);
            let flush_count_before = realm.presentations.primary().flush_count();
            let produced_before = realm.presentations.primary().clock().produced_count();
            let submits_before = backend.render_scene_calls;

            for pump in 0..10 {
                realm.set_now_secs_for_test(f64::from(pump) * 0.016);
                // Dirty the tree every pump -- demand exists, but the
                // presentation-level gate must still withhold the segment.
                realm.presentations.primary().pipeline().with_mut(|owner| {
                    if let Some(root_id) = owner.root_id() {
                        owner.mark_needs_paint(root_id);
                    }
                });
                let presented = realm.render_frame_entered(&mut backend);
                assert!(
                    !presented,
                    "pump {pump}: a hidden presentation must never present"
                );
            }

            assert_eq!(
                realm.presentations.primary().flush_count(),
                flush_count_before,
                "a hidden presentation's segment must never run, however often it is dirtied"
            );
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                produced_before,
                "a hidden presentation must never grant a produce"
            );
            assert_eq!(
                backend.render_scene_calls, submits_before,
                "a hidden presentation must never reach the raster backend -- zero GPU submits"
            );
        }

        /// A running controller (`Animation` demand, marked unconditionally
        /// every pump per the vsync-continuation wiring) must not spin a
        /// hidden presentation: the gate short-circuits before the demand
        /// mask is even consulted. Kills "a gated presentation with a
        /// running controller keeps producing".
        #[test]
        fn gated_and_animating_presentation_does_not_spin() {
            use std::time::Duration;

            use flui_animation::AnimationController;

            let realm = UiRealm::for_test();
            let presentation_id = realm.presentations.primary().id();
            let controller = AnimationController::new(
                Duration::from_secs(1),
                &flui_scheduler::UpdateScheduler::new(),
            );
            realm.vsync().register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            realm.set_presentation_hidden(presentation_id, true);

            for pump in 0..20 {
                realm.set_now_secs_for_test(f64::from(pump) * 0.01);
                let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
            }

            assert_eq!(
                realm.presentations.primary().flush_count(),
                0,
                "a gated presentation with a running controller must not spin -- \
                 its segment must never run"
            );
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                0,
                "and must never grant a produce"
            );

            controller.dispose();
        }

        /// The liveness-critical ungate->wake edge: a demand mark that
        /// arrives while hidden is retained by the mask but produces
        /// nothing; unhiding must both (a) let the very next pump produce
        /// exactly once, and (b) actually WAKE the platform loop itself --
        /// not merely be observable if some unrelated caller happens to
        /// pump again. Without the unconditional wake in
        /// `set_presentation_hidden`, this demand would strand: nothing
        /// self-wakes an idle `ControlFlow::Wait` loop.
        #[test]
        fn occlude_then_dirty_then_unocclude_wakes_exactly_once_and_produces_exactly_once() {
            let (wake, wake_count) = super::counting_wake();
            let realm = super::new_runtime(wake).expect("runtime");
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
                .expect("attach succeeds");
            let presentation_id = realm.presentations.primary().id();
            let mut backend = HiddenGateCountingBackend::new();

            // Settle the attach's own first paint.
            realm.set_now_secs_for_test(0.0);
            let _ = realm.render_frame_entered(&mut backend);
            let produced_before = realm.presentations.primary().clock().produced_count();
            let submits_before = backend.render_scene_calls;

            realm.set_presentation_hidden(presentation_id, true);
            wake_count.store(0, Ordering::Relaxed);

            // Dirty the real render tree once while hidden -- a genuine
            // repaint demand, not just the wake-only redraw bit, so the
            // eventual unhide pump below actually has something to paint.
            // This ALSO fires the render pipeline's own independent,
            // pre-existing wake wire (`PipelineOwner::
            // set_on_need_visual_update` -> the realm's `wake` directly,
            // unrelated to this slice's FrameClock gate) exactly once for
            // this clean->dirty transition -- expected, not the property
            // under test. What IS under test is the DELTA at unhide below:
            // one MORE wake beyond whatever this mark already caused.
            realm.presentations.primary().pipeline().with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_paint(root_id);
                }
            });

            for pump in 1..=3 {
                realm.set_now_secs_for_test(0.016 * f64::from(pump));
                let presented = realm.render_frame_entered(&mut backend);
                assert!(!presented, "pump {pump}: must not present while hidden");
            }
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                produced_before,
                "zero produces while hidden, however many times pumped"
            );
            assert_eq!(
                backend.render_scene_calls, submits_before,
                "zero submits while hidden"
            );
            let wake_count_before_unhide = wake_count.load(Ordering::Relaxed);

            // Unocclude: the retained demand must wake the platform loop
            // exactly one MORE time (the ungate edge) beyond whatever the
            // dirty mark's own independent wake already contributed...
            realm.set_presentation_hidden(presentation_id, false);
            assert_eq!(
                wake_count.load(Ordering::Relaxed),
                wake_count_before_unhide + 1,
                "unhiding with retained demand must wake the platform loop exactly once more"
            );

            // ...and produce exactly once on the very next pump, with no
            // further external input.
            realm.set_now_secs_for_test(0.1);
            let presented = realm.render_frame_entered(&mut backend);
            assert!(presented, "the unhide pump must present");
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                produced_before + 1,
                "exactly one produce after unhiding -- no lost frame, no extra one"
            );
            assert_eq!(
                backend.render_scene_calls,
                submits_before + 1,
                "exactly one submit after unhiding"
            );

            // Settling: a further pump with nothing newly dirtied produces
            // nothing more.
            realm.set_now_secs_for_test(0.2);
            let _ = realm.render_frame_entered(&mut backend);
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                produced_before + 1,
                "no further produce once the retained demand is consumed"
            );
        }

        /// Unhiding with an EMPTY mask (nothing was ever dirtied while
        /// hidden) must not spuriously wake the platform -- the wake is
        /// conditioned on retained demand, not unconditional on every
        /// unhide.
        #[test]
        fn unoccluding_an_undirtied_presentation_does_not_wake() {
            let (wake, wake_count) = super::counting_wake();
            let realm = super::new_runtime(wake).expect("runtime");
            let presentation_id = realm.presentations.primary().id();

            realm.set_presentation_hidden(presentation_id, true);
            wake_count.store(0, Ordering::Relaxed);

            realm.set_presentation_hidden(presentation_id, false);

            assert_eq!(
                wake_count.load(Ordering::Relaxed),
                0,
                "unhiding an idle (never-dirtied) presentation must not wake the platform"
            );
        }

        /// The ungate->wake edge's OWN defining hazard, discriminated: the
        /// unhide branch must wake on RETAINED DEMAND, not on
        /// `try_arm_redraw_request`'s own latch state -- and the two only
        /// disagree when the latch was armed BEFORE the hide (from an
        /// earlier, unrelated produce) and never consumed since, because
        /// nothing polled while hidden. Set up exactly that ordering: a
        /// visible pump first arms the latch (a running controller's own
        /// continuation-wake, `render_frame_entered`'s tail) and leaves
        /// demand retained (the controller keeps running), THEN hide, THEN
        /// unhide with no further pump in between. A version gated on
        /// `try_arm_redraw_request()` instead of `demand_mask().is_empty()`
        /// finds the latch ALREADY armed (from the visible pump) and
        /// silently declines to wake -- stranding the presentation despite
        /// genuinely retained demand.
        #[test]
        fn unhide_wakes_on_retained_demand_even_when_the_latch_was_armed_before_the_hide() {
            use std::time::Duration;

            use flui_animation::AnimationController;

            let (wake, wake_count) = super::counting_wake();
            let realm = super::new_runtime(wake).expect("runtime");
            let presentation_id = realm.presentations.primary().id();
            let mut backend = HiddenGateCountingBackend::new();

            let controller = AnimationController::new(
                Duration::from_secs(1),
                &flui_scheduler::UpdateScheduler::new(),
            );
            realm.vsync().register(controller.clone());
            controller.forward().expect("fresh controller forwards");

            // One VISIBLE pump: the segment produces off the controller's
            // own Animation demand, and render_frame_entered's tail --
            // since the controller is still running -- marks a FRESH
            // Animation demand for the next pump and arms
            // try_arm_redraw_request's latch (the ONLY production call
            // site of that method). After this call: demand mask is
            // nonempty (Animation, retained), and the latch is armed
            // (true) -- both facts this test's ordering depends on.
            realm.set_now_secs_for_test(0.0);
            let _ = realm.render_frame_entered(&mut backend);
            assert_eq!(
                realm.presentations.primary().clock().demand_mask(),
                flui_scheduler::DemandMask::ANIMATION,
                "precondition: the running controller must leave Animation demand retained"
            );

            // Hide with NO further pump -- the latch stays armed from the
            // visible pump above; nothing consumes it while hidden.
            realm.set_presentation_hidden(presentation_id, true);
            wake_count.store(0, Ordering::Relaxed);

            // Unhide: retained demand is still nonempty, so this must wake
            // -- regardless of the latch's own (already-armed, therefore
            // falsely "nothing to arm") state.
            realm.set_presentation_hidden(presentation_id, false);

            assert_eq!(
                wake_count.load(Ordering::Relaxed),
                1,
                "unhiding with retained demand must wake even when \
                 try_arm_redraw_request's latch was already armed before the hide -- a \
                 latch-gated implementation would find it pre-armed and stay silent, \
                 stranding the presentation"
            );

            controller.dispose();
        }

        /// Gesture-arena deadlines are input-correctness, not frame
        /// production (plan-level rule): they remain a realm pre-segment
        /// step that runs on every logical pump regardless of this
        /// presentation's own `FrameClock` gate. A gated presentation with
        /// an in-flight long-press must still resolve it, through the
        /// arena's own callback, with zero GPU submits -- kills "deadline
        /// rides the produce gate".
        #[test]
        fn gated_long_press_resolves_through_the_arenas_own_callback_with_zero_submits() {
            use std::time::Duration;

            use flui_interaction::{
                GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
            };

            let realm = mount_root_here();
            let presentation_id = realm.presentations.primary().id();
            let mut backend = HiddenGateCountingBackend::new();

            // Settle the attach's own first paint before gating -- baselines
            // captured AFTER settling, not absolute zero.
            let _ = realm.render_frame_entered(&mut backend);
            realm.set_presentation_hidden(presentation_id, true);
            let flush_count_before = realm.presentations.primary().flush_count();
            let produced_before = realm.presentations.primary().clock().produced_count();
            let submits_before = backend.render_scene_calls;

            // Arm a real long-press against the SAME arena
            // `draw_frame_entered`'s gesture-deadline tick polls -- not a
            // scripted stand-in.
            let arena = realm.presentations.primary().gestures().arena().clone();
            let fired = Arc::new(AtomicBool::new(false));
            let fired_for_callback = Arc::clone(&fired);
            let recognizer = LongPressGestureRecognizer::with_settings(
                arena,
                GestureSettings::touch_defaults()
                    .with_long_press_timeout(Duration::from_millis(30)),
            )
            .with_on_long_press(move || fired_for_callback.store(true, Ordering::SeqCst));
            let pointer = PointerId::new(2).expect("nonzero pointer id");
            recognizer.add_pointer(pointer, flui_types::Offset::new(px(10.0), px(10.0)));

            // Real wall-clock wait past the deadline: the arena's own clock
            // is the real OS clock here (`GestureBinding::new()`), same as
            // production.
            std::thread::sleep(Duration::from_millis(60));

            // Pump while still hidden: `draw_frame_entered`'s gesture tick
            // runs unconditionally, resolving the long-press through its
            // OWN callback, even though the segment gate skips this
            // presentation's build/layout/paint/submit entirely.
            for pump in 0..3 {
                realm.set_now_secs_for_test(f64::from(pump) * 0.016);
                let presented = realm.render_frame_entered(&mut backend);
                assert!(!presented, "pump {pump}: must not present while hidden");
            }

            assert!(
                fired.load(Ordering::SeqCst),
                "the long-press must fire through the arena's own poll_deadlines tick even \
                 though this presentation is gated"
            );
            assert_eq!(
                realm.presentations.primary().flush_count(),
                flush_count_before,
                "the gated presentation's segment must never run"
            );
            assert_eq!(
                realm.presentations.primary().clock().produced_count(),
                produced_before,
                "the gated presentation must never grant a produce"
            );
            assert_eq!(
                backend.render_scene_calls, submits_before,
                "zero GPU submits -- the long-press resolves through the arena's own \
                 callback, never through the produce path"
            );
        }

        /// `UiRealm::next_wake`'s own min-over-presentations level,
        /// discriminated -- production topology is exactly one realm
        /// hosting N presentations, so this is the level a cross-realm-only
        /// mutant test cannot see. Two presentations of ONE realm, a far
        /// deadline on the primary and a near one on the secondary: the
        /// aggregate must reflect the near one regardless of which
        /// presentation is primary.
        #[test]
        fn next_wake_is_the_min_deadline_across_two_presentations_of_one_realm() {
            use std::time::{Duration, Instant};

            use flui_interaction::{
                GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
            };

            let mut realm = UiRealm::for_test();
            let b_id = realm.install_second_presentation_for_test();
            let a_id = realm.presentations.primary().id();

            let arena_a = realm
                .presentations
                .get(a_id)
                .unwrap()
                .gestures()
                .arena()
                .clone();
            let recognizer_a = LongPressGestureRecognizer::with_settings(
                arena_a,
                GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_secs(5)),
            )
            .with_on_long_press(|| {});
            recognizer_a.add_pointer(
                PointerId::new(2).expect("nonzero pointer id"),
                flui_types::Offset::new(px(10.0), px(10.0)),
            );

            let before_b = Instant::now();
            let arena_b = realm
                .presentations
                .get(b_id)
                .unwrap()
                .gestures()
                .arena()
                .clone();
            let recognizer_b = LongPressGestureRecognizer::with_settings(
                arena_b,
                GestureSettings::touch_defaults()
                    .with_long_press_timeout(Duration::from_millis(50)),
            )
            .with_on_long_press(|| {});
            recognizer_b.add_pointer(
                PointerId::new(3).expect("nonzero pointer id"),
                flui_types::Offset::new(px(20.0), px(20.0)),
            );

            let next_wake = realm
                .next_wake()
                .expect("two armed deadlines pending -- next_wake must be Some");
            let until_wake = next_wake.saturating_duration_since(before_b);
            assert!(
                until_wake < Duration::from_secs(1),
                "next_wake must reflect presentation B's near (50ms) deadline, not A's far \
                 (5s) one, even though A is the primary -- got {until_wake:?} until wake"
            );
        }

        /// `GestureArena::next_deadline`'s own min-over-members level,
        /// discriminated -- production topology is any number of arena
        /// members competing on one presentation's arena (e.g. a detector
        /// combining long-press and double-tap on the same region), so
        /// this is the level a single-presentation, single-recognizer test
        /// cannot see. Two DIFFERENT recognizer kinds on the SAME arena, a
        /// far long-press timeout and a near double-tap give-up: the
        /// aggregate must reflect the near one.
        #[test]
        fn gesture_arena_next_deadline_is_the_min_across_two_recognizers_on_one_arena() {
            use std::time::{Duration, Instant};

            use flui_interaction::{
                DoubleTapGestureRecognizer, GestureRecognizer, GestureSettings,
                LongPressGestureRecognizer, PointerId,
            };

            let realm = UiRealm::for_test();
            let arena = realm.presentations.primary().gestures().arena().clone();

            let long_press = LongPressGestureRecognizer::with_settings(
                arena.clone(),
                GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_secs(5)),
            )
            .with_on_long_press(|| {});
            long_press.add_pointer(
                PointerId::new(2).expect("nonzero pointer id"),
                flui_types::Offset::new(px(10.0), px(10.0)),
            );

            let before_double_tap = Instant::now();
            let double_tap =
                DoubleTapGestureRecognizer::new(arena.clone()).with_on_double_tap_cancel(|_| {});
            let pointer = PointerId::new(3).expect("nonzero pointer id");
            let position = flui_types::Offset::new(px(20.0), px(20.0));
            double_tap.add_pointer(pointer, position);
            double_tap.handle_event(&flui_interaction::events::make_up_event(
                position,
                flui_interaction::events::PointerType::Touch,
            ));

            let next_deadline = arena
                .next_deadline()
                .expect("two armed deadlines pending -- next_deadline must be Some");
            let until_deadline = next_deadline.saturating_duration_since(before_double_tap);
            assert!(
                until_deadline < Duration::from_secs(1),
                "GestureArena::next_deadline must reflect the double-tap's near give-up \
                 window, not the long-press's far one -- got {until_deadline:?} until deadline"
            );
        }

        // ------------------------------------------------------------------
        // Frame telemetry: input->present attribution, driven through the
        // REAL production sequence (`handle_input_addressed` ->
        // `render_frame_entered`), never `FrameClock`'s own methods called
        // directly — the lesson this codebase has already paid for once in
        // this same area (see `render_frame_entered`'s own doc on why a
        // production-sequence probe caught what calling `draw_frame_entered`
        // directly could not).
        // ------------------------------------------------------------------

        /// A dispatched pointer event's arrival survives into the exported
        /// `FrameSnapshot` with a bounded, real, non-zero latency — value,
        /// not merely presence. Kills "epoch field exists but is zero/None"
        /// and "latency is a distribution with no attribution".
        #[test]
        fn dispatched_input_is_attributed_end_to_end_in_the_exported_frame_record() {
            use std::thread;
            use std::time::Duration;

            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();
            assert_eq!(
                realm
                    .presentations
                    .primary()
                    .clock()
                    .frames_since(None)
                    .len(),
                0,
                "precondition: nothing recorded before this test's own dispatch+produce"
            );

            let down = make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Mouse);
            let before_dispatch = std::time::Instant::now();
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
            });

            // A measurable, non-flaky real gap between dispatch and produce.
            thread::sleep(Duration::from_millis(5));

            let mut backend = AlwaysPresentBackend;
            let presented = realm.render_frame_entered(&mut backend);
            assert!(
                presented,
                "precondition: the frame actually reached present()"
            );
            let wall_clock_elapsed = before_dispatch.elapsed();

            let snapshots = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(snapshots.len(), 1, "exactly one frame must be recorded");
            let latencies: Vec<_> = snapshots[0].latencies().collect();
            assert_eq!(
                latencies.len(),
                1,
                "the dispatched input's epoch must survive into the exported record"
            );
            let (_, latency) = latencies[0];
            assert!(
                latency >= Duration::from_millis(5),
                "attributed latency must be at least the real sleep between dispatch and \
                 present -- got {latency:?}"
            );
            assert!(
                latency <= wall_clock_elapsed + Duration::from_millis(200),
                "attributed latency ({latency:?}) must not exceed the real wall-clock time \
                 actually elapsed since dispatch ({wall_clock_elapsed:?}) by more than \
                 measurement slop -- a stray wall-clock read from an unrelated instant would \
                 blow this bound"
            );
        }

        /// Two inputs dispatched before one produce: coalescing honesty --
        /// both survive with distinct latencies, the older one larger.
        /// Companion to the single-input test above, at the SAME
        /// production-sequence level (through `handle_input_addressed`, not
        /// `FrameClock::stamp_input_epoch` called directly).
        #[test]
        fn two_dispatched_inputs_before_one_produce_both_attributed_older_larger() {
            use std::thread;
            use std::time::Duration;

            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let first = make_down_event(Offset::new(Pixels(1.0), Pixels(1.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(first));
            });
            thread::sleep(Duration::from_millis(8));

            let second = make_down_event(Offset::new(Pixels(2.0), Pixels(2.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(second));
            });

            let mut backend = AlwaysPresentBackend;
            assert!(realm.render_frame_entered(&mut backend));

            let snapshots = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(snapshots.len(), 1);
            let mut latencies: Vec<_> = snapshots[0].latencies().collect();
            assert_eq!(
                latencies.len(),
                2,
                "both dispatched inputs must survive into the same produced frame's record \
                 -- coalescing must not be last-input-wins"
            );
            // Order as pushed: first dispatched (older) is index 0.
            latencies.sort_by_key(|(id, _)| id.get());
            let (_, older_latency) = latencies[0];
            let (_, newer_latency) = latencies[1];
            assert!(
                older_latency > newer_latency,
                "the earlier-dispatched input must show the larger latency: \
                 older={older_latency:?} newer={newer_latency:?}"
            );
        }

        /// `Skip(Backpressure)` vs `frames_dropped`, pinned at the REAL
        /// production path: a clock-level backpressure episode (simulated
        /// here since no production caller saturates `FrameClock`'s
        /// in-flight counter yet -- see `FrameClock::record_submit`'s own
        /// doc) must leave `UiRealm::frames_dropped` untouched, while a
        /// GENUINE submit failure through `render_frame_entered` (a real
        /// `SurfaceLost`) must increment it. Two different mechanisms,
        /// asserted against the same realm, so a mutant that folds either
        /// counter into the other is caught either way.
        #[test]
        fn backpressure_episode_is_not_counted_as_a_dropped_frame_but_a_real_submit_error_is() {
            let realm = mount_root_here();
            let presentation = realm.presentations.primary();

            // Saturate capacity so the very next poll defers rather than
            // produces -- `has_capacity` requires `in_flight < max`.
            presentation.clock().set_max_in_flight(1);
            presentation.clock().record_submit();
            presentation.mark_redraw_pending();

            let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

            assert_eq!(
                presentation.clock().backpressure_deferrals(),
                1,
                "the saturated clock must defer this pump's demand"
            );
            assert_eq!(
                realm.frames_dropped(),
                0,
                "a backpressure deferral must never be counted as a dropped frame"
            );

            // Free capacity so the retained demand can actually flow
            // through a real submit attempt next.
            presentation.clock().record_retire();
            presentation.mark_redraw_pending();

            let mut backend = AlwaysErrorsBackend;
            let presented = realm.render_frame_entered(&mut backend);

            assert!(!presented, "SurfaceLost never reaches present()");
            assert_eq!(
                realm.frames_dropped(),
                1,
                "a genuine submit failure must increment frames_dropped exactly once"
            );
            assert_eq!(
                presentation.clock().backpressure_deferrals(),
                1,
                "the real submit failure must not also inflate the deferral counter -- \
                 the two stay structurally separate"
            );
        }

        #[test]
        fn successful_submit_emits_structured_frame_telemetry_with_tail_quality() {
            let realm = mount_root_here();
            let capture = FrameEventCapture::default();
            let subscriber = Registry::default().with(capture.clone());
            let mut backend = AlwaysPresentBackend;

            tracing::subscriber::with_default(subscriber, || {
                assert!(realm.render_frame_entered(&mut backend));
            });

            let fields = capture
                .0
                .lock()
                .expect("BUG: frame capture is locked only by this test")
                .clone();
            let field = |name: &str| {
                fields
                    .iter()
                    .find_map(|(field, value)| (field == name).then_some(value.as_str()))
            };

            assert_eq!(field("event"), Some("frame_telemetry"));
            assert_eq!(field("present_outcome"), Some("presented"));
            assert_eq!(field("input_epochs_overflowed"), Some("false"));
            assert_eq!(field("produces_deferred"), Some("0"));
            assert_eq!(field("frames_dropped"), Some("0"));
            assert!(field("presentation").is_some());
            assert!(field("frame_id").is_some());
            assert!(field("produce_to_present_us").is_some());
        }

        /// Multi-presentation attribution, red-exploit for the bug where
        /// `record_submit_telemetry` hardcoded `primary()`: on a pump where
        /// the PRIMARY's segment is skipped (nothing dirty) and a SECONDARY's
        /// segment produces, the recorded `FrameSnapshot` must land on the
        /// SECONDARY's own clock, carrying THIS pump's own fresh segment
        /// span — never the primary's, and never a stale span the primary
        /// itself latched on an earlier pump. Reproduces the production
        /// topology `runner.rs`'s `install_presentation_alongside` (behind
        /// `open_secondary_window`) creates: more than one presentation
        /// resident in one realm.
        ///
        /// Before the fix, this failed two ways at once: A gained a SECOND,
        /// wrongly-attributed snapshot (guarded only by A's own
        /// `last_segment_span().is_some()`, which stayed `Some` forever
        /// once pump 1 set it — proving "a segment ran on SOME pump", not
        /// "this pump"), with a `frame_id` colliding with pump 1's own
        /// (A's `produced_count` never advanced this pump), while B — the
        /// presentation that actually produced — recorded nothing at all.
        #[test]
        fn a_pump_where_the_primary_skips_and_a_secondary_produces_attributes_telemetry_to_the_secondary_not_the_stale_primary()
         {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            // Pump 1: only A has content. A produces and submits,
            // establishing a REAL segment span and one recorded
            // `FrameSnapshot` on A's own clock — the exact state a latched-
            // forever span could later be misread as "still valid" once it
            // goes stale.
            realm
                .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
                .expect("A attaches");
            let mut backend = AlwaysPresentBackend;
            assert!(
                realm.render_frame_entered(&mut backend),
                "pump 1: A alone must produce and present"
            );
            let a_snapshots_after_pump_1 = realm
                .presentations
                .get(a_id)
                .expect("A installed")
                .clock()
                .frames_since(None);
            assert_eq!(
                a_snapshots_after_pump_1.len(),
                1,
                "sanity: A's own first pump must record exactly one snapshot"
            );

            // Pump 2: A is now settled (its segment will be SKIPPED this
            // pump — nothing dirty), B gets real content attached and
            // produces instead.
            realm
                .attach_root_widget_to_for_test(b_id, &flui_widgets::SizedBox::new(20.0, 20.0))
                .expect("B attaches");
            assert!(
                realm.render_frame_entered(&mut backend),
                "pump 2: B alone must produce and present"
            );

            let a = realm.presentations.get(a_id).expect("A installed");
            let b = realm.presentations.get(b_id).expect("B installed");

            let a_snapshots_after_pump_2 = a.clock().frames_since(None);
            assert_eq!(
                a_snapshots_after_pump_2.len(),
                1,
                "A must NOT gain a second, wrongly-attributed snapshot from B's own pump"
            );
            assert_eq!(
                a_snapshots_after_pump_2[0].frame_id, a_snapshots_after_pump_1[0].frame_id,
                "A's own recorded snapshot must be byte-for-byte untouched by B's pump"
            );

            let b_snapshots = b.clock().frames_since(None);
            assert_eq!(
                b_snapshots.len(),
                1,
                "B's own pump must record exactly one snapshot, on B's own clock"
            );
            assert_eq!(
                b_snapshots[0].presentation, b_id,
                "the recorded snapshot must name B, not A, as its producer"
            );
        }

        /// More than `MAX_COALESCED_INPUT_EPOCHS` raw pointer events
        /// dispatched before a single produce — routine for any drag
        /// against a high-frequency device, not an edge case (the
        /// `stamp_input_epoch` call in `handle_input_addressed` fires
        /// before `GestureBinding::flush_pending_moves` coalesces anything,
        /// so telemetry sees the RAW dispatch stream). The exported record
        /// must retain the NEWEST arrivals, not misattribute the oldest,
        /// longest-stale ones as if they were still representative of what
        /// this frame carried.
        #[test]
        fn more_than_max_coalesced_inputs_before_one_produce_keeps_the_newest_arrivals() {
            use std::thread;
            use std::time::Duration;

            use flui_interaction::events::{PointerType, make_move_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let dispatched = flui_scheduler::MAX_COALESCED_INPUT_EPOCHS + 4;
            for i in 0..dispatched {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "test-only pixel offsets, i stays far below f32's exact-integer range"
                )]
                let position = Offset::new(Pixels(i as f32), Pixels(i as f32));
                let event = make_move_event(position, PointerType::Mouse);
                realm.enter(|realm| {
                    realm.handle_input_addressed(primary_id, PlatformInput::Pointer(event));
                });
                thread::sleep(Duration::from_micros(200));
            }

            let mut backend = AlwaysPresentBackend;
            assert!(realm.render_frame_entered(&mut backend));

            let snapshots = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(snapshots.len(), 1);
            let mut latencies: Vec<_> = snapshots[0].latencies().collect();
            assert_eq!(
                latencies.len(),
                flui_scheduler::MAX_COALESCED_INPUT_EPOCHS,
                "the ring must retain exactly its bounded capacity, never silently grow"
            );
            latencies.sort_by_key(|(id, _)| id.get());
            let expected_first_kept =
                (dispatched - flui_scheduler::MAX_COALESCED_INPUT_EPOCHS) as u64;
            let ids: Vec<u64> = latencies.iter().map(|(id, _)| id.get()).collect();
            assert_eq!(
                ids,
                (expected_first_kept..dispatched as u64).collect::<Vec<_>>(),
                "must keep the newest-dispatched inputs, not the oldest -- the discard \
                 policy this test pins"
            );
        }

        /// A raster backend whose `render_scene` sleeps a known interval
        /// before returning `Ok(true)` — standing in for the production
        /// wgpu backend's `output.present()` under the default `Fifo`
        /// presentation mode, which blocks until the next vsync before
        /// `render_scene` returns. Proves `submit_at` is sampled AFTER
        /// `render_scene` returns, not before it.
        struct SleepingPresentBackend {
            sleep: std::time::Duration,
        }

        impl RasterBackend for SleepingPresentBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                std::thread::sleep(self.sleep);
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

        /// Finding: `submit_at` used to be sampled BEFORE `render_scene`
        /// was called, so any time the call spent blocked inside a
        /// synchronous `present()` (the production `Fifo` case) was
        /// invisible to the recorded latency — the shipped
        /// `input_to_present_histogram`/`produce_to_present_histogram`
        /// names would then be lying about what they measure (submit, not
        /// present). This backend simulates that blocking present with a
        /// real sleep; a `submit_at` sampled before the call would report a
        /// latency that does NOT include it.
        #[test]
        fn submit_latency_includes_time_spent_inside_render_scene_not_just_before_it() {
            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
            });

            let sleep = std::time::Duration::from_millis(30);
            let mut backend = SleepingPresentBackend { sleep };
            assert!(
                realm.render_frame_entered(&mut backend),
                "precondition: the frame actually reached present()"
            );

            let snapshots = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(snapshots.len(), 1);
            let (_, latency) = snapshots[0]
                .latencies()
                .next()
                .expect("the dispatched input's epoch must be attributed to this frame");
            assert!(
                latency >= sleep,
                "recorded input->present latency ({latency:?}) must include the time \
                 render_scene spent blocked inside its own present() call (simulated here as \
                 a {sleep:?} sleep) -- a submit_at sampled BEFORE render_scene would \
                 understate this by up to the sleep duration"
            );
        }

        /// Finding: on `SurfaceLost`, `record_submit_telemetry` used to
        /// unconditionally DRAIN this presentation's pending input epochs
        /// into the failed attempt's own `Errored` snapshot — but
        /// `render_frame_entered` arms a retry for exactly this outcome
        /// (`retry_needed = true` -> `wake_frame()`), so the frame that
        /// eventually reaches the screen would find the epoch buffer
        /// already empty and carry no attribution at all. Drives the exact
        /// sequence the finding names: input -> `SurfaceLost` -> retry ->
        /// the presented frame still carries the original epoch.
        #[test]
        fn surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
            });

            // First attempt: the surface is lost. A retry is armed, and the
            // failed attempt's own snapshot must still see the epoch that
            // was pending (diagnostic value), but must NOT consume it.
            let mut failing_backend = AlwaysErrorsBackend;
            let presented = realm.render_frame_entered(&mut failing_backend);
            assert!(!presented, "SurfaceLost never reaches present()");
            let after_failure = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(
                after_failure.len(),
                1,
                "the failed attempt is still recorded, for diagnostics"
            );
            assert_eq!(
                after_failure[0].latencies().count(),
                1,
                "the failed attempt's own snapshot still reports the pending epoch"
            );

            // Retry: as of #637's fix, `render_frame_entered`'s own
            // `retry_needed` arm already marks the root needs-paint (see
            // `mark_primary_needs_full_repaint`), so this hand mark is no
            // longer load-bearing for getting `render_scene` reached again
            // -- `a_mid_frame_submit_failure_retry_actually_reaches_
            // render_scene_again_on_a_static_tree` covers THAT invariant
            // without any hand-dirtying at all. This mark stays here to
            // isolate a DIFFERENT invariant this test is actually about
            // (epoch attribution across the retry), standing in for the
            // genuinely new external cause (input, animation, resize) a
            // real driver loop would eventually supply on top of the
            // fix's own repaint.
            let primary = realm.presentations.primary();
            primary.renderer().root_pipeline_owner().with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_layout(root_id);
                }
            });
            primary.mark_redraw_pending();
            let mut succeeding_backend = AlwaysPresentBackend;
            let presented = realm.render_frame_entered(&mut succeeding_backend);
            assert!(presented, "the retry must actually reach present()");

            let after_retry = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(
                after_retry.len(),
                2,
                "the retry adds a second snapshot alongside the failed attempt's"
            );
            let retry_snapshot = &after_retry[1];
            assert_eq!(
                retry_snapshot.present_outcome,
                flui_scheduler::PresentOutcome::Presented
            );
            assert_eq!(
                retry_snapshot.latencies().count(),
                1,
                "the presented retry frame must carry the ORIGINAL input epoch -- retaining \
                 it across the failed attempt must not have lost it"
            );
        }

        /// The `DeviceLost` counterpart to
        /// `surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame`
        /// above: before this change, `DeviceLost` used `EpochDisposition::Drain`
        /// unconditionally AND never armed a retry, so this pin would have been
        /// meaningless either way — a mistaken `Drain` on this arm is caught
        /// here, not just by inspection. Drives the same
        /// input -> failure -> retry -> attribution sequence, substituting
        /// `DeviceLost` for `SurfaceLost`.
        #[test]
        fn device_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
            });

            // First attempt: the device is lost. A retry is armed, and the
            // failed attempt's own snapshot must still see the epoch that
            // was pending (diagnostic value), but must NOT consume it.
            let mut failing_backend = AlwaysDeviceLostBackend;
            let presented = realm.render_frame_entered(&mut failing_backend);
            assert!(!presented, "DeviceLost never reaches present()");
            let after_failure = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(
                after_failure.len(),
                1,
                "the failed attempt is still recorded, for diagnostics"
            );
            assert_eq!(
                after_failure[0].latencies().count(),
                1,
                "the failed attempt's own snapshot still reports the pending epoch"
            );

            // Retry: as of #637's fix this hand mark is no longer required
            // to reach `render_scene` again (`render_frame_entered`'s own
            // `retry_needed` arm now does that -- see the `SurfaceLost` pin
            // above's comment for the non-vacuous test that covers it). Kept
            // here standing in for the renderer owner's recovery + wake, to
            // isolate this test's own epoch-attribution invariant rather
            // than actually rebuilding a device.
            let primary = realm.presentations.primary();
            primary.renderer().root_pipeline_owner().with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_layout(root_id);
                }
            });
            primary.mark_redraw_pending();
            let mut succeeding_backend = AlwaysPresentBackend;
            let presented = realm.render_frame_entered(&mut succeeding_backend);
            assert!(presented, "the retry must actually reach present()");

            let after_retry = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(
                after_retry.len(),
                2,
                "the retry adds a second snapshot alongside the failed attempt's"
            );
            let retry_snapshot = &after_retry[1];
            assert_eq!(
                retry_snapshot.present_outcome,
                flui_scheduler::PresentOutcome::Presented
            );
            assert_eq!(
                retry_snapshot.latencies().count(),
                1,
                "the presented retry frame must carry the ORIGINAL input epoch -- retaining \
                 it across the failed attempt must not have lost it"
            );
        }

        /// The `SurfaceValidation` counterpart to both pins above. Named
        /// finding: flipping `EpochDisposition::Retain` back to `Drain` on
        /// the `SurfaceValidation` arm specifically was NOT caught by
        /// anything in the suite before this test existed — 281/281 stayed
        /// green under that flip, because `surface_validation_keeps_needs_
        /// redraw_armed_for_a_retry` only pins the `needs_redraw` half of
        /// the fix, never epoch attribution. Same input -> failure -> retry
        /// -> attribution sequence as the two pins above, substituting
        /// `SurfaceValidation` for `DeviceLost`/`SurfaceLost`.
        #[test]
        fn surface_validation_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
            });

            // First attempt: the surface fails validation. A retry is
            // armed, and the failed attempt's own snapshot must still see
            // the epoch that was pending (diagnostic value), but must NOT
            // consume it.
            let mut failing_backend = AlwaysSurfaceValidationBackend;
            let presented = realm.render_frame_entered(&mut failing_backend);
            assert!(!presented, "SurfaceValidation never reaches present()");
            let after_failure = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(
                after_failure.len(),
                1,
                "the failed attempt is still recorded, for diagnostics"
            );
            assert_eq!(
                after_failure[0].latencies().count(),
                1,
                "the failed attempt's own snapshot still reports the pending epoch"
            );

            // Retry: as of #637's fix this hand mark is no longer required
            // to reach `render_scene` again (`render_frame_entered`'s own
            // `retry_needed` arm now does that -- see the `SurfaceLost` pin
            // above's comment for the non-vacuous test that covers it). Kept
            // here standing in for the external reconfigure a later wake
            // performs, to isolate this test's own epoch-attribution
            // invariant rather than actually reconfiguring a surface.
            let primary = realm.presentations.primary();
            primary.renderer().root_pipeline_owner().with_mut(|owner| {
                if let Some(root_id) = owner.root_id() {
                    owner.mark_needs_layout(root_id);
                }
            });
            primary.mark_redraw_pending();
            let mut succeeding_backend = AlwaysPresentBackend;
            let presented = realm.render_frame_entered(&mut succeeding_backend);
            assert!(presented, "the retry must actually reach present()");

            let after_retry = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(
                after_retry.len(),
                2,
                "the retry adds a second snapshot alongside the failed attempt's"
            );
            let retry_snapshot = &after_retry[1];
            assert_eq!(
                retry_snapshot.present_outcome,
                flui_scheduler::PresentOutcome::Presented
            );
            assert_eq!(
                retry_snapshot.latencies().count(),
                1,
                "the presented retry frame must carry the ORIGINAL input epoch -- retaining \
                 it across the failed attempt must not have lost it"
            );
        }

        /// A raster backend that fails its first `render_scene` call with a
        /// scripted [`EngineError`] and presents on every call after — the
        /// double [`a_mid_frame_submit_failure_retry_actually_reaches_render_scene_again_on_a_static_tree`]
        /// needs to observe whether a retry actually reaches `render_scene`
        /// a second time, not just whether bookkeeping (`needs_redraw()`,
        /// epoch attribution) looks right around it. Every `Always*Backend`
        /// above scripts a single outcome forever, which cannot express "the
        /// transient failure clears" — the shape a real retry produces.
        struct FailOnceThenPresentsBackend {
            error: Option<EngineError>,
            render_scene_calls: u32,
        }

        impl FailOnceThenPresentsBackend {
            fn new(error: EngineError) -> Self {
                Self {
                    error: Some(error),
                    render_scene_calls: 0,
                }
            }
        }

        impl RasterBackend for FailOnceThenPresentsBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                self.render_scene_calls += 1;
                match self.error.take() {
                    Some(e) => Err(e),
                    None => Ok(true),
                }
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

        /// Issue #637's own oracle requirement: the epoch-preservation pins
        /// above (`surface_lost_retry_preserves_...` and its two siblings)
        /// dirty the tree BY HAND after the scripted failure
        /// (`owner.mark_needs_layout` + `primary.mark_redraw_pending()`),
        /// standing in for whatever external cause would normally re-dirty
        /// it. That stand-in is exactly why they kept passing while the
        /// PRODUCTION retry path was silently a no-op: `wake_frame()` alone
        /// re-opens `draw_frame_entered`'s per-presentation segment gate,
        /// but never touches `PipelineOwner`'s own independent dirty
        /// tracking, so an otherwise-unchanged (static) tree produces
        /// `FramePaintOutcome::Idle` on the retry pump and `render_scene` is
        /// never reached again.
        ///
        /// This test dirties nothing itself, ever — not before the failure,
        /// not between the failure and the retry. Sequence: mount (the
        /// mount's own attach genuinely leaves build/layout dirty — nothing
        /// hand-poked), pump ONCE against a backend scripted to fail that
        /// very first submit (so the pipeline's build/layout/paint work is
        /// genuinely consumed producing real content, even though the
        /// submit of that content then fails), then pump again with no
        /// intervening dirtying of any kind. The static tree between the
        /// failure and the retry is the whole point: on `main`, nothing
        /// re-arms the pipeline's own dirty tracking after a submit
        /// failure, so this second pump finds nothing dirty and never
        /// reaches `render_scene` again. The assertion is a `render_scene`
        /// call counter, not `is_ok()` or `needs_redraw()` — either of
        /// which a no-op retry (gate reopened, pipeline never touched)
        /// would also satisfy.
        #[test]
        fn a_mid_frame_submit_failure_retry_actually_reaches_render_scene_again_on_a_static_tree() {
            let realm = mount_root_here();

            // Pump 1: the mount's own dirty state is genuinely consumed --
            // build, layout, and paint all run, producing real content --
            // but the submit of that content fails.
            let mut backend = FailOnceThenPresentsBackend::new(EngineError::SurfaceLost);
            let presented = realm.render_frame_entered(&mut backend);
            assert!(!presented, "SurfaceLost never reaches present()");
            assert_eq!(
                backend.render_scene_calls, 1,
                "precondition: the failing pump actually reached render_scene once"
            );
            assert!(realm.needs_redraw(), "the failure must arm a retry");

            // Pump 2, the retry -- driven by the same wake this test never
            // hand-dirties around. The tree is unchanged (static) since the
            // failure: whatever reaches render_scene again must come from
            // the failure arm's own fix, not from anything this test did.
            let presented = realm.render_frame_entered(&mut backend);
            assert!(presented, "the retry must actually reach present()");
            assert_eq!(
                backend.render_scene_calls, 2,
                "the retry must reach render_scene a SECOND time -- wake_frame() alone \
                 re-opens the segment gate but leaves PipelineOwner's own dirty tracking \
                 untouched, so an unmodified static tree produces Idle and render_scene is \
                 never called again"
            );
        }

        /// Regression for a review finding on this fix's own first draft: an
        /// earlier version called `self.mark_primary_needs_full_repaint()`
        /// unconditionally from `render_frame_entered`'s `retry_needed` arm
        /// — but this method's own doc, at the `producer` binding, already
        /// says `producer` (the presentation whose segment actually
        /// produced the failed submit) is NEVER assumed to be `primary()`,
        /// and every OTHER decision in the method reads `producer`
        /// specifically. With more than one presentation resident (the
        /// production topology `install_presentation_alongside`, behind
        /// `open_secondary_window`, creates), a primary-only mark repaints
        /// the WRONG tree whenever a non-primary presentation is the real
        /// producer — the actually-failing presentation's own pipeline is
        /// never re-dirtied, so ITS retry still parks, while the untouched
        /// primary spuriously repaints instead.
        ///
        /// Drives exactly that: A (primary) produces and settles, then B
        /// (a second, resident presentation) gets real content and becomes
        /// the producer whose submit fails. The oracle checks BOTH
        /// presentations' own clocks, not just a call counter — a call
        /// counter alone cannot distinguish "B's retry succeeded" from "A's
        /// stale content got spuriously re-marked, re-submitted, and
        /// coincidentally reached render_scene too", which is exactly what
        /// the primary-only draft of this fix would produce here: A's own
        /// segment would wrongly reopen (marked dirty by mistake) and
        /// produce a second, spurious snapshot while B — the one presentation
        /// that actually needed the retry — never got remarked and stayed at
        /// one snapshot (its own failed attempt, forever unretried).
        #[test]
        fn a_mid_frame_submit_failure_retry_repaints_the_actual_producer_not_the_primary() {
            let mut realm = UiRealm::for_test();
            let a_id = realm.presentation_id();
            let b_id = realm.install_second_presentation_for_test();

            // Pump 1: only A (primary) has content. A produces, presents,
            // and settles -- from here on A has nothing left to redo.
            realm
                .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
                .expect("A attaches");
            let mut warm_up = AlwaysPresentBackend;
            assert!(
                realm.render_frame_entered(&mut warm_up),
                "pump 1: A alone must produce and present"
            );
            assert_eq!(
                realm
                    .presentations
                    .get(a_id)
                    .expect("A installed")
                    .clock()
                    .frames_since(None)
                    .len(),
                1,
                "precondition: A recorded exactly its own pump-1 snapshot"
            );

            // B gets real content; A stays static/settled from here on.
            realm
                .attach_root_widget_to_for_test(b_id, &flui_widgets::SizedBox::new(20.0, 20.0))
                .expect("B attaches");

            // Pump 2: B is the producer (A's segment skips -- nothing dirty
            // there), and B's submit fails.
            let mut backend = FailOnceThenPresentsBackend::new(EngineError::SurfaceLost);
            let presented = realm.render_frame_entered(&mut backend);
            assert!(!presented, "SurfaceLost never reaches present()");
            assert_eq!(
                backend.render_scene_calls, 1,
                "precondition: B's segment actually reached render_scene once"
            );
            assert!(realm.needs_redraw(), "the failure must arm a retry");

            // Pump 3, the retry -- no hand-dirtying of either presentation.
            let presented = realm.render_frame_entered(&mut backend);
            assert!(presented, "the retry must actually reach present()");
            assert_eq!(
                backend.render_scene_calls, 2,
                "the retry must reach render_scene a second time"
            );

            let a_snapshots = realm
                .presentations
                .get(a_id)
                .expect("A installed")
                .clock()
                .frames_since(None)
                .len();
            let b_snapshots = realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .clock()
                .frames_since(None)
                .len();
            assert_eq!(
                a_snapshots, 1,
                "A (the primary) must stay untouched by a retry that belongs to B -- \
                 marking A's pipeline dirty instead of the real producer's is exactly \
                 the bug this test guards against"
            );
            assert_eq!(
                b_snapshots, 2,
                "B (the actual producer) must record both the failed attempt and the \
                 presented retry on its OWN clock -- a call counter alone cannot tell \
                 this apart from A's stale content being spuriously re-marked instead"
            );
            // The submit-count assertions above are one-sided: `frames_since`
            // counts SUBMITS, and only the per-pump `producer` submits, so an
            // extra segment run on a NON-producing presentation is invisible
            // to them. Marking A *in addition to* B — rather than instead of
            // it — passes both of the above while A really does run a
            // spurious full build/layout/paint. Its flush count is what sees
            // that: 1 when only B is marked, 2 when A is marked too.
            assert_eq!(
                realm.presentations.primary().flush_count(),
                1,
                "A must not run a second segment at all -- the assertions above \
                 cannot see this, because a non-producing presentation submits \
                 nothing for `frames_since` to count"
            );
        }

        /// Finding: `DragDrop` is stamped unconditionally before the match
        /// on input kind, even though its own arm traces the event as
        /// dropped without routing it anywhere or requesting a frame -- a
        /// phantom epoch that would sit in the ring until eviction,
        /// attributed to whatever unrelated frame happens to produce next.
        /// Dispatches a dropped `DragDrop` event followed by a real,
        /// routed `Pointer` event and confirms only the pointer's epoch
        /// survives to the produced frame.
        #[test]
        fn drag_drop_input_is_not_stamped_since_it_is_dropped_not_routed() {
            use flui_interaction::events::{PointerType, make_down_event};
            use flui_types::geometry::{Offset, Pixels};

            let realm = mount_root_here();
            let primary_id = realm.presentations.primary().id();

            let drag_drop = PlatformInput::DragDrop(DragDropEvent::Exited {
                id: flui_foundation::DataTransferId::new(1),
            });
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, drag_drop);
            });

            let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
            realm.enter(|realm| {
                realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
            });

            let mut backend = AlwaysPresentBackend;
            assert!(realm.render_frame_entered(&mut backend));

            let snapshots = realm.presentations.primary().clock().frames_since(None);
            assert_eq!(snapshots.len(), 1);
            let latencies: Vec<_> = snapshots[0].latencies().collect();
            assert_eq!(
                latencies.len(),
                1,
                "only the routed pointer event's epoch must survive -- the dropped drag-drop \
                 event must never have been stamped at all"
            );
            assert_eq!(
                latencies[0].0.get(),
                0,
                "the pointer event must have minted the FIRST epoch id on this clock -- if \
                 drag-drop had wrongly been stamped first, the pointer's own id would be 1"
            );
        }
    }
}
