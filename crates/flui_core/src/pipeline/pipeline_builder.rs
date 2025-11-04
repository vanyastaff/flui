//! Builder pattern for PipelineOwner configuration
//!
//! Provides a fluent API for constructing PipelineOwner with optional features.
//! This makes it clear which features are enabled and improves discoverability.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::pipeline::PipelineBuilder;
//! use std::time::Duration;
//!
//! // Production config
//! let owner = PipelineBuilder::new()
//!     .with_metrics()
//!     .with_error_recovery(RecoveryPolicy::UseLastGoodFrame)
//!     .with_batching(Duration::from_millis(16))
//!     .with_cancellation()
//!     .build();
//!
//! // Development config (minimal)
//! let dev_owner = PipelineBuilder::new()
//!     .with_error_recovery(RecoveryPolicy::ShowErrorWidget)
//!     .build();
//!
//! // Custom callback
//! let owner = PipelineBuilder::new()
//!     .with_build_callback(|| {
//!         println!("Build scheduled!");
//!     })
//!     .build();
//! ```

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

use super::{
    PipelineOwner, ElementTree, BuildPipeline, LayoutPipeline, PaintPipeline,
    PipelineMetrics, ErrorRecovery, RecoveryPolicy, CancellationToken,
    TripleBuffer,
};

/// Builder for PipelineOwner
///
/// Provides a fluent API for configuring PipelineOwner before construction.
/// All features are opt-in, making it clear what's enabled.
///
/// # Example
///
/// ```rust,ignore
/// let owner = PipelineBuilder::new()
///     .with_metrics()
///     .with_batching(Duration::from_millis(16))
///     .build();
/// ```
pub struct PipelineBuilder {
    /// Optional batching duration
    batching_duration: Option<Duration>,

    /// Optional metrics tracking
    enable_metrics: bool,

    /// Optional error recovery policy
    recovery_policy: Option<RecoveryPolicy>,

    /// Optional cancellation support
    enable_cancellation: bool,

    /// Optional frame buffer
    frame_buffer_initial: Option<Arc<crate::BoxedLayer>>,

    /// Optional build callback
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for PipelineBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineBuilder")
            .field("batching_duration", &self.batching_duration)
            .field("enable_metrics", &self.enable_metrics)
            .field("recovery_policy", &self.recovery_policy)
            .field("enable_cancellation", &self.enable_cancellation)
            .field("has_frame_buffer", &self.frame_buffer_initial.is_some())
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .finish()
    }
}

impl PipelineBuilder {
    /// Create a new builder with default configuration
    ///
    /// By default, all optional features are disabled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let builder = PipelineBuilder::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            batching_duration: None,
            enable_metrics: false,
            recovery_policy: None,
            enable_cancellation: false,
            frame_buffer_initial: None,
            on_build_scheduled: None,
        }
    }

    /// Enable performance metrics tracking
    ///
    /// Tracks FPS, frame times, phase timing, and cache hit rates.
    ///
    /// # Overhead
    ///
    /// - CPU: ~1%
    /// - Memory: 480 bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::new()
    ///     .with_metrics()
    ///     .build();
    /// ```
    #[must_use]
    pub fn with_metrics(mut self) -> Self {
        self.enable_metrics = true;
        self
    }

    /// Enable build batching with specified duration
    ///
    /// Batches multiple setState() calls within `duration` into a single rebuild.
    ///
    /// # Parameters
    ///
    /// - `duration`: Time window for batching (typically 16ms for 60fps)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// let owner = PipelineBuilder::new()
    ///     .with_batching(Duration::from_millis(16))
    ///     .build();
    /// ```
    #[must_use]
    pub fn with_batching(mut self, duration: Duration) -> Self {
        self.batching_duration = Some(duration);
        self
    }

    /// Enable error recovery with specified policy
    ///
    /// Defines how the pipeline handles errors during build/layout/paint.
    ///
    /// # Recovery Policies
    ///
    /// - `UseLastGoodFrame` - Production default, graceful degradation
    /// - `ShowErrorWidget` - Development mode, show error overlay
    /// - `SkipFrame` - Skip failed frame, continue with next
    /// - `Panic` - Testing mode, fail fast
    ///
    /// # Overhead
    ///
    /// - Memory: ~40 bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::pipeline::RecoveryPolicy;
    ///
    /// // Production config
    /// let owner = PipelineBuilder::new()
    ///     .with_error_recovery(RecoveryPolicy::UseLastGoodFrame)
    ///     .build();
    ///
    /// // Development config
    /// let dev = PipelineBuilder::new()
    ///     .with_error_recovery(RecoveryPolicy::ShowErrorWidget)
    ///     .build();
    /// ```
    #[must_use]
    pub fn with_error_recovery(mut self, policy: RecoveryPolicy) -> Self {
        self.recovery_policy = Some(policy);
        self
    }

    /// Enable cancellation support for timeouts
    ///
    /// Allows setting timeouts for long-running operations.
    ///
    /// # Overhead
    ///
    /// - Memory: ~24 bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::new()
    ///     .with_cancellation()
    ///     .build();
    ///
    /// // Later, set timeout
    /// if let Some(token) = owner.cancellation_token() {
    ///     token.set_timeout(Duration::from_millis(16));
    /// }
    /// ```
    #[must_use]
    pub fn with_cancellation(mut self) -> Self {
        self.enable_cancellation = true;
        self
    }

    /// Enable triple buffer for lock-free frame exchange
    ///
    /// Creates a TripleBuffer initialized with the provided layer.
    /// Allows compositor thread to read frames while renderer writes.
    ///
    /// # Parameters
    ///
    /// - `initial`: Initial layer for the frame buffer
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_engine::ContainerLayer;
    /// use std::sync::Arc;
    ///
    /// let initial = Arc::new(Box::new(ContainerLayer::new()) as crate::BoxedLayer);
    /// let owner = PipelineBuilder::new()
    ///     .with_frame_buffer(initial)
    ///     .build();
    /// ```
    #[must_use]
    pub fn with_frame_buffer(mut self, initial: Arc<crate::BoxedLayer>) -> Self {
        self.frame_buffer_initial = Some(initial);
        self
    }

    /// Set callback for when build is scheduled
    ///
    /// Called whenever `schedule_build_for()` is invoked.
    /// Useful for triggering frame rendering on demand.
    ///
    /// # Parameters
    ///
    /// - `callback`: Function to call when build is scheduled
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::new()
    ///     .with_build_callback(|| {
    ///         println!("Build scheduled - rendering frame!");
    ///     })
    ///     .build();
    /// ```
    #[must_use]
    pub fn with_build_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
        self
    }

    /// Build the PipelineOwner with configured options
    ///
    /// Consumes the builder and creates a PipelineOwner instance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::new()
    ///     .with_metrics()
    ///     .with_batching(Duration::from_millis(16))
    ///     .build();
    /// ```
    #[must_use]
    pub fn build(self) -> PipelineOwner {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        let mut build_pipeline = BuildPipeline::new();
        if let Some(duration) = self.batching_duration {
            build_pipeline.enable_batching(duration);
        }

        let mut owner = PipelineOwner {
            tree,
            build: build_pipeline,
            layout: LayoutPipeline::new(),
            paint: PaintPipeline::new(),
            root_element_id: None,
            on_build_scheduled: self.on_build_scheduled,
            metrics: if self.enable_metrics {
                Some(PipelineMetrics::new())
            } else {
                None
            },
            recovery: self.recovery_policy.map(ErrorRecovery::new),
            cancellation: if self.enable_cancellation {
                Some(CancellationToken::new())
            } else {
                None
            },
            frame_buffer: self.frame_buffer_initial.map(TripleBuffer::new),
        };

        owner
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Preset Configurations
// =============================================================================

impl PipelineBuilder {
    /// Production configuration preset
    ///
    /// Enables:
    /// - Metrics tracking
    /// - Error recovery (UseLastGoodFrame)
    /// - Build batching (16ms)
    /// - Cancellation support
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::production().build();
    /// ```
    #[must_use]
    pub fn production() -> Self {
        Self::new()
            .with_metrics()
            .with_error_recovery(RecoveryPolicy::UseLastGoodFrame)
            .with_batching(Duration::from_millis(16))
            .with_cancellation()
    }

    /// Development configuration preset
    ///
    /// Enables:
    /// - Error recovery (ShowErrorWidget)
    /// - Minimal overhead for fast iteration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::development().build();
    /// ```
    #[must_use]
    pub fn development() -> Self {
        Self::new()
            .with_error_recovery(RecoveryPolicy::ShowErrorWidget)
    }

    /// Testing configuration preset
    ///
    /// Enables:
    /// - Error recovery (Panic)
    /// - Fail fast for test failures
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::testing().build();
    /// ```
    #[must_use]
    pub fn testing() -> Self {
        Self::new()
            .with_error_recovery(RecoveryPolicy::Panic)
    }

    /// Minimal configuration preset
    ///
    /// No optional features enabled. Lowest overhead.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineBuilder::minimal().build();
    /// ```
    #[must_use]
    pub fn minimal() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_default() {
        let builder = PipelineBuilder::new();
        assert!(!builder.enable_metrics);
        assert!(builder.batching_duration.is_none());
        assert!(builder.recovery_policy.is_none());
        assert!(!builder.enable_cancellation);
        assert!(builder.frame_buffer_initial.is_none());
        assert!(builder.on_build_scheduled.is_none());
    }

    #[test]
    fn test_builder_with_metrics() {
        let builder = PipelineBuilder::new().with_metrics();
        assert!(builder.enable_metrics);
    }

    #[test]
    fn test_builder_with_batching() {
        let duration = Duration::from_millis(16);
        let builder = PipelineBuilder::new().with_batching(duration);
        assert_eq!(builder.batching_duration, Some(duration));
    }

    #[test]
    fn test_builder_with_error_recovery() {
        let builder = PipelineBuilder::new()
            .with_error_recovery(RecoveryPolicy::UseLastGoodFrame);
        assert_eq!(builder.recovery_policy, Some(RecoveryPolicy::UseLastGoodFrame));
    }

    #[test]
    fn test_builder_with_cancellation() {
        let builder = PipelineBuilder::new().with_cancellation();
        assert!(builder.enable_cancellation);
    }

    #[test]
    fn test_builder_build_minimal() {
        let owner = PipelineBuilder::new().build();
        assert!(owner.metrics().is_none());
        assert!(owner.error_recovery().is_none());
        assert!(owner.cancellation_token().is_none());
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_builder_build_with_features() {
        let owner = PipelineBuilder::new()
            .with_metrics()
            .with_batching(Duration::from_millis(16))
            .with_cancellation()
            .build();

        assert!(owner.metrics().is_some());
        assert!(owner.cancellation_token().is_some());
        assert!(owner.is_batching_enabled());
    }

    #[test]
    fn test_builder_production_preset() {
        let owner = PipelineBuilder::production().build();
        assert!(owner.metrics().is_some());
        assert!(owner.error_recovery().is_some());
        assert!(owner.cancellation_token().is_some());
        assert!(owner.is_batching_enabled());
    }

    #[test]
    fn test_builder_development_preset() {
        let owner = PipelineBuilder::development().build();
        assert!(owner.error_recovery().is_some());
        // Development preset has minimal overhead
        assert!(owner.metrics().is_none());
    }

    #[test]
    fn test_builder_testing_preset() {
        let owner = PipelineBuilder::testing().build();
        assert!(owner.error_recovery().is_some());
    }

    #[test]
    fn test_builder_minimal_preset() {
        let owner = PipelineBuilder::minimal().build();
        assert!(owner.metrics().is_none());
        assert!(owner.error_recovery().is_none());
        assert!(owner.cancellation_token().is_none());
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_builder_chaining() {
        let owner = PipelineBuilder::new()
            .with_metrics()
            .with_batching(Duration::from_millis(10))
            .with_error_recovery(RecoveryPolicy::SkipFrame)
            .with_cancellation()
            .build();

        assert!(owner.metrics().is_some());
        assert!(owner.error_recovery().is_some());
        assert!(owner.cancellation_token().is_some());
        assert!(owner.is_batching_enabled());
    }

    #[test]
    fn test_builder_callback() {
        use std::sync::{Arc, Mutex};

        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        let mut owner = PipelineBuilder::new()
            .with_build_callback(move || {
                *called_clone.lock().unwrap() = true;
            })
            .build();

        // Trigger callback
        owner.schedule_build_for(crate::ElementId::new(1), 0);

        assert!(*called.lock().unwrap());
    }
}
