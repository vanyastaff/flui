# Implementation Guide

**Step-by-step instructions for creating render objects**

---

## Quick Start

Creating a new render object in FLUI involves 4 steps:

1. Choose protocol and container
2. Define struct with delegation
3. Implement required traits
4. Add to module system

---

## Step 1: Choose Protocol and Container

### Select Protocol

| If your object... | Use Protocol | Trait |
|------------------|--------------|-------|
| Uses 2D box layout | `BoxProtocol` | `RenderBox` |
| Scrolls with viewport | `SliverProtocol` | `RenderSliver` |

### Select Container

| If your object has... | Use Container |
|-----------------------|---------------|
| No children | None (leaf) |
| 0-1 child, size = child | `Proxy<P>` |
| 0-1 child, custom position | `Shifted<P>` |
| 0-1 child, alignment | `Aligning<P>` |
| Multiple children | `Children<P, PD>` |

### Select Trait Level

```
RenderBox
  ├── SingleChildRenderBox (has child access)
  │   ├── RenderProxyBox (size = child)
  │   │   ├── HitTestProxy (custom hit testing)
  │   │   ├── ClipProxy (clipping)
  │   │   └── PhysicalModelProxy (elevation + shadow)
  │   └── RenderShiftedBox (custom positioning)
  │       └── RenderAligningShiftedBox (alignment)
  └── MultiChildRenderBox (multiple children)
```

---

## Step 2: Define Struct

### Example 1: Simple Proxy Object

```rust
use ambassador::Delegate;
use crate::prelude::*;

/// Applies opacity to its child
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        Self {
            proxy: ProxyBox::new(),
            opacity: opacity.clamp(0.0, 1.0),
        }
    }
    
    pub fn opacity(&self) -> f32 {
        self.opacity
    }
    
    pub fn set_opacity(&mut self, value: f32) {
        let clamped = value.clamp(0.0, 1.0);
        if self.opacity != clamped {
            self.opacity = clamped;
            self.mark_needs_paint();
        }
    }
}

// Implement the marker trait
impl RenderProxyBox for RenderOpacity {}
```

### Example 2: Shifted Box Object

```rust
use ambassador::Delegate;
use crate::prelude::*;

/// Adds padding around its child
#[derive(Debug, Delegate)]
#[delegate(SingleChildRenderBox, target = "shifted")]
pub struct RenderPadding {
    shifted: ShiftedBox,
    padding: EdgeInsets,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            shifted: ShiftedBox::new(),
            padding,
        }
    }
    
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }
    
    pub fn set_padding(&mut self, value: EdgeInsets) {
        if self.padding != value {
            self.padding = value;
            self.mark_needs_layout();
        }
    }
}

impl RenderShiftedBox for RenderPadding {
    fn child_offset(&self) -> Offset {
        *self.shifted.offset()
    }
}
```

### Example 3: Multi-Child Object

```rust
use crate::prelude::*;

/// Lays out children in a row or column
#[derive(Debug)]
pub struct RenderFlex {
    children: BoxChildren<FlexParentData>,
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    _size: Size,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self {
        Self {
            children: BoxChildren::new(),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            _size: Size::ZERO,
        }
    }
}

impl MultiChildRenderBox for RenderFlex {
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
        self.children.iter()
    }
    
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox> {
        self.children.iter_mut()
    }
    
    fn child_count(&self) -> usize {
        self.children.len()
    }
}
```

---

## Step 3: Implement Required Traits

### For RenderProxyBox Objects

Only need to implement marker trait - blanket impls provide everything:

```rust
impl RenderProxyBox for RenderOpacity {}

// Automatically get:
// ✅ SingleChildRenderBox (via blanket impl)
// ✅ RenderBox (via blanket impl)
// ✅ RenderObject (via blanket impl)
```

Override only what's needed:

```rust
impl RenderBox for RenderOpacity {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.opacity == 0.0 {
            return;  // Invisible
        }
        
        if let Some(child) = self.proxy.child() {
            if self.opacity == 1.0 {
                context.paint_child(child, offset);
            } else {
                context.push_opacity(
                    offset,
                    (self.opacity * 255.0) as u8,
                    |ctx| ctx.paint_child(child, offset)
                );
            }
        }
    }
    
    fn always_needs_compositing(&self) -> bool {
        self.opacity > 0.0 && self.opacity < 1.0
    }
}
```

### For RenderShiftedBox Objects

Implement `RenderShiftedBox` + override layout:

```rust
impl RenderShiftedBox for RenderPadding {
    fn child_offset(&self) -> Offset {
        *self.shifted.offset()
    }
}

impl RenderBox for RenderPadding {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Deflate constraints by padding
        let inner = BoxConstraints {
            min_width: (constraints.min_width - self.padding.horizontal()).max(0.0),
            max_width: (constraints.max_width - self.padding.horizontal()).max(0.0),
            min_height: (constraints.min_height - self.padding.vertical()).max(0.0),
            max_height: (constraints.max_height - self.padding.vertical()).max(0.0),
        };
        
        // Layout child with deflated constraints
        let child_size = if let Some(child) = self.shifted.child_mut() {
            child.perform_layout(inner)
        } else {
            Size::ZERO
        };
        
        // Calculate final size
        let size = Size {
            width: child_size.width + self.padding.horizontal(),
            height: child_size.height + self.padding.vertical(),
        };
        
        // Set child offset
        self.shifted.set_offset(Offset {
            dx: self.padding.left,
            dy: self.padding.top,
        });
        
        self.shifted.set_geometry(size);
        size
    }
    
    fn size(&self) -> Size {
        *self.shifted.geometry()
    }
}
```

### For MultiChildRenderBox Objects

Implement full trait manually:

```rust
impl MultiChildRenderBox for RenderFlex {
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
        self.children.iter()
    }
    
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox> {
        self.children.iter_mut()
    }
    
    fn child_count(&self) -> usize {
        self.children.len()
    }
}

impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // 1. Collect flex info
        let mut total_flex = 0;
        let mut allocated_size = 0.0;
        
        for child in self.children.iter() {
            let parent_data = child.parent_data::<FlexParentData>();
            if let Some(flex) = parent_data.flex {
                total_flex += flex;
            }
        }
        
        // 2. Layout inflexible children
        for child in self.children.iter_mut() {
            let parent_data = child.parent_data::<FlexParentData>();
            if parent_data.flex.is_none() {
                let size = child.perform_layout(constraints);
                allocated_size += match self.direction {
                    Axis::Horizontal => size.width,
                    Axis::Vertical => size.height,
                };
            }
        }
        
        // 3. Distribute remaining space
        let free_space = match self.direction {
            Axis::Horizontal => constraints.max_width - allocated_size,
            Axis::Vertical => constraints.max_height - allocated_size,
        }.max(0.0);
        
        let space_per_flex = if total_flex > 0 {
            free_space / total_flex as f32
        } else {
            0.0
        };
        
        // 4. Layout flexible children
        for child in self.children.iter_mut() {
            let parent_data = child.parent_data::<FlexParentData>();
            if let Some(flex) = parent_data.flex {
                let child_main_size = space_per_flex * flex as f32;
                let child_constraints = match self.direction {
                    Axis::Horizontal => BoxConstraints {
                        min_width: child_main_size,
                        max_width: child_main_size,
                        min_height: 0.0,
                        max_height: constraints.max_height,
                    },
                    Axis::Vertical => BoxConstraints {
                        min_width: 0.0,
                        max_width: constraints.max_width,
                        min_height: child_main_size,
                        max_height: child_main_size,
                    },
                };
                child.perform_layout(child_constraints);
            }
        }
        
        // 5. Position children
        let mut position = 0.0;
        for child in self.children.iter() {
            let size = child.size();
            let parent_data = child.parent_data_mut::<FlexParentData>();
            
            parent_data.offset = match self.direction {
                Axis::Horizontal => Offset::new(position, 0.0),
                Axis::Vertical => Offset::new(0.0, position),
            };
            
            position += match self.direction {
                Axis::Horizontal => size.width,
                Axis::Vertical => size.height,
            };
        }
        
        // 6. Return size
        let size = match self.direction {
            Axis::Horizontal => Size::new(position, constraints.max_height),
            Axis::Vertical => Size::new(constraints.max_width, position),
        };
        
        self._size = size;
        size
    }
    
    fn size(&self) -> Size {
        self._size
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        for child in self.children.iter() {
            let parent_data = child.parent_data::<FlexParentData>();
            context.paint_child(child, offset + parent_data.offset);
        }
    }
    
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        for child in self.children.iter().rev() {
            let parent_data = child.parent_data::<FlexParentData>();
            let child_position = position - parent_data.offset;
            if child.hit_test(result, child_position) {
                return true;
            }
        }
        false
    }
}
```

---

## Step 4: Add to Module System

### 1. Create file in appropriate category

```bash
# For box objects:
touch flui-rendering/src/objects/box/[category]/[name].rs

# For sliver objects:
touch flui-rendering/src/objects/sliver/[category]/[name].rs
```

### 2. Add to category mod.rs

```rust
// In objects/box/effects/mod.rs
mod opacity;
pub use opacity::RenderOpacity;
```

### 3. Add to main mod.rs

```rust
// In objects/box/mod.rs
pub mod effects;
pub use effects::*;
```

---

## Common Patterns

### Pattern 1: Property Changes Trigger Dirty Marking

```rust
pub fn set_property(&mut self, value: T) {
    if self.property != value {
        self.property = value;
        
        // If affects layout:
        self.mark_needs_layout();
        
        // If only affects paint:
        self.mark_needs_paint();
        
        // If affects compositing:
        self.mark_needs_compositing_bits_update();
    }
}
```

### Pattern 2: Always Check for Null Children

```rust
fn perform_layout(&mut self, constraints: Constraints) -> Geometry {
    if let Some(child) = self.container.child_mut() {
        let child_geometry = child.perform_layout(constraints);
        // ... use child_geometry
    } else {
        // No child - return default
        constraints.smallest()
    }
}
```

### Pattern 3: Use Parent Data for Child Metadata

```rust
fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
    for child in self.children.iter_mut() {
        // Access parent data
        let parent_data = child.parent_data::<MyParentData>();
        
        // Use data for layout
        if parent_data.some_flag {
            // ...
        }
        
        // Update parent data after layout
        let parent_data = child.parent_data_mut::<MyParentData>();
        parent_data.offset = computed_offset;
    }
    size
}
```

### Pattern 4: Setup Parent Data

```rust
impl RenderObject for RenderMyObject {
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        if child.parent_data::<MyParentData>().is_err() {
            child.set_parent_data(Box::new(MyParentData::default()));
        }
    }
}
```

---

## Testing

### Unit Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_creation() {
        let render_obj = RenderMyObject::new();
        assert_eq!(render_obj.property(), expected_value);
    }
    
    #[test]
    fn test_layout() {
        let mut render_obj = RenderMyObject::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = render_obj.perform_layout(constraints);
        assert_eq!(size, Size::new(100.0, 100.0));
    }
    
    #[test]
    fn test_property_change() {
        let mut render_obj = RenderMyObject::new();
        render_obj.set_property(new_value);
        assert_eq!(render_obj.property(), new_value);
    }
}
```

---

## Checklist

Before submitting a new render object:

- [ ] Protocol selected (Box or Sliver)
- [ ] Container selected (Proxy, Shifted, Children, etc.)
- [ ] Trait level selected (RenderProxyBox, RenderShiftedBox, etc.)
- [ ] Struct defined with delegation
- [ ] Required traits implemented
- [ ] Property setters trigger dirty marking
- [ ] Null children handled correctly
- [ ] Parent data setup if needed
- [ ] Added to module system
- [ ] Unit tests written
- [ ] Documentation added

---

## Examples by Complexity

### Minimal (< 50 lines)
- RenderConstrainedBox
- RenderSizedBox
- RenderAbsorbPointer

### Simple (50-100 lines)
- RenderOpacity
- RenderPadding
- RenderAlign

### Medium (100-200 lines)
- RenderClipRect
- RenderTransform
- RenderStack

### Complex (200+ lines)
- RenderFlex
- RenderTable
- RenderSliverList

---

## Next Steps

- [[Object Catalog]] - Browse all objects
- [[Trait Hierarchy]] - Understand trait system
- [[Protocol]] - Understand protocols
- [[Containers]] - Learn container usage

---

**See Also:**
- [[Parent Data]] - Setting up parent data
- [[Pipeline]] - Integration with pipeline
