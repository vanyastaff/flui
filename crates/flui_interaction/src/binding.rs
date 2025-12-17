//! Gesture Binding - Singleton coordinator for pointer event handling
//!
//! GestureBinding is the main entry point for handling pointer events in the
//! gesture system. It coordinates hit testing, event routing, arena management,
//! and pointer move event coalescing.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `GestureBinding` mixin:
//!
//! ```dart
//! mixin GestureBinding on BindingBase implements HitTestable, HitTestDispatcher, HitTestTarget {
//!   @override
//!   void initInstances() {
//!     super.initInstances();
//!     _instance = this;
//!     // ...
//!   }
//!
//!   static GestureBinding get instance => BindingBase.checkInstance(_instance);
//!   static GestureBinding? _instance;
//! }
//! ```
//!
//! # Architecture
//!
//! ```text
//! Platform Events (winit, etc.)
//!         │
//!         ▼
//! ┌─────────────────────┐
//! │   GestureBinding    │ (singleton)
//! │  ┌───────────────┐  │
//! │  │ Hit Test Cache│  │  (DashMap<PointerId, HitTestResult>)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ Pending Moves │  │  (DashMap<PointerId, PointerEvent> - coalescing)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ PointerRouter │  │  (routes events to handlers)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ GestureArena  │  │  (conflict resolution)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ GestureSettings│ │  (device-specific config)
//! │  └───────────────┘  │
//! └─────────────────────┘
//!         │
//!         ▼
//!    Gesture Recognizers
//! ```
//!
//! # Lifecycle
//!
//! 1. **Pointer Down**: Hit test → cache result → dispatch → close arena
//! 2. **Pointer Move**: Use cached hit test → dispatch (coalesced)
//! 3. **Pointer Up/Cancel**: Use cached hit test → dispatch → sweep arena → clear cache
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::GestureBinding;
//! use flui_interaction::events::PointerEvent;
//!
//! // Get the singleton instance
//! let binding = GestureBinding::instance();
//!
//! // Handle platform events
//! fn handle_event(event: &PointerEvent) {
//!     GestureBinding::instance().handle_pointer_event(event, |hit_test_position| {
//!         // Perform hit testing on your render tree
//!         my_render_tree.hit_test(hit_test_position)
//!     });
//! }
//! ```

use crate::arena::GestureArena;
use crate::ids::PointerId;
use crate::routing::{HitTestResult, PointerRouter};
use crate::settings::GestureSettings;
use dashmap::DashMap;
use flui_foundation::{impl_binding_singleton, BindingBase};
use flui_types::geometry::Offset;
use ui_events::pointer::{PointerEvent, PointerType};

/// Central coordinator for gesture event handling (singleton).
///
/// GestureBinding manages the complete lifecycle of pointer events:
/// - Performs hit testing on pointer down
/// - Caches hit test results for subsequent events
/// - Coalesces high-frequency pointer move events (100+ events/sec → 1 per frame)
/// - Routes events through the PointerRouter
/// - Manages arena lifecycle (close on down, sweep on up)
///
/// # Singleton Pattern
///
/// Access via `GestureBinding::instance()`:
///
/// ```rust,ignore
/// let binding = GestureBinding::instance();
/// binding.handle_pointer_event(&event, hit_test_fn);
/// ```
///
/// # Event Coalescing
///
/// Desktop platforms can generate 100+ mouse move events per second.
/// GestureBinding coalesces these by storing only the latest move event
/// per pointer. Call `flush_pending_moves()` once per frame to process
/// the coalesced events.
///
/// # Thread Safety
///
/// GestureBinding is fully thread-safe and can be shared across threads.
/// All internal state is protected by appropriate synchronization primitives.
pub struct GestureBinding {
    /// Cached hit test results per pointer.
    /// Avoids redundant hit testing for move/up events.
    hit_tests: DashMap<PointerId, HitTestResult>,

    /// Pending move events for coalescing.
    /// Only the latest move per pointer is kept.
    pending_moves: DashMap<PointerId, PointerEvent>,

    /// Routes pointer events to registered handlers.
    pointer_router: PointerRouter,

    /// Resolves conflicts between competing gesture recognizers.
    arena: GestureArena,

    /// Default gesture settings (can be overridden per device).
    default_settings: GestureSettings,
}

// Implement BindingBase trait
impl BindingBase for GestureBinding {
    fn init_instances(&mut self) {
        // GestureBinding initialization is done in new()
        // This is called automatically by the singleton macro
        tracing::debug!("GestureBinding initialized");
    }
}

// Implement singleton pattern via macro
impl_binding_singleton!(GestureBinding);

impl Default for GestureBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureBinding {
    /// Create a new GestureBinding with default settings.
    ///
    /// Note: Prefer using `GestureBinding::instance()` for singleton access.
    pub fn new() -> Self {
        let mut binding = Self {
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            pointer_router: PointerRouter::new(),
            arena: GestureArena::new(),
            default_settings: GestureSettings::default(),
        };
        binding.init_instances();
        binding
    }

    /// Create with specific settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        let mut binding = Self {
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            pointer_router: PointerRouter::new(),
            arena: GestureArena::new(),
            default_settings: settings,
        };
        binding.init_instances();
        binding
    }

    // ========================================================================
    // Component Accessors
    // ========================================================================

    /// Get the pointer router.
    pub fn pointer_router(&self) -> &PointerRouter {
        &self.pointer_router
    }

    /// Get the gesture arena.
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// Get the default gesture settings.
    pub fn default_settings(&self) -> &GestureSettings {
        &self.default_settings
    }

    /// Get settings for a specific device type.
    pub fn settings_for_device(&self, device_type: PointerType) -> GestureSettings {
        GestureSettings::for_device(device_type)
    }

    // ========================================================================
    // Event Handling
    // ========================================================================

    /// Handle a pointer event.
    ///
    /// This is the main entry point for processing pointer events.
    /// The `hit_test_fn` is called on pointer down to determine which
    /// targets are under the pointer.
    ///
    /// # Arguments
    ///
    /// * `event` - The pointer event to handle
    /// * `hit_test_fn` - Function to perform hit testing (called on pointer down)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// GestureBinding::instance().handle_pointer_event(&event, |position| {
    ///     render_tree.hit_test(position)
    /// });
    /// ```
    pub fn handle_pointer_event<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset) -> HitTestResult,
    {
        match event {
            PointerEvent::Down(e) => {
                let pointer_id = self.extract_pointer_id(event);
                let position = Offset::new(e.state.position.x as f32, e.state.position.y as f32);

                // Perform hit test
                let result = hit_test_fn(position);

                // Cache the result
                self.hit_tests.insert(pointer_id, result.clone());

                // Dispatch to targets
                self.dispatch_event(event, &result);

                // Close the arena for this pointer
                self.arena.close(pointer_id);
            }

            PointerEvent::Move(_) => {
                let pointer_id = self.extract_pointer_id(event);

                // Coalesce move events - store only the latest, process on flush
                self.pending_moves.insert(pointer_id, event.clone());
            }

            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                let pointer_id = self.extract_pointer_id(event);

                // Use cached hit test result
                if let Some((_, result)) = self.hit_tests.remove(&pointer_id) {
                    self.dispatch_event(event, &result);
                }

                // Sweep the arena
                self.arena.sweep(pointer_id);
            }

            PointerEvent::Enter(_) | PointerEvent::Leave(_) => {
                // Enter/Leave don't participate in gesture recognition
                // but we still dispatch them
                let pointer_id = self.extract_pointer_id(event);
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                }
            }

            PointerEvent::Scroll(e) => {
                let pointer_id = self.extract_pointer_id(event);

                // Scroll events might not have a cached hit test
                // Use the position to do a hit test if needed
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                } else {
                    let position =
                        Offset::new(e.state.position.x as f32, e.state.position.y as f32);
                    let result = hit_test_fn(position);
                    self.dispatch_event(event, &result);
                }
            }

            PointerEvent::Gesture(_) => {
                // Gesture events are high-level and handled separately
                let pointer_id = self.extract_pointer_id(event);
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                }
            }
        }
    }

    /// Handle pointer event without hit testing.
    ///
    /// Use this when you already have a hit test result or want to
    /// manually control hit testing.
    pub fn handle_pointer_event_with_result(&self, event: &PointerEvent, result: &HitTestResult) {
        let pointer_id = self.extract_pointer_id(event);

        match event {
            PointerEvent::Down(_) => {
                self.hit_tests.insert(pointer_id, result.clone());
                self.dispatch_event(event, result);
                self.arena.close(pointer_id);
            }

            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                self.dispatch_event(event, result);
                self.hit_tests.remove(&pointer_id);
                self.arena.sweep(pointer_id);
            }

            _ => {
                self.dispatch_event(event, result);
            }
        }
    }

    // ========================================================================
    // Event Coalescing
    // ========================================================================

    /// Flush pending coalesced move events.
    ///
    /// Call this once per frame to process all coalesced pointer move events.
    /// Returns the number of events processed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In your frame loop:
    /// fn on_frame(&mut self) {
    ///     // Process coalesced move events
    ///     GestureBinding::instance().flush_pending_moves();
    ///
    ///     // Then do layout, paint, etc.
    /// }
    /// ```
    pub fn flush_pending_moves(&self) -> usize {
        let mut count = 0;

        // Take all pending moves
        let pending: Vec<_> = self
            .pending_moves
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        self.pending_moves.clear();

        for (pointer_id, event) in pending {
            // Use cached hit test result
            if let Some(result) = self.hit_tests.get(&pointer_id) {
                self.dispatch_event(&event, &result);
                count += 1;
            }
        }

        count
    }

    /// Check if there are pending move events to process.
    pub fn has_pending_moves(&self) -> bool {
        !self.pending_moves.is_empty()
    }

    /// Get the number of pending move events.
    pub fn pending_move_count(&self) -> usize {
        self.pending_moves.len()
    }

    // ========================================================================
    // Hit Test Cache Management
    // ========================================================================

    /// Get the cached hit test result for a pointer.
    pub fn get_hit_test(&self, pointer_id: PointerId) -> Option<HitTestResult> {
        self.hit_tests.get(&pointer_id).map(|r| r.clone())
    }

    /// Check if there's a cached hit test for a pointer.
    pub fn has_hit_test(&self, pointer_id: PointerId) -> bool {
        self.hit_tests.contains_key(&pointer_id)
    }

    /// Clear the hit test cache for a pointer.
    pub fn clear_hit_test(&self, pointer_id: PointerId) {
        self.hit_tests.remove(&pointer_id);
    }

    /// Clear all cached hit tests.
    pub fn clear_all_hit_tests(&self) {
        self.hit_tests.clear();
    }

    /// Get the number of active pointers (with cached hit tests).
    pub fn active_pointer_count(&self) -> usize {
        self.hit_tests.len()
    }

    // ========================================================================
    // Arena Management
    // ========================================================================

    /// Manually close the arena for a pointer.
    ///
    /// Normally called automatically on pointer down.
    pub fn close_arena(&self, pointer_id: PointerId) {
        self.arena.close(pointer_id);
    }

    /// Manually sweep the arena for a pointer.
    ///
    /// Normally called automatically on pointer up/cancel.
    pub fn sweep_arena(&self, pointer_id: PointerId) {
        self.arena.sweep(pointer_id);
    }

    /// Resolve any timed out arenas.
    ///
    /// Call this periodically (e.g., on frame tick) to handle disambiguation
    /// timeouts.
    pub fn resolve_timed_out_arenas(&self) -> usize {
        self.arena.resolve_default_timed_out_arenas()
    }

    // ========================================================================
    // Internal Methods
    // ========================================================================

    /// Extract pointer ID from event.
    fn extract_pointer_id(&self, event: &PointerEvent) -> PointerId {
        // Use a hash of the pointer info to create a stable ID
        let info = match event {
            PointerEvent::Down(e) => &e.pointer,
            PointerEvent::Up(e) => &e.pointer,
            PointerEvent::Move(e) => &e.pointer,
            PointerEvent::Cancel(e) => e,
            PointerEvent::Enter(e) => e,
            PointerEvent::Leave(e) => e,
            PointerEvent::Scroll(e) => &e.pointer,
            PointerEvent::Gesture(e) => &e.pointer,
        };

        // Use pointer_id if available, otherwise use device ID
        let id = if let Some(pid) = info.pointer_id {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            pid.hash(&mut hasher);
            (hasher.finish() & 0x7FFFFFFF) as i32
        } else if let Some(did) = info.persistent_device_id {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            did.hash(&mut hasher);
            (hasher.finish() & 0x7FFFFFFF) as i32
        } else {
            0 // Primary pointer
        };

        PointerId::new(id)
    }

    /// Dispatch event to hit test targets.
    fn dispatch_event(&self, event: &PointerEvent, result: &HitTestResult) {
        // Route the event through the pointer router
        self.pointer_router.route(event);

        // Also dispatch to hit test entries with handlers
        for entry in result.path() {
            if let Some(ref handler) = entry.handler {
                handler(event);
            }
        }
    }
}

impl std::fmt::Debug for GestureBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureBinding")
            .field("active_pointers", &self.hit_tests.len())
            .field("pending_moves", &self.pending_moves.len())
            .field("arena_count", &self.arena.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::HasInstance;

    #[test]
    fn test_binding_singleton() {
        let binding1 = GestureBinding::instance();
        let binding2 = GestureBinding::instance();

        // Should be the same instance
        assert!(std::ptr::eq(binding1, binding2));
    }

    #[test]
    fn test_binding_is_initialized() {
        // Ensure instance exists
        let _ = GestureBinding::instance();

        // Should be initialized
        assert!(GestureBinding::is_initialized());
    }

    #[test]
    fn test_binding_creation() {
        let binding = GestureBinding::new();
        assert_eq!(binding.active_pointer_count(), 0);
    }

    #[test]
    fn test_binding_with_settings() {
        let settings = GestureSettings::mouse_defaults();
        let binding = GestureBinding::with_settings(settings.clone());
        assert_eq!(
            binding.default_settings().touch_slop(),
            settings.touch_slop()
        );
    }

    #[test]
    fn test_hit_test_cache() {
        let binding = GestureBinding::new();
        let pointer = PointerId::new(1);
        let result = HitTestResult::new();

        binding.hit_tests.insert(pointer, result.clone());
        assert!(binding.has_hit_test(pointer));

        let cached = binding.get_hit_test(pointer);
        assert!(cached.is_some());

        binding.clear_hit_test(pointer);
        assert!(!binding.has_hit_test(pointer));
    }

    #[test]
    fn test_clear_all_hit_tests() {
        let binding = GestureBinding::new();

        binding
            .hit_tests
            .insert(PointerId::new(1), HitTestResult::new());
        binding
            .hit_tests
            .insert(PointerId::new(2), HitTestResult::new());
        binding
            .hit_tests
            .insert(PointerId::new(3), HitTestResult::new());

        assert_eq!(binding.active_pointer_count(), 3);

        binding.clear_all_hit_tests();
        assert_eq!(binding.active_pointer_count(), 0);
    }

    #[test]
    fn test_settings_for_device() {
        let binding = GestureBinding::new();

        let touch_settings = binding.settings_for_device(PointerType::Touch);
        let mouse_settings = binding.settings_for_device(PointerType::Mouse);

        // Touch should have larger slop than mouse
        assert!(touch_settings.touch_slop() > mouse_settings.touch_slop());
    }
}
