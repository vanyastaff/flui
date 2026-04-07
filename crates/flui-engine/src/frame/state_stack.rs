//! Clip/transform/opacity state stack for nested rendering contexts.

use glam::Mat4;

use crate::frame::submission::ScissorRect;

// ---------------------------------------------------------------------------
// ClipRect
// ---------------------------------------------------------------------------

/// A clip rectangle in logical (unscaled) coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClipRect {
    /// Left edge in logical pixels.
    pub x: f32,
    /// Top edge in logical pixels.
    pub y: f32,
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
}

impl ClipRect {
    /// Returns the intersection of two clip rectangles, or `None` if they
    /// do not overlap.
    pub fn intersect(&self, other: &ClipRect) -> Option<ClipRect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);

        let w = x1 - x0;
        let h = y1 - y0;

        if w > 0.0 && h > 0.0 {
            Some(ClipRect {
                x: x0,
                y: y0,
                width: w,
                height: h,
            })
        } else {
            None
        }
    }

    /// Converts this logical clip rect to a physical-pixel [`ScissorRect`],
    /// clamped to the given viewport dimensions.
    pub fn to_scissor(&self, viewport_w: u32, viewport_h: u32, scale: f32) -> ScissorRect {
        let px = (self.x * scale).round().max(0.0) as u32;
        let py = (self.y * scale).round().max(0.0) as u32;
        let pw = (self.width * scale).round() as u32;
        let ph = (self.height * scale).round() as u32;

        let x = px.min(viewport_w);
        let y = py.min(viewport_h);
        let width = pw.min(viewport_w.saturating_sub(x));
        let height = ph.min(viewport_h.saturating_sub(y));

        ScissorRect {
            x,
            y,
            width,
            height,
        }
    }
}

// ---------------------------------------------------------------------------
// TransformStack
// ---------------------------------------------------------------------------

/// A stack of composed affine transforms.
pub struct TransformStack {
    stack: Vec<Mat4>,
}

impl TransformStack {
    /// Creates a new stack starting with the identity transform.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Pushes a new transform, composing it with the current top.
    pub fn push(&mut self, transform: Mat4) {
        let composed = self.current() * transform;
        self.stack.push(composed);
    }

    /// Pops the top transform. Does nothing if the stack is empty.
    pub fn pop(&mut self) {
        self.stack.pop();
    }

    /// Returns the current composed transform, or identity if the stack is
    /// empty.
    pub fn current(&self) -> Mat4 {
        self.stack.last().copied().unwrap_or(Mat4::IDENTITY)
    }

    /// Returns the current depth (number of pushed frames).
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Clears the stack back to identity.
    pub fn reset(&mut self) {
        self.stack.clear();
    }
}

impl Default for TransformStack {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ClipStack
// ---------------------------------------------------------------------------

/// A stack of intersected clip rectangles.
pub struct ClipStack {
    stack: Vec<ClipRect>,
}

impl ClipStack {
    /// Creates a new empty clip stack (no clipping).
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Pushes a clip rectangle, intersecting it with the current clip.
    ///
    /// If the intersection is empty the rect is still pushed (as a
    /// zero-area sentinel) so that `pop` remains balanced.
    pub fn push_rect(&mut self, rect: ClipRect) {
        let effective = match self.stack.last() {
            Some(current) => current.intersect(&rect).unwrap_or(ClipRect {
                x: rect.x,
                y: rect.y,
                width: 0.0,
                height: 0.0,
            }),
            None => rect,
        };
        self.stack.push(effective);
    }

    /// Pops the top clip rectangle. Does nothing if the stack is empty.
    pub fn pop(&mut self) {
        self.stack.pop();
    }

    /// Returns the current clip rectangle, or `None` if no clip is active.
    pub fn current_clip(&self) -> Option<ClipRect> {
        self.stack.last().copied()
    }

    /// Returns the current clip as a physical-pixel scissor rect, or `None`
    /// if no clip is active.
    pub fn current_scissor(
        &self,
        viewport_w: u32,
        viewport_h: u32,
        scale: f32,
    ) -> Option<ScissorRect> {
        self.stack
            .last()
            .map(|clip| clip.to_scissor(viewport_w, viewport_h, scale))
    }

    /// Clears the clip stack.
    pub fn reset(&mut self) {
        self.stack.clear();
    }
}

impl Default for ClipStack {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// OpacityStack
// ---------------------------------------------------------------------------

/// A stack of multiplied opacity values.
pub struct OpacityStack {
    stack: Vec<f32>,
}

impl OpacityStack {
    /// Creates a new stack starting at full opacity (1.0).
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Pushes an opacity value, multiplying it with the current opacity.
    pub fn push(&mut self, opacity: f32) {
        let composed = self.current() * opacity;
        self.stack.push(composed);
    }

    /// Pops the top opacity value. Does nothing if the stack is empty.
    pub fn pop(&mut self) {
        self.stack.pop();
    }

    /// Returns the current composed opacity, or 1.0 if the stack is empty.
    pub fn current(&self) -> f32 {
        self.stack.last().copied().unwrap_or(1.0)
    }

    /// Clears the opacity stack back to 1.0.
    pub fn reset(&mut self) {
        self.stack.clear();
    }
}

impl Default for OpacityStack {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// StateStack
// ---------------------------------------------------------------------------

/// Combined transform, clip, and opacity state for nested rendering contexts.
pub struct StateStack {
    /// The transform state.
    pub transform: TransformStack,
    /// The clip state.
    pub clip: ClipStack,
    /// The opacity state.
    pub opacity: OpacityStack,
    /// Current stencil nesting depth for non-rectangular clipping.
    pub stencil_depth: u32,
}

impl StateStack {
    /// Creates a new state stack with default (identity / no-clip / opaque)
    /// state.
    pub fn new() -> Self {
        Self {
            transform: TransformStack::new(),
            clip: ClipStack::new(),
            opacity: OpacityStack::new(),
            stencil_depth: 0,
        }
    }

    /// Resets all stacks to their initial state.
    pub fn reset(&mut self) {
        self.transform.reset();
        self.clip.reset();
        self.opacity.reset();
        self.stencil_depth = 0;
    }
}

impl Default for StateStack {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};

    #[test]
    fn transform_stack_starts_identity() {
        let stack = TransformStack::new();
        assert_eq!(stack.current(), Mat4::IDENTITY);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn transform_stack_push_pop() {
        let mut stack = TransformStack::new();
        let t = Mat4::from_translation(Vec3::new(10.0, 20.0, 0.0));

        stack.push(t);
        assert_eq!(stack.current(), t);
        assert_eq!(stack.depth(), 1);

        stack.pop();
        assert_eq!(stack.current(), Mat4::IDENTITY);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn transform_stack_composes() {
        let mut stack = TransformStack::new();
        let t1 = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0));
        let t2 = Mat4::from_translation(Vec3::new(0.0, 20.0, 0.0));

        stack.push(t1);
        stack.push(t2);

        assert_eq!(stack.current(), t1 * t2);
        assert_eq!(stack.depth(), 2);
    }

    #[test]
    fn clip_stack_starts_empty() {
        let stack = ClipStack::new();
        assert!(stack.current_scissor(800, 600, 1.0).is_none());
        assert!(stack.current_clip().is_none());
    }

    #[test]
    fn clip_stack_push_pop() {
        let mut stack = ClipStack::new();
        let rect = ClipRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };

        stack.push_rect(rect);
        let scissor = stack.current_scissor(800, 600, 1.0).unwrap();
        assert_eq!(
            scissor,
            ScissorRect {
                x: 10,
                y: 20,
                width: 100,
                height: 200,
            }
        );

        stack.pop();
        assert!(stack.current_scissor(800, 600, 1.0).is_none());
    }

    #[test]
    fn clip_stack_intersects() {
        let mut stack = ClipStack::new();

        stack.push_rect(ClipRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        });
        stack.push_rect(ClipRect {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
        });

        let clip = stack.current_clip().unwrap();
        assert_eq!(clip.x, 50.0);
        assert_eq!(clip.y, 50.0);
        assert_eq!(clip.width, 50.0);
        assert_eq!(clip.height, 50.0);
    }

    #[test]
    fn opacity_stack_starts_opaque() {
        let stack = OpacityStack::new();
        assert_eq!(stack.current(), 1.0);
    }

    #[test]
    fn opacity_stack_multiplies() {
        let mut stack = OpacityStack::new();

        stack.push(0.5);
        assert!((stack.current() - 0.5).abs() < f32::EPSILON);

        stack.push(0.5);
        assert!((stack.current() - 0.25).abs() < f32::EPSILON);

        stack.pop();
        assert!((stack.current() - 0.5).abs() < f32::EPSILON);
    }
}
