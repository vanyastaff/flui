//! WidgetsBinding - owner-local binding for one UI realm.
//!
//! This module provides the binding that coordinates:
//! - BuildOwner for managing element rebuilds
//! - ElementTree for storing elements
//! - Root element attachment
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `WidgetsBinding` mixin:
//!
//! ```dart
//! mixin WidgetsBinding on BindingBase, ServicesBinding, SchedulerBinding,
//!     GestureBinding, RendererBinding, SemanticsBinding {
//!   @override
//!   void initInstances() {
//!     super.initInstances();
//!     _instance = this;
//!     // ...
//!   }
//!
//!   static WidgetsBinding get instance => BindingBase.checkInstance(_instance);
//!   static WidgetsBinding? _instance;
//! }
//! ```
//!
//! # Architecture
//!
//! ```text
//! WidgetsBinding (owned by UiRealm / headless harness / plugin pipeline)
//!   ├── build_owner: BuildOwner     (manages dirty elements)
//!   ├── element_tree: ElementTree   (stores elements)
//!   ├── root_element: ElementId     (root of element tree)
//!   └── observers: Vec<Observer>    (lifecycle notifications)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_view::WidgetsBinding;
//!
//! // The UiRealm owns the binding; construct it directly.
//! let binding = WidgetsBinding::new();
//!
//! // Attach root widget
//! binding.attach_root_widget(&MyApp);
//!
//! // In frame loop
//! binding.draw_frame();
//! ```

use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use flui_foundation::ElementId;
use flui_interaction::FocusManager;
use flui_rendering::pipeline::PipelineCell;
use parking_lot::RwLock;

use crate::{
    owner::{BuildOwner, RecoveredPanic},
    tree::ElementTree,
    view::{RootRenderView, View},
};

/// Default physical size, in pixels, for the root [`RenderView`] created
/// when [`WidgetsBinding::attach_root_widget`] bootstraps the render tree.
///
/// `flui-view`'s binding is intra-crate — it has no window object to
/// query, so the root [`RootRenderView`] is seeded with a default size
/// rather than a real window size. `800x600` is deliberately the same
/// value `flui_app::AppConfig::default()` uses for the initial window
/// size, keeping one consistent default across the workspace.
///
/// This is only a *seed*: the size is not permanent. `RootRenderView`
/// is itself a [`View`], so a later rebuild with a differently-sized
/// `RootRenderView` flows the real window dimensions in through
/// `RootRenderElement::update`, which re-applies the
/// [`ViewConfiguration`](flui_rendering::view::ViewConfiguration).
///
/// [`RenderView`]: flui_rendering::view::RenderView
const DEFAULT_ROOT_VIEW_SIZE: (f64, f64) = (800.0, 600.0);

// ============================================================================
// Route Information
// ============================================================================

/// Information about a route for navigation.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `RouteInformation` from `router.dart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInformation {
    /// The URI of the route (path + query + fragment).
    pub uri: String,
    /// Optional state key associated with this route.
    /// Unlike Flutter which uses arbitrary state, we use a string key
    /// that can reference stored state elsewhere.
    pub state_key: Option<String>,
}

impl RouteInformation {
    /// Create a new RouteInformation with just a URI.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            state_key: None,
        }
    }

    /// Create a new RouteInformation with URI and state key.
    pub fn with_state_key(uri: impl Into<String>, state_key: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            state_key: Some(state_key.into()),
        }
    }
}

// ============================================================================
// App Exit Response
// ============================================================================

/// Response to an app exit request.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `AppExitResponse` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppExitResponse {
    /// Allow the app to exit.
    Exit,
    /// Cancel the exit request.
    Cancel,
}

// ============================================================================
// View Focus Event
// ============================================================================

/// Event describing a change in view focus state.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `ViewFocusEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewFocusEvent {
    /// The view ID that changed focus.
    pub view_id: u64,
    /// Whether the view gained or lost focus.
    pub state: ViewFocusState,
    /// The direction of focus change.
    pub direction: ViewFocusDirection,
}

/// The state of view focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewFocusState {
    /// View gained focus.
    Focused,
    /// View lost focus.
    Unfocused,
}

/// The direction of focus change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewFocusDirection {
    /// Focus moved forward (e.g., Tab).
    Forward,
    /// Focus moved backward (e.g., Shift+Tab).
    Backward,
    /// Focus changed without direction (e.g., mouse click).
    Undefined,
}

// ============================================================================
// Predictive Back Event (Android)
// ============================================================================

/// Event for predictive back gesture (Android 13+).
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `PredictiveBackEvent`.
#[derive(Debug, Clone, Copy)]
pub struct PredictiveBackEvent {
    /// Progress of the back gesture (0.0 to 1.0).
    pub progress: f64,
    /// X coordinate of the touch.
    pub touch_x: f64,
    /// Y coordinate of the touch.
    pub touch_y: f64,
    /// Whether the swipe is from the left edge.
    pub swipe_edge_left: bool,
}

// ============================================================================
// WidgetsBindingObserver
// ============================================================================

/// Observer for widgets binding lifecycle events.
///
/// Implement this trait to receive notifications about:
/// - Locale changes
/// - Metrics changes (window resize)
/// - App lifecycle changes
/// - Memory pressure
/// - Navigation events
/// - Back gestures (Android predictive back)
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `WidgetsBindingObserver` mixin class.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{WidgetsBindingObserver, AppLifecycleState};
/// use std::future::Future;
/// use std::pin::Pin;
///
/// struct MyObserver;
///
/// impl WidgetsBindingObserver for MyObserver {
///     fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
///         println!("App lifecycle changed to: {:?}", state);
///     }
///
///     fn did_pop_route(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
///         Box::pin(async {
///             // Handle back navigation
///             true // We handled it
///         })
///     }
/// }
/// ```
pub trait WidgetsBindingObserver {
    // ========================================================================
    // Navigation
    // ========================================================================

    /// Called when the system tells the app to pop the current route.
    ///
    /// This is triggered by the system back button or back gesture.
    /// Return `true` if handled (e.g., by closing a dialog), `false` otherwise.
    /// If no observer returns `true`, the application may quit.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBindingObserver.didPopRoute()`.
    fn did_pop_route(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async { false })
    }

    /// Called when the host tells the app to push a new route.
    ///
    /// Return `true` if handled, `false` otherwise.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBindingObserver.didPushRouteInformation()`.
    fn did_push_route_information(
        &self,
        _route: &RouteInformation,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async { false })
    }

    // ========================================================================
    // Predictive Back Gesture (Android 13+)
    //
    // REMOVE_BY: 2026-12-22 — scheduled cleanup reminder. These four trait
    // methods + the matching `WidgetsBinding::handle_*_back_gesture`
    // impls + the `back_gesture_observers` storage are Android-13+
    // infrastructure waiting on the `flui-platform` Android wire-up. No
    // in-workspace `impl WidgetsBindingObserver` overrides them today.
    // Still deferred: the `flui-platform` Android backend is compiled
    // and linted in CI but dispatches no back-gesture events — nothing
    // in the workspace constructs a `PredictiveBackEvent` outside this
    // crate — so there is still nothing to wire this surface to. By
    // this date either delete the whole surface (no consumer
    // materialized) OR wire the platform side and drop this marker.
    // ========================================================================

    /// Called at the start of a predictive back gesture.
    ///
    /// Return `true` to handle the gesture (start animation), `false`
    /// otherwise. If `true`, subsequent gesture events will be sent to this
    /// observer.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBindingObserver.handleStartBackGesture()`.
    fn handle_start_back_gesture(&self, _event: PredictiveBackEvent) -> bool {
        false
    }

    /// Called when a predictive back gesture moves.
    ///
    /// Only called if `handle_start_back_gesture` returned `true`.
    fn handle_update_back_gesture_progress(&self, _event: PredictiveBackEvent) {}

    /// Called when a predictive back gesture is committed.
    ///
    /// The route should be popped.
    fn handle_commit_back_gesture(&self) {}

    /// Called when a predictive back gesture is canceled.
    ///
    /// The animation should be reversed.
    fn handle_cancel_back_gesture(&self) {}

    // ========================================================================
    // Metrics and Display
    // ========================================================================

    /// Called when the system locale changes.
    fn did_change_locales(&self) {}

    /// Called when window metrics change (size, DPI, etc).
    fn did_change_metrics(&self) {}

    /// Called when text scale factor changes.
    fn did_change_text_scale_factor(&self) {}

    /// Called when platform brightness changes (light/dark mode).
    fn did_change_platform_brightness(&self) {}

    // ========================================================================
    // App Lifecycle
    // ========================================================================

    /// Called when app lifecycle state changes.
    fn did_change_app_lifecycle_state(&self, _state: AppLifecycleState) {}

    /// Called when the view focus changes.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBindingObserver.didChangeViewFocus()`.
    fn did_change_view_focus(&self, _event: ViewFocusEvent) {}

    /// Called when a request is received from the system to exit the
    /// application.
    ///
    /// Return `AppExitResponse::Cancel` to prevent exit.
    /// All observers are asked before exiting.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBindingObserver.didRequestAppExit()`.
    fn did_request_app_exit(&self) -> Pin<Box<dyn Future<Output = AppExitResponse> + Send + '_>> {
        Box::pin(async { AppExitResponse::Exit })
    }

    // ========================================================================
    // System Events
    // ========================================================================

    /// Called when system is running low on memory.
    fn did_have_memory_pressure(&self) {}

    /// Called when accessibility features change.
    fn did_change_accessibility_features(&self) {}
}

/// Application lifecycle states.
///
/// Re-exported from [`flui_scheduler::AppLifecycleState`] — the canonical
/// Flutter-parity lifecycle enum (`UpdateScheduler::handle_app_lifecycle_state_change`,
/// binding.dart:414-441). `flui-view` previously defined its own parallel
/// `Resumed`/`Inactive`/`Hidden`/`Paused`/`Detached` enum; the two were
/// consolidated onto the scheduler's copy (ADR-0035) since it is the one
/// tied to real frame-scheduling behavior (`frames_enabled`).
pub use flui_scheduler::AppLifecycleState;

/// The owner-local binding for one widgets layer.
///
/// WidgetsBinding manages:
/// - A single ElementTree rooted at `root_element`
/// - A BuildOwner that tracks dirty elements
/// - Lifecycle observers
/// - First frame tracking
///
/// # Ownership
///
/// Not a singleton: the binding is owned by its `UiRealm` (one per UI
/// session; `HeadlessBinding` owns its own for tests). Construct via
/// [`WidgetsBinding::new`]:
///
/// ```rust,ignore
/// let binding = WidgetsBinding::new();
/// binding.attach_root_widget(&my_view);
/// ```
///
/// # Thread Safety
///
/// WidgetsBinding uses internal RwLock for thread-safe mutable access.
pub struct WidgetsBinding {
    pub(crate) lifecycle: crate::lifecycle::LifecycleSource,
    /// Inner mutable state. `Arc` so the GlobalKey registry closures can
    /// hold a `Weak` back-reference to *this* binding's tree (a dead
    /// binding's keys resolve to `None` — the weak-callback pattern);
    /// the lock itself is the pre-lease interior-mutability shape (C5
    /// endgame removes it once `&mut self` threading lands).
    inner: Arc<RwLock<WidgetsBindingInner>>,

    /// This binding's GlobalKey lookup handle. It is activated by the owning
    /// `UiRealm` for the dynamic extent of each realm entry.
    pub(crate) global_key_registry: crate::key::registry::GlobalKeyRegistryHandle,

    /// Callback when a frame is needed.
    on_need_frame: RwLock<Option<Box<dyn Fn() + Send + Sync>>>,

    /// Whether the first frame has been rasterized.
    first_frame_rasterized: AtomicBool,

    /// Whether binding is ready to produce frames.
    ready_to_produce_frames: AtomicBool,

    /// Whether we are currently building dirty elements (Flutter parity flag).
    ///
    /// Hoisted out of `WidgetsBindingInner` so that `handle_build_scheduled`
    /// can check it WITHOUT taking the `inner` RwLock. This eliminates the
    /// deadlock that would occur when `schedule_build_for` is called from
    /// inside `build_scope` (which holds `inner.write()`) and the
    /// `on_build_scheduled` callback calls `handle_build_scheduled`:
    ///
    /// * Before (broken): callback → `handle_build_scheduled` → `inner.read()`
    ///   → parking_lot non-reentrant read-under-write → deadlock.
    /// * After (E0a): callback → `handle_build_scheduled` → atomic load →
    ///   no lock acquired.
    ///
    /// The flag is set/cleared in `draw_frame` at the same program points
    /// the previous `inner.debug_building_dirty_elements` field was, so
    /// Flutter-parity semantics are preserved.
    #[cfg(debug_assertions)]
    debug_building_dirty_elements: AtomicBool,
}

/// Inner mutable state of WidgetsBinding
struct WidgetsBindingInner {
    /// The build owner manages dirty elements and rebuild scheduling.
    build_owner: BuildOwner,

    /// The element tree stores all elements.
    element_tree: ElementTree,

    /// The root element ID (set after attachRootWidget).
    root_element: Option<ElementId>,

    /// Owner-local handle to the render tree's `PipelineOwner`.
    /// This is set by the application binding (`flui-app`'s `UiRealm`)
    /// and propagated to elements during mounting.
    pipeline_owner: Option<PipelineCell>,

    /// Lifecycle observers.
    observers: Vec<Arc<dyn WidgetsBindingObserver>>,

    /// Observers currently handling a predictive back gesture (Android).
    ///
    // REMOVE_BY: 2026-12-22 — scheduled cleanup reminder. The predictive-
    // back-gesture surface (`handle_*_back_gesture` trait methods +
    // `back_gesture_observers` storage + `WidgetsBinding::handle_*_
    // back_gesture` impls) is Android-13+ infrastructure waiting on the
    // `flui-platform` Android side, which still dispatches no such
    // events — see the matching marker on the trait surface above for
    // the verified detail. By this date either delete this surface (no
    // consumer materialized) OR wire the platform side and drop this
    // marker.
    back_gesture_observers: Vec<Arc<dyn WidgetsBindingObserver>>,

    /// Whether a build has been scheduled.
    build_scheduled: bool,

    /// Whether we need to report the first frame.
    need_to_report_first_frame: bool,
}

impl Default for WidgetsBinding {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned by [`WidgetsBinding::attach_root_widget`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AttachError {
    /// A root widget is already attached; call `detach_root_widget` first.
    #[error("Root widget already attached. Call detach_root_widget first.")]
    AlreadyAttached,
}

impl WidgetsBinding {
    /// Create a binding with a fresh, isolated focus manager.
    ///
    /// Production presentation composition should use
    /// [`Self::with_focus_manager`] so its input dispatcher and element tree
    /// receive the same manager.
    ///
    /// # GlobalKey registry ownership
    ///
    /// Constructing the binding creates (but does not globally install) a
    /// [`GlobalKey`](crate::GlobalKey) lookup handle whose closures hold a
    /// `Weak` reference to **this** binding's element tree, so
    /// `GlobalKey::current_element` / `with_current_state` resolve against
    /// the tree that actually hosts the elements. (The previous shape
    /// captured the `WidgetsBinding::instance()` singleton lazily, so
    /// production lookups resolved against an empty tree the moment a
    /// non-singleton binding drove the frames.) The owning runtime activates
    /// the handle only while entering this realm.
    pub fn new() -> Self {
        Self::with_focus_manager(FocusManager::new())
    }

    /// Create a binding using the presentation's exact focus manager.
    ///
    /// The manager is passed directly into the binding's `BuildOwner`; no
    /// ambient registry or later setter participates in focus ownership.
    pub fn with_focus_manager(focus_manager: Rc<FocusManager>) -> Self {
        let lifecycle = crate::lifecycle::LifecycleSource::new();
        let mut build_owner = BuildOwner::with_focus_manager(focus_manager);
        build_owner.set_lifecycle_handle(lifecycle.handle());
        let inner = Arc::new(RwLock::new(WidgetsBindingInner {
            build_owner,
            element_tree: ElementTree::new(),
            root_element: None,
            pipeline_owner: None,
            observers: Vec::new(),
            back_gesture_observers: Vec::new(),
            build_scheduled: false,
            need_to_report_first_frame: true,
        }));
        let global_key_registry = Self::make_global_key_registry(&inner);
        Self {
            lifecycle,
            inner,
            global_key_registry,
            on_need_frame: RwLock::new(None),
            first_frame_rasterized: AtomicBool::new(false),
            ready_to_produce_frames: AtomicBool::new(false),
            #[cfg(debug_assertions)]
            debug_building_dirty_elements: AtomicBool::new(false),
        }
    }

    /// Build the `GlobalKey` registry handle for one binding.
    ///
    /// The `lookup`/`visit` closures hold a `Weak` reference to the
    /// binding's inner state: lookups resolve against **this** binding's
    /// element tree while it lives, and become inert `None`s once it
    /// drops (the weak-callback pattern — a dead tree is a miss, never a
    /// dangle and never a panic).
    ///
    /// Neither closure blocks. The binding holds its own write lock across a
    /// frame, attach, detach and layout-builder build, and user code inside
    /// those (build, lifecycle hooks, dispose) may read a `GlobalKey`; a
    /// blocking read there would wait on its own thread forever. The binding
    /// is `!Send`, so a lock that cannot be taken at once is held by this
    /// thread, and the closure reports `RegistryBusy` instead. The read is
    /// recursive because `GlobalKey::with_current_state`'s callback runs
    /// inside the visit's read guard and may look up a second key; with a
    /// fair lock a plain nested read could queue behind a waiting writer.
    fn make_global_key_registry(
        inner: &Arc<RwLock<WidgetsBindingInner>>,
    ) -> crate::key::registry::GlobalKeyRegistryHandle {
        use crate::key::registry::RegistryBusy;

        let lookup_inner = Arc::downgrade(inner);
        let visit_inner = Arc::downgrade(inner);
        crate::key::registry::GlobalKeyRegistryHandle::new(
            move |key| {
                let Some(inner) = lookup_inner.upgrade() else {
                    return Ok(None);
                };
                let Some(inner) = inner.try_read_recursive() else {
                    return Err(RegistryBusy);
                };
                Ok(inner.build_owner.element_for_global_key(key))
            },
            move |id, f| {
                let Some(inner) = visit_inner.upgrade() else {
                    return Ok(());
                };
                let Some(inner) = inner.try_read_recursive() else {
                    return Err(RegistryBusy);
                };
                if let Some(node) = inner.element_tree.get(id) {
                    f(node.element());
                }
                Ok(())
            },
        )
    }

    /// This binding's own `GlobalKey` registry handle, for composing into a
    /// [`GlobalKeyRegistryComposite`](crate::__runtime::GlobalKeyRegistryComposite)
    /// spanning several bindings.
    pub(crate) fn global_key_registry_handle(
        &self,
    ) -> crate::key::registry::GlobalKeyRegistryHandle {
        self.global_key_registry.clone()
    }

    /// Set the [`PipelineCell`] for render tree management.
    ///
    /// This should be called by the application binding (`flui-app`'s
    /// `UiRealm`) before attaching the root widget. The pipeline cell will
    /// be propagated to elements during mounting so they can create their
    /// RenderObjects.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this is handled by the RendererBinding mixin which provides
    /// access to `pipelineOwner` and `rootPipelineOwner`.
    pub fn set_pipeline_owner(&self, owner: PipelineCell) {
        self.inner.write().pipeline_owner = Some(owner);
        tracing::debug!("WidgetsBinding: PipelineOwner set");
    }

    /// Get the [`PipelineCell`] if set.
    ///
    /// `PipelineCell` is a lock-free, closure-scoped handle, not a lock
    /// guard.
    pub fn pipeline_owner(&self) -> Option<PipelineCell> {
        self.inner.read().pipeline_owner.clone()
    }

    /// Set the callback for when a frame is needed.
    pub fn set_on_need_frame<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let _prev = self.on_need_frame.write().replace(Box::new(callback));
    }

    // ========================================================================
    // Build Owner Access
    // ========================================================================

    /// Execute a function with read access to the build owner.
    pub fn with_build_owner<R>(&self, f: impl FnOnce(&BuildOwner) -> R) -> R {
        f(&self.inner.read().build_owner)
    }

    /// Execute a function with write access to the build owner.
    pub fn with_build_owner_mut<R>(&self, f: impl FnOnce(&mut BuildOwner) -> R) -> R {
        f(&mut self.inner.write().build_owner)
    }

    /// Drain this presentation's recovered panics — every lifecycle-hook
    /// panic a per-child containment seam caught and recovered from since the
    /// last drain.
    ///
    /// Drain after the build segment before forwarding diagnostics. If a
    /// host omits the drain, the next
    /// [`Self::draw_frame`] discards the stale records with an aggregate
    /// warning. `flui-testing`'s `HeadlessBinding::build_owner_mut` reaches
    /// the same underlying `BuildOwner::take_recovered_panics` directly for
    /// tests that want the drain without a realm in the loop.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_view::WidgetsBinding;
    ///
    /// let binding = WidgetsBinding::new();
    /// let recovered = binding.take_recovered_panics();
    /// assert!(recovered.is_empty());
    /// ```
    #[must_use = "discarding the drain loses lifecycle-panic diagnostics"]
    pub fn take_recovered_panics(&self) -> Vec<RecoveredPanic> {
        self.inner.write().build_owner.take_recovered_panics()
    }

    /// Atomic seeded observer install (ADR-0040 §3): under ONE `inner`
    /// write guard, replay the current tree into `observer`
    /// (`ElementTree::replay_mounts`), then install it as the realm's
    /// observer. Every frame drive also holds the `inner` write lock, so no
    /// tree mutation can interleave between replay and install — the
    /// consumer's baseline is exact.
    ///
    /// A panic during replay aborts the install: nothing is registered and
    /// the aborted install is logged (the observer proved untrustworthy
    /// before it was ever installed).
    pub fn install_tree_observer(
        &self,
        observer: std::sync::Arc<dyn flui_foundation::observe::TreeObserver>,
    ) {
        let mut inner = self.inner.write();
        let replay_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inner.element_tree.replay_mounts(&*observer);
        }))
        .is_ok();
        if replay_ok {
            inner.build_owner.set_tree_observer(observer);
        } else {
            tracing::error!(
                "TreeObserver panicked during seeded replay; install aborted, nothing registered"
            );
        }
    }

    /// Symmetric removal (install/clear symmetry); fires `detached()` on
    /// the outgoing observer. Idempotent.
    pub fn remove_tree_observer(&self) {
        self.inner.write().build_owner.clear_tree_observer();
    }

    // ========================================================================
    // Element Tree Access
    // ========================================================================

    /// Execute a function with read access to the element tree.
    pub fn with_element_tree<R>(&self, f: impl FnOnce(&ElementTree) -> R) -> R {
        f(&self.inner.read().element_tree)
    }

    /// Execute a function with write access to the element tree.
    pub fn with_element_tree_mut<R>(&self, f: impl FnOnce(&mut ElementTree) -> R) -> R {
        f(&mut self.inner.write().element_tree)
    }

    /// Get the root element ID.
    pub fn root_element(&self) -> Option<ElementId> {
        self.inner.read().root_element
    }

    // ========================================================================
    // Root Widget Attachment
    // ========================================================================

    /// Attach a root widget to the binding.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// The PipelineOwner (if set via `set_pipeline_owner`) will be passed
    /// to the root element during mounting, enabling RenderObjectElements
    /// to create their RenderObjects.
    ///
    /// # Root bootstrap
    ///
    /// The user `view` is not mounted directly. It is wrapped in a
    /// [`RootRenderView`], and *that* is mounted as the element-tree
    /// root. [`RootRenderView`] / `RootRenderElement` own the root
    /// [`RenderView`](flui_rendering::view::RenderView) and bootstrap
    /// the render tree (creating the `RenderView`, setting it as the
    /// `PipelineOwner`'s root node). This is the single root-bootstrap
    /// path — there is no parallel direct-mount of the user view.
    ///
    /// # Flutter Equivalent
    ///
    /// Mirrors `WidgetsBinding.attachRootWidget` →
    /// `RootWidget.attach` → `RenderObjectToWidgetAdapter`
    /// (`packages/flutter/lib/src/widgets/binding.dart`), where the
    /// user widget is likewise wrapped in a root widget that owns the
    /// `RenderView` before being attached to the build owner.
    ///
    /// # Errors
    ///
    /// Returns [`AttachError::AlreadyAttached`] if a root widget is
    /// already attached.
    pub fn attach_root_widget<V>(&self, view: &V) -> Result<(), AttachError>
    where
        V: View + Clone + 'static,
    {
        self.attach_root_widget_with_size(view, DEFAULT_ROOT_VIEW_SIZE.0, DEFAULT_ROOT_VIEW_SIZE.1)
    }

    /// Attach a root widget sizing the root `RenderView` to an explicit
    /// logical `width` × `height` (e.g. the platform window's surface size)
    /// instead of the crate-internal default root view size.
    ///
    /// This is the entry point the platform runner uses so the root view is
    /// born at the real window size rather than the 800×600 fallback. Per-frame
    /// layout constraints are still supplied by `draw_frame`; this only sets the
    /// root view object's intrinsic size at bootstrap. See [`attach_root_widget`]
    /// for the full bootstrap contract.
    ///
    /// [`attach_root_widget`]: Self::attach_root_widget
    ///
    /// # Errors
    ///
    /// Returns [`AttachError::AlreadyAttached`] if a root widget is already
    /// attached.
    pub fn attach_root_widget_with_size<V>(
        &self,
        view: &V,
        width: f64,
        height: f64,
    ) -> Result<(), AttachError>
    where
        V: View + Clone + 'static,
    {
        let mut inner = self.inner.write();

        if inner.root_element.is_some() {
            return Err(AttachError::AlreadyAttached);
        }

        // Wrap the user view in `RootRenderView` so the render tree is
        // bootstrapped through `RootRenderElement` (Flutter's
        // `RenderObjectToWidgetAdapter` shape) instead of mounting the
        // user view directly.
        //
        // The user view is cloned (not `BoxedView`-wrapped) so the
        // concrete `V` is preserved as the `RootRenderView<V>` /
        // `RootRenderElement<V>` type parameter. On subsequent root
        // rebuilds `RootRenderElement::perform_build` hands the stored
        // child to `Element<V>::update_view` via `&dyn View`; that
        // method downcasts the trait object back to `V`. A `BoxedView`
        // wrap would make the runtime type `BoxedView` (not `V`), the
        // downcast in `ElementCore::update_view` would fail, and the
        // root update would be silently skipped — this was caught as a
        // real regression, so keep the clone-not-wrap shape.
        //
        // The `Clone + 'static` bound is no real
        // restriction in practice — every concrete `View` in this
        // codebase already satisfies it (see `Element<V, A, B>`'s
        // own bound).
        let root_render_view = RootRenderView::new(view.clone(), width, height);

        // Mount the `RootRenderView` as the element-tree root with the
        // PipelineOwner. This ensures `RootRenderElement` (and the
        // RenderObjectElements below it) can create their RenderObjects.
        // Split the borrow so the BuildOwner-derived ElementOwner handle
        // and the ElementTree borrow don't overlap.
        let pipeline_owner = inner.pipeline_owner.clone();
        let root_id = {
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            element_tree.mount_root_with_pipeline_owner(
                &root_render_view,
                pipeline_owner,
                &mut build_owner.element_owner_mut(),
            )
        };
        inner.root_element = Some(root_id);

        // Schedule initial build
        inner
            .build_owner
            .schedule_build_for(root_id, 0, crate::RebuildReason::InitialMount);
        inner.build_scheduled = true;

        tracing::debug!(?root_id, "Root widget attached");

        // Request a frame
        drop(inner); // Release lock before calling callback
        self.handle_build_scheduled();

        Ok(())
    }

    /// Detach the root widget.
    ///
    /// This clears the element tree — the WHOLE tree, not just the root
    /// node: `ElementTree::remove_subtree` (not the single-node `remove`)
    /// walks every descendant deepest-first, running each one's own
    /// `unmount`/`State::dispose` hook, exactly as a real permanent removal
    /// must. A plain `remove` would only unmount the root's own element
    /// (in practice the internal `RootRenderView` wrapper every attached
    /// view is mounted under — see [`Self::attach_root_widget`]'s doc) and
    /// silently orphan everything beneath it: no descendant's `dispose`
    /// would ever run, leaking subscriptions/listeners/timers for the life
    /// of this binding.
    pub fn detach_root_widget(&self) {
        let mut inner = self.inner.write();

        if let Some(root_id) = inner.root_element.take() {
            // Remove the whole subtree (this clears the tree since root_id
            // is its root).
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            // `Finalize`: permanent teardown. Nothing runs `finalize_tree`
            // after this and no frame follows, so a keyed descendant left
            // deactivated would never have `dispose` called and a later
            // `attach_root_widget` could retake its stale state.
            element_tree.remove_subtree(
                root_id,
                &mut build_owner.element_owner_mut(),
                crate::tree::SubtreeRemoval::Finalize,
            );
            tracing::debug!(?root_id, "Root widget detached");
        }
    }

    // ========================================================================
    // Build Scheduling
    // ========================================================================

    /// Schedule a build if not already scheduled.
    pub fn schedule_build(&self) {
        let mut inner = self.inner.write();
        if !inner.build_scheduled {
            inner.build_scheduled = true;
            drop(inner); // Release lock before calling callback
            self.handle_build_scheduled();
        }
    }

    /// Schedule the root element and all its descendants for rebuild.
    ///
    /// This is useful for animation demos where the entire tree needs to
    /// rebuild each frame to reflect updated animation values.
    pub fn schedule_root_rebuild(&self) {
        let mut inner = self.inner.write();
        if let Some(root_id) = inner.root_element {
            // Collect all element IDs first to avoid borrow issues
            let elements_to_mark = Self::collect_all_elements(&inner.element_tree, root_id, 0);

            // Now mark all as dirty
            for (id, depth) in elements_to_mark {
                inner.element_tree.mark_needs_build(id);
                inner.build_owner.schedule_build_for(
                    id,
                    depth,
                    crate::RebuildReason::AnimationTick,
                );
            }

            if !inner.build_scheduled {
                inner.build_scheduled = true;
                drop(inner);
                self.handle_build_scheduled();
            }
        }
    }

    /// Iteratively collect every `(ElementId, depth)` pair reachable from
    /// `id`, in pre-order DFS order (parent before its children, children
    /// in `ElementNode::child_ids` slot order).
    ///
    /// The earlier recursive shape did
    /// `result.extend(recursive_call(child))` once per child, so each
    /// `extend` re-copied its child's entire subtree into the parent's
    /// vec. For a balanced tree of N elements that totals `O(N log N)`
    /// allocation+copy; for a degenerate chain (the FLUI worst case where
    /// many `StatelessView`s nest linearly) it is `O(N²)`. The recursion
    /// also burned stack proportional to tree depth.
    ///
    /// This implementation pre-sizes a single `Vec<(ElementId, usize)>`
    /// to `tree.len()` (every node in the slab is at most one entry) and
    /// drives the walk with an explicit `Vec` work-stack. Total work is
    /// `O(N)` with one heap allocation amortised across the whole walk;
    /// stack depth is the constant size of two `Vec`s.
    ///
    /// **Ordering contract.** The previous recursive shape pushed the
    /// current node first, then for each child appended that child's
    /// entire pre-order subtree before moving to the next child — i.e.
    /// classic pre-order DFS in `child_ids` slot order. To preserve that
    /// ordering on a LIFO work-stack we visit each node when popped and
    /// then push its children **in reverse `child_ids` order**, so
    /// the leftmost child is on top of the stack and popped next. The
    /// per-element pop / visit / push-children sequence is identical to
    /// the recursive function call sequence.
    fn collect_all_elements(
        tree: &ElementTree,
        root_id: ElementId,
        root_depth: usize,
    ) -> Vec<(ElementId, usize)> {
        // Tree-len upper-bounds the number of reachable nodes. We may
        // visit strictly fewer (the walk is rooted at `root_id`, not the
        // full slab), but over-reserving by a few entries is cheaper than
        // reallocating during the walk.
        let mut result: Vec<(ElementId, usize)> = Vec::with_capacity(tree.len());
        let mut stack: Vec<(ElementId, usize)> = Vec::with_capacity(16);
        let mut child_buf: Vec<ElementId> = Vec::with_capacity(8);

        stack.push((root_id, root_depth));

        while let Some((id, depth)) = stack.pop() {
            result.push((id, depth));

            let Some(node) = tree.get(id) else {
                continue;
            };

            // E3 (atomic box→arena swap): children come from the slab
            // node's `child_ids` list — the single element graph. They
            // are already in forward slot order; we collect into a scratch
            // buffer so we can push them back onto the LIFO stack in
            // reverse, preserving the recursive shape's pre-order visit
            // order.
            child_buf.clear();
            child_buf.extend_from_slice(node.child_ids());
            for child_id in child_buf.iter().rev() {
                stack.push((*child_id, depth + 1));
            }
        }

        result
    }

    /// Called when a build has been scheduled.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this checks that we're not currently building and calls
    /// `ensureVisualUpdate()` which schedules a frame via `SchedulerBinding`.
    ///
    /// # Deadlock-safety
    ///
    /// This method is intentionally **lock-free w.r.t. `self.inner`**.
    ///
    /// It is called from inside `build_scope` (via the `on_build_scheduled`
    /// callback on `BuildOwner`) while `draw_frame` holds `inner.write()`.
    /// Taking `inner.read()` or `inner.write()` here would deadlock on
    /// parking_lot's non-reentrant `RwLock`. Instead the building flag is
    /// an `AtomicBool` on `WidgetsBinding` itself (outside `inner`), and
    /// `on_need_frame` is its own separate `RwLock` that is never held
    /// across any `inner` critical section.
    ///
    /// # Panics (debug only — Flutter parity)
    ///
    /// Panics if called while building dirty elements. In Flutter this check
    /// catches `setState()` called from a layout or paint callback.
    pub fn handle_build_scheduled(&self) {
        #[cfg(debug_assertions)]
        {
            assert!(
                !self.debug_building_dirty_elements.load(Ordering::Relaxed),
                "setState() or markNeedsBuild() called during build.\n\
                 While the widget tree was being built, laid out, and painted, \
                 a new frame was scheduled to rebuild the widget tree.\n\
                 Do not call setState() from a build, layout, or paint callback."
            );
        }

        // Request a frame from the scheduler (ensureVisualUpdate).
        // `on_need_frame` is a leaf RwLock — it is never held across any
        // acquisition of `self.inner`, so taking it here is deadlock-free.
        if let Some(ref callback) = *self.on_need_frame.read() {
            callback();
        }
    }

    /// Check if there are pending builds.
    pub fn has_pending_builds(&self) -> bool {
        self.inner.read().build_owner.has_dirty_elements()
    }

    /// Get the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.inner.read().build_owner.dirty_count()
    }

    /// Flutter `WidgetsBinding.performReassemble()` — hot reload entry point.
    ///
    /// Marks every element dirty without unmounting or disposing state. The
    /// next [`draw_frame`](Self::draw_frame) re-runs all `build()` methods while
    /// preserving [`StatefulView`](crate::StatefulView) state in the element tree.
    pub fn perform_reassemble(&self) {
        {
            let mut inner = self.inner.write();
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.reassemble(element_tree);
        }
        self.handle_build_scheduled();
    }

    // ========================================================================
    // Frame Drawing
    // ========================================================================

    /// Pump the build and rendering pipeline to generate a frame.
    ///
    /// This method is called by `handleDrawFrame`, which is called
    /// automatically by the engine when it is time to lay out and paint a
    /// frame.
    ///
    /// The application frame driver calls this once for every produced
    /// presentation segment, even when [`Self::has_pending_builds`] is false.
    /// The build drain below remains conditional, while the frame-entry
    /// expiry of undrained recovered-panic records and inactive-element
    /// finalization still run for pipeline-only and lazy-service-only frames.
    ///
    /// # Frame phases
    ///
    /// 1. **Build phase**: All dirty `Element`s in the widget tree are rebuilt.
    ///    See `State.setState` for details on marking a widget dirty.
    ///
    /// 2. **Layout phase**: (handled by RendererBinding.drawFrame)
    ///
    /// 3. **Paint phase**: (handled by RendererBinding.drawFrame)
    ///
    /// 4. **Finalization phase**: Inactive elements are unmounted. This causes
    ///    [State.dispose] to be invoked on removed widgets.
    ///
    /// # Panics
    ///
    /// In debug mode, panics if called while already building dirty elements
    /// (to catch accidental frame scheduling during build).
    pub fn draw_frame(&self) {
        self.draw_frame_impl(|| {});
    }

    pub(crate) fn draw_frame_impl(&self, before_finalize: impl FnOnce()) {
        let mut inner = self.inner.write();

        #[cfg(debug_assertions)]
        let _building_guard = {
            assert!(
                !self.debug_building_dirty_elements.load(Ordering::Relaxed),
                "draw_frame called while already building dirty elements; \
                 ensure draw_frame is not called recursively or from a build callback"
            );
            // Set before build_scope so that any on_build_scheduled callback
            // fired from within build_scope sees building=true and panics with
            // the Flutter-parity message rather than enqueuing a second frame.
            //
            // Cleared by RAII rather than a store at the end of this
            // function: a panic that unwinds out of `build_scope` or
            // `finalize_tree` (e.g. a panicking `ViewState::dispose`) is
            // caught at the app runtime's per-presentation frame boundary
            // and the frame is retried on a later pump — a flag latched
            // `true` across that unwind would turn every retry into this
            // assert's false positive ("recursive draw_frame") instead of
            // re-running the frame.
            struct BuildingFlagReset<'a>(&'a AtomicBool);
            impl Drop for BuildingFlagReset<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Relaxed);
                }
            }
            self.debug_building_dirty_elements
                .store(true, Ordering::Relaxed);
            BuildingFlagReset(&self.debug_building_dirty_elements)
        };

        let discarded_recovered_panics = inner.build_owner.discard_stale_recovered_panics();
        if discarded_recovered_panics != 0 {
            tracing::warn!(
                count = discarded_recovered_panics,
                "discarding recovered lifecycle panics left undrained from the previous frame"
            );
        }

        inner.build_scheduled = false;

        // Build phase: rebuild all dirty elements
        if inner.build_owner.has_dirty_elements() {
            tracing::debug!(
                dirty_count = inner.build_owner.dirty_count(),
                "Building dirty elements"
            );

            // Process all dirty elements.
            // We need to split the borrow to satisfy the borrow checker.
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.build_scope(element_tree);

            tracing::debug!("Build phase complete");
        }

        // Note: Layout and paint phases would be called here via super.draw_frame()
        // in a full implementation with RendererBinding

        before_finalize();

        // Finalization phase: unmount inactive elements
        {
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.finalize_tree(element_tree);
        }

        // `debug_building_dirty_elements` resets when `_building_guard`
        // drops at the end of this function (and equally during an unwind).

        // Report first frame if needed
        if inner.need_to_report_first_frame {
            inner.need_to_report_first_frame = false;
            tracing::info!("First frame rendered");
        }
    }

    /// Service pending lazy-sliver child-build requests accumulated during the
    /// most recent layout pass.
    ///
    /// This is the **production entry point** for lazy `SliverList` /
    /// `ListView::builder` child building. It mirrors step 6 of
    /// `HeadlessBinding::pump_frame`, and must be called immediately after
    /// `PipelineOwner::run_frame` releases its write-lock so no `NodePtr`
    /// alias is live.
    ///
    /// `UiRealm::draw_frame` (`flui-app`) calls this method after the
    /// `run_frame` write-guard drops, passing the same shared `PipelineOwner`
    /// that `run_frame` used. The lock order is `widgets → pipeline`: the
    /// `widgets` write-lock (`WidgetsBinding::inner`) is held here; the brief
    /// `pipeline.write()` taken inside the delegate is a narrower, nested
    /// acquisition. This matches the order established in Phase 1 (`build_scope`
    /// → `pipeline`) and does not introduce a new ordering edge.
    ///
    /// # True cost on a no-lazy-list app
    ///
    /// This method is **not** a free no-op on applications without lazy lists.
    /// It always takes a brief `pipeline.write()` to drain the two pending
    /// `Vec`s (`pending_child_requests` and `pending_retain_bands`) before
    /// the early-return triggered by their being empty. On a no-lazy-list app
    /// that is two short lock acquisitions on empty vecs per frame (~microseconds
    /// on an uncontested `parking_lot::RwLock`). Callers that need to avoid even
    /// this overhead can gate on `pipeline.read().has_pending_child_requests()`
    /// before calling, but the unconditional call is simpler and the cost is
    /// negligible in practice.
    ///
    /// # Flutter parity — production↔headless convergence point
    ///
    /// This call site is the **production↔headless convergence point** for the
    /// post-`run_frame` pipeline tail steps. `HeadlessBinding::pump_frame`
    /// step 6 and `UiRealm::draw_frame` (via this method) now execute the
    /// same code path. Future gap-#2 work (production Vsync / implicit-animation
    /// tick) will land at the same `draw_frame` call site immediately after this
    /// call, keeping both bindings in sync.
    pub fn service_child_requests(&self, pipeline: &PipelineCell) {
        let mut inner = self.inner.write();
        let WidgetsBindingInner {
            ref mut build_owner,
            ref mut element_tree,
            ..
        } = *inner;
        build_owner.service_child_requests(element_tree, pipeline);
    }

    /// Run one frame, settling every build-during-layout node before paint.
    ///
    /// Threads the `PipelineOwner` **by lock**, not by value: each layout pass
    /// takes it out under the write guard and restores it before the builders
    /// run, because `build_scope` mounts render objects through that same lock.
    ///
    /// This is the second production↔headless convergence point:
    /// `UiRealm::draw_frame` reaches the shared fixpoint through here, and
    /// `HeadlessBinding::pump_frame` calls
    /// `BuildOwner::run_frame_with_layout_builders` directly (it owns its
    /// `BuildOwner` and `ElementTree` without a lock). Both end up in the same
    /// helper; neither hand-rolls the loop.
    ///
    /// With no `LayoutBuilder` mounted, this is exactly `PipelineOwner::run_frame`.
    pub fn run_frame_with_layout_builders(
        &self,
        pipeline: &PipelineCell,
    ) -> flui_rendering::error::RenderResult<Option<flui_rendering::layer::LayerTree>> {
        let mut inner = self.inner.write();
        let WidgetsBindingInner {
            ref mut build_owner,
            ref mut element_tree,
            ..
        } = *inner;
        build_owner.run_frame_with_layout_builders(element_tree, pipeline)
    }

    /// Check if we are currently building dirty elements.
    ///
    /// Reads the atomic flag directly — no lock acquired.
    #[cfg(debug_assertions)]
    pub fn is_building(&self) -> bool {
        self.debug_building_dirty_elements.load(Ordering::Relaxed)
    }

    // ========================================================================
    // Observers
    // ========================================================================

    /// Add a lifecycle observer.
    pub fn add_observer(&self, observer: Arc<dyn WidgetsBindingObserver>) {
        self.inner.write().observers.push(observer);
    }

    /// Remove a lifecycle observer.
    pub fn remove_observer(&self, observer: &Arc<dyn WidgetsBindingObserver>) {
        self.inner
            .write()
            .observers
            .retain(|o| !Arc::ptr_eq(o, observer));
    }

    /// Notify all observers of locale change.
    ///
    /// Snapshots the observer list under the read lock and releases the
    /// lock before invoking callbacks. An observer callback
    /// that re-enters the binding (e.g., adds or removes an observer,
    /// reads `observer_count`, or schedules a build) would deadlock if
    /// the iteration held the lock across the dispatch.
    pub fn handle_locale_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_locales();
        }
    }

    /// Notify all observers of metrics change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale.
    pub fn handle_metrics_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_metrics();
        }
    }

    /// Notify all observers of text scale factor change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale.
    pub fn handle_text_scale_factor_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_text_scale_factor();
        }
    }

    /// Notify all observers of platform brightness change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale.
    pub fn handle_platform_brightness_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_platform_brightness();
        }
    }

    /// Commit a local lifecycle observation and notify legacy and scoped listeners.
    /// Legacy observers retain their snapshot iteration behavior. Scoped
    /// subscriptions still drain when a legacy observer panics.
    pub fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState) {
        if self.lifecycle.commit(state).is_ok() {
            self.notify_lifecycle(state);
        }
    }

    pub(crate) fn notify_lifecycle(&self, state: AppLifecycleState) {
        let mut first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let observers: Vec<Arc<dyn WidgetsBindingObserver>> =
                self.inner.read().observers.clone();
            for observer in &observers {
                observer.did_change_app_lifecycle_state(state);
            }
        }))
        .err();
        crate::lifecycle::preserve(
            &mut first,
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.lifecycle.drain())).err(),
        );
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
        }
    }

    /// Notify all observers of memory pressure.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale.
    pub fn handle_memory_pressure(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_have_memory_pressure();
        }
    }

    /// Notify all observers of accessibility features change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale.
    pub fn handle_accessibility_features_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_accessibility_features();
        }
    }

    /// Get the number of elements in the tree.
    pub fn element_count(&self) -> usize {
        self.inner.read().element_tree.len()
    }

    /// Get the number of observers.
    pub fn observer_count(&self) -> usize {
        self.inner.read().observers.len()
    }

    // ========================================================================
    // First Frame Tracking
    // ========================================================================

    /// Whether the first frame has been rasterized.
    ///
    /// Usually, the time that a frame is rasterized is very close to the time
    /// it gets presented on the display.
    pub fn first_frame_rasterized(&self) -> bool {
        self.first_frame_rasterized.load(Ordering::Acquire)
    }

    /// Mark the first frame as rasterized.
    ///
    /// Called by the engine after the first frame is painted.
    pub fn mark_first_frame_rasterized(&self) {
        self.first_frame_rasterized.store(true, Ordering::Release);
        tracing::debug!("First frame rasterized");
    }

    // Note: the first-frame *deferral counter* (`defer_first_frame` /
    // `allow_first_frame` / `send_frames_to_engine`) used to be duplicated
    // here as its own independent `AtomicU32` + `AtomicBool` pair. Flutter
    // has exactly one such counter, on `RendererBinding` (`WidgetsBinding`
    // is a mixin on top of it and shares the same state); a second,
    // unrelated counter on this binding could drift from the real one and
    // never actually gated anything reachable from the production frame
    // path. It has been removed — the single canonical counter lives on
    // `RenderingFlutterBinding` (`crates/flui-app/src/bindings/
    // renderer_binding.rs`), forwarded through `UiRealm::defer_first_frame`
    // / `allow_first_frame` / `send_frames_to_engine`, and consulted by
    // `UiRealm::render_frame_entered`.

    /// Whether the binding is ready to produce frames.
    pub fn is_ready_to_produce_frames(&self) -> bool {
        self.ready_to_produce_frames.load(Ordering::Acquire)
    }

    /// Mark the binding as ready to produce frames.
    pub fn mark_ready_to_produce_frames(&self) {
        self.ready_to_produce_frames.store(true, Ordering::Release);
    }

    // ========================================================================
    // Navigation Handling
    // ========================================================================

    /// Handle a pop route request from the system.
    ///
    /// Notifies observers until one returns `true`, meaning it handled the
    /// request. If none return `true`, the application may quit.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBinding.handlePopRoute()`.
    pub async fn handle_pop_route(&self) -> bool {
        let observers: Vec<_> = self.inner.read().observers.clone();
        for observer in observers {
            if observer.did_pop_route().await {
                return true;
            }
        }
        // No observer handled - application may quit
        false
    }

    /// Handle a push route request from the host.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBinding.handlePushRoute()`.
    pub async fn handle_push_route(&self, route: &RouteInformation) -> bool {
        let observers: Vec<_> = self.inner.read().observers.clone();
        for observer in observers {
            if observer.did_push_route_information(route).await {
                return true;
            }
        }
        false
    }

    // ========================================================================
    // Predictive Back Gesture (Android)
    //
    // REMOVE_BY: 2026-12-22 — scheduled cleanup reminder. See the matching
    // marker on the `WidgetsBindingObserver::handle_*_back_gesture` trait
    // surface for the rationale and dispose-or-wire decision rule.
    // ========================================================================

    /// Handle the start of a predictive back gesture.
    ///
    /// Returns `true` if any observer is handling the gesture.
    pub fn handle_start_back_gesture(&self, event: PredictiveBackEvent) -> bool {
        let mut inner = self.inner.write();
        inner.back_gesture_observers.clear();

        // Clone observers to avoid holding lock during callback
        let observers: Vec<_> = inner.observers.clone();
        drop(inner);

        let mut handling_observers = Vec::new();
        for observer in observers {
            if observer.handle_start_back_gesture(event) {
                handling_observers.push(observer);
            }
        }

        if handling_observers.is_empty() {
            false
        } else {
            self.inner.write().back_gesture_observers = handling_observers;
            true
        }
    }

    /// Handle progress update for a predictive back gesture.
    pub fn handle_update_back_gesture_progress(&self, event: PredictiveBackEvent) {
        let observers: Vec<_> = self.inner.read().back_gesture_observers.clone();
        for observer in &observers {
            observer.handle_update_back_gesture_progress(event);
        }
    }

    /// Handle commit of a predictive back gesture.
    ///
    /// If no observer was handling the gesture, falls back to
    /// `handle_pop_route`.
    pub async fn handle_commit_back_gesture(&self) {
        let observers: Vec<_> = self.inner.read().back_gesture_observers.clone();
        if observers.is_empty() {
            // No predictive handler - fall back to normal pop
            self.handle_pop_route().await;
            return;
        }
        for observer in &observers {
            observer.handle_commit_back_gesture();
        }
    }

    /// Handle cancellation of a predictive back gesture.
    pub fn handle_cancel_back_gesture(&self) {
        let observers: Vec<_> = self.inner.read().back_gesture_observers.clone();
        for observer in &observers {
            observer.handle_cancel_back_gesture();
        }
    }

    // ========================================================================
    // View Focus
    // ========================================================================

    /// Handle view focus change.
    ///
    /// Snapshots the observer list under the read lock and releases the
    /// lock before invoking callbacks. See
    /// [`Self::handle_locale_changed`] for the deadlock-safety rationale.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBinding.handleViewFocusChanged()`.
    pub fn handle_view_focus_changed(&self, event: ViewFocusEvent) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_view_focus(event);
        }
    }

    // ========================================================================
    // App Exit Request
    // ========================================================================

    /// Handle an app exit request from the system.
    ///
    /// All observers are asked. If any returns `Cancel`, the exit is prevented.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBinding.handleRequestAppExit()`.
    pub async fn handle_request_app_exit(&self) -> AppExitResponse {
        let observers: Vec<_> = self.inner.read().observers.clone();
        let mut should_cancel = false;

        for observer in observers {
            if observer.did_request_app_exit().await == AppExitResponse::Cancel {
                should_cancel = true;
                // Don't return early - all observers should be notified
            }
        }

        if should_cancel {
            AppExitResponse::Cancel
        } else {
            AppExitResponse::Exit
        }
    }
}

impl std::fmt::Debug for WidgetsBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.read();
        f.debug_struct("WidgetsBinding")
            .field("root_element", &inner.root_element)
            .field("build_scheduled", &inner.build_scheduled)
            .field(
                "first_frame_rasterized",
                &self.first_frame_rasterized.load(Ordering::Relaxed),
            )
            .field(
                "ready_to_produce_frames",
                &self.ready_to_produce_frames.load(Ordering::Relaxed),
            )
            .field("dirty_count", &inner.build_owner.dirty_count())
            .field("element_count", &inner.element_tree.len())
            .field("observer_count", &inner.observers.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::view::IntoView;

    use std::rc::Rc;

    use flui_objects::RenderSizedBox;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::__runtime::BindingRuntime as _;
    use crate::RootRenderElement;

    // Carries a `PipelineCell` (via `CoreState::pipeline_owner`), so it must
    // stay pinned `!Send + !Sync` -- a stray `unsafe impl` here would let a
    // `WidgetsBinding` cross threads while its `PipelineCell` still aliases
    // the owner-thread-confined `PipelineOwner`/`RenderTree` underneath.
    assert_not_impl_any!(WidgetsBinding: Send, Sync);

    /// A render-family leaf view with no child views.
    #[derive(Clone)]
    struct LeafView;

    impl crate::RenderView for LeafView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for LeafView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    #[derive(Clone)]
    struct RegistryStateView {
        key: crate::GlobalKey<RegistryState>,
        value: i32,
    }

    struct RegistryState(i32);

    impl crate::StatefulView for RegistryStateView {
        type State = RegistryState;

        fn create_state(&self) -> Self::State {
            RegistryState(self.value)
        }
    }

    impl crate::ViewState<RegistryStateView> for RegistryState {
        fn build(
            &self,
            _view: &RegistryStateView,
            _ctx: &dyn crate::BuildContext,
        ) -> impl IntoView {
            LeafView
        }
    }

    impl View for RegistryStateView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    fn binding_is_not_a_singleton_two_instances_are_independent() {
        // The binding is realm-owned; two bindings are two
        // independent trees (HeadlessBinding's "many can exist" contract,
        // now true of the widgets binding itself).
        let binding1 = WidgetsBinding::new();
        let binding2 = WidgetsBinding::new();
        assert!(!Arc::ptr_eq(&binding1.inner, &binding2.inner));
    }

    /// Run `f` on a fresh thread and fail the test if it has not returned
    /// within ten seconds.
    ///
    /// The binding is `!Send`, so `f` builds it on that thread. A lookup that
    /// re-enters the binding's frame lock parks the thread forever; the
    /// timeout turns that hang into a test failure, and the parked thread
    /// does not keep the test process alive.
    fn within_deadline<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("deadlock: GlobalKey lookup re-entered the binding's frame lock")
    }

    /// Every result a probe read, in order.
    type Seen<T> = Rc<std::cell::RefCell<Vec<Option<T>>>>;

    /// Reads `key`'s state from its own `build` and records what it saw.
    #[derive(Clone)]
    struct LookupInBuild {
        key: crate::GlobalKey<RegistryState>,
        seen: Seen<i32>,
    }

    impl crate::StatelessView for LookupInBuild {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> impl IntoView {
            self.seen
                .borrow_mut()
                .push(self.key.with_current_state(|state| state.0));
            LeafView
        }
    }

    impl View for LookupInBuild {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    /// A keyed stateful parent whose child is a [`LookupInBuild`].
    #[derive(Clone)]
    struct KeyedLookupParent {
        key: crate::GlobalKey<RegistryState>,
        value: i32,
        child: LookupInBuild,
    }

    impl crate::StatefulView for KeyedLookupParent {
        type State = RegistryState;

        fn create_state(&self) -> Self::State {
            RegistryState(self.value)
        }
    }

    impl crate::ViewState<KeyedLookupParent> for RegistryState {
        fn build(&self, view: &KeyedLookupParent, _ctx: &dyn crate::BuildContext) -> impl IntoView {
            view.child.clone()
        }
    }

    impl View for KeyedLookupParent {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    fn global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking() {
        let (during, after) = within_deadline(|| {
            let key = crate::GlobalKey::<RegistryState>::new();
            let seen = Seen::default();
            let binding = WidgetsBinding::new();
            binding
                .attach_root_widget(&KeyedLookupParent {
                    key: key.clone(),
                    value: 10,
                    child: LookupInBuild {
                        key: key.clone(),
                        seen: Rc::clone(&seen),
                    },
                })
                .expect("attach succeeds");
            binding.with_global_key_registry(|| binding.draw_frame());
            let after =
                binding.with_global_key_registry(|| key.with_current_state(|state| state.0));
            (seen.take(), after)
        });
        assert_eq!(
            during,
            vec![None],
            "a read inside the binding's own frame resolves to nothing"
        );
        assert_eq!(after, Some(10), "the same read after the frame resolves");
    }

    /// The mounted root element produces a working render-tree root
    /// when a `PipelineOwner` is wired — `RootRenderElement` inserts the
    /// `RenderView` and sets it as the pipeline owner's root node.
    fn test_attach_root_widget_bootstraps_render_tree() {
        let binding = WidgetsBinding::new();
        let pipeline_owner = PipelineCell::new(PipelineOwner::new());
        binding.set_pipeline_owner(pipeline_owner.clone());

        binding
            .attach_root_widget(&LeafView)
            .expect("attach succeeds");

        let root_id = binding.root_element().expect("root element is set");

        // The RootRenderElement created a RenderView and registered it
        // as the PipelineOwner's root node.
        binding.with_element_tree(|tree| {
            let element = tree.get(root_id).expect("root node").element();
            let root_render_element = element
                .as_any()
                .downcast_ref::<RootRenderElement<LeafView>>()
                .expect("root is RootRenderElement");
            assert!(
                root_render_element.render_id().is_some(),
                "RootRenderElement bootstrapped a RenderView (render_id set)"
            );
        });
        assert!(
            pipeline_owner.with(|owner| owner.root_id().is_some()),
            "PipelineOwner's root node is wired to the RenderView"
        );
    }

    #[derive(Clone)]
    struct DisposeMarkerView {
        disposed: Arc<parking_lot::Mutex<bool>>,
    }

    struct DisposeMarkerState {
        disposed: Arc<parking_lot::Mutex<bool>>,
    }

    impl crate::StatefulView for DisposeMarkerView {
        type State = DisposeMarkerState;

        fn create_state(&self) -> Self::State {
            DisposeMarkerState {
                disposed: Arc::clone(&self.disposed),
            }
        }
    }

    impl crate::ViewState<DisposeMarkerView> for DisposeMarkerState {
        fn build(
            &self,
            _view: &DisposeMarkerView,
            _ctx: &dyn crate::BuildContext,
        ) -> impl IntoView {
            LeafView
        }

        fn dispose(&mut self) {
            *self.disposed.lock() = true;
        }
    }

    impl View for DisposeMarkerView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }
    }

    /// `detach_root_widget` must cascade through the WHOLE tree, not just
    /// unmount the root's own node: the attached root is always wrapped in
    /// an internal `RootRenderView` (see this method's own doc), so even a
    /// single-widget tree is already two elements deep, and a real tree is
    /// deeper still (this one plants the dispose-observing element as a
    /// stateful GRANDCHILD of the reconciled `ParentView`/`LeafView` shape
    /// used elsewhere in this file). A single-node `remove` would only ever
    /// unmount `RootRenderView`'s own element and silently leak every
    /// descendant's `State::dispose` — this test is the regression guard for
    /// that exact class of bug.
    fn detach_root_widget_disposes_every_descendant_not_just_the_root() {
        #[derive(Clone)]
        struct ParentOfDisposeMarker {
            child: DisposeMarkerView,
        }

        impl crate::StatelessView for ParentOfDisposeMarker {
            fn build(&self, _ctx: &dyn crate::BuildContext) -> impl IntoView {
                self.child.clone()
            }
        }

        impl View for ParentOfDisposeMarker {
            fn create_element(&self) -> crate::element::ElementKind {
                crate::element::ElementKind::stateless(self)
            }
        }

        let binding = WidgetsBinding::new();
        let disposed = Arc::new(parking_lot::Mutex::new(false));
        let view = ParentOfDisposeMarker {
            child: DisposeMarkerView {
                disposed: Arc::clone(&disposed),
            },
        };

        binding.attach_root_widget(&view).expect("attach succeeds");
        binding.draw_frame();
        assert!(!*disposed.lock(), "precondition: nothing has torn down yet");

        binding.detach_root_widget();

        assert!(
            *disposed.lock(),
            "detach_root_widget must run State::dispose for every descendant, \
             not just unmount the (internal RootRenderView-wrapped) root node"
        );
    }

    #[test]
    fn binding_lifecycle_matrix() {
        crate::table_test::run_table(
            "binding_lifecycle_matrix",
            &[
                (
                    "binding_is_not_a_singleton_two_instances_are_independent",
                    binding_is_not_a_singleton_two_instances_are_independent as fn(),
                ),
                (
                    "global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking",
                    global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking
                        as fn(),
                ),
                (
                    "test_attach_root_widget_bootstraps_render_tree",
                    test_attach_root_widget_bootstraps_render_tree as fn(),
                ),
                (
                    "detach_root_widget_disposes_every_descendant_not_just_the_root",
                    detach_root_widget_disposes_every_descendant_not_just_the_root as fn(),
                ),
            ],
        );
    }

    // ========================================================================
    // collect_all_elements — iterative O(N) walk
    // ========================================================================
    //
    // These tests pin the contract of
    // `WidgetsBinding::collect_all_elements`:
    //
    // 1. It returns every `(ElementId, depth)` pair reachable from the
    //    starting `root_id`, with depths offset by the supplied
    //    `root_depth` (i.e. the root's recorded depth equals
    //    `root_depth`, each child recorded depth equals `root_depth + 1`,
    //    and so on).
    // 2. The traversal is pre-order DFS in each element's `child_ids`
    //    slot order — parent first, then each child's entire subtree
    //    before moving on to the next sibling.
    // 3. Order is deterministic across runs.
    // 4. Deep linear chains do not exhaust the stack (the walk is
    //    iterative).

    // ========================================================================
    // Snapshot-then-fire fix — regression tests for sync handle_* event
    // handlers
    // ========================================================================

    // ========================================================================
    // E0a — deadlock-safe wake chain
    // ========================================================================

    // ========================================================================
    // did_change_app_lifecycle_state dispatch
    // ========================================================================
}
