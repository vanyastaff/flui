//! WidgetsBinding - The glue between the widgets layer and the engine.
//!
//! This module provides the binding that coordinates:
//! - BuildOwner for managing element rebuilds
//! - ElementTree for storing elements
//! - Root element attachment
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `WidgetsBinding` mixin in
//! `widgets/binding.dart`.
//!
//! # Architecture
//!
//! ```text
//! WidgetsBinding
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
//! let mut binding = WidgetsBinding::new();
//! binding.attach_root_view(MyApp);
//! binding.draw_frame(); // builds dirty elements
//! ```

use crate::owner::BuildOwner;
use crate::tree::ElementTree;
use crate::view::View;
use flui_foundation::ElementId;
use parking_lot::RwLock;
use std::sync::Arc;

/// Observer for widgets binding lifecycle events.
///
/// Implement this trait to receive notifications about:
/// - Locale changes
/// - Metrics changes (window resize)
/// - App lifecycle changes
/// - Memory pressure
pub trait WidgetsBindingObserver: Send + Sync {
    /// Called when the system locale changes.
    fn did_change_locales(&self) {}

    /// Called when window metrics change (size, DPI, etc).
    fn did_change_metrics(&self) {}

    /// Called when text scale factor changes.
    fn did_change_text_scale_factor(&self) {}

    /// Called when platform brightness changes (light/dark mode).
    fn did_change_platform_brightness(&self) {}

    /// Called when app lifecycle state changes.
    fn did_change_app_lifecycle_state(&self, _state: AppLifecycleState) {}

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

/// The glue between the widgets layer and the engine.
///
/// WidgetsBinding manages:
/// - A single ElementTree rooted at `root_element`
/// - A BuildOwner that tracks dirty elements
/// - Lifecycle observers
///
/// # Thread Safety
///
/// WidgetsBinding is designed to be used from a single thread.
/// For multi-threaded access, wrap in `Arc<RwLock<WidgetsBinding>>`.
pub struct WidgetsBinding {
    /// The build owner manages dirty elements and rebuild scheduling.
    build_owner: BuildOwner,

    /// The element tree stores all elements.
    element_tree: ElementTree,

    /// The root element ID (set after attachRootWidget).
    root_element: Option<ElementId>,

    /// Lifecycle observers.
    observers: Vec<Arc<dyn WidgetsBindingObserver>>,

    /// Whether a build has been scheduled.
    build_scheduled: bool,

    /// Callback when a frame is needed.
    #[allow(clippy::type_complexity)]
    on_need_frame: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Default for WidgetsBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl WidgetsBinding {
    /// Create a new WidgetsBinding.
    pub fn new() -> Self {
        Self {
            build_owner: BuildOwner::new(),
            element_tree: ElementTree::new(),
            root_element: None,
            observers: Vec::new(),
            build_scheduled: false,
            on_need_frame: None,
        }
    }

    /// Set the callback for when a frame is needed.
    pub fn set_on_need_frame<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_need_frame = Some(Box::new(callback));
    }

    // ========================================================================
    // Build Owner Access
    // ========================================================================

    /// Get a reference to the build owner.
    pub fn build_owner(&self) -> &BuildOwner {
        &self.build_owner
    }

    /// Get a mutable reference to the build owner.
    pub fn build_owner_mut(&mut self) -> &mut BuildOwner {
        &mut self.build_owner
    }

    // ========================================================================
    // Element Tree Access
    // ========================================================================

    /// Get a reference to the element tree.
    pub fn element_tree(&self) -> &ElementTree {
        &self.element_tree
    }

    /// Get a mutable reference to the element tree.
    pub fn element_tree_mut(&mut self) -> &mut ElementTree {
        &mut self.element_tree
    }

    /// Get the root element ID.
    pub fn root_element(&self) -> Option<ElementId> {
        self.root_element
    }

    // ========================================================================
    // Root Widget Attachment
    // ========================================================================

    /// Attach a root widget to the binding.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached.
    pub fn attach_root_widget<V: View>(&mut self, view: &V) {
        assert!(
            self.root_element.is_none(),
            "Root widget already attached. Call detach_root_widget first."
        );

        // Mount root element
        let root_id = self.element_tree.mount_root(view);
        self.root_element = Some(root_id);

        // Schedule initial build
        self.build_owner.schedule_build_for(root_id, 0);
        self.schedule_build();

        tracing::debug!(?root_id, "Root widget attached");
    }

    /// Detach the root widget.
    ///
    /// This clears the element tree.
    pub fn detach_root_widget(&mut self) {
        if let Some(root_id) = self.root_element.take() {
            // Remove root element (this clears the tree since it's the root)
            let _ = self.element_tree.remove(root_id);
            tracing::debug!(?root_id, "Root widget detached");
        }
    }

    // ========================================================================
    // Build Scheduling
    // ========================================================================

    /// Schedule a build if not already scheduled.
    fn schedule_build(&mut self) {
        if !self.build_scheduled {
            self.build_scheduled = true;
            self.handle_build_scheduled();
        }
    }

    /// Called when a build has been scheduled.
    fn handle_build_scheduled(&self) {
        // Request a frame from the scheduler
        if let Some(ref callback) = self.on_need_frame {
            callback();
        }
    }

    /// Check if there are pending builds.
    pub fn has_pending_builds(&self) -> bool {
        self.build_owner.has_dirty_elements()
    }

    // ========================================================================
    // Frame Drawing
    // ========================================================================

    /// Build all dirty elements.
    ///
    /// This is called once per frame to rebuild dirty elements.
    /// After building, layout and paint should be performed by
    /// the rendering layer.
    pub fn draw_frame(&mut self) {
        self.build_scheduled = false;

        if !self.build_owner.has_dirty_elements() {
            return;
        }

        tracing::debug!(
            dirty_count = self.build_owner.dirty_count(),
            "Building dirty elements"
        );

        // Process all dirty elements
        self.build_owner.build_scope(&mut self.element_tree);

        tracing::debug!("Build phase complete");
    }

    // ========================================================================
    // Observers
    // ========================================================================

    /// Add a lifecycle observer.
    pub fn add_observer(&mut self, observer: Arc<dyn WidgetsBindingObserver>) {
        self.observers.push(observer);
    }

    /// Remove a lifecycle observer.
    pub fn remove_observer(&mut self, observer: &Arc<dyn WidgetsBindingObserver>) {
        self.observers.retain(|o| !Arc::ptr_eq(o, observer));
    }

    /// Notify all observers of locale change.
    pub fn handle_locale_changed(&self) {
        for observer in &self.observers {
            observer.did_change_locales();
        }
    }

    /// Notify all observers of metrics change.
    pub fn handle_metrics_changed(&self) {
        for observer in &self.observers {
            observer.did_change_metrics();
        }
    }

    /// Notify all observers of text scale factor change.
    pub fn handle_text_scale_factor_changed(&self) {
        for observer in &self.observers {
            observer.did_change_text_scale_factor();
        }
    }

    /// Notify all observers of platform brightness change.
    pub fn handle_platform_brightness_changed(&self) {
        for observer in &self.observers {
            observer.did_change_platform_brightness();
        }
    }

    /// Notify all observers of app lifecycle change.
    pub fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState) {
        for observer in &self.observers {
            observer.did_change_app_lifecycle_state(state);
        }
    }

    /// Notify all observers of memory pressure.
    pub fn handle_memory_pressure(&self) {
        for observer in &self.observers {
            observer.did_have_memory_pressure();
        }
    }

    /// Notify all observers of accessibility features change.
    pub fn handle_accessibility_features_changed(&self) {
        for observer in &self.observers {
            observer.did_change_accessibility_features();
        }
    }
}

impl std::fmt::Debug for WidgetsBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetsBinding")
            .field("root_element", &self.root_element)
            .field("build_scheduled", &self.build_scheduled)
            .field("dirty_count", &self.build_owner.dirty_count())
            .field("element_count", &self.element_tree.len())
            .field("observer_count", &self.observers.len())
            .finish()
    }
}

/// Thread-safe wrapper for WidgetsBinding.
///
/// Use this when you need to share the binding across threads.
pub type SharedWidgetsBinding = Arc<RwLock<WidgetsBinding>>;

/// Create a new shared widgets binding.
pub fn create_shared_binding() -> SharedWidgetsBinding {
    Arc::new(RwLock::new(WidgetsBinding::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildContext, StatelessView};

    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(TestView)
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(crate::StatelessElement::new(self))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_binding_creation() {
        let binding = WidgetsBinding::new();
        assert!(binding.root_element().is_none());
        assert!(!binding.has_pending_builds());
    }

    #[test]
    fn test_attach_root_widget() {
        let mut binding = WidgetsBinding::new();
        let view = TestView;

        binding.attach_root_widget(&view);

        assert!(binding.root_element().is_some());
        assert!(binding.has_pending_builds());
    }

    #[test]
    fn test_draw_frame() {
        let mut binding = WidgetsBinding::new();
        let view = TestView;

        binding.attach_root_widget(&view);
        assert!(binding.has_pending_builds());

        binding.draw_frame();
        assert!(!binding.has_pending_builds());
    }

    #[test]
    fn test_detach_root_widget() {
        let mut binding = WidgetsBinding::new();
        let view = TestView;

        binding.attach_root_widget(&view);
        assert!(binding.root_element().is_some());

        binding.detach_root_widget();
        assert!(binding.root_element().is_none());
    }

    #[test]
    #[should_panic(expected = "Root widget already attached")]
    fn test_double_attach_panics() {
        let mut binding = WidgetsBinding::new();
        let view = TestView;

        binding.attach_root_widget(&view);
        binding.attach_root_widget(&view); // Should panic
    }

    #[test]
    fn test_shared_binding() {
        let binding = create_shared_binding();

        {
            let mut b = binding.write();
            let view = TestView;
            b.attach_root_widget(&view);
        }

        {
            let b = binding.read();
            assert!(b.root_element().is_some());
        }
    }
}
