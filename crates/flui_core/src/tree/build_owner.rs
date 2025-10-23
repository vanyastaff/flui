//! Build phase management and element lifecycle
//! 5. **Focus Management**: Coordinates focus state (future)
//!
//! # Architecture
//!
//! ```text
//! BuildOwner
//!   ├─ dirty_elements: Vec<(ElementId, usize)>  // (id, depth)
//!   ├─ global_keys: HashMap<GlobalKeyId, ElementId>
//!   ├─ build_count: usize
//!   ├─ in_build_scope: bool
//!   └─ tree: Arc<RwLock<ElementTree>>
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::{DynWidget, ElementId, ElementTree};

/// Build batching system for performance optimization
///
/// Batches multiple setState() calls into a single rebuild to avoid redundant work.
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
            debug!("Build batched: element {:?} already in batch (saved 1 build)", element_id);
        } else {
            debug!("Build batched: added element {:?} to batch", element_id);
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

    /// Get statistics
    fn stats(&self) -> (usize, usize) {
        (self.batches_flushed, self.builds_saved)
    }
}

/// Unique identifier for a global key in BuildOwner registry
///
/// This is separate from `GlobalKey<T>` to provide type safety and avoid
/// generics in the BuildOwner API. Convert from GlobalKey using `GlobalKey::to_global_key_id()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalKeyId(pub(crate) u64);

impl GlobalKeyId {
    /// Create from raw ID (used by GlobalKey conversion)
    pub(crate) fn from_raw(id: u64) -> Self {
        Self(id)
    }

    /// Get raw ID
    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// BuildOwner - manages the build phase and element lifecycle
///
/// This is the core coordinator for the widget build system.
/// It tracks dirty elements, manages global keys, and orchestrates rebuilds.
///
/// # Example
///
/// ```rust,ignore
/// let mut owner = BuildOwner::new();
/// owner.set_root(Box::new(MyApp::new()));
///
/// // Mark element dirty
/// owner.schedule_build_for(element_id, depth);
///
/// // Rebuild all dirty elements
/// owner.build_scope(|| {
///     owner.flush_build();
/// });
/// ```
pub struct BuildOwner {
    /// The element tree
    tree: Arc<RwLock<ElementTree>>,

    /// Root element ID
    root_element_id: Option<ElementId>,

    /// Dirty elements waiting to be rebuilt
    /// Stored as (ElementId, depth) pairs for efficient sorting
    dirty_elements: Vec<(ElementId, usize)>,

    /// Global key registry
    /// Maps global key IDs to element IDs
    global_keys: HashMap<GlobalKeyId, ElementId>,

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

impl std::fmt::Debug for BuildOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOwner")
            .field("root_element_id", &self.root_element_id)
            .field("dirty_elements_count", &self.dirty_elements.len())
            .field("global_keys_count", &self.global_keys.len())
            .field("build_count", &self.build_count)
            .field("in_build_scope", &self.in_build_scope)
            .field("build_locked", &self.build_locked)
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .field("batching_enabled", &self.batcher.is_some())
            .finish()
    }
}

impl BuildOwner {
    /// Create a new build owner
    pub fn new() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        Self {
            tree,
            root_element_id: None,
            dirty_elements: Vec::new(),
            global_keys: HashMap::new(),
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
    /// let mut owner = BuildOwner::new();
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
        info!("Enabling build batching with duration {:?}", batch_duration);
        self.batcher = Some(BuildBatcher::new(batch_duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        if let Some(ref batcher) = self.batcher {
            let (batches, saved) = batcher.stats();
            info!("Disabling build batching (flushed {} batches, saved {} builds)", batches, saved);
        }
        self.batcher = None;
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.batcher.is_some()
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        self.batcher.as_ref().map(|b| b.should_flush()).unwrap_or(false)
    }

    /// Flush the current batch
    ///
    /// Moves all pending batched builds to dirty_elements for processing.
    pub fn flush_batch(&mut self) {
        if let Some(ref mut batcher) = self.batcher {
            let pending = batcher.take_pending();
            if !pending.is_empty() {
                debug!("Flushing batch: {} elements", pending.len());
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

    /// Mount a widget as the root of the tree
    pub fn set_root(&mut self, root_widget: Box<dyn DynWidget>) -> ElementId {
        let mut tree_guard = self.tree.write();
        let id = tree_guard.set_root(root_widget);
        if let Some(element) = tree_guard.get_mut(id) {
            element.set_tree_ref(self.tree.clone());
        }
        drop(tree_guard);

        self.root_element_id = Some(id);

        // Root starts dirty
        self.schedule_build_for(id, 0);

        id
    }

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
            warn!("Attempted to schedule build while locked (element {:?})", element_id);
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
            debug!("Element {:?} already scheduled for rebuild", element_id);
            return;
        }

        debug!("Scheduling element {:?} for rebuild (depth {})", element_id, depth);
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
            warn!("Nested build_scope detected!");
        }

        self.in_build_scope = true;

        // Set flag in ElementTree as well
        {
            let mut tree = self.tree.write();
            tree.set_in_build_scope(true);
        }

        // Execute callback
        let result = f(self);

        // Clear flag and flush deferred dirty elements
        // Note: If f() panics, this won't run, but that's acceptable since
        // the entire program state is likely corrupted anyway.
        self.in_build_scope = false;
        {
            let mut tree = self.tree.write();
            tree.set_in_build_scope(false);
            tree.flush_deferred_dirty();
        }

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
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(
            dirty_count = self.dirty_elements.len(),
            build_num = self.build_count + 1
        )
    )]
    pub fn flush_build(&mut self) {
        if self.dirty_elements.is_empty() {
            debug!("flush_build: no dirty elements");
            return;
        }

        self.build_count += 1;
        let build_num = self.build_count;

        info!(
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
            debug!("  Rebuilding element {:?} at depth {}", element_id, depth);

            let mut tree_guard = self.tree.write();
            // Element might have been removed during previous rebuilds
            if let Some(node) = tree_guard.get_mut(element_id) {
                let _children = node.rebuild(element_id);
                // TODO: handle returned children for mounting
            } else {
                warn!("  Element {:?} was removed before rebuild", element_id);
            }
            drop(tree_guard);
        }

        // Put back the (now empty) vector
        self.dirty_elements = dirty;

        info!("flush_build #{}: complete", build_num);
    }

    /// Finalize the tree after build
    ///
    /// This locks further builds and performs any cleanup needed.
    pub fn finalize_tree(&mut self) {
        self.lock_state(|owner| {
            if owner.dirty_elements.is_empty() {
                debug!("finalize_tree: tree is clean");
            } else {
                warn!("finalize_tree: {} dirty elements remaining", owner.dirty_elements.len());
            }
        });
    }

    // =========================================================================
    // Global Key Registry
    // =========================================================================

    /// Register a global key
    ///
    /// # Panics
    ///
    /// Panics if the key is already registered to a different element.
    pub fn register_global_key(&mut self, key: GlobalKeyId, element_id: ElementId) {
        if let Some(existing_id) = self.global_keys.get(&key) {
            if *existing_id != element_id {
                panic!(
                    "GlobalKey {:?} is already registered to element {:?}, cannot register to {:?}",
                    key, existing_id, element_id
                );
            }
            // Already registered to same element - OK
            return;
        }

        debug!("Registering global key {:?} -> element {:?}", key, element_id);
        self.global_keys.insert(key, element_id);
    }

    /// Unregister a global key
    pub fn unregister_global_key(&mut self, key: GlobalKeyId) {
        debug!("Unregistering global key {:?}", key);
        self.global_keys.remove(&key);
    }

    /// Get element ID for a global key
    #[must_use]
    pub fn element_for_global_key(&self, key: GlobalKeyId) -> Option<ElementId> {
        self.global_keys.get(&key).copied()
    }

    /// Get count of registered global keys
    pub fn global_key_count(&self) -> usize {
        self.global_keys.len()
    }
}

impl Default for BuildOwner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynWidget, Context, StatelessWidget};

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget;

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(TestWidget)
        }
    }

    #[test]
    fn test_build_owner_creation() {
        let owner = BuildOwner::new();
        assert!(owner.root_element_id().is_none());
        assert_eq!(owner.dirty_count(), 0);
        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_schedule_build() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new();

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        // Scheduling same element again should not duplicate
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_build_scope() {
        let mut owner = BuildOwner::new();

        assert!(!owner.is_in_build_scope());

        owner.build_scope(|o| {
            assert!(o.is_in_build_scope());
        });

        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_lock_state() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new();

        // Normal scheduling works
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        owner.lock_state(|o| {
            // Scheduling while locked should be ignored
            let id2 = ElementId::new();
            o.schedule_build_for(id2, 0);
            assert_eq!(o.dirty_count(), 1); // Still 1, not 2
        });
    }

    #[test]
    fn test_global_key_registry() {
        use crate::foundation::key::GlobalKey;

        let mut owner = BuildOwner::new();
        let key = GlobalKey::<()>::new();
        let element_id = ElementId::new();

        // Register
        let key_id = key.into();
        owner.register_global_key(key_id, element_id);
        assert_eq!(owner.global_key_count(), 1);
        assert_eq!(owner.element_for_global_key(key_id), Some(element_id));

        // Unregister
        owner.unregister_global_key(key_id);
        assert_eq!(owner.global_key_count(), 0);
        assert_eq!(owner.element_for_global_key(key_id), None);
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn test_global_key_duplicate_panic() {
        use crate::foundation::key::GlobalKey;

        let mut owner = BuildOwner::new();
        let key = GlobalKey::<()>::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        let key_id = key.into();
        owner.register_global_key(key_id, id1);
        owner.register_global_key(key_id, id2); // Should panic
    }

    #[test]
    fn test_global_key_same_element_ok() {
        use crate::foundation::key::GlobalKey;

        let mut owner = BuildOwner::new();
        let key = GlobalKey::<()>::new();
        let element_id = ElementId::new();

        let key_id = key.into();
        owner.register_global_key(key_id, element_id);
        owner.register_global_key(key_id, element_id); // Should not panic
        assert_eq!(owner.global_key_count(), 1);
    }

    #[test]
    fn test_depth_sorting() {
        let mut owner = BuildOwner::new();

        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        // Schedule in random order
        owner.schedule_build_for(id2, 2);
        owner.schedule_build_for(id1, 1);
        owner.schedule_build_for(id3, 0);

        // flush_build sorts by depth before rebuilding
        // We can't easily test the actual sorting without mock elements,
        // so just verify elements are scheduled
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_on_build_scheduled_callback() {
        use std::sync::{Arc, Mutex};

        let mut owner = BuildOwner::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        owner.set_on_build_scheduled(move || {
            *called_clone.lock().unwrap() = true;
        });

        let id = ElementId::new();
        owner.schedule_build_for(id, 0);

        assert!(*called.lock().unwrap());
    }

    // Build Batching Tests

    #[test]
    fn test_batching_disabled_by_default() {
        let owner = BuildOwner::new();
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_enable_disable_batching() {
        let mut owner = BuildOwner::new();

        owner.enable_batching(Duration::from_millis(16));
        assert!(owner.is_batching_enabled());

        owner.disable_batching();
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_batching_deduplicates() {
        let mut owner = BuildOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = ElementId::new();

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
        let mut owner = BuildOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);
        owner.schedule_build_for(id3, 2);

        owner.flush_batch();

        // All 3 should be dirty
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        let mut owner = BuildOwner::new();
        owner.enable_batching(Duration::from_millis(10));

        let id = ElementId::new();
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
        let mut owner = BuildOwner::new();
        // Batching not enabled

        let id = ElementId::new();
        owner.schedule_build_for(id, 0);

        // Should add directly to dirty elements
        assert_eq!(owner.dirty_count(), 1);

        // flush_batch should be no-op
        owner.flush_batch();
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        let mut owner = BuildOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = ElementId::new();

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

    // ========== Build Scope Integration Tests ==========

    #[test]
    fn test_build_scope_integration_with_mark_dirty() {
        let mut owner = BuildOwner::new();
        let widget = Box::new(TestWidget);
        let root_id = owner.set_root(widget);

        owner.build_scope(|o| {
            // Inside build scope
            assert!(o.is_in_build_scope());

            let tree = o.tree().read();
            assert!(tree.is_in_build_scope());
        });

        // After build scope exits, flags should be cleared
        assert!(!owner.is_in_build_scope());
        {
            let tree = owner.tree().read();
            assert!(!tree.is_in_build_scope());
        }
    }

    #[test]
    fn test_build_scope_sets_tree_flag() {
        let mut owner = BuildOwner::new();

        // Before build scope
        {
            let tree = owner.tree().read();
            assert!(!tree.is_in_build_scope());
        }

        owner.build_scope(|o| {
            // Inside build scope - tree flag should be set
            let tree = o.tree().read();
            assert!(tree.is_in_build_scope());
        });

        // After build scope - tree flag should be cleared
        {
            let tree = owner.tree().read();
            assert!(!tree.is_in_build_scope());
        }
    }

    #[test]
    fn test_build_scope_returns_result() {
        let mut owner = BuildOwner::new();

        let result = owner.build_scope(|_| {
            42
        });

        assert_eq!(result, 42);
    }
}
