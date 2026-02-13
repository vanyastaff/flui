//! Frame rendering coordination
//!
//! Orchestrates the frame rendering pipeline and handles
//! surface errors gracefully.

use flui_engine::wgpu::Renderer;
use flui_engine::RenderError;
use flui_layer::Scene;

/// Frame rendering result
#[derive(Debug)]
pub enum FrameResult {
    /// Frame rendered successfully
    Success,
    /// Surface lost, will retry next frame
    SurfaceLost,
    /// Surface outdated, will retry next frame
    SurfaceOutdated,
    /// No content to render
    Empty,
    /// Render error occurred
    Error(String),
}

impl FrameResult {
    /// Check if frame was successful or empty (both OK)
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Success | Self::Empty)
    }

    /// Check if frame should be retried
    pub fn should_retry(&self) -> bool {
        matches!(self, Self::SurfaceLost | Self::SurfaceOutdated)
    }
}

/// Frame rendering coordinator
///
/// Orchestrates the rendering pipeline and handles surface errors.
///
/// # Responsibilities
///
/// - Execute render pass on GPU
/// - Handle surface lost/outdated errors
/// - Track frame statistics
#[derive(Debug, Default)]
pub struct FrameCoordinator {
    /// Total frames rendered
    frames_rendered: u64,

    /// Frames dropped (surface errors)
    frames_dropped: u64,
}

impl FrameCoordinator {
    /// Create a new frame coordinator
    pub fn new() -> Self {
        Self {
            frames_rendered: 0,
            frames_dropped: 0,
        }
    }

    /// Render a scene to the GPU
    ///
    /// Handles surface errors gracefully and tracks statistics.
    /// Uses `render_scene` to traverse the full layer tree.
    #[tracing::instrument(level = "trace", skip_all, fields(frame = scene.frame_number()))]
    pub fn render_scene(&mut self, renderer: &mut Renderer, scene: &Scene) -> FrameResult {
        tracing::debug!(
            "FrameCoordinator::render_scene called, has_content={}",
            scene.has_content()
        );
        if !scene.has_content() {
            tracing::debug!("Empty scene, skipping render");
            return FrameResult::Empty;
        }

        tracing::debug!("FrameCoordinator: calling renderer.render_scene");
        // Use render_scene to traverse the full layer tree (not just root layer)
        match renderer.render_scene(scene) {
            Ok(()) => {
                self.frames_rendered += 1;
                tracing::trace!(
                    frame = scene.frame_number(),
                    total = self.frames_rendered,
                    "Frame rendered successfully"
                );
                FrameResult::Success
            }
            Err(RenderError::SurfaceLost) => {
                self.frames_dropped += 1;
                tracing::debug!("Surface lost, will retry next frame");
                FrameResult::SurfaceLost
            }
            Err(RenderError::SurfaceOutdated) => {
                self.frames_dropped += 1;
                tracing::debug!("Surface outdated, will retry next frame");
                FrameResult::SurfaceOutdated
            }
            Err(e) => {
                self.frames_dropped += 1;
                tracing::error!("Render error: {:?}", e);
                FrameResult::Error(format!("{:?}", e))
            }
        }
    }

    /// Get total frames rendered
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered
    }

    /// Get frames dropped due to errors
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped
    }

    /// Get frame success rate (0.0 - 1.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.frames_rendered + self.frames_dropped;
        if total == 0 {
            1.0
        } else {
            self.frames_rendered as f64 / total as f64
        }
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.frames_rendered = 0;
        self.frames_dropped = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_coordinator_new() {
        let coord = FrameCoordinator::new();
        assert_eq!(coord.frames_rendered(), 0);
        assert_eq!(coord.frames_dropped(), 0);
        assert_eq!(coord.success_rate(), 1.0);
    }

    #[test]
    fn test_frame_result_is_ok() {
        assert!(FrameResult::Success.is_ok());
        assert!(FrameResult::Empty.is_ok());
        assert!(!FrameResult::SurfaceLost.is_ok());
        assert!(!FrameResult::Error("test".to_string()).is_ok());
    }

    #[test]
    fn test_frame_result_should_retry() {
        assert!(FrameResult::SurfaceLost.should_retry());
        assert!(FrameResult::SurfaceOutdated.should_retry());
        assert!(!FrameResult::Success.should_retry());
        assert!(!FrameResult::Error("test".to_string()).should_retry());
    }
}
