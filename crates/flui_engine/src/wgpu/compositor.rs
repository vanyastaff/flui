//! Layer compositor for scene rendering
//!
//! This module handles compositing of layers with transforms, opacity, and blend modes.
//! Manages the transform stack and applies layer effects during rendering.

use super::scene::{BlendMode, LayerBatch};
use glam::Mat4;

/// Transform stack for hierarchical rendering
///
/// Maintains a stack of transformation matrices for nested layers.
/// Each layer can apply its own transform, which is composed with parent transforms.
#[derive(Debug, Clone)]
pub struct TransformStack {
    /// Stack of transformation matrices
    stack: Vec<Mat4>,
}

impl TransformStack {
    /// Create a new transform stack with identity matrix
    #[must_use]
    pub fn new() -> Self {
        Self {
            stack: vec![Mat4::IDENTITY],
        }
    }

    /// Push a new transform onto the stack
    ///
    /// The new transform is composed with the current transform.
    pub fn push(&mut self, transform: Mat4) {
        let current = self.current();
        self.stack.push(current * transform);
    }

    /// Pop the last transform from the stack
    ///
    /// # Panics
    ///
    /// Panics if trying to pop the last (identity) transform
    pub fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        } else {
            panic!("Cannot pop identity transform from stack");
        }
    }

    /// Get the current composed transform
    #[must_use]
    pub fn current(&self) -> Mat4 {
        *self.stack.last().unwrap_or(&Mat4::IDENTITY)
    }

    /// Get stack depth
    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Reset to identity transform
    pub fn reset(&mut self) {
        self.stack.clear();
        self.stack.push(Mat4::IDENTITY);
    }
}

impl Default for TransformStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Layer compositor
///
/// Manages layer composition with transforms, opacity, and blend modes.
/// Handles the rendering order and state management for complex scenes.
pub struct Compositor {
    /// Transform stack
    transform_stack: TransformStack,

    /// Current opacity stack (for nested layers)
    opacity_stack: Vec<f32>,

    /// Current blend mode stack
    blend_stack: Vec<BlendMode>,
}

impl Compositor {
    /// Create a new compositor
    #[must_use]
    pub fn new() -> Self {
        Self {
            transform_stack: TransformStack::new(),
            opacity_stack: vec![1.0],
            blend_stack: vec![BlendMode::Normal],
        }
    }

    /// Begin compositing a layer batch
    ///
    /// Pushes layer's transform, opacity, and blend mode onto stacks.
    pub fn begin_layer(&mut self, batch: &LayerBatch) {
        // Push transform
        self.transform_stack.push(batch.transform);

        // Push opacity (compose with parent)
        let parent_opacity = self.current_opacity();
        self.opacity_stack.push(parent_opacity * batch.opacity);

        // Push blend mode
        self.blend_stack.push(batch.blend_mode);
    }

    /// End compositing a layer batch
    ///
    /// Pops layer's state from stacks.
    pub fn end_layer(&mut self) {
        self.transform_stack.pop();

        if self.opacity_stack.len() > 1 {
            self.opacity_stack.pop();
        }

        if self.blend_stack.len() > 1 {
            self.blend_stack.pop();
        }
    }

    /// Get current composed transform
    #[must_use]
    pub fn current_transform(&self) -> Mat4 {
        self.transform_stack.current()
    }

    /// Get current composed opacity
    #[must_use]
    pub fn current_opacity(&self) -> f32 {
        *self.opacity_stack.last().unwrap_or(&1.0)
    }

    /// Get current blend mode
    #[must_use]
    pub fn current_blend_mode(&self) -> BlendMode {
        *self.blend_stack.last().unwrap_or(&BlendMode::Normal)
    }

    /// Reset compositor to initial state
    pub fn reset(&mut self) {
        self.transform_stack.reset();
        self.opacity_stack.clear();
        self.opacity_stack.push(1.0);
        self.blend_stack.clear();
        self.blend_stack.push(BlendMode::Normal);
    }

    /// Get transform stack depth
    #[must_use]
    pub fn depth(&self) -> usize {
        self.transform_stack.depth()
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

/// Render context for a single frame
///
/// Combines compositor state with rendering resources.
/// Passed to rendering functions to track state during frame rendering.
pub struct RenderContext {
    /// Compositor for state management
    pub compositor: Compositor,

    /// Frame number (for debugging)
    pub frame_number: u64,

    /// Viewport width
    pub viewport_width: u32,

    /// Viewport height
    pub viewport_height: u32,
}

impl RenderContext {
    /// Create a new render context
    #[must_use]
    pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
        Self {
            compositor: Compositor::new(),
            frame_number: 0,
            viewport_width,
            viewport_height,
        }
    }

    /// Begin a new frame
    pub fn begin_frame(&mut self) {
        self.compositor.reset();
        self.frame_number += 1;
    }

    /// Get aspect ratio
    #[must_use]
    pub fn aspect_ratio(&self) -> f32 {
        self.viewport_width as f32 / self.viewport_height as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_stack_creation() {
        let stack = TransformStack::new();
        assert_eq!(stack.current(), Mat4::IDENTITY);
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_transform_stack_push_pop() {
        let mut stack = TransformStack::new();
        let transform = Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0));

        stack.push(transform);
        assert_eq!(stack.depth(), 2);

        stack.pop();
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.current(), Mat4::IDENTITY);
    }

    #[test]
    fn test_transform_stack_composition() {
        let mut stack = TransformStack::new();
        let translate1 = Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0));
        let translate2 = Mat4::from_translation(glam::Vec3::new(5.0, 0.0, 0.0));

        stack.push(translate1);
        stack.push(translate2);

        let composed = stack.current();
        let expected = translate1 * translate2;
        assert_eq!(composed, expected);
    }

    #[test]
    fn test_transform_stack_reset() {
        let mut stack = TransformStack::new();
        stack.push(Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0)));
        stack.push(Mat4::from_translation(glam::Vec3::new(5.0, 10.0, 0.0)));

        assert_eq!(stack.depth(), 3);

        stack.reset();
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.current(), Mat4::IDENTITY);
    }

    #[test]
    fn test_compositor_creation() {
        let compositor = Compositor::new();
        assert_eq!(compositor.current_opacity(), 1.0);
        assert_eq!(compositor.current_blend_mode(), BlendMode::Normal);
        assert_eq!(compositor.current_transform(), Mat4::IDENTITY);
    }

    #[test]
    fn test_compositor_opacity_stacking() {
        let mut compositor = Compositor::new();

        let batch1 = LayerBatch {
            primitives: super::super::scene::PrimitiveBatch {
                primitive_type: super::super::scene::PrimitiveType::Rect,
                primitives: vec![],
                texture_id: None,
            },
            transform: Mat4::IDENTITY,
            opacity: 0.8,
            blend_mode: BlendMode::Normal,
            clip: None,
        };

        compositor.begin_layer(&batch1);
        assert_eq!(compositor.current_opacity(), 0.8);

        let batch2 = LayerBatch {
            primitives: super::super::scene::PrimitiveBatch {
                primitive_type: super::super::scene::PrimitiveType::Rect,
                primitives: vec![],
                texture_id: None,
            },
            transform: Mat4::IDENTITY,
            opacity: 0.5,
            blend_mode: BlendMode::Normal,
            clip: None,
        };

        compositor.begin_layer(&batch2);
        assert_eq!(compositor.current_opacity(), 0.4); // 0.8 * 0.5

        compositor.end_layer();
        assert_eq!(compositor.current_opacity(), 0.8);

        compositor.end_layer();
        assert_eq!(compositor.current_opacity(), 1.0);
    }

    #[test]
    fn test_render_context_creation() {
        let context = RenderContext::new(1920, 1080);
        assert_eq!(context.viewport_width, 1920);
        assert_eq!(context.viewport_height, 1080);
        assert_eq!(context.frame_number, 0);
    }

    #[test]
    fn test_render_context_begin_frame() {
        let mut context = RenderContext::new(800, 600);

        context.begin_frame();
        assert_eq!(context.frame_number, 1);

        context.begin_frame();
        assert_eq!(context.frame_number, 2);
    }

    #[test]
    fn test_render_context_aspect_ratio() {
        let context = RenderContext::new(1920, 1080);
        assert!((context.aspect_ratio() - 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn test_compositor_reset() {
        let mut compositor = Compositor::new();

        let batch = LayerBatch {
            primitives: super::super::scene::PrimitiveBatch {
                primitive_type: super::super::scene::PrimitiveType::Rect,
                primitives: vec![],
                texture_id: None,
            },
            transform: Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0)),
            opacity: 0.5,
            blend_mode: BlendMode::Multiply,
            clip: None,
        };

        compositor.begin_layer(&batch);
        assert_ne!(compositor.current_transform(), Mat4::IDENTITY);

        compositor.reset();
        assert_eq!(compositor.current_transform(), Mat4::IDENTITY);
        assert_eq!(compositor.current_opacity(), 1.0);
        assert_eq!(compositor.current_blend_mode(), BlendMode::Normal);
    }
}
