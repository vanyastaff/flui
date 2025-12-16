//! BuildOwner - Element lifecycle orchestrator
//!
//! `BuildOwner` manages the build phase of the element lifecycle, similar to
//! Flutter's `BuildOwner`. It coordinates element rebuilds and prevents
//! improper state modifications during builds.
//!
//! # Architecture
//!
//! ```text
//! BuildOwner (this crate - flui-element)
//!   └── build_scope() - Coordinates element builds
//!   └── lock_state() - Prevents state changes
//!   └── is_building() - Query build state
//!
//! BuildPipeline (flui_core) - Uses BuildOwner
//!   └── dirty_elements - Tracks what needs rebuild
//!   └── batching - Performance optimization
//!   └── rebuild_dirty() - Actual rebuild logic
//!
//! PipelineOwner (flui_core) - Orchestrates full pipeline
//!   └── build_owner: BuildOwner
//!   └── flush_build() → uses build_owner.build_scope()
//!   └── flush_layout()
//!   └── flush_paint()
//! ```
//!
//! # Flutter Equivalence
//!
//! | Flutter | FLUI |
//! |---------|------|
//! | `BuildOwner.buildScope()` | `BuildOwner::build_scope()` |
//! | `BuildOwner.lockState()` | `BuildOwner::lock_state()` |
//! | `BuildOwner._dirtyElements` | `BuildPipeline::dirty_elements` (in flui_core) |
//!
//! **Architectural Decision:** In Flutter, `BuildOwner` owns the dirty list and manages
//! widget rebuilds separately from `PipelineOwner` (rendering). FLUI takes a different,
//! more unified approach:
//!
//! - **BuildOwner** (this crate): Lifecycle coordination only (build_scope, lock_state)
//! - **PipelineOwner** (flui_core): Unified Build + Layout + Paint pipeline
//!   - Contains BuildPipeline with dirty_elements tracking
//!   - Coordinates all three phases in one place
//!
//! This unified pipeline design is intentional and offers advantages for Rust:
//! 1. **Cohesion**: Build→Layout→Paint is one atomic transaction
//! 2. **Performance**: Easier cross-phase optimization
//! 3. **Simplicity**: Single coordinator API vs coordinating two owners
//! 4. **Type Safety**: Phase ordering guaranteed at compile time
//! 5. **Ownership**: Avoids complex data sharing between multiple owners
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_element::BuildOwner;
//!
//! let build_owner = BuildOwner::new();
//!
//! // Execute code in build scope
//! build_owner.build_scope(|| {
//!     // Rebuild elements here
//!     // Nested builds are detected and warned
//! });
//!
//! // Lock state changes temporarily
//! build_owner.lock_state(|| {
//!     // State changes here will be deferred
//! });
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// BuildOwner - Coordinates element build lifecycle
///
/// Provides:
/// - **Build scope management**: Prevents nested builds, tracks build state
/// - **State locking**: Temporarily prevents state changes during critical sections
/// - **Lifecycle hooks**: Callbacks for build start/end
///
/// # Thread Safety
///
/// `BuildOwner` uses atomic operations for state flags, making it safe to
/// query state from multiple threads. However, `build_scope()` and `lock_state()`
/// should typically be called from a single thread (the UI thread).
///
/// # Design Notes
///
/// ## Unified Pipeline Architecture
///
/// FLUI intentionally differs from Flutter's split BuildOwner/PipelineOwner design.
/// Instead of separating widget rebuilds (BuildOwner) from rendering (PipelineOwner),
/// FLUI unifies all three phases (Build + Layout + Paint) in a single `PipelineOwner`.
///
/// **This `BuildOwner` serves a focused role:**
/// - Lifecycle coordination: `build_scope()`, `lock_state()`
/// - State flags: `is_building()`, `is_locked()`
/// - Build callbacks: `on_build_start`, `on_build_end`
///
/// **It does NOT:**
/// - Own the dirty elements list (that's in `BuildPipeline` within `PipelineOwner`)
/// - Manage layout or paint (those phases are also in `PipelineOwner`)
/// - Coordinate cross-phase dependencies (handled by unified `PipelineOwner`)
///
/// This unified approach is **more suitable for Rust** because:
/// - **Atomicity**: Build→Layout→Paint as single transaction with clear ownership
/// - **Performance**: Cross-phase optimization without data synchronization overhead
/// - **Type Safety**: Compile-time phase ordering guarantees
/// - **Simplicity**: One coordinator API instead of coordinating two separate owners
///
/// See `flui_core::pipeline::PipelineOwner` for the unified pipeline implementation.
pub struct BuildOwner {
    /// Whether currently in a build scope
    in_build_scope: AtomicBool,

    /// Whether state changes are locked
    state_locked: AtomicBool,

    /// Build counter (for debugging/profiling)
    build_count: AtomicU64,

    /// Optional: callback when build scope starts
    on_build_start: Option<Box<dyn Fn() + Send + Sync>>,

    /// Optional: callback when build scope ends
    on_build_end: Option<Box<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for BuildOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOwner")
            .field("in_build_scope", &self.in_build_scope)
            .field("state_locked", &self.state_locked)
            .field("build_count", &self.build_count)
            .field("has_on_build_start", &self.on_build_start.is_some())
            .field("has_on_build_end", &self.on_build_end.is_some())
            .finish()
    }
}

impl BuildOwner {
    /// Create a new BuildOwner
    pub fn new() -> Self {
        Self {
            in_build_scope: AtomicBool::new(false),
            state_locked: AtomicBool::new(false),
            build_count: AtomicU64::new(0),
            on_build_start: None,
            on_build_end: None,
        }
    }

    // =========================================================================
    // Build Scope
    // =========================================================================

    /// Execute code within a build scope
    ///
    /// During a build scope:
    /// - `is_building()` returns `true`
    /// - Nested `build_scope()` calls are detected and warned
    /// - Build callbacks are invoked
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// build_owner.build_scope(|| {
    ///     // Rebuild dirty elements
    ///     for element in dirty_elements {
    ///         element.rebuild();
    ///     }
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// Does not panic on nested calls, but logs a warning. The callback
    /// will still execute.
    pub fn build_scope<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        // Check for nested builds
        let was_building = self.in_build_scope.swap(true, Ordering::AcqRel);
        if was_building {
            tracing::warn!(
                "Nested build_scope detected! This may indicate incorrect usage. \
                 Consider deferring the inner build."
            );
        }

        // Increment build counter
        self.build_count.fetch_add(1, Ordering::Relaxed);

        // Call start callback
        if let Some(ref callback) = self.on_build_start {
            callback();
        }

        // Execute the build
        let result = f();

        // Call end callback
        if let Some(ref callback) = self.on_build_end {
            callback();
        }

        // Restore previous state (handle nested case correctly)
        self.in_build_scope.store(was_building, Ordering::Release);

        result
    }

    /// Check if currently in a build scope
    ///
    /// Returns `true` if code is executing inside a `build_scope()` call.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if build_owner.is_building() {
    ///     // Defer state change
    /// } else {
    ///     // Apply state change immediately
    /// }
    /// ```
    #[inline]
    pub fn is_building(&self) -> bool {
        self.in_build_scope.load(Ordering::Acquire)
    }

    /// Set the building flag directly
    ///
    /// This is an internal API for use by `BuildPipeline` guards.
    /// Prefer using `build_scope()` for normal usage.
    #[inline]
    pub fn set_building(&self, value: bool) {
        self.in_build_scope.store(value, Ordering::Release);
    }

    // =========================================================================
    // State Locking
    // =========================================================================

    /// Execute code with state locked
    ///
    /// While state is locked:
    /// - `is_locked()` returns `true`
    /// - State change attempts should be deferred (caller's responsibility)
    ///
    /// This is used during critical sections where state changes would
    /// cause inconsistencies (e.g., during tree traversal).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// build_owner.lock_state(|| {
    ///     // Traverse tree safely
    ///     // Any setState() calls should be deferred
    /// });
    /// ```
    pub fn lock_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let was_locked = self.state_locked.swap(true, Ordering::AcqRel);

        let result = f();

        // Restore previous state
        self.state_locked.store(was_locked, Ordering::Release);

        result
    }

    /// Check if state changes are locked
    ///
    /// Returns `true` if code is executing inside a `lock_state()` call.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.state_locked.load(Ordering::Acquire)
    }

    /// Set the locked flag directly
    ///
    /// This is an internal API for use by `BuildPipeline` guards.
    /// Prefer using `lock_state()` for normal usage.
    #[inline]
    pub fn set_locked(&self, value: bool) {
        self.state_locked.store(value, Ordering::Release);
    }

    /// Check if state changes should be deferred
    ///
    /// Returns `true` if either:
    /// - Currently in a build scope
    /// - State is explicitly locked
    ///
    /// Use this to decide whether to apply state changes immediately
    /// or defer them.
    #[inline]
    pub fn should_defer_state_change(&self) -> bool {
        self.is_building() || self.is_locked()
    }

    // =========================================================================
    // Statistics & Callbacks
    // =========================================================================

    /// Get total number of builds executed
    ///
    /// This counter increments each time `build_scope()` is called.
    /// Useful for debugging and profiling.
    #[inline]
    pub fn build_count(&self) -> u64 {
        self.build_count.load(Ordering::Relaxed)
    }

    /// Set callback for build scope start
    ///
    /// Called at the beginning of each `build_scope()`.
    pub fn set_on_build_start<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_start = Some(Box::new(callback));
    }

    /// Set callback for build scope end
    ///
    /// Called at the end of each `build_scope()`.
    pub fn set_on_build_end<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_end = Some(Box::new(callback));
    }

    /// Clear all callbacks
    pub fn clear_callbacks(&mut self) {
        self.on_build_start = None;
        self.on_build_end = None;
    }
}

impl Default for BuildOwner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_new() {
        let owner = BuildOwner::new();
        assert!(!owner.is_building());
        assert!(!owner.is_locked());
        assert_eq!(owner.build_count(), 0);
    }

    #[test]
    fn test_build_scope() {
        let owner = BuildOwner::new();

        assert!(!owner.is_building());

        let result = owner.build_scope(|| {
            assert!(owner.is_building());
            42
        });

        assert_eq!(result, 42);
        assert!(!owner.is_building());
        assert_eq!(owner.build_count(), 1);
    }

    #[test]
    fn test_nested_build_scope() {
        let owner = BuildOwner::new();

        owner.build_scope(|| {
            assert!(owner.is_building());

            // Nested build scope (should warn but still work)
            owner.build_scope(|| {
                assert!(owner.is_building());
            });

            assert!(owner.is_building());
        });

        assert!(!owner.is_building());
        assert_eq!(owner.build_count(), 2);
    }

    #[test]
    fn test_lock_state() {
        let owner = BuildOwner::new();

        assert!(!owner.is_locked());

        let result = owner.lock_state(|| {
            assert!(owner.is_locked());
            "locked"
        });

        assert_eq!(result, "locked");
        assert!(!owner.is_locked());
    }

    #[test]
    fn test_should_defer_state_change() {
        let owner = BuildOwner::new();

        // Not in build, not locked
        assert!(!owner.should_defer_state_change());

        // In build
        owner.build_scope(|| {
            assert!(owner.should_defer_state_change());
        });

        // Locked
        owner.lock_state(|| {
            assert!(owner.should_defer_state_change());
        });
    }

    #[test]
    fn test_callbacks() {
        use std::sync::atomic::AtomicUsize;

        let start_count = Arc::new(AtomicUsize::new(0));
        let end_count = Arc::new(AtomicUsize::new(0));

        let mut owner = BuildOwner::new();

        let start_clone = start_count.clone();
        owner.set_on_build_start(move || {
            start_clone.fetch_add(1, Ordering::SeqCst);
        });

        let end_clone = end_count.clone();
        owner.set_on_build_end(move || {
            end_clone.fetch_add(1, Ordering::SeqCst);
        });

        owner.build_scope(|| {});
        owner.build_scope(|| {});

        assert_eq!(start_count.load(Ordering::SeqCst), 2);
        assert_eq!(end_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_thread_safe_query() {
        use std::thread;

        let owner = Arc::new(BuildOwner::new());
        let owner_clone = owner.clone();

        // Query from another thread
        let handle = thread::spawn(move || {
            assert!(!owner_clone.is_building());
            assert!(!owner_clone.is_locked());
        });

        handle.join().unwrap();
    }
}
