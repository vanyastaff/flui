# Enum RenderObject Architecture Design

## 🎯 Goal: Complete Symmetry

```
Widget (enum) → Element (enum) → RenderObject (enum)
```

All three core types as enums for:
- ✅ Consistent architecture
- ✅ Exhaustive pattern matching
- ✅ Compile-time arity checking via enum variants
- ✅ No blanket impl conflicts
- ✅ Object-safe traits

## 📊 RenderObject Enum Design

### Core Enum

```rust
/// RenderObject - unified enum for all render object types
///
/// Instead of a trait with associated Arity type, we use enum variants
/// to represent different child counts at the type level.
#[derive(Debug)]
pub enum RenderObject {
    /// Leaf - no children
    /// Examples: Text, Image, Canvas
    Leaf(Box<dyn LeafRenderObject>),

    /// Single - exactly one child
    /// Examples: Opacity, Transform, ClipRect, Padding
    Single {
        render: Box<dyn SingleChildRenderObject>,
        child: Box<RenderObject>,
    },

    /// Multi - multiple children
    /// Examples: Row, Column, Stack, Flex
    Multi {
        render: Box<dyn MultiChildRenderObject>,
        children: Vec<RenderObject>,
    },
}
```

### Arity-Specific Traits

```rust
// ============================================================================
// LeafRenderObject - No children
// ============================================================================

/// LeafRenderObject - render object with no children
///
/// Leaf render objects handle their own layout and painting.
/// They don't have any child render objects.
///
/// # Examples
///
/// - Text
/// - Image
/// - CustomPaint
/// - Placeholder
pub trait LeafRenderObject: Debug + Send + Sync + 'static {
    /// Perform layout with given constraints
    ///
    /// # Parameters
    ///
    /// - `constraints` - The box constraints from parent
    ///
    /// # Returns
    ///
    /// The size this render object will occupy
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// # Returns
    ///
    /// A Layer representing the painted content
    fn paint(&self, size: Size) -> BoxedLayer;

    /// Optional: Hit test for pointer events
    fn hit_test(&self, position: Offset, size: Size) -> bool {
        Rect::from_size(size).contains(position)
    }

    /// Clone into boxed trait object
    fn clone_boxed(&self) -> Box<dyn LeafRenderObject>;

    /// Downcast support
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// SingleChildRenderObject - One child
// ============================================================================

/// SingleChildRenderObject - render object with exactly one child
///
/// Single-child render objects wrap another render object and can:
/// - Transform the child (rotation, scale, etc.)
/// - Clip the child
/// - Apply effects (opacity, blur, etc.)
/// - Add padding/constraints
///
/// # Examples
///
/// - Opacity
/// - Transform
/// - ClipRect
/// - Padding
/// - ConstrainedBox
pub trait SingleChildRenderObject: Debug + Send + Sync + 'static {
    /// Perform layout with one child
    ///
    /// This method receives mutable access to the child render object
    /// and can lay it out with modified constraints.
    ///
    /// # Parameters
    ///
    /// - `constraints` - Constraints from parent
    /// - `child` - The single child render object
    ///
    /// # Returns
    ///
    /// The size this render object will occupy
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn layout(&mut self, constraints: BoxConstraints, child: &mut RenderObject) -> Size {
    ///     // Layout child with same constraints
    ///     let child_size = match child {
    ///         RenderObject::Leaf(leaf) => leaf.layout(constraints),
    ///         RenderObject::Single { render, child } => render.layout(constraints, child),
    ///         RenderObject::Multi { render, children } => render.layout(constraints, children),
    ///     };
    ///
    ///     child_size
    /// }
    /// ```
    fn layout(&mut self, constraints: BoxConstraints, child: &mut RenderObject) -> Size;

    /// Paint this render object with child's layer
    ///
    /// The child is already painted, and this method can wrap/transform
    /// the child's layer.
    ///
    /// # Parameters
    ///
    /// - `child_layer` - Layer from the painted child
    /// - `size` - Size from layout
    ///
    /// # Returns
    ///
    /// A Layer representing this render object and its child
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer {
    ///     let mut opacity_layer = OpacityLayer::new(self.opacity);
    ///     opacity_layer.add_child(child_layer);
    ///     Box::new(opacity_layer)
    /// }
    /// ```
    fn paint(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer;

    /// Optional: Hit test
    fn hit_test(&self, position: Offset, size: Size) -> bool {
        Rect::from_size(size).contains(position)
    }

    /// Clone into boxed trait object
    fn clone_boxed(&self) -> Box<dyn SingleChildRenderObject>;

    /// Downcast support
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// MultiChildRenderObject - Multiple children
// ============================================================================

/// MultiChildRenderObject - render object with multiple children
///
/// Multi-child render objects arrange multiple child render objects.
/// They handle complex layout algorithms like flex, stack, grid, etc.
///
/// # Examples
///
/// - Row (horizontal flex)
/// - Column (vertical flex)
/// - Stack (overlapping children)
/// - Wrap (flowing children)
/// - CustomMultiChildLayout
pub trait MultiChildRenderObject: Debug + Send + Sync + 'static {
    /// Perform layout with multiple children
    ///
    /// # Parameters
    ///
    /// - `constraints` - Constraints from parent
    /// - `children` - Slice of child render objects
    ///
    /// # Returns
    ///
    /// The size this render object will occupy
    ///
    /// # Example (Row layout)
    ///
    /// ```ignore
    /// fn layout(&mut self, constraints: BoxConstraints, children: &mut [RenderObject]) -> Size {
    ///     let mut total_width = 0.0;
    ///     let mut max_height = 0.0;
    ///
    ///     for child in children {
    ///         let child_constraints = BoxConstraints::new(
    ///             Size::ZERO,
    ///             Size::new(f32::INFINITY, constraints.max.height)
    ///         );
    ///
    ///         let child_size = match child {
    ///             RenderObject::Leaf(leaf) => leaf.layout(child_constraints),
    ///             // ... handle other variants
    ///         };
    ///
    ///         total_width += child_size.width;
    ///         max_height = max_height.max(child_size.height);
    ///     }
    ///
    ///     Size::new(total_width, max_height)
    /// }
    /// ```
    fn layout(&mut self, constraints: BoxConstraints, children: &mut [RenderObject]) -> Size;

    /// Paint this render object with children's layers
    ///
    /// # Parameters
    ///
    /// - `child_layers` - Layers from painted children (in order)
    /// - `size` - Size from layout
    ///
    /// # Returns
    ///
    /// A Layer representing this render object and its children
    fn paint(&self, child_layers: Vec<BoxedLayer>, size: Size) -> BoxedLayer;

    /// Optional: Hit test
    fn hit_test(&self, position: Offset, size: Size) -> bool {
        Rect::from_size(size).contains(position)
    }

    /// Clone into boxed trait object
    fn clone_boxed(&self) -> Box<dyn MultiChildRenderObject>;

    /// Downcast support
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

## 🔧 RenderObject Enum Implementation

```rust
impl RenderObject {
    /// Create a Leaf render object
    pub fn leaf(render: impl LeafRenderObject) -> Self {
        RenderObject::Leaf(Box::new(render))
    }

    /// Create a Single-child render object
    pub fn single(render: impl SingleChildRenderObject, child: RenderObject) -> Self {
        RenderObject::Single {
            render: Box::new(render),
            child: Box::new(child),
        }
    }

    /// Create a Multi-child render object
    pub fn multi(render: impl MultiChildRenderObject, children: Vec<RenderObject>) -> Self {
        RenderObject::Multi {
            render: Box::new(render),
            children,
        }
    }

    /// Perform layout on this render object
    pub fn layout(&mut self, constraints: BoxConstraints) -> Size {
        match self {
            RenderObject::Leaf(leaf) => leaf.layout(constraints),
            RenderObject::Single { render, child } => render.layout(constraints, child),
            RenderObject::Multi { render, children } => render.layout(constraints, children),
        }
    }

    /// Paint this render object
    pub fn paint(&self, size: Size) -> BoxedLayer {
        match self {
            RenderObject::Leaf(leaf) => leaf.paint(size),

            RenderObject::Single { render, child } => {
                let child_layer = child.paint(size);
                render.paint(child_layer, size)
            }

            RenderObject::Multi { render, children } => {
                let child_layers: Vec<BoxedLayer> = children
                    .iter()
                    .map(|child| child.paint(size))
                    .collect();
                render.paint(child_layers, size)
            }
        }
    }

    /// Hit test at position
    pub fn hit_test(&self, position: Offset, size: Size) -> bool {
        match self {
            RenderObject::Leaf(leaf) => leaf.hit_test(position, size),
            RenderObject::Single { render, .. } => render.hit_test(position, size),
            RenderObject::Multi { render, .. } => render.hit_test(position, size),
        }
    }

    /// Clone the render object
    pub fn clone_render_object(&self) -> RenderObject {
        match self {
            RenderObject::Leaf(leaf) => RenderObject::Leaf(leaf.clone_boxed()),
            RenderObject::Single { render, child } => RenderObject::Single {
                render: render.clone_boxed(),
                child: Box::new(child.clone_render_object()),
            },
            RenderObject::Multi { render, children } => RenderObject::Multi {
                render: render.clone_boxed(),
                children: children.iter().map(|c| c.clone_render_object()).collect(),
            },
        }
    }

    /// Get the arity as a string (for debugging)
    pub fn arity_name(&self) -> &'static str {
        match self {
            RenderObject::Leaf(_) => "Leaf",
            RenderObject::Single { .. } => "Single",
            RenderObject::Multi { .. } => "Multi",
        }
    }

    /// Downcast to concrete type
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        match self {
            RenderObject::Leaf(leaf) => leaf.as_any().downcast_ref(),
            RenderObject::Single { render, .. } => render.as_any().downcast_ref(),
            RenderObject::Multi { render, .. } => render.as_any().downcast_ref(),
        }
    }

    /// Check if render object is of specific type
    pub fn is<T: 'static>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }
}

impl Clone for RenderObject {
    fn clone(&self) -> Self {
        self.clone_render_object()
    }
}
```

## 📝 Example Implementations

### Example 1: RenderText (Leaf)

```rust
#[derive(Debug, Clone)]
pub struct RenderText {
    pub text: String,
    pub font_size: f32,
    pub color: Color,
}

impl LeafRenderObject for RenderText {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Calculate text size
        let width = self.text.len() as f32 * self.font_size * 0.6;
        let height = self.font_size * 1.2;

        constraints.constrain(Size::new(width, height))
    }

    fn paint(&self, size: Size) -> BoxedLayer {
        let mut layer = PictureLayer::new();
        layer.draw_text(
            Rect::from_size(size),
            &self.text,
            self.font_size,
            self.color,
        );
        Box::new(layer)
    }

    fn clone_boxed(&self) -> Box<dyn LeafRenderObject> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Usage:
let render_text = RenderObject::leaf(RenderText {
    text: "Hello, World!".into(),
    font_size: 16.0,
    color: Color::BLACK,
});
```

### Example 2: RenderOpacity (Single)

```rust
#[derive(Debug, Clone)]
pub struct RenderOpacity {
    pub opacity: f32,
}

impl SingleChildRenderObject for RenderOpacity {
    fn layout(&mut self, constraints: BoxConstraints, child: &mut RenderObject) -> Size {
        // Just forward to child
        child.layout(constraints)
    }

    fn paint(&self, child_layer: BoxedLayer, _size: Size) -> BoxedLayer {
        let mut opacity_layer = OpacityLayer::new(self.opacity);
        opacity_layer.add_child(child_layer);
        Box::new(opacity_layer)
    }

    fn clone_boxed(&self) -> Box<dyn SingleChildRenderObject> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Usage:
let render_opacity = RenderObject::single(
    RenderOpacity { opacity: 0.5 },
    RenderObject::leaf(RenderText { /* ... */ }),
);
```

### Example 3: RenderFlex/Row (Multi)

```rust
#[derive(Debug, Clone)]
pub struct RenderFlex {
    pub direction: Axis,
    pub main_axis_alignment: MainAxisAlignment,
}

impl MultiChildRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints, children: &mut [RenderObject]) -> Size {
        let mut total_main = 0.0;
        let mut max_cross = 0.0;

        for child in children.iter_mut() {
            let child_size = child.layout(constraints);

            match self.direction {
                Axis::Horizontal => {
                    total_main += child_size.width;
                    max_cross = max_cross.max(child_size.height);
                }
                Axis::Vertical => {
                    total_main += child_size.height;
                    max_cross = max_cross.max(child_size.width);
                }
            }
        }

        match self.direction {
            Axis::Horizontal => Size::new(total_main, max_cross),
            Axis::Vertical => Size::new(max_cross, total_main),
        }
    }

    fn paint(&self, child_layers: Vec<BoxedLayer>, _size: Size) -> BoxedLayer {
        let mut flex_layer = FlexLayer::new(self.direction);

        for child_layer in child_layers {
            flex_layer.add_child(child_layer);
        }

        Box::new(flex_layer)
    }

    fn clone_boxed(&self) -> Box<dyn MultiChildRenderObject> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Usage:
let row = RenderObject::multi(
    RenderFlex {
        direction: Axis::Horizontal,
        main_axis_alignment: MainAxisAlignment::Start,
    },
    vec![
        RenderObject::leaf(RenderText { /* ... */ }),
        RenderObject::leaf(RenderText { /* ... */ }),
    ],
);
```

## ✅ Benefits

1. **Arity at type level** - Leaf/Single/Multi encoded in enum variant
2. **No associated types** - All traits are object-safe
3. **Exhaustive matching** - Compiler ensures all cases handled
4. **Consistent architecture** - Widget, Element, RenderObject all enums
5. **Simple navigation** - Direct access to children via pattern matching
6. **Clear semantics** - Variant name shows child count

## 🔄 Integration with Widget and Element

```rust
// Widget enum
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
    RenderObject(Box<dyn RenderObjectWidget>),
    // ...
}

// Element enum
pub enum Element {
    Component(ComponentElement),
    Stateful(StatefulElement),
    Render(RenderElement),  // ← Contains RenderObject enum
}

// RenderObject enum
pub enum RenderObject {
    Leaf(Box<dyn LeafRenderObject>),
    Single { render: Box<dyn SingleChildRenderObject>, child: Box<RenderObject> },
    Multi { render: Box<dyn MultiChildRenderObject>, children: Vec<RenderObject> },
}

// RenderElement holds RenderObject
pub struct RenderElement {
    widget: Box<dyn RenderObjectWidget>,
    render_object: RenderObject,  // ← Enum!
}
```

## 🚀 Migration Path

1. Create new `render_object_enum.rs` with enum RenderObject
2. Create `render_traits.rs` with Leaf/Single/Multi traits
3. Keep old RenderObject trait as deprecated
4. Gradually migrate render objects to new traits
5. Update RenderElement to use enum RenderObject
6. Remove old trait once migration complete

This gives us the holy trinity:
```
Widget (enum) → Element (enum) → RenderObject (enum)
```

Perfect symmetry and consistency! 🎯
