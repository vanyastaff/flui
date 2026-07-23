//! Semantics binding for platform integration.
//!
//! This module provides the binding between the semantics system and the
//! platform. It manages when semantics are enabled/disabled and handles
//! accessibility features.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_foundation::{BindingBase, impl_binding_singleton};
use parking_lot::RwLock;

use crate::{event::SemanticsEvent, role::Assertiveness};

// ============================================================================
// AccessibilityFeatures
// ============================================================================

/// Platform accessibility features.
///
/// This struct represents the accessibility settings that the platform
/// has enabled, such as reduced motion or high contrast mode.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `AccessibilityFeatures` from dart:ui.
#[allow(clippy::struct_excessive_bools)] // Mirrors Flutter's AccessibilityFeatures flags
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessibilityFeatures {
    /// Whether accessible navigation is enabled.
    pub accessible_navigation: bool,

    /// Whether to invert colors.
    pub invert_colors: bool,

    /// Whether to disable animations.
    pub disable_animations: bool,

    /// Whether bold text is enabled.
    pub bold_text: bool,

    /// Whether to reduce motion.
    pub reduce_motion: bool,

    /// Whether high contrast mode is enabled.
    pub high_contrast: bool,

    /// Whether on/off labels should be shown on switches.
    pub on_off_switch_labels: bool,
}

impl AccessibilityFeatures {
    /// Creates new accessibility features with all options disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether any accessibility feature is enabled.
    pub fn any_enabled(&self) -> bool {
        self.accessible_navigation
            || self.invert_colors
            || self.disable_animations
            || self.bold_text
            || self.reduce_motion
            || self.high_contrast
            || self.on_off_switch_labels
    }
}

// ============================================================================
// SemanticsHandle
// ============================================================================

/// A handle that keeps semantics enabled while held.
///
/// Semantics information is only collected when there are clients interested
/// in it. Clients express their interest by holding a `SemanticsHandle`.
/// When all handles are dropped, semantics collection stops.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsHandle`.
///
/// # Example
///
/// ```rust,ignore
/// let binding = SemanticsBinding::new();
///
/// // Request semantics - returns a handle
/// let handle = binding.ensure_semantics();
///
/// // Semantics are now enabled
/// assert!(binding.semantics_enabled());
///
/// // Drop the handle to disable semantics
/// drop(handle);
/// ```
pub struct SemanticsHandle {
    /// Reference to the binding's handle counter.
    counter: Arc<AtomicUsize>,
}

impl SemanticsHandle {
    /// Creates a new semantics handle.
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for SemanticsHandle {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for SemanticsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsHandle")
            .field("active_handles", &self.counter.load(Ordering::SeqCst))
            .finish()
    }
}

// ============================================================================
// SemanticsBinding
// ============================================================================

/// The binding between the semantics system and the platform.
///
/// `SemanticsBinding` manages:
/// - Whether semantics are enabled (via reference counting)
/// - Platform accessibility features
/// - Accessibility announcements
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsBinding` mixin.
///
/// # Thread Safety
///
/// `SemanticsBinding` is thread-safe and can be shared across threads
/// using `Arc<SemanticsBinding>`.
pub struct SemanticsBinding {
    /// Number of active semantics handles.
    handle_count: Arc<AtomicUsize>,

    /// Whether the platform has requested semantics.
    platform_semantics_enabled: AtomicBool,

    /// Current accessibility features.
    accessibility_features: RwLock<AccessibilityFeatures>,

    /// Callback for accessibility announcements.
    #[allow(clippy::type_complexity)]
    announce_callback: RwLock<Option<Arc<dyn Fn(&str, Assertiveness) + Send + Sync>>>,

    /// Callback for semantics action events.
    #[allow(clippy::type_complexity)]
    action_callback: RwLock<Option<Arc<dyn Fn(SemanticsActionEvent) + Send + Sync>>>,

    /// Callback for semantics events dispatched via
    /// [`SemanticsService::send_event`]. Set by the platform embedder when
    /// the accessibility surface is brought up; cleared when the platform
    /// goes silent. Mirrors [`Self::announce_callback`]'s shape.
    #[allow(clippy::type_complexity)]
    event_callback: RwLock<Option<Arc<dyn Fn(&SemanticsEvent) + Send + Sync>>>,
}

impl SemanticsBinding {
    /// Creates a new semantics binding.
    pub fn new() -> Self {
        Self {
            handle_count: Arc::new(AtomicUsize::new(0)),
            platform_semantics_enabled: AtomicBool::new(false),
            accessibility_features: RwLock::new(AccessibilityFeatures::default()),
            announce_callback: RwLock::new(None),
            action_callback: RwLock::new(None),
            event_callback: RwLock::new(None),
        }
    }

    // ========== Semantics Enabled State ==========

    /// Returns whether semantics are currently enabled.
    ///
    /// Semantics are enabled if either:
    /// - The platform has requested semantics
    /// - There are outstanding `SemanticsHandle`s
    pub fn semantics_enabled(&self) -> bool {
        self.platform_semantics_enabled.load(Ordering::SeqCst)
            || self.handle_count.load(Ordering::SeqCst) > 0
    }

    /// Returns the number of outstanding semantics handles.
    pub fn outstanding_handles(&self) -> usize {
        self.handle_count.load(Ordering::SeqCst)
    }

    /// Creates a new `SemanticsHandle` and enables semantics collection.
    ///
    /// The returned handle keeps semantics enabled until it is dropped.
    pub fn ensure_semantics(&self) -> SemanticsHandle {
        SemanticsHandle::new(Arc::clone(&self.handle_count))
    }

    /// Sets whether the platform has requested semantics.
    ///
    /// This is typically called by the platform embedder when accessibility
    /// services are activated or deactivated.
    pub fn set_platform_semantics_enabled(&self, enabled: bool) {
        self.platform_semantics_enabled
            .store(enabled, Ordering::SeqCst);
    }

    /// Returns whether the platform has requested semantics.
    pub fn platform_semantics_enabled(&self) -> bool {
        self.platform_semantics_enabled.load(Ordering::SeqCst)
    }

    // ========== Accessibility Features ==========

    /// Returns the current accessibility features.
    pub fn accessibility_features(&self) -> AccessibilityFeatures {
        *self.accessibility_features.read()
    }

    /// Updates the accessibility features.
    ///
    /// This is typically called by the platform embedder when accessibility
    /// settings change.
    pub fn set_accessibility_features(&self, features: AccessibilityFeatures) {
        *self.accessibility_features.write() = features;
    }

    /// Returns whether animations should be disabled.
    pub fn disable_animations(&self) -> bool {
        self.accessibility_features.read().disable_animations
    }

    // ========== Announcements ==========

    /// Sets the callback for accessibility announcements.
    pub fn set_announce_callback<F>(&self, callback: F)
    where
        F: Fn(&str, Assertiveness) + Send + Sync + 'static,
    {
        *self.announce_callback.write() = Some(Arc::new(callback));
    }

    /// Announces a message to assistive technology.
    ///
    /// Uses the clone-and-release lock pattern (see [`Self::dispatch_event`]).
    ///
    /// # Arguments
    ///
    /// * `message` - The message to announce.
    /// * `assertiveness` - How urgently to announce the message.
    pub fn announce(&self, message: &str, assertiveness: Assertiveness) {
        let cb = self.announce_callback.read().as_ref().map(Arc::clone);
        if let Some(cb) = cb {
            cb(message, assertiveness);
        }
    }

    // ========== Action Events ==========

    /// Sets the callback for semantics action events.
    pub fn set_action_callback<F>(&self, callback: F)
    where
        F: Fn(SemanticsActionEvent) + Send + Sync + 'static,
    {
        *self.action_callback.write() = Some(Arc::new(callback));
    }

    /// Dispatches a semantics action event.
    ///
    /// This is called by the platform when an assistive technology
    /// requests an action on a semantics node.
    pub fn dispatch_action(&self, event: SemanticsActionEvent) {
        // Clone-and-release: pull the Arc out of the read-lock and invoke
        // outside the lock guard so the callback can reach back into the
        // binding (e.g. read accessibility features) without deadlocking
        // on its own lock.
        let cb = self.action_callback.read().as_ref().map(Arc::clone);
        if let Some(cb) = cb {
            cb(event);
        }
    }

    // ========== Semantics Events ==========

    /// Sets the callback for semantics events dispatched via
    /// [`SemanticsService::send_event`].
    ///
    /// Set by the platform embedder when the accessibility surface is
    /// brought up; pass `None` (via re-setting to a no-op closure) when
    /// the platform goes silent. Mirrors [`Self::set_announce_callback`].
    pub fn set_event_callback<F>(&self, callback: F)
    where
        F: Fn(&SemanticsEvent) + Send + Sync + 'static,
    {
        *self.event_callback.write() = Some(Arc::new(callback));
    }

    /// Dispatches a semantics event to the registered platform callback.
    ///
    /// Uses the **clone-and-release** lock-handling pattern: the
    /// `Arc<dyn Fn>` is cloned out of the read-lock before the callback
    /// runs, so user code reaching back into the binding (e.g. reading
    /// accessibility features or registering another callback) cannot
    /// deadlock on the binding's own lock.
    ///
    /// Called by [`SemanticsService::send_event`] when the binding is
    /// initialized.
    pub fn dispatch_event(&self, event: &SemanticsEvent) {
        let cb = self.event_callback.read().as_ref().map(Arc::clone);
        if let Some(cb) = cb {
            cb(event);
        }
    }
}

impl Default for SemanticsBinding {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// BindingBase Implementation
// ============================================================================

impl BindingBase for SemanticsBinding {
    fn init_instances(&mut self) {
        // SemanticsBinding initialization is handled in new()
        // This is called by the singleton infrastructure
    }
}

// Implement singleton pattern using the macro from flui-foundation
impl_binding_singleton!(SemanticsBinding);

impl std::fmt::Debug for SemanticsBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsBinding")
            .field("semantics_enabled", &self.semantics_enabled())
            .field("outstanding_handles", &self.outstanding_handles())
            .field(
                "platform_semantics_enabled",
                &self.platform_semantics_enabled(),
            )
            .field("accessibility_features", &self.accessibility_features())
            .finish()
    }
}

// ============================================================================
// SemanticsActionEvent
// ============================================================================

/// An event representing a semantics action request from the platform.
///
/// This is sent when an assistive technology (like a screen reader)
/// requests an action on a semantics node.
#[derive(Debug, Clone)]
pub struct SemanticsActionEvent {
    /// The ID of the semantics node.
    pub node_id: u64,

    /// The action to perform.
    pub action: crate::SemanticsAction,

    /// Optional arguments for the action.
    pub arguments: Option<crate::ActionArgs>,
}

impl SemanticsActionEvent {
    /// Creates a new semantics action event.
    pub fn new(node_id: u64, action: crate::SemanticsAction) -> Self {
        Self {
            node_id,
            action,
            arguments: None,
        }
    }

    /// Creates a new semantics action event with arguments.
    pub fn with_arguments(
        node_id: u64,
        action: crate::SemanticsAction,
        arguments: crate::ActionArgs,
    ) -> Self {
        Self {
            node_id,
            action,
            arguments: Some(arguments),
        }
    }
}

// ============================================================================
// SemanticsService
// ============================================================================

/// Static service for making accessibility announcements.
///
/// This provides a convenient way to make announcements without
/// needing a reference to the `SemanticsBinding`.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsService` class.
///
/// # Example
///
/// ```rust,ignore
/// use flui_semantics::{SemanticsService, Assertiveness};
///
/// // Make a polite announcement
/// SemanticsService::announce("Item added to cart");
///
/// // Make an assertive announcement
/// SemanticsService::announce_with_assertiveness("Error occurred", Assertiveness::Assertive);
/// ```
#[derive(Debug)]
pub struct SemanticsService;

impl SemanticsService {
    /// Announces a message with polite assertiveness.
    ///
    /// Uses the global `SemanticsBinding` singleton.
    pub fn announce(message: &str) {
        Self::announce_with_assertiveness(message, Assertiveness::Polite);
    }

    /// Announces a message with the specified assertiveness.
    ///
    /// Uses the global `SemanticsBinding` singleton.
    pub fn announce_with_assertiveness(message: &str, assertiveness: Assertiveness) {
        use flui_foundation::HasInstance;

        if SemanticsBinding::is_initialized() {
            SemanticsBinding::instance().announce(message, assertiveness);
        } else {
            tracing::debug!(
                message = message,
                assertiveness = ?assertiveness,
                "SemanticsService::announce (binding not initialized)"
            );
        }
    }

    /// Sends a semantics event to the platform accessibility surface.
    ///
    /// Routes through [`SemanticsBinding::dispatch_event`] when the binding
    /// is initialized; falls back to a `tracing::debug!` log otherwise (the
    /// platform embedder hasn't called `set_event_callback` yet).
    ///
    /// Takes `event` by value to keep the call shape consistent with
    /// Flutter's `dart:ui SemanticsService.sendEvent` (the engine
    /// integration consumes the event), even though the dispatch path
    /// hands the callback a borrow. The `let _ = event;` after the
    /// log branch makes the unused-on-the-fallback-path branch explicit.
    #[allow(clippy::needless_pass_by_value)] // consumed by future engine wiring
    pub fn send_event(event: SemanticsEvent) {
        use flui_foundation::HasInstance;

        if SemanticsBinding::is_initialized() {
            SemanticsBinding::instance().dispatch_event(&event);
        } else {
            tracing::debug!(
                event = ?event,
                "SemanticsService::send_event (binding not initialized)"
            );
        }
    }

    /// Announces a tooltip.
    pub fn tooltip(message: &str) {
        let event = SemanticsEvent::tooltip(message);
        Self::send_event(event);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_handle_reference_counting() {
        let binding = SemanticsBinding::new();

        assert!(!binding.semantics_enabled());
        assert_eq!(binding.outstanding_handles(), 0);

        let handle1 = binding.ensure_semantics();
        assert!(binding.semantics_enabled());
        assert_eq!(binding.outstanding_handles(), 1);

        let handle2 = binding.ensure_semantics();
        assert_eq!(binding.outstanding_handles(), 2);

        drop(handle1);
        assert!(binding.semantics_enabled());
        assert_eq!(binding.outstanding_handles(), 1);

        drop(handle2);
        assert!(!binding.semantics_enabled());
        assert_eq!(binding.outstanding_handles(), 0);
    }

    #[test]
    fn test_platform_semantics() {
        let binding = SemanticsBinding::new();

        assert!(!binding.semantics_enabled());

        binding.set_platform_semantics_enabled(true);
        assert!(binding.semantics_enabled());
        assert!(binding.platform_semantics_enabled());

        binding.set_platform_semantics_enabled(false);
        assert!(!binding.semantics_enabled());
    }

    #[test]
    fn test_combined_semantics_enabled() {
        let binding = SemanticsBinding::new();

        // Neither platform nor handles
        assert!(!binding.semantics_enabled());

        // Only platform
        binding.set_platform_semantics_enabled(true);
        assert!(binding.semantics_enabled());

        // Both platform and handle
        let handle = binding.ensure_semantics();
        assert!(binding.semantics_enabled());

        // Only handle (platform disabled)
        binding.set_platform_semantics_enabled(false);
        assert!(binding.semantics_enabled());

        // Neither (handle dropped)
        drop(handle);
        assert!(!binding.semantics_enabled());
    }

    #[test]
    fn test_accessibility_features() {
        let binding = SemanticsBinding::new();

        let features = binding.accessibility_features();
        assert!(!features.disable_animations);
        assert!(!features.reduce_motion);

        binding.set_accessibility_features(AccessibilityFeatures {
            disable_animations: true,
            reduce_motion: true,
            ..Default::default()
        });

        let features = binding.accessibility_features();
        assert!(features.disable_animations);
        assert!(features.reduce_motion);
        assert!(binding.disable_animations());
    }

    #[test]
    fn test_accessibility_features_any_enabled() {
        let features = AccessibilityFeatures::default();
        assert!(!features.any_enabled());

        let features = AccessibilityFeatures {
            bold_text: true,
            ..Default::default()
        };
        assert!(features.any_enabled());
    }

    #[test]
    fn test_announce_callback() {
        use std::sync::atomic::AtomicUsize;

        let binding = SemanticsBinding::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        binding.set_announce_callback(move |_msg, _assertiveness| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        binding.announce("Test message", Assertiveness::Polite);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        binding.announce("Another message", Assertiveness::Assertive);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_semantics_action_event() {
        use crate::SemanticsAction;

        let event = SemanticsActionEvent::new(42, SemanticsAction::Tap);
        assert_eq!(event.node_id, 42);
        assert_eq!(event.action, SemanticsAction::Tap);
        assert!(event.arguments.is_none());

        let event = SemanticsActionEvent::with_arguments(
            10,
            SemanticsAction::SetText,
            crate::ActionArgs::SetText {
                text: "Hello".to_string(),
            },
        );
        assert_eq!(event.node_id, 10);
        assert!(event.arguments.is_some());
    }

    #[test]
    fn test_event_callback() {
        use std::sync::atomic::AtomicUsize;

        let binding = SemanticsBinding::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        binding.set_event_callback(move |_event| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        binding.dispatch_event(&SemanticsEvent::tooltip("hi"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        binding.dispatch_event(&SemanticsEvent::tooltip("again"));
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_event_callback_clone_and_release_no_deadlock() {
        // Verify the clone-and-release lock pattern: a callback that
        // mutates binding state (registers another callback) must not
        // deadlock on the binding's own RwLock.
        let binding = Arc::new(SemanticsBinding::new());
        let binding_clone = Arc::clone(&binding);

        binding.set_event_callback(move |_event| {
            // Reach back into the binding from inside the callback.
            // Pre-cycle (`if let Some(ref cb) = *self.event_callback.read()`)
            // would hold the read lock here, deadlocking on the write
            // attempt below.
            binding_clone.set_event_callback(|_| {});
        });

        binding.dispatch_event(&SemanticsEvent::tooltip("first"));
        // If we got here, the clone-and-release pattern released the
        // read lock before invoking the callback.
    }

    #[test]
    fn test_dispatch_event_without_callback_is_a_no_op() {
        // No callback registered — must not panic.
        let binding = SemanticsBinding::new();
        binding.dispatch_event(&SemanticsEvent::tooltip("nobody home"));
    }
}
