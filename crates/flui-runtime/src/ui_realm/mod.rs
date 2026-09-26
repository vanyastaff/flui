//! `UiRealm` — the owner-affine UI-session composition root.
//!
//! One realm, one owner: the runtime is the single logical owner of a
//! window's UI state and is structurally `!Send + !Sync`. Everything that
//! crosses a thread boundary does so as a typed [`UiCommandSender`]
//! capability feeding a **bounded** inbox, whose contents the owner commits
//! **only while the scheduler phase is Idle** — at frame boundaries, never
//! inside the frame transaction. This is the generalization of the
//! `RebuildHandle`/`RenderInvalidationHandle` pattern: enqueue-and-wake, never
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

mod presentation_lifecycle;
use presentation_lifecycle::HostLifecycle;

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
#[cfg(test)]
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(test)]
use std::rc::Rc;
use std::sync::Arc;
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
use crate::sink::SubmitVerdict;
use crossbeam_channel::Receiver;
#[cfg(test)]
use flui_foundation::PresentationId;
use flui_foundation::RealmId;
use flui_interaction::InteractionLane;
use flui_layer::Scene;
#[cfg(test)]
use flui_platform_api::{DragDropEvent, PlatformInput, PlatformWindow};
#[cfg(test)]
use flui_rendering::binding::RendererBinding as _;
#[cfg(test)]
use flui_rendering::constraints::BoxConstraints;
#[cfg(test)]
use flui_rendering::pipeline::PipelineOwner;
#[cfg(test)]
use flui_scheduler::{AppLifecycleState, SchedulerPhase};
use flui_scheduler::{LocalPostFrameLane, UpdateScheduler};
#[cfg(test)]
use flui_types::{Size, geometry::px};
use flui_view::GlobalKeyScope;
#[cfg(test)]
use parking_lot::RwLock;

#[cfg(test)]
use crate::epoch::FrameCommitState;

#[cfg(test)]
use super::frame_failure::{FailureDisposition, FrameFailureKind, SegmentPhase};
use super::frame_failure::{FrameFailureDetail, FrameFailureHandler};
use super::presentation_forest::PresentationForest;

/// Default bound of the owner inbox, matching the pipeline dirty-channel
/// precedent (`DEFAULT_DIRTY_CHANNEL_CAPACITY`)). Observable at
/// runtime via `UiCommandSender::capacity`; not part of the public API.
const DEFAULT_COMMAND_CAPACITY: usize = 256;

/// How many consecutive frames a presentation will re-arm after the backend
/// produced content it could not put on screen, before giving up and parking.
///
/// A frame lost this way cannot be recovered by waiting: the pipeline
/// consumed the work that produced it, so the scene would never be redrawn —
/// hence the retry. But the retry re-dirties the presentation on every
/// attempt, so an *unbounded* one is a self-sustaining repaint loop paced by
/// the runner's fallback gate (~9.5 ms, ~105 Hz). This bound is what separates
/// riding out a transient from spinning on a permanently unavailable
/// drawable.
///
/// Chosen against measurement, not intuition. Across six cold starts of the
/// native macOS backend with the drawable withdrawn at first paint, the
/// transient took 2, 2 and 3 consecutive attempts on the three runs that
/// carry this cap, and 12 (~132 ms) on the coldest, the run that motivated
/// the retention in the first place. 128 sits ~10x above the worst observed
/// (~1.2 s at the same pace), which is the direction to err: too generous
/// costs one bounded burst of wasted frames on a surface that never comes
/// back, while too tight reintroduces the blank window this bound exists to
/// sit behind. On the runs measured with the cap in place it never engaged —
/// `streak` peaked at 3 — so it is a backstop against the pathological case,
/// not a limit the ordinary transient touches.
///
/// Exhausting the budget is not permanent. The streak is per-presentation and
/// cleared by any frame that ends otherwise (see
/// `PresentationState::clear_not_shown_streak`), and a real platform event
/// still dirties the tree and produces an attempt of its own — so recovery
/// after the bound does not depend on this counter ever being reset by hand.
const MAX_NOT_SHOWN_RETRIES: u32 = 128;

mod attach;
mod commands;
mod construct;
mod frame;
mod frame_clock;
mod input;
mod presentations;

pub use commands::{CommandSendError, DrainReport, UiCommand, UiCommandSender};
use input::FocusCoordinator;
#[cfg(test)]
use input::input_dropped_by_lifecycle;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors constructing a [`UiRealm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UiRealmError {
    /// The owner-local interaction lane could not be created.
    #[error("failed to create the realm interaction lane: {0}")]
    InteractionLane(#[from] flui_interaction::InteractionDispatchError),
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
pub struct UiRealm {
    realm_id: RealmId,
    /// Owner-local callback queue, activated with the realm's other TLS scope.
    local_post_frame: LocalPostFrameLane,
    /// Owner-local interaction callback storage, activated with the realm scope.
    interaction_lane: InteractionLane,
    /// This realm's cross-tree `GlobalKey` uniqueness domain (ADR-0043 §1),
    /// installed into every presentation's `BuildOwner` at assembly time
    /// (`PresentationState::new`). Retained here (not just handed off once)
    /// so a later-installed presentation can share the same scope: both
    /// [`Self::assemble_presentation`] (production, issue #555's
    /// addressed-routing slice) and its `install_second_presentation_for_test`
    /// counterpart (the isolation test suite) read it back to assemble a
    /// second presentation sharing this exact scope.
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
    host_lifecycle: Cell<HostLifecycle>,
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
    #[cfg(any(test, feature = "test-support"))]
    now_secs_override: AtomicU64,
    rx: Receiver<UiCommand>,
    /// Prototype for [`Self::command_sender`]: crossbeam receivers cannot
    /// mint senders, so the runtime keeps one sender to clone from. Holding
    /// it here does not keep the channel alive past the runtime: `rx` drops
    /// with the runtime and every outstanding sender turns `OwnerGone`.
    sender_prototype: UiCommandSender,
    redraw_pending: Arc<AtomicBool>,
    /// This realm's OWN scheduler — the strong root every `WeakUpdateScheduler`
    /// this realm vends (tickers, `PostFrameHandle`s) upgrades against.
    /// Built fresh per realm by [`RealmServices::construct`](crate::realm_services::RealmServices::construct), never a
    /// process-global singleton: when this realm drops, this field drops
    /// with it, and every retained weak handle starts failing closed.
    /// Read directly for the idle-only commit-gate phase probe in
    /// [`Self::drain_commands`] and vended to `runner.rs`'s lifecycle sites
    /// via [`Self::scheduler`].
    scheduler: UpdateScheduler,
    /// The embedder's typed frame-failure callback, if one was registered
    /// (`AppConfig::with_frame_failure_handler`, wired by each backend's
    /// bootstrap via [`Self::set_frame_failure_handler`]). Realm-scoped,
    /// never process-global; cloned out of the cell before every delivery
    /// so the callback runs with no realm borrow held.
    frame_failure_handler: RefCell<Option<FrameFailureHandler>>,
    /// Realm-scoped policy for retaining unstructured frame-failure text.
    /// Set once by the runner from `AppConfig`; a shared-realm secondary
    /// presentation inherits this existing realm policy.
    frame_failure_detail: Cell<FrameFailureDetail>,
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

/// Outcome of one complete presentation segment, from build through
/// finalization, pipeline work, post-pipeline tail, and scene construction.
/// `Idle` and `Errored` both produce no scene to submit, but only `Errored`
/// forces a retry rather than being treated as a clean segment (see
/// [`UiRealm::render_frame`]'s retry gate). Moved here from the retired
/// `AppBinding`.
#[derive(Debug)]
pub enum FramePaintOutcome {
    /// A fresh layer tree was painted and turned into a `Scene`. Holds
    /// `Scene` by value, not `Arc<Scene>`: the sole reader (the frame
    /// transaction immediately below) MOVES it into the submit sink —
    /// on the raster-lane path it crosses the raster boundary as an owned
    /// `SceneSnapshot` (that type's own "never `Arc<Scene>`" contract in
    /// `scene_snapshot.rs`), on the direct path it is borrowed internally
    /// and dropped when the render returns. Nothing shares this value, so
    /// an `Arc` bought nothing here.
    Painted(Scene),
    /// Nothing was dirty this frame; no new content to composite.
    Idle,
    /// The complete segment failed: a structured pipeline error, or a panic
    /// escaped from Build, Finalize, Pipeline, Tail, or Scene and was caught by
    /// the presentation boundary. The frame was dropped and must be retried.
    Errored,
}

/// Whether [`UiRealm::record_submit_telemetry`] drains a presentation's
/// pending input epochs into the [`flui_scheduler::FrameSnapshot`] it
/// records, or merely reads them without consuming them.
///
/// `Retain` exists for exactly one shape: a submit failure whose own caller
/// has already armed a retry. **Two** verdict arms in the frame transaction
/// behind [`UiRealm::render_frame`] are that shape —
/// `SurfaceStale` (covering a lost surface, a validation failure, and a
/// stale surface-generation stamp, which used to be two separate
/// `SurfaceLost`/`SurfaceValidation` arms) and `DeviceLost`. `DeviceLost`
/// and the validation flavor joined when device loss gained a retry at all;
/// before that they drained, and this sentence named only `SurfaceLost`.
/// Each underlying failure flavor is pinned by its own
/// `*_retry_preserves_the_original_input_epoch_for_the_presented_frame`
/// test, so flipping either arm back to `Drain` turns one red — the
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

impl Drop for UiRealm {
    fn drop(&mut self) {
        // Every live presentation this realm hosts closes when the realm
        // drops. The forest supports N resident presentations, and addressed
        // close may already have removed any subset of siblings before this
        // whole-realm teardown runs.
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
mod frame_failure_phase_tests;

#[cfg(test)]
mod frame_commit_state_tests;

#[cfg(test)]
mod frame_failure_detail_tests;

#[cfg(test)]
mod frame_failure_recovery_tests;

#[cfg(test)]
mod tests;
