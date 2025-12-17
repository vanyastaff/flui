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

use crate::owner::BuildOwner;
use crate::tree::ElementTree;
use crate::view::View;
use flui_foundation::{impl_binding_singleton, BindingBase, ElementId};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

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
    // ========================================================================

    /// Called at the start of a predictive back gesture.
    ///
    /// Return `true` to handle the gesture (start animation), `false` otherwise.
    /// If `true`, subsequent gesture events will be sent to this observer.
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

    /// Called when a request is received from the system to exit the application.
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

impl WidgetsBinding {
    /// Create a new WidgetsBinding.
    ///
    /// Note: Prefer using `WidgetsBinding::instance()` for singleton access.
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
        binding
    }

    /// Set the PipelineOwner for render tree management.
    ///
    /// This should be called by the application binding (e.g., WidgetsFlutterBinding)
    /// before attaching the root widget. The PipelineOwner will be propagated
    /// to elements during mounting so they can create their RenderObjects.
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

    /// Attach a root widget to the binding.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// The PipelineOwner (if set via `set_pipeline_owner`) will be passed
    /// to the root element during mounting, enabling RenderObjectElements
    /// to create their RenderObjects.
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached.
    pub fn attach_root_widget<V: View>(&self, view: &V) {
        let mut inner = self.inner.write();

        assert!(
            inner.root_element.is_none(),
            "Root widget already attached. Call detach_root_widget first."
        );

        // Mount root element with PipelineOwner
        // This ensures RenderObjectElements can create their RenderObjects
        let pipeline_owner = inner.pipeline_owner.clone();
        let root_id = inner
            .element_tree
            .mount_root_with_pipeline_owner(view, pipeline_owner);
        inner.root_element = Some(root_id);

        // Schedule initial build
        inner.build_owner.schedule_build_for(root_id, 0);
        inner.build_scheduled = true;

        tracing::debug!(?root_id, "Root widget attached");

        // Request a frame
        drop(inner); // Release lock before calling callback
        self.handle_build_scheduled();
    }

    /// Detach the root widget.
    ///
    /// This clears the element tree.
    pub fn detach_root_widget(&self) {
        let mut inner = self.inner.write();

        if let Some(root_id) = inner.root_element.take() {
            // Remove root element (this clears the tree since it's the root)
            let _ = inner.element_tree.remove(root_id);
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
    /// This is useful for animation demos where the entire tree needs to rebuild
    /// each frame to reflect updated animation values.
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

    /// Collect all element IDs in the tree recursively.
    fn collect_all_elements(
        tree: &ElementTree,
        id: ElementId,
        depth: usize,
    ) -> Vec<(ElementId, usize)> {
        let mut result = vec![(id, depth)];

        // Collect children
        if let Some(node) = tree.get(id) {
            node.element().visit_children(&mut |child_id| {
                result.extend(Self::collect_all_elements(tree, child_id, depth + 1));
            });
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
            if inner.debug_building_dirty_elements {
                panic!(
                    "Build scheduled during frame.\n\
                     While the widget tree was being built, laid out, and painted, \
                     a new frame was scheduled to rebuild the widget tree.\n\
                     This might be because setState() was called from a layout or \
                     paint callback."
                );
            }
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
    /// This method is called by `handleDrawFrame`, which is called automatically
    /// by the engine when it is time to lay out and paint a frame.
    ///
    /// # Frame phases
    ///
    /// 1. **Build phase**: All dirty [Element]s in the widget tree are rebuilt.
    ///    See [State.setState] for details on marking a widget dirty.
    ///
    /// 2. **Layout phase**: (handled by RendererBinding.drawFrame)
    ///
    /// 3. **Paint phase**: (handled by RendererBinding.drawFrame)
    ///
    /// 4. **Finalization phase**: Inactive elements are unmounted.
    ///    This causes [State.dispose] to be invoked on removed widgets.
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
    pub fn handle_locale_changed(&self) {
        let inner = self.inner.read();
        for observer in &inner.observers {
            observer.did_change_locales();
        }
    }

    /// Notify all observers of metrics change.
    pub fn handle_metrics_changed(&self) {
        let inner = self.inner.read();
        for observer in &inner.observers {
            observer.did_change_metrics();
        }
    }

    /// Notify all observers of text scale factor change.
    pub fn handle_text_scale_factor_changed(&self) {
        let inner = self.inner.read();
        for observer in &inner.observers {
            observer.did_change_text_scale_factor();
        }
    }

    /// Notify all observers of platform brightness change.
    pub fn handle_platform_brightness_changed(&self) {
        let inner = self.inner.read();
        for observer in &inner.observers {
            observer.did_change_platform_brightness();
        }
    }

    /// Notify all observers of app lifecycle change.
    pub fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState) {
        let inner = self.inner.read();
        for observer in &inner.observers {
            observer.did_change_app_lifecycle_state(state);
        }
    }

    /// Notify all observers of memory pressure.
    pub fn handle_memory_pressure(&self) {
        let inner = self.inner.read();
        for observer in &inner.observers {
            observer.did_have_memory_pressure();
        }
    }

    /// Notify all observers of accessibility features change.
    pub fn handle_accessibility_features_changed(&self) {
        let inner = self.inner.read();
        for observer in &inner.observers {
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
    /// Notifies observers until one returns `true`, meaning it handled the request.
    /// If none return `true`, the application may quit.
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

        if !handling_observers.is_empty() {
            self.inner.write().back_gesture_observers = handling_observers;
            true
        } else {
            false
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
    /// If no observer was handling the gesture, falls back to `handle_pop_route`.
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
    /// # Flutter Equivalent
    ///
    /// Corresponds to `WidgetsBinding.handleViewFocusChanged()`.
    pub fn handle_view_focus_changed(&self, event: ViewFocusEvent) {
        let inner = self.inner.read();
        for observer in &inner.observers {
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

/// Thread-safe wrapper for WidgetsBinding.
///
/// Deprecated: Use `WidgetsBinding::instance()` instead.
#[deprecated(since = "0.2.0", note = "Use WidgetsBinding::instance() instead")]
pub type SharedWidgetsBinding = Arc<RwLock<WidgetsBinding>>;

/// Create a new shared widgets binding.
///
/// Deprecated: Use `WidgetsBinding::instance()` instead.
#[deprecated(since = "0.2.0", note = "Use WidgetsBinding::instance() instead")]
pub fn create_shared_binding() -> Arc<RwLock<WidgetsBinding>> {
    Arc::new(RwLock::new(WidgetsBinding::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::HasInstance;
    use std::any::TypeId;

    /// A leaf element that doesn't create children (prevents infinite recursion)
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

        fn mount(&mut self, _parent: Option<flui_foundation::ElementId>, slot: usize) {
            self.depth = slot;
            self.lifecycle = crate::Lifecycle::Active;
        }

        fn unmount(&mut self) {
            self.lifecycle = crate::Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = crate::Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = crate::Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View) {}

        fn mark_needs_build(&mut self) {}

        fn perform_build(&mut self) {
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

        binding.attach_root_widget(&view);

        assert!(binding.root_element().is_some());
        assert!(binding.has_pending_builds());
    }

    #[test]
    fn test_draw_frame() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding.attach_root_widget(&view);
        assert!(binding.has_pending_builds());

        binding.draw_frame();
        assert!(!binding.has_pending_builds());
    }

    #[test]
    fn test_detach_root_widget() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding.attach_root_widget(&view);
        assert!(binding.root_element().is_some());

        binding.detach_root_widget();
        assert!(binding.root_element().is_none());
    }

    #[test]
    #[should_panic(expected = "Root widget already attached")]
    fn test_double_attach_panics() {
        let binding = WidgetsBinding::new();
        let view = LeafView;

        binding.attach_root_widget(&view);
        binding.attach_root_widget(&view); // Should panic
    }
}
