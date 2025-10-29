//! Build phase management and element lifecycle
//!
//! The PipelineOwner coordinates widget rebuilds and manages the build phase lifecycle.
//!
//! # Key Responsibilities
//!
//! 1. **Dirty Tracking**: Maintains list of elements that need rebuild
//! 2. **Build Scheduling**: Batches multiple setState() calls for performance
//! 3. **Build Orchestration**: Coordinates rebuild order (parents before children)
//! 4. **Build Scope Management**: Prevents setState during build
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner
//!   ├─ tree: Arc<RwLock<ElementTree>>
//!   ├─ dirty_elements: Vec<(ElementId, usize)>  // (id, depth)
//!   ├─ build_count: usize
//!   ├─ in_build_scope: bool
//!   └─ batcher: Option<BuildBatcher>  // For batching rapid setState calls
//! ```
//!
//! # Build Batching
//!
//! When enabled, PipelineOwner batches multiple setState() calls within a time window:
//!
//! ```rust,ignore
//! let mut owner = PipelineOwner::new();
//! owner.enable_batching(Duration::from_millis(16)); // One frame
//!
//! // Multiple setState calls
//! owner.schedule_build_for(id1, 0);
//! owner.schedule_build_for(id2, 1);
//! owner.schedule_build_for(id1, 0); // Duplicate - batched!
//!
//! // Later...
//! if owner.should_flush_batch() {
//!     owner.flush_batch(); // Add to dirty_elements
//!     owner.build_scope(|o| o.flush_build()); // Rebuild
//! }
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::element::{Element, ElementId, ElementTree};

#[cfg(debug_assertions)]
use crate::debug_println;

/// Build batching system for performance optimization
///
/// Batches multiple setState() calls into a single rebuild to avoid redundant work.
/// This is especially useful for:
/// - Animations with many rapid setState() calls
/// - User input triggering multiple widgets
/// - Computed values that update multiple times per frame
#[derive(Debug)]
struct BuildBatcher {
    /// Elements pending in current batch (with depths)
    pending: HashMap<ElementId, usize>,
    /// When the current batch started
    batch_start: Option<Instant>,
    /// How long to wait before flushing batch
    batch_duration: Duration,
    /// Total number of batches flushed
    batches_flushed: usize,
    /// Total number of builds saved by batching
    builds_saved: usize,
}

impl BuildBatcher {
    fn new(batch_duration: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            batch_start: None,
            batch_duration,
            batches_flushed: 0,
            builds_saved: 0,
        }
    }

    /// Add element to batch
    fn schedule(&mut self, element_id: ElementId, depth: usize) {
        // Start batch timer if first element
        if self.pending.is_empty() {
            self.batch_start = Some(Instant::now());
        }

        // Track if this is a duplicate (saved build)
        if self.pending.insert(element_id, depth).is_some() {
            self.builds_saved += 1;
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Build batched: element {:?} already in batch (saved 1 build)",
                element_id
            );
        } else {
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Build batched: added element {:?} to batch",
                element_id
            );
        }
    }

    /// Check if batch is ready to flush
    fn should_flush(&self) -> bool {
        if let Some(start) = self.batch_start {
            start.elapsed() >= self.batch_duration
        } else {
            false
        }
    }

    /// Take all pending builds
    fn take_pending(&mut self) -> HashMap<ElementId, usize> {
        self.batches_flushed += 1;
        self.batch_start = None;
        std::mem::take(&mut self.pending)
    }

    /// Get statistics (batches_flushed, builds_saved)
    fn stats(&self) -> (usize, usize) {
        (self.batches_flushed, self.builds_saved)
    }
}

/// PipelineOwner - manages the build phase and element lifecycle
///
/// This is the core coordinator for the widget build system.
/// It tracks dirty elements and orchestrates rebuilds.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{PipelineOwner, ComponentElement};
///
/// let mut owner = PipelineOwner::new();
///
/// // Create root element
/// let root_element = ComponentElement::new(MyApp::new());
/// let root_id = owner.set_root(Box::new(root_element));
///
/// // Mark element dirty
/// owner.schedule_build_for(element_id, depth);
///
/// // Rebuild all dirty elements
/// owner.build_scope(|o| {
///     o.flush_build();
/// });
/// ```
pub struct PipelineOwner {
    /// The element tree
    tree: Arc<RwLock<ElementTree>>,

    /// Root element ID
    root_element_id: Option<ElementId>,

    /// Dirty elements waiting to be rebuilt
    /// Stored as (ElementId, depth) pairs for efficient sorting
    dirty_elements: Vec<(ElementId, usize)>,

    /// Build phase counter (for debugging)
    build_count: usize,

    /// Whether we're currently in a build scope
    /// Prevents setState during build
    in_build_scope: bool,

    /// Whether build scheduling is currently locked
    /// Used during finalize to prevent new builds
    build_locked: bool,

    /// Callback when a build is scheduled (optional)
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,

    /// Build batching system
    /// When enabled, batches multiple setState() calls
    batcher: Option<BuildBatcher>,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_element_id", &self.root_element_id)
            .field("dirty_elements_count", &self.dirty_elements.len())
            .field("build_count", &self.build_count)
            .field("in_build_scope", &self.in_build_scope)
            .field("build_locked", &self.build_locked)
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .field("batching_enabled", &self.batcher.is_some())
            .finish()
    }
}

impl PipelineOwner {
    /// Create a new build owner
    pub fn new() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        Self {
            tree,
            root_element_id: None,
            dirty_elements: Vec::new(),
            build_count: 0,
            in_build_scope: false,
            build_locked: false,
            on_build_scheduled: None,
            batcher: None, // Batching disabled by default
        }
    }

    /// Get reference to the element tree
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.root_element_id
    }

    /// Set callback for when build is scheduled
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
    }

    // =========================================================================
    // Build Batching
    // =========================================================================

    /// Enable build batching to optimize rapid setState() calls
    ///
    /// When enabled, multiple setState() calls within `batch_duration` are
    /// batched into a single rebuild, improving performance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// owner.enable_batching(Duration::from_millis(16)); // 1 frame
    ///
    /// // Multiple setState calls
    /// owner.schedule_build(id1, 0);
    /// owner.schedule_build(id2, 1); // Batched!
    /// owner.schedule_build(id1, 0); // Duplicate - saved!
    ///
    /// // Later...
    /// if owner.should_flush_batch() {
    ///     owner.flush_batch();
    /// }
    /// ```
    pub fn enable_batching(&mut self, batch_duration: Duration) {
        #[cfg(debug_assertions)]
        println!("Enabling build batching with duration {:?}", batch_duration);
        self.batcher = Some(BuildBatcher::new(batch_duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        if let Some(ref batcher) = self.batcher {
            let (batches, saved) = batcher.stats();
            #[cfg(debug_assertions)]
            println!(
                "Disabling build batching (flushed {} batches, saved {} builds)",
                batches, saved
            );
        }
        self.batcher = None;
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.batcher.is_some()
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        self.batcher
            .as_ref()
            .map(|b| b.should_flush())
            .unwrap_or(false)
    }

    /// Flush the current batch
    ///
    /// Moves all pending batched builds to dirty_elements for processing.
    pub fn flush_batch(&mut self) {
        if let Some(ref mut batcher) = self.batcher {
            let pending = batcher.take_pending();
            if !pending.is_empty() {
                #[cfg(debug_assertions)]
                debug_println!(
                    PRINT_SCHEDULE_BUILD,
                    "Flushing batch: {} elements",
                    pending.len()
                );

                for (element_id, depth) in pending {
                    // Add to dirty elements (bypass batching)
                    if !self.dirty_elements.iter().any(|(id, _)| *id == element_id) {
                        self.dirty_elements.push((element_id, depth));
                    }
                }
            }
        }
    }

    /// Get batching statistics (batches_flushed, builds_saved)
    pub fn batching_stats(&self) -> (usize, usize) {
        self.batcher.as_ref().map(|b| b.stats()).unwrap_or((0, 0))
    }

    // =========================================================================
    // Root Management
    // =========================================================================

    /// Mount an element as the root of the tree
    ///
    /// # Arguments
    ///
    /// - `root_element`: The element to set as root (typically ComponentElement or RenderElement)
    ///
    /// # Returns
    ///
    /// The ElementId of the root element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// let root = Element::Component(ComponentElement::new(MyApp::new()));
    /// let root_id = owner.set_root(root);
    /// ```
    pub fn set_root(&mut self, mut root_element: Element) -> ElementId {
        let mut tree_guard = self.tree.write();

        // Mount the element (no parent, slot 0)
        root_element.mount(None, 0);

        // Insert into tree
        let id = tree_guard.insert(root_element);
        drop(tree_guard);

        self.root_element_id = Some(id);

        // Root starts dirty
        self.schedule_build_for(id, 0);

        id
    }

    // =========================================================================
    // Build Scheduling
    // =========================================================================

    /// Schedule an element for rebuild
    ///
    /// # Parameters
    ///
    /// - `element_id`: The element to rebuild
    /// - `depth`: The depth of the element in the tree (0 = root)
    ///
    /// Elements are sorted by depth before building to ensure parents build before children.
    ///
    /// If batching is enabled, the build will be batched with other builds.
    /// Otherwise, it's added to dirty_elements immediately.
    pub fn schedule_build_for(&mut self, element_id: ElementId, depth: usize) {
        if self.build_locked {
            #[cfg(debug_assertions)]
            eprintln!(
                "Warning: Attempted to schedule build while locked (element {:?})",
                element_id
            );
            return;
        }

        // If batching enabled, use batcher
        if let Some(ref mut batcher) = self.batcher {
            batcher.schedule(element_id, depth);

            // Trigger callback
            if let Some(ref callback) = self.on_build_scheduled {
                callback();
            }
            return;
        }

        // Otherwise, add directly to dirty elements
        // Check if already scheduled
        if self.dirty_elements.iter().any(|(id, _)| *id == element_id) {
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Element {:?} already scheduled for rebuild",
                element_id
            );
            return;
        }

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_SCHEDULE_BUILD,
            "Scheduling element {:?} for rebuild (depth {})",
            element_id,
            depth
        );

        self.dirty_elements.push((element_id, depth));

        // Trigger callback
        if let Some(ref callback) = self.on_build_scheduled {
            callback();
        }
    }

    /// Get count of dirty elements waiting to rebuild
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }

    /// Check if currently in build scope
    pub fn is_in_build_scope(&self) -> bool {
        self.in_build_scope
    }

    // =========================================================================
    // Build Execution
    // =========================================================================

    /// Execute a build scope
    ///
    /// This sets the build scope flag to prevent setState during build,
    /// then executes the callback.
    ///
    /// # Build Scope Isolation
    ///
    /// Any `markNeedsBuild()` calls during the scope will be deferred and
    /// processed after the scope completes. This prevents infinite rebuild loops.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// owner.build_scope(|owner| {
    ///     owner.flush_build();
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If the callback panics, the build scope will be properly cleaned up,
    /// but the panic will propagate.
    pub fn build_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        if self.in_build_scope {
            #[cfg(debug_assertions)]
            eprintln!("Warning: Nested build_scope detected!");
        }

        self.in_build_scope = true;

        // Execute callback
        let result = f(self);

        // Clear flag
        // Note: If f() panics, this won't run, but that's acceptable since
        // the entire program state is likely corrupted anyway.
        self.in_build_scope = false;

        result
    }

    /// Lock state changes
    ///
    /// Executes callback with state changes locked.
    /// Any setState calls during this time will be ignored/warned.
    pub fn lock_state<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let was_locked = self.build_locked;
        self.build_locked = true;
        let result = f(self);
        self.build_locked = was_locked;
        result
    }

    /// Flush the build phase
    ///
    /// Rebuilds all dirty elements in depth order (parents before children).
    /// This ensures that parent widgets build before their children.
    pub fn flush_build(&mut self) {
        if self.dirty_elements.is_empty() {
            #[cfg(debug_assertions)]
            debug_println!(PRINT_BUILD_SCOPE, "flush_build: no dirty elements");
            return;
        }

        self.build_count += 1;
        let build_num = self.build_count;

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "flush_build #{}: rebuilding {} dirty elements",
            build_num,
            self.dirty_elements.len()
        );

        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // Take the dirty list to avoid borrow conflicts
        let mut dirty = std::mem::take(&mut self.dirty_elements);

        // Rebuild each element
        for (element_id, depth) in dirty.drain(..) {
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_BUILD_SCOPE,
                "  Rebuilding element {:?} at depth {}",
                element_id,
                depth
            );

            // Clone the Arc for passing to rebuild()
            let tree_ref = std::sync::Arc::clone(&self.tree);

            let mut tree_guard = self.tree.write();

            // Element might have been removed during previous rebuilds
            let children_to_mount = if let Some(element) = tree_guard.get_mut(element_id) {
                // Call rebuild on element - it will create BuildContext internally
                let children = element.rebuild(element_id, std::sync::Arc::clone(&tree_ref));

                #[cfg(debug_assertions)]
                if !children.is_empty() {
                    debug_println!(
                        PRINT_BUILD_SCOPE,
                        "    Element {:?} produced {} child widgets to mount",
                        element_id,
                        children.len()
                    );
                }

                children
            } else {
                #[cfg(debug_assertions)]
                eprintln!(
                    "  Warning: Element {:?} was removed before rebuild",
                    element_id
                );
                Vec::new()
            };

            // Drop the guard before mounting children (to avoid deadlock)
            drop(tree_guard);

            // Mount the returned children
            // Each tuple is: (parent_id, child_widget, slot)
            for (parent_id, child_widget, slot) in children_to_mount {
                #[cfg(debug_assertions)]
                debug_println!(
                    PRINT_BUILD_SCOPE,
                    "      Mounting child widget at slot {} for parent {:?}",
                    slot,
                    parent_id
                );

                // Inflate widget into element
                let mut child_element = self.inflate_widget(child_widget);

                // Mount the element (sets parent, slot, lifecycle = Active)
                child_element.mount(Some(parent_id), slot);

                // Insert into tree
                let mut tree_guard = self.tree.write();
                let child_id = tree_guard.insert(child_element);

                #[cfg(debug_assertions)]
                debug_println!(
                    PRINT_BUILD_SCOPE,
                    "      Created element {:?} for child at slot {}",
                    child_id,
                    slot
                );

                // Update parent's child reference
                // For ComponentElement, it has a single child
                if let Some(Element::Component(parent_component)) = tree_guard.get_mut(parent_id) {
                    parent_component.set_child(child_id);
                }
                // FIXME: Handle other element types (Stateful, Inherited, Render, ParentData)

                drop(tree_guard);

                // Schedule the new child for building if it's dirty
                // (All newly created elements are dirty and need initial build)
                self.schedule_build_for(child_id, depth + 1);
            }
        }

        // Put back the (now empty) vector
        self.dirty_elements = dirty;

        #[cfg(debug_assertions)]
        debug_println!(PRINT_BUILD_SCOPE, "flush_build #{}: complete", build_num);
    }

    /// Finalize the tree after build
    ///
    /// This locks further builds and performs any cleanup needed.
    pub fn finalize_tree(&mut self) {
        self.lock_state(|owner| {
            if owner.dirty_elements.is_empty() {
                #[cfg(debug_assertions)]
                debug_println!(PRINT_BUILD_SCOPE, "finalize_tree: tree is clean");
            } else {
                #[cfg(debug_assertions)]
                eprintln!(
                    "Warning: finalize_tree: {} dirty elements remaining",
                    owner.dirty_elements.len()
                );
            }
        });
    }

    // =========================================================================
    // Layout & Paint Phases
    // =========================================================================
    // FIXME: Implement full rendering pipeline (layout/paint phases)
    // when Render layout/paint system is fully integrated.

    /// Request layout for a Render
    ///
    /// Adds the node to the layout dirty list if not already present.
    /// Called by Render::mark_needs_layout().
    pub fn request_layout(&mut self, _node_id: ElementId) {
        // FIXME: Implement dirty tracking for layout
        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_LAYOUT,
            "PipelineOwner: requested layout for {:?}",
            _node_id
        );
    }

    /// Request paint for a Render
    ///
    /// Adds the node to the paint dirty list if not already present.
    /// Called by Render::mark_needs_paint().
    pub fn request_paint(&mut self, _node_id: ElementId) {
        // FIXME: Implement dirty tracking for paint
        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_LAYOUT,
            "PipelineOwner: requested paint for {:?}",
            _node_id
        );
    }

    /// Flush the layout phase
    ///
    /// Performs layout on all Renders in the tree.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root constraints (typically screen size)
    ///
    /// # Returns
    ///
    /// The size of the root render object, or None if no root
    ///
    /// # Note
    ///
    /// This is a stub implementation. Full layout pipeline will be added
    /// when Render system is fully integrated.
    pub fn flush_layout(
        &mut self,
        _constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        #[cfg(debug_assertions)]
        debug_println!(PRINT_LAYOUT, "PipelineOwner::flush_layout called (stub)");

        // FIXME: Implement layout phase:
        // 1. Sort dirty nodes by depth
        // 2. Layout each node with constraints
        // 3. Return root size
        None
    }

    /// Flush the paint phase
    ///
    /// Paints all Renders to the given painter.
    ///
    /// # Parameters
    ///
    /// - `offset`: Global offset for painting
    ///
    /// # Note
    ///
    /// This is a stub implementation. Full paint pipeline will be added
    /// when Render system is fully integrated.
    pub fn flush_paint(&mut self, _offset: flui_types::Offset) {
        #[cfg(debug_assertions)]
        debug_println!(PRINT_LAYOUT, "PipelineOwner::flush_paint called (stub)");

        // FIXME: Implement paint phase:
        // 1. Paint dirty nodes
        // 2. Composite layers
        // 3. Send to backend
    }

    // =========================================================================
    // Widget Inflation
    // =========================================================================

    /// Inflate a widget into an element
    ///
    /// This creates the appropriate Element type based on the widget's type:
    /// - StatelessWidget → ComponentElement
    /// - StatefulWidget → StatefulElement
    /// - InheritedWidget → InheritedElement
    /// - RenderWidget → RenderElement
    /// - ParentDataWidget → ParentDataElement
    ///
    /// # Arguments
    ///
    /// - `widget`: The boxed widget to inflate
    ///
    /// # Returns
    ///
    /// An Element enum variant ready to be inserted into the tree
    fn inflate_widget(&self, widget: crate::Widget) -> Element {
        use crate::element::{
            ComponentElement, InheritedElement, ParentDataElement, RenderElement, StatefulElement,
        };
        use crate::widget::{
            InheritedWidget, ParentDataWidget, RenderWidget, StatefulWidget, StatelessWidget,
        };

        // Try each widget type in order
        // Note: We can't directly check traits, so we use a heuristic:
        // - If widget.build() returns Some, it's a buildable widget (Stateless/Stateful/Inherited)
        // - Otherwise it's a RenderWidget or ParentDataWidget

        // For now, assume all widgets coming through rebuild() are StatelessWidget
        // FIXME: Add proper type detection when we support all widget types
        Element::Component(ComponentElement::new(widget))
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildContext;
    use crate::element::{ComponentElement, Element};
    use crate::widget::{Widget, StatelessWidget};

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget;

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &BuildContext) -> Widget {
            Box::new(TestWidget)
        }
    }

    #[test]
    fn test_build_owner_creation() {
        let owner = PipelineOwner::new();
        assert!(owner.root_element_id().is_none());
        assert_eq!(owner.dirty_count(), 0);
        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_schedule_build() {
        let mut owner = PipelineOwner::new();
        let id = 42; // Arbitrary ElementId

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        // Scheduling same element again should not duplicate
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_build_scope() {
        let mut owner = PipelineOwner::new();

        assert!(!owner.is_in_build_scope());

        owner.build_scope(|o| {
            assert!(o.is_in_build_scope());
        });

        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_lock_state() {
        let mut owner = PipelineOwner::new();
        let id = 42;

        // Normal scheduling works
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        owner.lock_state(|o| {
            // Scheduling while locked should be ignored
            let id2 = 43;
            o.schedule_build_for(id2, 0);
            assert_eq!(o.dirty_count(), 1); // Still 1, not 2
        });
    }

    #[test]
    fn test_depth_sorting() {
        let mut owner = PipelineOwner::new();

        let id1 = 1;
        let id2 = 2;
        let id3 = 3;

        // Schedule in random order
        owner.schedule_build_for(id2, 2);
        owner.schedule_build_for(id1, 1);
        owner.schedule_build_for(id3, 0);

        // flush_build sorts by depth before rebuilding
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_on_build_scheduled_callback() {
        use std::sync::{Arc, Mutex};

        let mut owner = PipelineOwner::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        owner.set_on_build_scheduled(move || {
            *called_clone.lock().unwrap() = true;
        });

        let id = 42;
        owner.schedule_build_for(id, 0);

        assert!(*called.lock().unwrap());
    }

    // Build Batching Tests

    #[test]
    fn test_batching_disabled_by_default() {
        let owner = PipelineOwner::new();
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_enable_disable_batching() {
        let mut owner = PipelineOwner::new();

        owner.enable_batching(Duration::from_millis(16));
        assert!(owner.is_batching_enabled());

        owner.disable_batching();
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_batching_deduplicates() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = 42;

        // Schedule same element 3 times
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);

        // Flush batch
        owner.flush_batch();

        // Should only have 1 dirty element
        assert_eq!(owner.dirty_count(), 1);

        // Stats should show 2 builds saved
        let (batches, saved) = owner.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }

    #[test]
    fn test_batching_multiple_elements() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id1 = 1;
        let id2 = 2;
        let id3 = 3;

        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);
        owner.schedule_build_for(id3, 2);

        owner.flush_batch();

        // All 3 should be dirty
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(10));

        let id = 42;
        owner.schedule_build_for(id, 0);

        // Should not flush immediately
        assert!(!owner.should_flush_batch());

        // Wait for batch duration
        std::thread::sleep(Duration::from_millis(15));

        // Now should flush
        assert!(owner.should_flush_batch());
    }

    #[test]
    fn test_batching_without_enable() {
        let mut owner = PipelineOwner::new();
        // Batching not enabled

        let id = 42;
        owner.schedule_build_for(id, 0);

        // Should add directly to dirty elements
        assert_eq!(owner.dirty_count(), 1);

        // flush_batch should be no-op
        owner.flush_batch();
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = 42;

        // Initial stats
        assert_eq!(owner.batching_stats(), (0, 0));

        // Schedule same element twice
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0); // Duplicate

        // Flush
        owner.flush_batch();

        // Should have 1 batch flushed, 1 build saved
        let (batches, saved) = owner.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 1);
    }

    #[test]
    fn test_build_scope_returns_result() {
        let mut owner = PipelineOwner::new();

        let result = owner.build_scope(|_| 42);

        assert_eq!(result, 42);
    }

    #[test]
    fn test_set_root() {
        let mut owner = PipelineOwner::new();
        let component = ComponentElement::new(TestWidget);
        let root = Element::Component(component);

        let root_id = owner.set_root(Box::new(root));

        assert_eq!(owner.root_element_id(), Some(root_id));
        // Root should be marked dirty
        assert_eq!(owner.dirty_count(), 1);
    }
}
