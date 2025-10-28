//! Scene builder for constructing layer trees
//!
//! SceneBuilder provides a stack-based API for building complex scene graphs
//! incrementally. This matches Flutter's SceneBuilder pattern.

use crate::layer::{
    Layer, BoxedLayer, ContainerLayer, TransformLayer, OpacityLayer, ClipLayer,
};
use crate::scene::Scene;
use crate::painter::RRect;
use flui_types::{Size, Rect, Offset};

/// A layer entry on the scene builder stack
enum StackEntry {
    /// A transform layer being built
    Transform {
        offset: Option<Offset>,
        rotation: Option<f32>,
        scale: Option<(f32, f32)>,
        children: Vec<BoxedLayer>,
    },
    /// An opacity layer being built
    Opacity {
        opacity: f32,
        children: Vec<BoxedLayer>,
    },
    /// A clip layer being built
    ClipRect {
        rect: Rect,
        children: Vec<BoxedLayer>,
    },
    /// A rounded clip layer being built
    ClipRRect {
        rrect: RRect,
        children: Vec<BoxedLayer>,
    },
}

impl StackEntry {
    fn children_mut(&mut self) -> &mut Vec<BoxedLayer> {
        match self {
            StackEntry::Transform { children, .. } => children,
            StackEntry::Opacity { children, .. } => children,
            StackEntry::ClipRect { children, .. } => children,
            StackEntry::ClipRRect { children, .. } => children,
        }
    }

    fn into_layer(self) -> BoxedLayer {
        match self {
            StackEntry::Transform { offset, rotation, scale, children } => {
                let mut container = ContainerLayer::new();
                for child in children {
                    container.add_child(child);
                }

                let boxed_container: BoxedLayer = Box::new(container);

                if let Some(off) = offset {
                    Box::new(TransformLayer::translate(boxed_container, off))
                } else if let Some(rot) = rotation {
                    Box::new(TransformLayer::rotate(boxed_container, rot))
                } else if let Some((sx, sy)) = scale {
                    Box::new(TransformLayer::scale_xy(boxed_container, sx, sy))
                } else {
                    boxed_container
                }
            }
            StackEntry::Opacity { opacity, children } => {
                let mut container = ContainerLayer::new();
                for child in children {
                    container.add_child(child);
                }
                Box::new(OpacityLayer::new(Box::new(container), opacity))
            }
            StackEntry::ClipRect { rect, children } => {
                let mut container = ContainerLayer::new();
                for child in children {
                    container.add_child(child);
                }
                Box::new(ClipLayer::rect(Box::new(container), rect))
            }
            StackEntry::ClipRRect { rrect, children } => {
                let mut container = ContainerLayer::new();
                for child in children {
                    container.add_child(child);
                }
                Box::new(ClipLayer::rrect(Box::new(container), rrect))
            }
        }
    }
}

/// Builder for constructing scenes incrementally
///
/// SceneBuilder provides a stack-based API for building complex
/// layer trees. This matches Flutter's SceneBuilder pattern.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::{SceneBuilder, layer::PictureLayer, painter::Paint};
/// use flui_types::{Size, Rect};
///
/// let mut builder = SceneBuilder::new();
///
/// // Push a transform
/// builder.push_offset(Offset::new(10.0, 20.0));
///
/// // Push opacity
/// builder.push_opacity(0.8);
///
/// // Add a picture layer
/// let mut picture = PictureLayer::new();
/// picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
/// builder.add_picture(picture);
///
/// builder.pop(); // Pop opacity
/// builder.pop(); // Pop offset
///
/// let scene = builder.build(Size::new(800.0, 600.0));
/// ```
pub struct SceneBuilder {
    /// Stack of layer entries being built
    layer_stack: Vec<StackEntry>,

    /// Root layer (base of the tree)
    root: ContainerLayer,
}

impl SceneBuilder {
    /// Create a new scene builder
    pub fn new() -> Self {
        Self {
            layer_stack: Vec::new(),
            root: ContainerLayer::new(),
        }
    }

    /// Push a transform layer with an offset onto the stack
    ///
    /// All subsequent layers will be translated by this offset
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `offset` - The translation offset to apply
    pub fn push_offset(&mut self, offset: Offset) {
        self.layer_stack.push(StackEntry::Transform {
            offset: Some(offset),
            rotation: None,
            scale: None,
            children: Vec::new(),
        });
    }

    /// Push a transform layer with rotation onto the stack
    ///
    /// All subsequent layers will be rotated by this angle
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `radians` - Rotation angle in radians
    pub fn push_rotation(&mut self, radians: f32) {
        self.layer_stack.push(StackEntry::Transform {
            offset: None,
            rotation: Some(radians),
            scale: None,
            children: Vec::new(),
        });
    }

    /// Push a transform layer with scale onto the stack
    ///
    /// All subsequent layers will be scaled by these factors
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `sx` - Horizontal scale factor
    /// * `sy` - Vertical scale factor
    pub fn push_scale(&mut self, sx: f32, sy: f32) {
        self.layer_stack.push(StackEntry::Transform {
            offset: None,
            rotation: None,
            scale: Some((sx, sy)),
            children: Vec::new(),
        });
    }

    /// Push a clip rect layer onto the stack
    ///
    /// All subsequent layers will be clipped to this rectangle
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `rect` - The clipping rectangle
    pub fn push_clip_rect(&mut self, rect: Rect) {
        self.layer_stack.push(StackEntry::ClipRect {
            rect,
            children: Vec::new(),
        });
    }

    /// Push a clip rounded rect layer onto the stack
    ///
    /// All subsequent layers will be clipped to this rounded rectangle
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `rrect` - The clipping rounded rectangle
    pub fn push_clip_rrect(&mut self, rrect: RRect) {
        self.layer_stack.push(StackEntry::ClipRRect {
            rrect,
            children: Vec::new(),
        });
    }

    /// Push an opacity layer onto the stack
    ///
    /// All subsequent layers will have their opacity multiplied by this value
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque)
    pub fn push_opacity(&mut self, opacity: f32) {
        self.layer_stack.push(StackEntry::Opacity {
            opacity,
            children: Vec::new(),
        });
    }

    /// Add a layer to the current container
    ///
    /// The layer is added as a child of the current top of the stack.
    ///
    /// # Arguments
    /// * `layer` - The layer to add
    pub fn add_layer(&mut self, layer: BoxedLayer) {
        if let Some(entry) = self.layer_stack.last_mut() {
            entry.children_mut().push(layer);
        } else {
            self.root.add_child(layer);
        }
    }

    /// Add a picture layer (convenience method)
    ///
    /// # Arguments
    /// * `picture` - The picture layer to add
    pub fn add_picture(&mut self, picture: impl Layer + 'static) {
        self.add_layer(Box::new(picture));
    }

    /// Pop the current layer off the stack
    ///
    /// This must be called for each `push_*` call to maintain
    /// the layer hierarchy.
    ///
    /// # Panics
    ///
    /// Panics if there are no layers on the stack to pop.
    pub fn pop(&mut self) {
        if let Some(entry) = self.layer_stack.pop() {
            // Convert the entry into a layer
            let layer = entry.into_layer();

            // Add to parent or root
            if let Some(parent_entry) = self.layer_stack.last_mut() {
                parent_entry.children_mut().push(layer);
            } else {
                self.root.add_child(layer);
            }
        } else {
            panic!("SceneBuilder::pop() called with empty stack");
        }
    }

    /// Build the final scene
    ///
    /// Consumes the builder and returns the constructed scene.
    /// Any remaining layers on the stack are automatically popped.
    ///
    /// # Arguments
    /// * `viewport_size` - The size of the viewport
    pub fn build(mut self, viewport_size: Size) -> Scene {
        // Pop any remaining layers
        while !self.layer_stack.is_empty() {
            self.pop();
        }

        Scene::from_root(self.root, viewport_size)
    }

    /// Get the current stack depth
    ///
    /// This is useful for debugging to ensure push/pop calls are balanced.
    pub fn depth(&self) -> usize {
        self.layer_stack.len()
    }
}

impl Default for SceneBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;

    #[test]
    fn test_scene_builder_creation() {
        let builder = SceneBuilder::new();
        assert_eq!(builder.depth(), 0);
    }

    #[test]
    fn test_push_pop() {
        let mut builder = SceneBuilder::new();

        builder.push_opacity(0.5);
        assert_eq!(builder.depth(), 1);

        builder.push_offset(Offset::new(10.0, 20.0));
        assert_eq!(builder.depth(), 2);

        builder.pop();
        assert_eq!(builder.depth(), 1);

        builder.pop();
        assert_eq!(builder.depth(), 0);
    }

    #[test]
    fn test_add_layer() {
        let mut builder = SceneBuilder::new();
        builder.add_picture(PictureLayer::new());

        let scene = builder.build(Size::new(800.0, 600.0));
        assert_eq!(scene.layer_count(), 1);
    }

    #[test]
    fn test_nested_layers() {
        let mut builder = SceneBuilder::new();

        // Push opacity
        builder.push_opacity(0.8);

        // Add a picture inside opacity
        builder.add_picture(PictureLayer::new());

        // Pop opacity
        builder.pop();

        let scene = builder.build(Size::new(800.0, 600.0));
        // Should have 1 layer (the opacity layer containing the picture)
        assert_eq!(scene.layer_count(), 1);
    }

    #[test]
    #[should_panic(expected = "pop() called with empty stack")]
    fn test_pop_empty_stack() {
        let mut builder = SceneBuilder::new();
        builder.pop(); // Should panic
    }

    #[test]
    fn test_auto_pop_on_build() {
        let mut builder = SceneBuilder::new();

        builder.push_opacity(0.5);
        builder.push_offset(Offset::new(10.0, 10.0));

        // Build without manually popping - should auto-pop
        let scene = builder.build(Size::new(800.0, 600.0));

        // Should not panic, layers should be properly nested
        assert!(scene.layer_count() > 0);
    }
}
