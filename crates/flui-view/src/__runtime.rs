//! The composition-root seam of ADR-0081 §4.
//!
//! These items let the crates that own a realm drive a [`WidgetsBinding`]
//! from outside `flui-view`: activating its `GlobalKey` registry for one
//! realm entry, stamping the frame phase at the build-to-finalize boundary,
//! and running the presentation's terminal lifecycle ladder; and the
//! development agent's port ([`AgentPort`]), which only the runtime
//! implements and through which it builds the [`AgentWindow`]s a
//! [`DevAgentHook`](crate::dev_agent::DevAgentHook) is handed. Their intended
//! users are `flui-runtime`, `flui-app`, `flui-testing` and `flui-hot-reload`.
//!
//! **No semver promise.** Anything here may change or disappear in any
//! release. The module is always compiled and `#[doc(hidden)]`, and the
//! whole-crate re-exports in `flui-sdk` and the `flui` facade shadow it, so
//! `flui_sdk::view::__runtime` and `flui::view::__runtime` do not resolve.
//! The seam's methods on [`WidgetsBinding`] live on the sealed
//! [`BindingRuntime`] trait rather than as inherent methods, so they are not
//! part of the binding's public surface either: they resolve only where the
//! trait is imported.

use std::any::Any;
use std::cell::Cell;
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_protocol::{ActionRequest, ReadQuery, Tree, WindowId};
use flui_scheduler::AppLifecycleState;

use crate::WidgetsBinding;
use crate::dev_agent::{AgentAnswer, AgentFault, AgentWindow};
pub use crate::lifecycle::LifecycleSource;

/// What an [`AgentWindow`] calls: one window's read and act, enqueued on its
/// owner. The runtime's semantics agent is the implementation.
pub trait AgentPort: Send + Sync {
    /// Enqueue a read of the window's committed semantics tree.
    ///
    /// # Errors
    ///
    /// When the request cannot be enqueued.
    fn read(&self, query: ReadQuery) -> Result<AgentAnswer<Tree>, AgentFault>;

    /// Enqueue an action on one of the window's elements.
    ///
    /// # Errors
    ///
    /// When the request cannot be enqueued.
    fn act(&self, request: ActionRequest) -> Result<AgentAnswer<()>, AgentFault>;
}

/// The pending half of an [`AgentAnswer`].
pub trait PendingAnswer<T>: Send {
    /// The answer if it has come; `None` while it has not, and once taken.
    fn try_take(&mut self) -> Option<Result<T, AgentFault>>;

    /// The answer, waiting up to `timeout`; `None` if it has not come by then,
    /// and once taken.
    fn recv_timeout(&mut self, timeout: Duration) -> Option<Result<T, AgentFault>>;
}

/// The [`AgentWindow`] for window `id`, answering through `port` while the
/// port is alive and `gone` after, and holding `collecting` (the window's
/// semantics handle) while any clone of it is alive.
#[must_use]
pub fn agent_window(
    id: WindowId,
    port: Weak<dyn AgentPort>,
    collecting: Arc<dyn Any + Send + Sync>,
) -> AgentWindow {
    AgentWindow::new(id, port, collecting)
}

/// Wrap a pending answer.
pub fn agent_answer<T>(pending: impl PendingAnswer<T> + 'static) -> AgentAnswer<T> {
    AgentAnswer::new(Box::new(pending))
}

/// Data-only phase cell shared with an internal frame composition driver.
///
/// It lets the widget binding stamp an externally-owned `Copy` phase value
/// at its exact build-to-finalize boundary without invoking foreign code
/// while the binding's inner write guard is held.
#[derive(Debug)]
pub struct FramePhaseMarker<T: Copy> {
    phase: Cell<T>,
    /// Data-only fault injection for downstream test-utils consumers.
    /// Consumed at the next build-to-finalize boundary.
    #[cfg(any(test, feature = "test-utils"))]
    panic_once_at_boundary: Cell<bool>,
}

impl<T: Copy> FramePhaseMarker<T> {
    /// Create a marker with its initial phase.
    pub fn new(initial_phase: T) -> Self {
        Self {
            phase: Cell::new(initial_phase),
            #[cfg(any(test, feature = "test-utils"))]
            panic_once_at_boundary: Cell::new(false),
        }
    }

    /// Store the phase that is about to run.
    pub fn set(&self, phase: T) {
        self.phase.set(phase);
    }

    pub(crate) fn set_at_frame_boundary(&self, phase: T) {
        self.phase.set(phase);
        #[cfg(any(test, feature = "test-utils"))]
        let should_panic = self.panic_once_at_boundary.replace(false);
        #[cfg(any(test, feature = "test-utils"))]
        assert!(
            !should_panic,
            "frame phase marker — intentional one-shot test panic"
        );
    }

    /// Read the last phase stored.
    #[must_use]
    pub fn get(&self) -> T {
        self.phase.get()
    }

    /// Arm a fixed one-shot panic at the next build-to-finalize boundary.
    ///
    /// This is a data-only runtime test seam: it stores only a phase value
    /// and never accepts executable code across the widget binding's lock.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn arm_test_panic_once(&self) {
        self.panic_once_at_boundary.set(true);
    }
}

/// A realm-level `GlobalKey` registry spanning several [`WidgetsBinding`]s —
/// one per presentation sharing a realm's `GlobalKeyScope` (ADR-0043 §1).
///
/// Assembled once over the presentations installed at the time
/// [`Self::assemble`] runs, tried in the given order (a realm's mount
/// order). `GlobalKeyScope`'s uniqueness invariant guarantees at most one
/// binding ever answers a given hash, so trying each in turn and returning
/// the first hit is exact, not a heuristic — see
/// `key::registry::build_composite`'s doc for why resolving the follow-up
/// `with_element` call correctly (rather than by the same blind scan) needs
/// a small correlation cache.
#[derive(Debug)]
pub struct GlobalKeyRegistryComposite(crate::key::registry::GlobalKeyRegistryHandle);

impl GlobalKeyRegistryComposite {
    /// Assemble a composite spanning `bindings`' own registries, in order.
    #[must_use]
    pub fn assemble<'a>(bindings: impl IntoIterator<Item = &'a WidgetsBinding>) -> Self {
        let handles = bindings
            .into_iter()
            .map(WidgetsBinding::global_key_registry_handle)
            .collect();
        Self(crate::key::registry::build_composite(handles))
    }

    /// Activate this composite for the dynamic extent of `f` — the
    /// multi-presentation counterpart to
    /// [`BindingRuntime::with_global_key_registry`].
    pub fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        crate::key::registry::with_active_registry(&self.0, f)
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::WidgetsBinding {}
}

/// The composition root's access to one [`WidgetsBinding`].
///
/// Sealed: [`WidgetsBinding`] is its only implementation. Callers import it
/// as `use flui_view::__runtime::BindingRuntime as _;` and keep calling the
/// methods on the binding.
pub trait BindingRuntime: sealed::Sealed {
    /// Run one owner-runtime entry with this binding's `GlobalKey` registry
    /// active on the current thread.
    ///
    /// Activation is nested and unwind-safe; after `f` returns or panics the
    /// previous realm is restored. Raw TLS/registry handles remain private to
    /// `flui-view`.
    fn with_global_key_registry<R>(&self, f: impl FnOnce() -> R) -> R;

    /// Pump a widget frame and stamp `finalize_phase` at the exact boundary
    /// between the build drain and inactive-element finalization.
    ///
    /// Only a [`FramePhaseMarker`] write occurs while the binding's inner
    /// write guard and debug building-flag guard remain active; no external
    /// executable callback crosses that lock boundary. Ordinary users call
    /// [`WidgetsBinding::draw_frame`].
    fn draw_frame_with_phase_marker<T: Copy>(
        &self,
        marker: &FramePhaseMarker<T>,
        finalize_phase: T,
    );

    /// This binding's exact local lifecycle source.
    fn lifecycle_source(&self) -> &LifecycleSource;

    /// Notify legacy observers and scoped subscriptions of a state already
    /// committed through [`Self::lifecycle_source`] (the terminal ladder
    /// commits with `commit_terminal`, which
    /// [`WidgetsBinding::handle_app_lifecycle_state_changed`] refuses).
    fn notify_committed_lifecycle(&self, state: AppLifecycleState);
}

impl BindingRuntime for WidgetsBinding {
    fn with_global_key_registry<R>(&self, f: impl FnOnce() -> R) -> R {
        crate::key::registry::with_active_registry(&self.global_key_registry, f)
    }

    fn draw_frame_with_phase_marker<T: Copy>(
        &self,
        marker: &FramePhaseMarker<T>,
        finalize_phase: T,
    ) {
        self.draw_frame_impl(|| marker.set_at_frame_boundary(finalize_phase));
    }

    fn lifecycle_source(&self) -> &LifecycleSource {
        &self.lifecycle
    }

    fn notify_committed_lifecycle(&self, state: AppLifecycleState) {
        self.notify_lifecycle(state);
    }
}
