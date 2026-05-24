//! WidgetsBinding - Singleton binding for the widgets layer.
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
//! WidgetsBinding (singleton)
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
//! // Get the singleton instance
//! let binding = WidgetsBinding::instance();
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
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use flui_foundation::{BindingBase, ElementId, impl_binding_singleton};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;

use crate::{
    owner::BuildOwner,
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
const DEFAULT_ROOT_VIEW_SIZE: (f32, f32) = (800.0, 600.0);

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
    pub progress: f32,
    /// X coordinate of the touch.
    pub touch_x: f32,
    /// Y coordinate of the touch.
    pub touch_y: f32,
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
pub trait WidgetsBindingObserver: Send + Sync {
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
    // REMOVE_BY: 2026-09-22 — audit V-24 cadence marker. These four trait
    // methods + the matching `WidgetsBinding::handle_*_back_gesture`
    // impls + the `back_gesture_observers` storage are Android-13+
    // infrastructure waiting on the `flui-platform` Android wire-up. No
    // in-workspace `impl WidgetsBindingObserver` overrides them today.
    // By the cadence date either delete the whole surface (no consumer
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLifecycleState {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// App is visible and responding to user input.
    Resumed,
    /// App is inactive (e.g., incoming call).
    Inactive,
    /// App is not visible but running.
    Hidden,
    /// App is paused (backgrounded).
    Paused,
    /// App is being destroyed.
    Detached,
}

/// The singleton binding for the widgets layer.
///
/// WidgetsBinding manages:
/// - A single ElementTree rooted at `root_element`
/// - A BuildOwner that tracks dirty elements
/// - Lifecycle observers
/// - First frame tracking
///
/// # Singleton Pattern
///
/// Access via `WidgetsBinding::instance()`:
///
/// ```rust,ignore
/// let binding = WidgetsBinding::instance();
/// binding.attach_root_widget(&my_view);
/// ```
///
/// # Thread Safety
///
/// WidgetsBinding uses internal RwLock for thread-safe mutable access.
pub struct WidgetsBinding {
    /// Inner mutable state protected by RwLock
    inner: RwLock<WidgetsBindingInner>,

    /// Callback when a frame is needed.
    #[allow(clippy::type_complexity)]
    on_need_frame: RwLock<Option<Box<dyn Fn() + Send + Sync>>>,

    /// Whether the first frame has been rasterized.
    first_frame_rasterized: AtomicBool,

    /// Count of deferred first frame requests.
    /// When > 0, the first frame is deferred (e.g., for splash screens).
    first_frame_deferred_count: AtomicU32,

    /// Whether the first frame has been sent to the engine.
    first_frame_sent: AtomicBool,

    /// Whether binding is ready to produce frames.
    ready_to_produce_frames: AtomicBool,
}

/// Inner mutable state of WidgetsBinding
struct WidgetsBindingInner {
    /// The build owner manages dirty elements and rebuild scheduling.
    build_owner: BuildOwner,

    /// The element tree stores all elements.
    element_tree: ElementTree,

    /// The root element ID (set after attachRootWidget).
    root_element: Option<ElementId>,

    /// Pipeline owner for render tree management.
    /// This is set by the application binding (e.g., WidgetsFlutterBinding)
    /// and propagated to elements during mounting.
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,

    /// Lifecycle observers.
    observers: Vec<Arc<dyn WidgetsBindingObserver>>,

    /// Observers currently handling a predictive back gesture (Android).
    ///
    // REMOVE_BY: 2026-09-22 — audit V-24 cadence marker. The predictive-
    // back-gesture surface (`handle_*_back_gesture` trait methods +
    // `back_gesture_observers` storage + `WidgetsBinding::handle_*_
    // back_gesture` impls) is Android-13+ infrastructure waiting on the
    // `flui-platform` Android side. By the cadence date either delete
    // this surface (no consumer materialized) OR wire the platform side
    // and drop this marker.
    back_gesture_observers: Vec<Arc<dyn WidgetsBindingObserver>>,

    /// Whether a build has been scheduled.
    build_scheduled: bool,

    /// Whether we need to report the first frame.
    need_to_report_first_frame: bool,

    /// Whether we are currently building dirty elements.
    ///
    /// This is used to verify that frames are not scheduled redundantly.
    /// In debug mode, scheduling a frame while building will panic.
    #[cfg(debug_assertions)]
    debug_building_dirty_elements: bool,
}

// Implement BindingBase trait
impl BindingBase for WidgetsBinding {
    fn init_instances(&mut self) {
        // WidgetsBinding initialization is done in new()
        tracing::debug!("WidgetsBinding initialized");
    }
}

// Implement singleton pattern via macro
impl_binding_singleton!(WidgetsBinding);

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
    /// Create a new WidgetsBinding.
    ///
    /// Note: Prefer using `WidgetsBinding::instance()` for singleton access.
    ///
    /// # GlobalKey registry installation
    ///
    /// Constructing the binding also installs a process-wide
    /// [`GlobalKey`](crate::GlobalKey) lookup handle pointed at this
    /// binding's singleton instance (`WidgetsBinding::instance()`), so
    /// `GlobalKey::current_element` / `with_current_state` resolve to
    /// the actively-mounted element tree in production. Tests that
    /// bypass the binding install their own handle via the explicit
    /// `crate::test_only_set_global_key_registry` shim.
    pub fn new() -> Self {
        let mut binding = Self {
            inner: RwLock::new(WidgetsBindingInner {
                build_owner: BuildOwner::new(),
                element_tree: ElementTree::new(),
                root_element: None,
                pipeline_owner: None,
                observers: Vec::new(),
                back_gesture_observers: Vec::new(),
                build_scheduled: false,
                need_to_report_first_frame: true,
                #[cfg(debug_assertions)]
                debug_building_dirty_elements: false,
            }),
            on_need_frame: RwLock::new(None),
            first_frame_rasterized: AtomicBool::new(false),
            first_frame_deferred_count: AtomicU32::new(0),
            first_frame_sent: AtomicBool::new(false),
            ready_to_produce_frames: AtomicBool::new(false),
        };
        binding.init_instances();
        Self::install_global_key_registry();
        binding
    }

    /// Install a closure-based `GlobalKey` registry handle pointing at
    /// the singleton `WidgetsBinding::instance()`.
    ///
    /// The handle's `lookup` and `visit` closures capture the binding's
    /// `&'static` singleton reference (produced by
    /// `impl_binding_singleton!`) and acquire the binding's
    /// `RwLock<WidgetsBindingInner>` read-lock per call. No additional
    /// `Arc<RwLock<_>>` wrapping is needed on the binding's storage —
    /// the singleton lifetime carries the registry's reachability.
    ///
    /// Idempotent: the registry slot is a `RwLock<Option<…>>`, so calls
    /// past the first replace the previous handle with an equivalent
    /// one.
    fn install_global_key_registry() {
        let handle = crate::key::registry::GlobalKeyRegistryHandle::new(
            |hash| {
                let binding = <WidgetsBinding as flui_foundation::HasInstance>::instance();
                let inner = binding.inner.read();
                inner.build_owner.element_for_global_key(hash)
            },
            |id, f| {
                let binding = <WidgetsBinding as flui_foundation::HasInstance>::instance();
                let inner = binding.inner.read();
                if let Some(node) = inner.element_tree.get(id) {
                    f(node.element());
                }
            },
        );
        let _ = crate::key::registry::install_registry(handle);
    }

    /// Set the PipelineOwner for render tree management.
    ///
    /// This should be called by the application binding (e.g.,
    /// WidgetsFlutterBinding) before attaching the root widget. The
    /// PipelineOwner will be propagated to elements during mounting so they
    /// can create their RenderObjects.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this is handled by the RendererBinding mixin which provides
    /// access to `pipelineOwner` and `rootPipelineOwner`.
    pub fn set_pipeline_owner(&self, owner: Arc<RwLock<PipelineOwner>>) {
        self.inner.write().pipeline_owner = Some(owner);
        tracing::debug!("WidgetsBinding: PipelineOwner set");
    }

    /// Get the PipelineOwner if set.
    pub fn pipeline_owner(&self) -> Option<Arc<RwLock<PipelineOwner>>> {
        // PORT-CHECK-OK-SP6: binding layer Arc<RwLock<PipelineOwner>> leak; consolidation tracked under architecture-correction-plan SP-6
        self.inner.read().pipeline_owner.clone()
    }

    /// Set the callback for when a frame is needed.
    pub fn set_on_need_frame<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.on_need_frame.write() = Some(Box::new(callback));
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

    // PORT-TARGET: flui-app runner root-bootstrap consolidation, pending Cycle 6 element-ownership unification (V-7 deferral)
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
        V: View + Clone + Send + Sync + 'static,
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
        // root update would be silently skipped (PR #119 review —
        // codex P1).
        //
        // The `Clone + Send + Sync + 'static` bound is no real
        // restriction in practice — every concrete `View` in this
        // codebase already satisfies it (see `Element<V, A, B>`'s
        // own bound).
        let root_render_view = RootRenderView::new(
            view.clone(),
            DEFAULT_ROOT_VIEW_SIZE.0,
            DEFAULT_ROOT_VIEW_SIZE.1,
        );

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
        inner.build_owner.schedule_build_for(root_id, 0);
        inner.build_scheduled = true;

        tracing::debug!(?root_id, "Root widget attached");

        // Request a frame
        drop(inner); // Release lock before calling callback
        self.handle_build_scheduled();

        Ok(())
    }

    /// Detach the root widget.
    ///
    /// This clears the element tree.
    pub fn detach_root_widget(&self) {
        let mut inner = self.inner.write();

        if let Some(root_id) = inner.root_element.take() {
            // Remove root element (this clears the tree since it's the root)
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            let _ = element_tree.remove(root_id, &mut build_owner.element_owner_mut());
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
                inner.build_owner.schedule_build_for(id, depth);
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
    /// in `visit_children` order).
    ///
    /// Plan §U12 / R15 — audit V-16. The earlier recursive shape did
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
    /// classic pre-order DFS in `visit_children` order. To preserve that
    /// ordering on a LIFO work-stack we visit each node when popped and
    /// then push its children **in reverse `visit_children` order**, so
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

            // `visit_children` walks forward; we collect into a scratch
            // buffer so we can push the children back onto the LIFO stack
            // in reverse, preserving the recursive shape's pre-order
            // visit order.
            child_buf.clear();
            node.element().visit_children(&mut |child_id| {
                child_buf.push(child_id);
            });
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
    /// # Panics
    ///
    /// In debug mode, panics if called while building dirty elements.
    fn handle_build_scheduled(&self) {
        #[cfg(debug_assertions)]
        {
            let inner = self.inner.read();
            assert!(
                !inner.debug_building_dirty_elements,
                "Build scheduled during frame.\n\
                 While the widget tree was being built, laid out, and painted, \
                 a new frame was scheduled to rebuild the widget tree.\n\
                 This might be because setState() was called from a layout or \
                 paint callback."
            );
        }

        // Request a frame from the scheduler (ensureVisualUpdate)
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

    // ========================================================================
    // Frame Drawing
    // ========================================================================

    /// Pump the build and rendering pipeline to generate a frame.
    ///
    /// This method is called by `handleDrawFrame`, which is called
    /// automatically by the engine when it is time to lay out and paint a
    /// frame.
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
        let mut inner = self.inner.write();

        #[cfg(debug_assertions)]
        {
            assert!(
                !inner.debug_building_dirty_elements,
                "draw_frame called while already building dirty elements"
            );
            inner.debug_building_dirty_elements = true;
        }

        inner.build_scheduled = false;

        // Build phase: rebuild all dirty elements
        if inner.build_owner.has_dirty_elements() {
            tracing::debug!(
                dirty_count = inner.build_owner.dirty_count(),
                "Building dirty elements"
            );

            // Process all dirty elements
            // We need to split the borrow to satisfy the borrow checker
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

        // Finalization phase: unmount inactive elements
        {
            let WidgetsBindingInner {
                ref mut build_owner,
                ref mut element_tree,
                ..
            } = *inner;
            build_owner.finalize_tree(element_tree);
        }

        #[cfg(debug_assertions)]
        {
            inner.debug_building_dirty_elements = false;
        }

        // Report first frame if needed
        if inner.need_to_report_first_frame {
            inner.need_to_report_first_frame = false;
            tracing::info!("First frame rendered");
        }
    }

    /// Check if we are currently building dirty elements.
    ///
    /// This is used to verify that frames are not scheduled redundantly.
    #[cfg(debug_assertions)]
    pub fn is_building(&self) -> bool {
        self.inner.read().debug_building_dirty_elements
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
    /// lock before invoking callbacks (audit V-21). An observer callback
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
    /// rationale (audit V-21).
    pub fn handle_metrics_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_metrics();
        }
    }

    /// Notify all observers of text scale factor change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale (audit V-21).
    pub fn handle_text_scale_factor_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_text_scale_factor();
        }
    }

    /// Notify all observers of platform brightness change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale (audit V-21).
    pub fn handle_platform_brightness_changed(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_platform_brightness();
        }
    }

    /// Notify all observers of app lifecycle change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale (audit V-21).
    pub fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_change_app_lifecycle_state(state);
        }
    }

    /// Notify all observers of memory pressure.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale (audit V-21).
    pub fn handle_memory_pressure(&self) {
        let observers: Vec<Arc<dyn WidgetsBindingObserver>> = self.inner.read().observers.clone();
        for observer in &observers {
            observer.did_have_memory_pressure();
        }
    }

    /// Notify all observers of accessibility features change.
    ///
    /// See [`Self::handle_locale_changed`] for the snapshot-then-fire
    /// rationale (audit V-21).
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

    /// Whether the first frame has been sent to the engine.
    ///
    /// This is set after `draw_frame` completes for the first time.
    pub fn debug_did_send_first_frame_event(&self) -> bool {
        self.first_frame_sent.load(Ordering::Acquire)
    }

    /// Defer the first frame.
    ///
    /// Used for splash screens that need to delay showing content.
    /// Call `allow_first_frame` to release.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `RendererBinding.deferFirstFrame()`.
    pub fn defer_first_frame(&self) {
        self.first_frame_deferred_count
            .fetch_add(1, Ordering::AcqRel);
        tracing::debug!("First frame deferred");
    }

    /// Allow the first frame after a previous `defer_first_frame`.
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to `RendererBinding.allowFirstFrame()`.
    pub fn allow_first_frame(&self) {
        let prev = self
            .first_frame_deferred_count
            .fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            // No more deferrals, we can send frames now
            tracing::debug!("First frame allowed - ready to produce frames");
        }
    }

    /// Whether frames should be sent to the engine.
    ///
    /// Returns false if the first frame is deferred.
    pub fn send_frames_to_engine(&self) -> bool {
        self.first_frame_deferred_count.load(Ordering::Acquire) == 0
    }

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
    // REMOVE_BY: 2026-09-22 — audit V-24 cadence marker. See the matching
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
    /// lock before invoking callbacks (audit V-21). See
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
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::view::IntoView;
    use crate::view::ViewExt;
    use std::any::TypeId;

    use flui_foundation::HasInstance;

    use super::*;
    use crate::RootRenderElement;

    /// A leaf element that doesn't create children (prevents infinite
    /// recursion)
    struct LeafElement {
        depth: usize,
        lifecycle: crate::Lifecycle,
    }

    impl LeafElement {
        fn new() -> Self {
            Self {
                depth: 0,
                lifecycle: crate::Lifecycle::Initial,
            }
        }
    }

    impl crate::ElementBase for LeafElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<LeafView>()
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn lifecycle(&self) -> crate::Lifecycle {
            self.lifecycle
        }

        fn mount(
            &mut self,
            _parent: Option<flui_foundation::ElementId>,
            slot: usize,
            _owner: &mut crate::ElementOwner<'_>,
        ) {
            self.depth = slot;
            self.lifecycle = crate::Lifecycle::Active;
        }

        fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {
            self.lifecycle = crate::Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = crate::Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = crate::Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {}

        fn mark_needs_build(&mut self) {}

        fn perform_build(&mut self, _owner: &mut crate::ElementOwner<'_>) {
            // Leaf - no children to build
        }

        fn visit_children(&self, _visitor: &mut dyn FnMut(flui_foundation::ElementId)) {
            // No children
        }
    }

    /// A leaf view that creates a LeafElement (no children)
    #[derive(Clone)]
    struct LeafView;

    impl View for LeafView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(LeafElement::new())
        }
    }

    /// A stateless view that builds a non-trivial child subtree: it
    /// produces a `LeafView` child each build. `build` returns a leaf
    /// (not `self`) so the element tree terminates — a self-returning
    /// view describes an infinitely deep tree and overflows the stack.
    #[derive(Clone)]
    struct ParentView;

    impl crate::StatelessView for ParentView {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> impl IntoView {
            LeafView.boxed()
        }
    }

    impl View for ParentView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(crate::StatelessElement::new(self, crate::StatelessBehavior))
        }
    }

    #[test]
    fn test_binding_singleton() {
        let binding1 = WidgetsBinding::instance();
        let binding2 = WidgetsBinding::instance();

        // Should be the same instance
        assert!(std::ptr::eq(binding1, binding2));
    }

    #[test]
    fn test_binding_is_initialized() {
        // Ensure instance exists
        let _ = WidgetsBinding::instance();

        // Should be initialized
        assert!(WidgetsBinding::is_initialized());
    }

    #[test]
    fn test_binding_creation() {
        let binding = WidgetsBinding::new();
        assert!(binding.root_element().is_none());
        assert!(!binding.has_pending_builds());
    }

    #[test]
    fn test_attach_root_widget() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding
            .attach_root_widget(&view)
            .expect("first attach succeeds");

        assert!(binding.root_element().is_some());
        assert!(binding.has_pending_builds());
    }

    /// U6 / AE3: `attach_root_widget` bootstraps the root through
    /// `RootRenderView` — the element-tree root is a
    /// `RootRenderElement<LeafView>`, NOT the user view's element
    /// mounted directly.
    #[test]
    fn test_attach_root_widget_routes_through_root_render_view() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding
            .attach_root_widget(&view)
            .expect("first attach succeeds");

        let root_id = binding.root_element().expect("root element is set");

        binding.with_element_tree(|tree| {
            let node = tree.get(root_id).expect("root node exists");
            let element = node.element();

            // The mounted root is the `RootRenderElement`, identified by
            // the `RootRenderView<LeafView>` view type it reports — the
            // user view's concrete type is preserved as the type
            // parameter (no `BoxedView` wrap; see PR #119 review fix).
            assert_eq!(
                element.view_type_id(),
                TypeId::of::<RootRenderView<LeafView>>(),
                "root element must be RootRenderElement<LeafView>, \
                 proving the bootstrap routes through RootRenderView"
            );

            // It is concretely a `RootRenderElement<LeafView>` — the
            // direct-mount path would have produced a `LeafElement`.
            assert!(
                element
                    .as_any()
                    .downcast_ref::<RootRenderElement<LeafView>>()
                    .is_some(),
                "root element downcasts to RootRenderElement<LeafView>"
            );
            assert!(
                element.as_any().downcast_ref::<LeafElement>().is_none(),
                "root element is NOT the user view's element mounted directly"
            );
        });
    }

    /// U6: the mounted root element produces a working render-tree root
    /// when a `PipelineOwner` is wired — `RootRenderElement` inserts the
    /// `RenderView` and sets it as the pipeline owner's root node.
    #[test]
    fn test_attach_root_widget_bootstraps_render_tree() {
        let binding = WidgetsBinding::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        binding.set_pipeline_owner(Arc::clone(&pipeline_owner));

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
            pipeline_owner.read().root_id().is_some(),
            "PipelineOwner's root node is wired to the RenderView"
        );
    }

    /// U6 edge case: a root view with zero children bootstraps
    /// correctly through `RootRenderView`.
    #[test]
    fn test_attach_root_widget_zero_child_subtree() {
        let binding = WidgetsBinding::new();

        // `LeafView` creates a `LeafElement` with no children.
        binding
            .attach_root_widget(&LeafView)
            .expect("attach succeeds");
        binding.draw_frame();

        let root_id = binding.root_element().expect("root element is set");
        binding.with_element_tree(|tree| {
            let element = tree.get(root_id).expect("root node").element();
            assert_eq!(element.lifecycle(), crate::Lifecycle::Active);
        });
    }

    /// U6 edge case: a root view that builds a non-trivial child
    /// subtree bootstraps correctly through `RootRenderView`.
    #[test]
    fn test_attach_root_widget_with_child_subtree() {
        let binding = WidgetsBinding::new();

        // `ParentView` builds a `LeafView` child each build.
        binding
            .attach_root_widget(&ParentView)
            .expect("attach succeeds");
        binding.draw_frame();

        let root_id = binding.root_element().expect("root element is set");
        binding.with_element_tree(|tree| {
            let element = tree.get(root_id).expect("root node").element();
            assert_eq!(
                element.view_type_id(),
                TypeId::of::<RootRenderView<ParentView>>(),
                "root with a child subtree still routes through RootRenderView"
            );
            assert_eq!(element.lifecycle(), crate::Lifecycle::Active);
        });
    }

    #[test]
    fn test_draw_frame() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding.attach_root_widget(&view).expect("attach succeeds");
        assert!(binding.has_pending_builds());

        binding.draw_frame();
        assert!(!binding.has_pending_builds());
    }

    #[test]
    fn test_detach_root_widget() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding.attach_root_widget(&view).expect("attach succeeds");
        assert!(binding.root_element().is_some());

        binding.detach_root_widget();
        assert!(binding.root_element().is_none());
    }

    #[test]
    fn test_double_attach_errors() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding
            .attach_root_widget(&view)
            .expect("first attach succeeds");

        assert!(matches!(
            binding.attach_root_widget(&view),
            Err(AttachError::AlreadyAttached)
        ));
    }

    // ========================================================================
    // V-16 collect_all_elements — iterative O(N) walk
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
    // 2. The traversal is pre-order DFS in the order yielded by each
    //    element's `visit_children` — parent first, then each child's
    //    entire subtree before moving on to the next sibling.
    // 3. Order is deterministic across runs.
    // 4. Deep linear chains do not exhaust the stack (the walk is
    //    iterative).

    /// Test fixture element whose `visit_children` returns the IDs in
    /// `children`. The element is configured manually after insertion via
    /// `set_children` so the test can wire up arbitrary tree shapes
    /// without relying on a full `View::build` round-trip.
    #[derive(Default)]
    struct MultiNodeElement {
        depth: usize,
        lifecycle: crate::Lifecycle,
        children: Vec<flui_foundation::ElementId>,
    }

    impl MultiNodeElement {
        fn new() -> Self {
            Self {
                depth: 0,
                lifecycle: crate::Lifecycle::Initial,
                children: Vec::new(),
            }
        }
    }

    impl crate::ElementBase for MultiNodeElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<MultiNodeView>()
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn lifecycle(&self) -> crate::Lifecycle {
            self.lifecycle
        }

        fn mount(
            &mut self,
            _parent: Option<flui_foundation::ElementId>,
            slot: usize,
            _owner: &mut crate::ElementOwner<'_>,
        ) {
            self.depth = slot;
            self.lifecycle = crate::Lifecycle::Active;
        }

        fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {
            self.lifecycle = crate::Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = crate::Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = crate::Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {}

        fn mark_needs_build(&mut self) {}

        fn perform_build(&mut self, _owner: &mut crate::ElementOwner<'_>) {}

        fn visit_children(&self, visitor: &mut dyn FnMut(flui_foundation::ElementId)) {
            for &child_id in &self.children {
                visitor(child_id);
            }
        }
    }

    #[derive(Clone)]
    struct MultiNodeView;

    impl View for MultiNodeView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(MultiNodeElement::new())
        }
    }

    /// Helper: insert a `MultiNodeView` as a child of `parent`, returning
    /// its new `ElementId`.
    fn insert_multi_child(
        tree: &mut crate::tree::ElementTree,
        build_owner: &mut crate::BuildOwner,
        parent: flui_foundation::ElementId,
        slot: usize,
    ) -> flui_foundation::ElementId {
        let view = MultiNodeView;
        tree.insert(&view, parent, slot, &mut build_owner.element_owner_mut())
    }

    /// Helper: configure the children list on the element backing `id`.
    fn set_children_for(
        tree: &mut crate::tree::ElementTree,
        id: flui_foundation::ElementId,
        children: Vec<flui_foundation::ElementId>,
    ) {
        let node = tree.get_mut(id).expect("node exists");
        let element = node
            .element_mut()
            .as_any_mut()
            .downcast_mut::<MultiNodeElement>()
            .expect("element is MultiNodeElement");
        element.children = children;
    }

    /// Happy path: a tree of `root → [a, b], a → [a1]`. The walk visits
    /// every node with the correct depths, in pre-order DFS.
    #[test]
    fn test_collect_all_elements_happy_path() {
        let mut tree = crate::tree::ElementTree::new();
        let mut build_owner = crate::BuildOwner::new();

        let root_id = tree.mount_root(&MultiNodeView, &mut build_owner.element_owner_mut());
        let alpha = insert_multi_child(&mut tree, &mut build_owner, root_id, 0);
        let bravo = insert_multi_child(&mut tree, &mut build_owner, root_id, 1);
        let alpha_child = insert_multi_child(&mut tree, &mut build_owner, alpha, 0);

        set_children_for(&mut tree, root_id, vec![alpha, bravo]);
        set_children_for(&mut tree, alpha, vec![alpha_child]);

        let walk = WidgetsBinding::collect_all_elements(&tree, root_id, 0);

        assert_eq!(
            walk,
            vec![(root_id, 0), (alpha, 1), (alpha_child, 2), (bravo, 1)],
            "pre-order DFS: parent before children, children in visit_children order"
        );
    }

    /// Edge case: a deeply unbalanced chain. The iterative walk must
    /// terminate without overflowing the stack. 1024 is well past the
    /// 50-deep threshold the plan calls out and far past what naive
    /// recursion would tolerate on Windows's smaller default thread
    /// stack.
    #[test]
    fn test_collect_all_elements_deep_chain() {
        let mut tree = crate::tree::ElementTree::new();
        let mut build_owner = crate::BuildOwner::new();

        let root_id = tree.mount_root(&MultiNodeView, &mut build_owner.element_owner_mut());

        let mut ids = vec![root_id];
        let mut current = root_id;
        for _ in 0..1024 {
            let next = insert_multi_child(&mut tree, &mut build_owner, current, 0);
            set_children_for(&mut tree, current, vec![next]);
            ids.push(next);
            current = next;
        }

        let walk = WidgetsBinding::collect_all_elements(&tree, root_id, 0);

        assert_eq!(walk.len(), ids.len(), "every chain node is visited");
        for (i, &(id, depth)) in walk.iter().enumerate() {
            assert_eq!(id, ids[i], "chain visit order is parent-first");
            assert_eq!(depth, i, "depth grows with chain index");
        }
    }

    /// Edge case: a wide shallow tree (root → 64 leaf children). The
    /// walk must visit the root then every child in `visit_children`
    /// order.
    #[test]
    fn test_collect_all_elements_wide_tree() {
        let mut tree = crate::tree::ElementTree::new();
        let mut build_owner = crate::BuildOwner::new();

        let root_id = tree.mount_root(&MultiNodeView, &mut build_owner.element_owner_mut());

        let mut children = Vec::with_capacity(64);
        for slot in 0..64 {
            children.push(insert_multi_child(
                &mut tree,
                &mut build_owner,
                root_id,
                slot,
            ));
        }
        set_children_for(&mut tree, root_id, children.clone());

        let walk = WidgetsBinding::collect_all_elements(&tree, root_id, 0);

        assert_eq!(walk.len(), 1 + children.len());
        assert_eq!(walk[0], (root_id, 0));
        for (i, &child_id) in children.iter().enumerate() {
            assert_eq!(walk[1 + i], (child_id, 1));
        }
    }

    /// Stability: running the walk twice on the same tree returns the
    /// exact same sequence — the iterative shape must not introduce
    /// any ordering nondeterminism.
    #[test]
    fn test_collect_all_elements_is_deterministic() {
        let mut tree = crate::tree::ElementTree::new();
        let mut build_owner = crate::BuildOwner::new();

        let root_id = tree.mount_root(&MultiNodeView, &mut build_owner.element_owner_mut());
        let alpha = insert_multi_child(&mut tree, &mut build_owner, root_id, 0);
        let bravo = insert_multi_child(&mut tree, &mut build_owner, root_id, 1);
        let charlie = insert_multi_child(&mut tree, &mut build_owner, root_id, 2);
        let bravo_first = insert_multi_child(&mut tree, &mut build_owner, bravo, 0);
        let bravo_second = insert_multi_child(&mut tree, &mut build_owner, bravo, 1);

        set_children_for(&mut tree, root_id, vec![alpha, bravo, charlie]);
        set_children_for(&mut tree, bravo, vec![bravo_first, bravo_second]);

        let first = WidgetsBinding::collect_all_elements(&tree, root_id, 0);
        let second = WidgetsBinding::collect_all_elements(&tree, root_id, 0);

        assert_eq!(first, second, "walk output is deterministic");
        assert_eq!(
            first,
            vec![
                (root_id, 0),
                (alpha, 1),
                (bravo, 1),
                (bravo_first, 2),
                (bravo_second, 2),
                (charlie, 1),
            ],
        );
    }

    /// The `root_depth` argument is offset onto every recorded depth —
    /// pin this so callers that recurse into a subtree at a non-zero
    /// depth still get useful values.
    #[test]
    fn test_collect_all_elements_root_depth_offset() {
        let mut tree = crate::tree::ElementTree::new();
        let mut build_owner = crate::BuildOwner::new();

        let root_id = tree.mount_root(&MultiNodeView, &mut build_owner.element_owner_mut());
        let child_id = insert_multi_child(&mut tree, &mut build_owner, root_id, 0);
        set_children_for(&mut tree, root_id, vec![child_id]);

        let walk = WidgetsBinding::collect_all_elements(&tree, root_id, 5);
        assert_eq!(walk, vec![(root_id, 5), (child_id, 6)]);
    }

    // ========================================================================
    // V-21 — snapshot-then-fire on sync handle_* event handlers
    // ========================================================================

    /// Observer whose callback re-enters the binding by taking a
    /// `write()` lock (via `add_observer`). Before the V-21 fix the
    /// `handle_*` dispatch held a `read()` lock on `self.inner` across
    /// the iteration, so this re-entrant `write()` would deadlock the
    /// thread under `parking_lot`'s non-reentrant `RwLock`. After the
    /// snapshot-then-fire fix the callbacks run with no lock held, so
    /// re-entering the binding is safe.
    struct ReentrantObserver {
        binding: &'static WidgetsBinding,
        fired: std::sync::atomic::AtomicUsize,
    }

    /// Inert observer added from inside `ReentrantObserver`'s callback to
    /// force a `write()` lock acquisition on `self.inner`. The struct is
    /// intentionally trivial; only the act of `add_observer`-ing matters.
    struct InertObserver;
    impl WidgetsBindingObserver for InertObserver {}

    impl WidgetsBindingObserver for ReentrantObserver {
        fn did_change_metrics(&self) {
            self.fired
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Re-enter the binding from inside the observer callback.
            // `add_observer` takes a `write()` lock on `self.inner` ---
            // pre-V-21 this deadlocks; post-V-21 it is safe because the
            // dispatch released the `read()` lock before iterating.
            self.binding.add_observer(Arc::new(InertObserver));
        }
    }

    /// Pre-V-21: this test would deadlock `cargo test -p flui-view --lib`.
    /// Post-V-21: `handle_metrics_changed` snapshots the observer Vec
    /// before iterating, so the re-entrant `add_observer` write lock can
    /// be acquired without blocking. The test asserts (a) the observer
    /// callback fired and (b) the re-entrant `add_observer` completed
    /// (observer_count went from 1 to 2).
    ///
    /// We intentionally `Box::leak` the binding so the `'static` lifetime
    /// on `ReentrantObserver::binding` is sound for the duration of the
    /// test. The leaked binding is small and bounded by the test run.
    #[test]
    fn handle_metrics_changed_does_not_deadlock_on_reentrant_observer() {
        // `Box::leak` gives us `&'static WidgetsBinding`, which lets
        // `ReentrantObserver` close over a borrow with a sound lifetime
        // for the duration of the test. The leaked binding is small and
        // bounded by the test run.
        let binding: &'static WidgetsBinding = Box::leak(Box::new(WidgetsBinding::new()));

        let observer = Arc::new(ReentrantObserver {
            binding,
            fired: std::sync::atomic::AtomicUsize::new(0),
        });
        binding.add_observer(observer.clone() as Arc<dyn WidgetsBindingObserver>);
        assert_eq!(binding.observer_count(), 1);

        // Pre-V-21: this call deadlocks (read lock held + observer wants
        // write lock). Post-V-21: returns normally.
        binding.handle_metrics_changed();

        assert_eq!(
            observer.fired.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "observer's did_change_metrics must fire exactly once"
        );
        assert_eq!(
            binding.observer_count(),
            2,
            "re-entrant add_observer inside the callback must complete"
        );
    }
}
