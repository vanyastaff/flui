//! Gesture Binding - Central coordinator for pointer event handling
//!
//! GestureBinding is the main entry point for handling pointer events in the
//! gesture system. It coordinates hit testing, event routing, and arena management.
//!
//! # Architecture
//!
//! ```text
//! Platform Events (winit, etc.)
//!         │
//!         ▼
//! ┌─────────────────────┐
//! │   GestureBinding    │
//! │  ┌───────────────┐  │
//! │  │ Hit Test Cache│  │  (DashMap<PointerId, HitTestResult>)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ PointerRouter │  │  (routes events to handlers)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ GestureArena  │  │  (conflict resolution)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ GestureSettings│  │  (device-specific config)
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
//! 2. **Pointer Move**: Use cached hit test → dispatch
//! 3. **Pointer Up/Cancel**: Use cached hit test → dispatch → sweep arena → clear cache
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::binding::GestureBinding;
//! use flui_interaction::events::PointerEvent;
//!
//! let binding = GestureBinding::new();
//!
//! // Handle platform events
//! fn handle_event(binding: &GestureBinding, event: &PointerEvent) {
//!     binding.handle_pointer_event(event, |hit_test_position| {
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
use flui_types::geometry::Offset;
use std::sync::Arc;
use ui_events::pointer::{PointerEvent, PointerType};

/// Central coordinator for gesture event handling.
///
/// GestureBinding manages the complete lifecycle of pointer events:
/// - Performs hit testing on pointer down
/// - Caches hit test results for subsequent events
/// - Routes events through the PointerRouter
/// - Manages arena lifecycle (close on down, sweep on up)
///
/// # Thread Safety
///
/// GestureBinding is fully thread-safe and can be shared across threads.
/// All internal state is protected by appropriate synchronization primitives.
#[derive(Clone)]
pub struct GestureBinding {
    /// Cached hit test results per pointer.
    /// Avoids redundant hit testing for move/up events.
    hit_tests: Arc<DashMap<PointerId, HitTestResult>>,

    /// Routes pointer events to registered handlers.
    pointer_router: Arc<PointerRouter>,

    /// Resolves conflicts between competing gesture recognizers.
    arena: Arc<GestureArena>,

    /// Default gesture settings (can be overridden per device).
    default_settings: GestureSettings,
}

impl Default for GestureBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureBinding {
    /// Create a new GestureBinding with default settings.
    pub fn new() -> Self {
        Self {
            hit_tests: Arc::new(DashMap::new()),
            pointer_router: Arc::new(PointerRouter::new()),
            arena: Arc::new(GestureArena::new()),
            default_settings: GestureSettings::default(),
        }
    }

    /// Create with specific settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        Self {
            hit_tests: Arc::new(DashMap::new()),
            pointer_router: Arc::new(PointerRouter::new()),
            arena: Arc::new(GestureArena::new()),
            default_settings: settings,
        }
    }

    /// Create with custom components.
    pub fn with_components(
        pointer_router: Arc<PointerRouter>,
        arena: Arc<GestureArena>,
        settings: GestureSettings,
    ) -> Self {
        Self {
            hit_tests: Arc::new(DashMap::new()),
            pointer_router,
            arena,
            default_settings: settings,
        }
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

    /// Get a clone of the arena Arc.
    pub fn arena_arc(&self) -> Arc<GestureArena> {
        self.arena.clone()
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
    /// binding.handle_pointer_event(&event, |position| {
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

            PointerEvent::Move(e) => {
                let pointer_id = self.extract_pointer_id(event);

                // Use cached hit test result
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                } else {
                    // No cached result - this shouldn't happen normally
                    // but we can handle it by doing a new hit test
                    let position =
                        Offset::new(e.current.position.x as f32, e.current.position.y as f32);
                    let result = hit_test_fn(position);
                    self.hit_tests.insert(pointer_id, result.clone());
                    self.dispatch_event(event, &result);
                }
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
            .field("arena_count", &self.arena.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_arena_access() {
        let binding = GestureBinding::new();
        let pointer = PointerId::new(1);

        // Arena should be accessible
        assert!(!binding.arena().contains(pointer));

        // Should be able to get Arc clone
        let arena = binding.arena_arc();
        assert!(Arc::ptr_eq(&arena, &binding.arena));
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
