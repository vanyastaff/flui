//! Optional production features for PipelineOwner
//!
//! This module extracts optional features from PipelineOwner to improve
//! Single Responsibility Principle compliance. Features are opt-in and
//! can be enabled independently.
//!
//! # Available Features
//!
//! - **Metrics**: Performance monitoring and statistics
//! - **Recovery**: Error recovery policies and graceful degradation
//! - **Cancellation**: Frame timeout and cancellation support
//! - **FrameBuffer**: Triple-buffered frame exchange for lock-free rendering
//! - **HitTestCache**: Caches hit test results for performance (~5-15% CPU savings)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::pipeline::{PipelineOwner, PipelineFeatures};
//!
//! let mut owner = PipelineOwner::new();
//! let mut features = PipelineFeatures::new();
//!
//! // Enable metrics
//! features.enable_metrics();
//!
//! // Enable hit test cache
//! features.enable_hit_test_cache();
//!
//! // Attach to pipeline
//! owner.set_features(features);
//! ```

use super::{CancellationToken, ErrorRecovery, HitTestCache, PipelineMetrics, TripleBuffer};
use flui_engine::CanvasLayer;
use std::sync::Arc;

/// Optional production features for PipelineOwner
///
/// # Design Rationale
///
/// This structure separates optional production features from core pipeline
/// functionality, following the Single Responsibility Principle. Each feature
/// can be enabled/disabled independently without modifying PipelineOwner.
///
/// # Thread-Safety
///
/// All features are designed for single-threaded access (pipeline runs on main thread).
/// Thread-safe coordination happens at a higher level (via `Arc<RwLock<PipelineOwner>>`).
#[derive(Default)]
pub struct PipelineFeatures {
    /// Performance metrics (optional)
    ///
    /// Tracks frame times, rebuild counts, layout/paint statistics.
    /// Overhead: ~1-2% CPU per frame when enabled.
    metrics: Option<PipelineMetrics>,

    /// Error recovery policy (optional)
    ///
    /// Handles build/layout/paint errors with configurable recovery strategies:
    /// - Skip frame (default)
    /// - Retry once
    /// - Fallback widget
    recovery: Option<ErrorRecovery>,

    /// Cancellation token (optional)
    ///
    /// Allows cancelling long-running frames (build/layout/paint) when deadline exceeded.
    /// Useful for maintaining 60 FPS under heavy load.
    cancellation: Option<CancellationToken>,

    /// Triple buffer for lock-free frame exchange (optional)
    ///
    /// Enables producer (pipeline) and consumer (renderer) to exchange frames
    /// without locks. Useful for multi-threaded rendering.
    #[allow(clippy::redundant_allocation)]
    frame_buffer: Option<TripleBuffer<Arc<Box<CanvasLayer>>>>,

    /// Hit test result cache (optional)
    ///
    /// Caches hit test results when tree is unchanged. Provides ~5-15% CPU savings
    /// on pointer events by avoiding redundant tree traversals.
    hit_test_cache: Option<HitTestCache>,
}

impl PipelineFeatures {
    /// Create new features manager with all features disabled
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let features = PipelineFeatures::new();
    /// assert!(!features.has_metrics());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // Metrics Feature
    // =========================================================================

    /// Enable performance metrics
    ///
    /// Metrics track:
    /// - Frame times (build, layout, paint)
    /// - Rebuild counts per frame
    /// - Average FPS
    /// - P95/P99 latencies
    ///
    /// # Overhead
    ///
    /// ~1-2% CPU per frame when enabled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// features.enable_metrics();
    /// // ... run frames ...
    /// if let Some(metrics) = features.metrics() {
    ///     println!("Avg frame time: {:?}", metrics.avg_frame_time());
    /// }
    /// ```
    pub fn enable_metrics(&mut self) {
        self.metrics = Some(PipelineMetrics::new());
    }

    /// Disable performance metrics
    pub fn disable_metrics(&mut self) {
        self.metrics = None;
    }

    /// Check if metrics are enabled
    pub fn has_metrics(&self) -> bool {
        self.metrics.is_some()
    }

    /// Get reference to metrics (if enabled)
    pub fn metrics(&self) -> Option<&PipelineMetrics> {
        self.metrics.as_ref()
    }

    /// Get mutable reference to metrics (if enabled)
    pub fn metrics_mut(&mut self) -> Option<&mut PipelineMetrics> {
        self.metrics.as_mut()
    }

    // =========================================================================
    // Recovery Feature
    // =========================================================================

    /// Enable error recovery with default policy (skip frame)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// features.enable_recovery();
    /// ```
    pub fn enable_recovery(&mut self) {
        self.recovery = Some(ErrorRecovery::new(super::RecoveryPolicy::SkipFrame));
    }

    /// Enable error recovery with custom policy
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::pipeline::RecoveryPolicy;
    ///
    /// features.enable_recovery_with_policy(RecoveryPolicy::RetryOnce);
    /// ```
    pub fn enable_recovery_with_policy(&mut self, policy: super::RecoveryPolicy) {
        self.recovery = Some(ErrorRecovery::new(policy));
    }

    /// Disable error recovery
    pub fn disable_recovery(&mut self) {
        self.recovery = None;
    }

    /// Check if error recovery is enabled
    pub fn has_recovery(&self) -> bool {
        self.recovery.is_some()
    }

    /// Get reference to error recovery (if enabled)
    pub fn recovery(&self) -> Option<&ErrorRecovery> {
        self.recovery.as_ref()
    }

    /// Get mutable reference to error recovery (if enabled)
    pub fn recovery_mut(&mut self) -> Option<&mut ErrorRecovery> {
        self.recovery.as_mut()
    }

    // =========================================================================
    // Cancellation Feature
    // =========================================================================

    /// Enable frame cancellation with timeout
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// features.enable_cancellation(Duration::from_millis(16)); // 60 FPS budget
    /// ```
    pub fn enable_cancellation(&mut self, timeout: std::time::Duration) {
        let token = CancellationToken::new();
        token.set_timeout(timeout);
        self.cancellation = Some(token);
    }

    /// Disable frame cancellation
    pub fn disable_cancellation(&mut self) {
        self.cancellation = None;
    }

    /// Check if cancellation is enabled
    pub fn has_cancellation(&self) -> bool {
        self.cancellation.is_some()
    }

    /// Get reference to cancellation token (if enabled)
    pub fn cancellation(&self) -> Option<&CancellationToken> {
        self.cancellation.as_ref()
    }

    /// Get mutable reference to cancellation token (if enabled)
    pub fn cancellation_mut(&mut self) -> Option<&mut CancellationToken> {
        self.cancellation.as_mut()
    }

    // =========================================================================
    // FrameBuffer Feature
    // =========================================================================

    /// Enable triple buffering for lock-free frame exchange
    ///
    /// # Use Case
    ///
    /// Triple buffering allows the pipeline (producer) and renderer (consumer)
    /// to exchange frames without blocking. The producer writes to a back buffer
    /// while the consumer reads from a front buffer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// features.enable_frame_buffer();
    ///
    /// // Producer (pipeline)
    /// if let Some(buffer) = features.frame_buffer_mut() {
    ///     buffer.write(Arc::new(Box::new(layer)));
    /// }
    ///
    /// // Consumer (renderer)
    /// if let Some(buffer) = features.frame_buffer() {
    ///     if let Some(layer) = buffer.read() {
    ///         gpu_renderer.render(layer);
    ///     }
    /// }
    /// ```
    pub fn enable_frame_buffer(&mut self) {
        // Create empty initial layer for triple buffer
        let empty_layer = Arc::new(Box::new(CanvasLayer::new()));
        self.frame_buffer = Some(TripleBuffer::new(empty_layer));
    }

    /// Disable triple buffering
    pub fn disable_frame_buffer(&mut self) {
        self.frame_buffer = None;
    }

    /// Check if frame buffer is enabled
    pub fn has_frame_buffer(&self) -> bool {
        self.frame_buffer.is_some()
    }

    /// Get reference to frame buffer (if enabled)
    pub fn frame_buffer(&self) -> Option<&TripleBuffer<Arc<Box<CanvasLayer>>>> {
        self.frame_buffer.as_ref()
    }

    /// Get mutable reference to frame buffer (if enabled)
    pub fn frame_buffer_mut(&mut self) -> Option<&mut TripleBuffer<Arc<Box<CanvasLayer>>>> {
        self.frame_buffer.as_mut()
    }

    // =========================================================================
    // HitTestCache Feature
    // =========================================================================

    /// Enable hit test result caching
    ///
    /// # Performance
    ///
    /// Provides ~5-15% CPU savings on pointer events by caching hit test results
    /// when the element tree is unchanged.
    ///
    /// # Invalidation
    ///
    /// Cache is automatically invalidated when layout or paint occurs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// features.enable_hit_test_cache();
    /// ```
    pub fn enable_hit_test_cache(&mut self) {
        self.hit_test_cache = Some(HitTestCache::new());
    }

    /// Disable hit test caching
    pub fn disable_hit_test_cache(&mut self) {
        self.hit_test_cache = None;
    }

    /// Check if hit test cache is enabled
    pub fn has_hit_test_cache(&self) -> bool {
        self.hit_test_cache.is_some()
    }

    /// Get reference to hit test cache (if enabled)
    pub fn hit_test_cache(&self) -> Option<&HitTestCache> {
        self.hit_test_cache.as_ref()
    }

    /// Get mutable reference to hit test cache (if enabled)
    pub fn hit_test_cache_mut(&mut self) -> Option<&mut HitTestCache> {
        self.hit_test_cache.as_mut()
    }

    /// Invalidate hit test cache (if enabled)
    ///
    /// Called automatically after layout/paint to ensure cache consistency.
    pub fn invalidate_hit_test_cache(&mut self) {
        if let Some(cache) = &mut self.hit_test_cache {
            cache.invalidate();
        }
    }
}

impl std::fmt::Debug for PipelineFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineFeatures")
            .field("has_metrics", &self.has_metrics())
            .field("has_recovery", &self.has_recovery())
            .field("has_cancellation", &self.has_cancellation())
            .field("has_frame_buffer", &self.has_frame_buffer())
            .field("has_hit_test_cache", &self.has_hit_test_cache())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_default() {
        let features = PipelineFeatures::new();
        assert!(!features.has_metrics());
        assert!(!features.has_recovery());
        assert!(!features.has_cancellation());
        assert!(!features.has_frame_buffer());
        assert!(!features.has_hit_test_cache());
    }

    #[test]
    fn test_enable_disable_metrics() {
        let mut features = PipelineFeatures::new();

        features.enable_metrics();
        assert!(features.has_metrics());
        assert!(features.metrics().is_some());

        features.disable_metrics();
        assert!(!features.has_metrics());
        assert!(features.metrics().is_none());
    }

    #[test]
    fn test_enable_disable_recovery() {
        let mut features = PipelineFeatures::new();

        features.enable_recovery();
        assert!(features.has_recovery());

        features.disable_recovery();
        assert!(!features.has_recovery());
    }

    #[test]
    fn test_enable_disable_hit_test_cache() {
        let mut features = PipelineFeatures::new();

        features.enable_hit_test_cache();
        assert!(features.has_hit_test_cache());

        features.disable_hit_test_cache();
        assert!(!features.has_hit_test_cache());
    }

    #[test]
    fn test_invalidate_hit_test_cache() {
        let mut features = PipelineFeatures::new();
        features.enable_hit_test_cache();

        // Should not panic even if cache enabled
        features.invalidate_hit_test_cache();

        features.disable_hit_test_cache();

        // Should not panic even if cache disabled
        features.invalidate_hit_test_cache();
    }
}
