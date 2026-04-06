//! Debug encoder that traces all dispatched commands without GPU rendering.
//!
//! [`DebugEncoder`] is useful for testing and debugging the command pipeline.
//! It processes a [`Scene`] through the same dispatch path as the real
//! [`FrameEncoder`](crate::frame::encoder::FrameEncoder), but never touches
//! a GPU device — it simply counts the resulting draw primitives.

use flui_layer::Scene;

use crate::frame::dispatch::{traverse_scene, Batchers};
use crate::frame::state_stack::StateStack;

/// Debug encoder that traces all dispatched commands without GPU rendering.
/// Useful for testing and debugging the command pipeline.
pub struct DebugEncoder {
    batchers: Batchers,
    state: StateStack,
    command_count: usize,
}

impl DebugEncoder {
    /// Creates a new, empty debug encoder.
    pub fn new() -> Self {
        Self {
            batchers: Batchers::new(),
            state: StateStack::new(),
            command_count: 0,
        }
    }

    /// Process a scene and count all dispatched commands.
    pub fn process_scene(&mut self, scene: &Scene) {
        let _span = tracing::debug_span!("debug_process_scene").entered();
        traverse_scene(scene, &mut self.batchers, &mut self.state, 1.0);
        self.command_count = self.count_commands();
    }

    /// Total number of recorded draw primitives across all batchers.
    pub fn command_count(&self) -> usize {
        self.command_count
    }

    /// Number of rectangle draw commands.
    pub fn rect_count(&self) -> usize {
        self.batchers.shapes.rect_count()
    }

    /// Number of circle draw commands.
    pub fn circle_count(&self) -> usize {
        self.batchers.shapes.circle_count()
    }

    /// Number of text run draw commands.
    pub fn text_run_count(&self) -> usize {
        self.batchers.text.run_count()
    }

    /// Number of tessellated path draw ranges.
    pub fn path_draw_count(&self) -> usize {
        self.batchers.paths.draw_range_count()
    }

    /// Number of image groups.
    pub fn image_group_count(&self) -> usize {
        self.batchers.images.group_count()
    }

    /// Number of effect commands (gradients + shadows).
    pub fn effect_count(&self) -> usize {
        self.batchers.effects.linear_gradient_count()
            + self.batchers.effects.radial_gradient_count()
            + self.batchers.effects.shadow_count()
    }

    /// Number of compositing operations.
    pub fn compositing_op_count(&self) -> usize {
        self.batchers.compositing.op_count()
    }

    /// Reset for next frame, clearing all batchers and state.
    pub fn reset(&mut self) {
        self.batchers.clear_all();
        self.state.reset();
        self.command_count = 0;
    }

    fn count_commands(&self) -> usize {
        self.batchers.shapes.rect_count()
            + self.batchers.shapes.circle_count()
            + self.batchers.shapes.arc_count()
            + self.batchers.shapes.line_count()
            + self.batchers.paths.draw_range_count()
            + self.batchers.text.run_count()
            + self.batchers.images.total_instance_count()
            + self.batchers.effects.linear_gradient_count()
            + self.batchers.effects.radial_gradient_count()
            + self.batchers.effects.shadow_count()
            + self.batchers.effects.blur_count()
            + self.batchers.compositing.op_count()
    }
}

impl Default for DebugEncoder {
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

    #[test]
    fn debug_encoder_starts_empty() {
        let encoder = DebugEncoder::new();
        assert_eq!(encoder.command_count(), 0);
        assert_eq!(encoder.rect_count(), 0);
    }

    #[test]
    fn debug_encoder_reset() {
        let mut encoder = DebugEncoder::new();
        // Manually add some items to batchers to verify reset works
        encoder.batchers.shapes.add_rect(
            0.0,
            0.0,
            10.0,
            10.0,
            [1.0; 4],
            [0.0; 4],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!(encoder.batchers.shapes.rect_count() > 0);
        encoder.reset();
        assert_eq!(encoder.rect_count(), 0);
        assert_eq!(encoder.command_count(), 0);
    }

    #[test]
    fn debug_encoder_processes_empty_scene() {
        use flui_types::geometry::units::px;
        use flui_types::geometry::Size;

        let scene = Scene::empty(Size::new(px(800.0), px(600.0)));
        let mut encoder = DebugEncoder::new();
        encoder.process_scene(&scene);
        assert_eq!(encoder.command_count(), 0);
    }
}
