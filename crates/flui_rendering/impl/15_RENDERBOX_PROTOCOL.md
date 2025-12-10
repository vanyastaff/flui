# RenderBox Protocol

This document describes the RenderBox protocol - the foundational layout system for 2D rectangular content.

## Overview

RenderBox specializes RenderObject for 2D Cartesian coordinate systems with:
- **BoxConstraints** - min/max width and height from parent
- **Size** - computed dimensions after layout
- **Offset** - position relative to parent (via ParentData)

```
RenderObject (abstract base)
    │
    └── RenderBox (2D box protocol)
            │
            ├── uses BoxConstraints (input)
            └── produces Size (output)
```

## BoxConstraints

Constraints passed from parent to child during layout.

### Flutter Definition

```dart
class BoxConstraints extends Constraints {
  final double minWidth;
  final double maxWidth;
  final double minHeight;
  final double maxHeight;
  
  // Factory constructors
  const BoxConstraints.tight(Size size);      // Exact size required
  const BoxConstraints.loose(Size size);       // Maximum size, minimum 0
  const BoxConstraints.expand({double? width, double? height}); // Fill available
  const BoxConstraints.tightFor({double? width, double? height});
  
  // Constraint operations
  BoxConstraints deflate(EdgeInsets edges);    // Shrink by padding
  BoxConstraints enforce(BoxConstraints constraints); // Apply additional
  BoxConstraints loosen();                     // Remove minimums
  BoxConstraints tighten({double? width, double? height});
  
  // Size computation
  Size constrain(Size size);                   // Clamp to constraints
  Size constrainDimensions(double width, double height);
  double constrainWidth([double width = double.infinity]);
  double constrainHeight([double height = double.infinity]);
  
  // Queries
  bool get isTight;        // min == max for both dimensions
  bool get hasTightWidth;  // minWidth == maxWidth
  bool get hasTightHeight; // minHeight == maxHeight
  bool get hasInfiniteWidth;
  bool get hasInfiniteHeight;
  bool get hasBoundedWidth;
  bool get hasBoundedHeight;
  bool isSatisfiedBy(Size size);
}
```

### Rust Translation

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    pub min_width: f64,
    pub max_width: f64,
    pub min_height: f64,
    pub max_height: f64,
}

impl BoxConstraints {
    pub const ZERO: Self = Self {
        min_width: 0.0, max_width: 0.0,
        min_height: 0.0, max_height: 0.0,
    };
    
    /// Exact size required
    pub const fn tight(size: Size) -> Self {
        Self {
            min_width: size.width, max_width: size.width,
            min_height: size.height, max_height: size.height,
        }
    }
    
    /// Maximum size, minimum 0
    pub const fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0, max_width: size.width,
            min_height: 0.0, max_height: size.height,
        }
    }
    
    /// Fill available space
    pub const fn expand() -> Self {
        Self {
            min_width: f64::INFINITY, max_width: f64::INFINITY,
            min_height: f64::INFINITY, max_height: f64::INFINITY,
        }
    }
    
    /// Shrink constraints by padding
    pub fn deflate(&self, edges: EdgeInsets) -> Self {
        let horizontal = edges.left + edges.right;
        let vertical = edges.top + edges.bottom;
        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }
    
    /// Apply additional constraints (intersection)
    pub fn enforce(&self, other: BoxConstraints) -> Self {
        Self {
            min_width: self.min_width.clamp(other.min_width, other.max_width),
            max_width: self.max_width.clamp(other.min_width, other.max_width),
            min_height: self.min_height.clamp(other.min_height, other.max_height),
            max_height: self.max_height.clamp(other.min_height, other.max_height),
        }
    }
    
    /// Remove minimum constraints
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0, max_width: self.max_width,
            min_height: 0.0, max_height: self.max_height,
        }
    }
    
    /// Clamp size to constraints
    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.clamp(self.min_width, self.max_width),
            height: size.height.clamp(self.min_height, self.max_height),
        }
    }
    
    /// Smallest size satisfying constraints
    pub fn smallest(&self) -> Size {
        Size { width: self.min_width, height: self.min_height }
    }
    
    /// Largest size satisfying constraints
    pub fn biggest(&self) -> Size {
        Size { width: self.max_width, height: self.max_height }
    }
    
    // Queries
    pub fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }
    
    pub fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }
    
    pub fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }
    
    pub fn has_bounded_width(&self) -> bool {
        self.max_width < f64::INFINITY
    }
    
    pub fn has_bounded_height(&self) -> bool {
        self.max_height < f64::INFINITY
    }
    
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width && size.width <= self.max_width &&
        size.height >= self.min_height && size.height <= self.max_height
    }
}
```

## RenderBox Trait

The core trait for 2D rectangular layout.

### Key Methods

```rust
pub trait RenderBox: RenderObject {
    // === Layout ===
    
    /// Main layout method. Compute size and position children.
    fn perform_layout(&mut self);
    
    /// Resize-only layout (when sizedByParent is true).
    fn perform_resize(&mut self) {
        // Default: do nothing. Override if sizedByParent.
    }
    
    /// Compute size without side effects (for intrinsic sizing).
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size;
    
    // === Constraints & Size ===
    
    /// Current constraints from parent.
    fn constraints(&self) -> BoxConstraints;
    
    /// Computed size (valid after layout).
    fn size(&self) -> Size;
    
    /// Set size during layout.
    fn set_size(&mut self, size: Size);
    
    /// Whether size depends only on constraints (optimization).
    fn sized_by_parent(&self) -> bool { false }
    
    // === Painting ===
    
    /// Paint this box at the given offset.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    // === Hit Testing ===
    
    /// Hit test this box and children.
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool;
    
    /// Hit test only this box (not children).
    fn hit_test_self(&self, position: Offset) -> bool { false }
    
    /// Hit test only children.
    fn hit_test_children(&self, result: &mut HitTestResult, position: Offset) -> bool { false }
    
    // === Intrinsic Sizing ===
    
    /// Minimum width that avoids clipping, given height.
    fn compute_min_intrinsic_width(&self, height: f64) -> f64 { 0.0 }
    
    /// Width beyond which more space doesn't help.
    fn compute_max_intrinsic_width(&self, height: f64) -> f64 { 0.0 }
    
    /// Minimum height that avoids clipping, given width.
    fn compute_min_intrinsic_height(&self, width: f64) -> f64 { 0.0 }
    
    /// Height beyond which more space doesn't help.
    fn compute_max_intrinsic_height(&self, width: f64) -> f64 { 0.0 }
    
    // === Baseline ===
    
    /// Distance from top to first text baseline.
    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f64> { None }
}
```

### Layout Protocol

The constraint-based layout follows "constraints go down, sizes go up":

```
Parent
   │
   │ layout(constraints)
   ▼
Child.performLayout()
   │
   │ computes size within constraints
   │
   ▼
Parent reads child.size
```

```rust
impl SomeRenderBox {
    fn perform_layout(&mut self) {
        // 1. Get constraints from parent
        let constraints = self.constraints();
        
        // 2. Layout children (if any)
        if let Some(child) = &mut self.child {
            // Pass constraints to child
            child.layout(constraints.loosen());
            
            // Position child via ParentData
            child.parent_data_mut().offset = Offset::ZERO;
        }
        
        // 3. Compute own size
        let size = if let Some(child) = &self.child {
            constraints.constrain(child.size())
        } else {
            constraints.smallest()
        };
        
        self.set_size(size);
    }
}
```

## Child Positioning via ParentData

RenderBox uses BoxParentData for child positioning:

```rust
/// Parent data for RenderBox children.
pub struct BoxParentData {
    /// Offset from parent's origin to child's origin.
    pub offset: Offset,
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self { offset: Offset::ZERO }
    }
}
```

During paint, apply the offset:

```rust
fn paint(&self, context: &mut PaintingContext, offset: Offset) {
    if let Some(child) = &self.child {
        let child_offset = child.parent_data().offset;
        context.paint_child(child, offset + child_offset);
    }
}
```

## Intrinsic Sizing

Intrinsic sizing allows querying "natural" dimensions without full layout.

### Use Cases

1. **Table columns**: Need to know minimum column width before layout
2. **IntrinsicWidth/Height widgets**: Size to content
3. **Text wrapping**: Determine where to break lines

### Implementation Pattern

```rust
fn compute_min_intrinsic_width(&self, height: f64) -> f64 {
    // For leaf nodes: return content's minimum width
    // For containers: aggregate children's intrinsic widths
    
    if let Some(child) = &self.child {
        child.get_min_intrinsic_width(height) + self.padding.horizontal()
    } else {
        0.0
    }
}

fn compute_max_intrinsic_width(&self, height: f64) -> f64 {
    // Width beyond which more space doesn't improve layout
    
    if let Some(child) = &self.child {
        child.get_max_intrinsic_width(height) + self.padding.horizontal()
    } else {
        0.0
    }
}
```

### Performance Warning

Intrinsic sizing can be O(N²) in worst case - use sparingly!

```rust
// EXPENSIVE: Causes intrinsic measurement of entire subtree
let width = child.get_min_intrinsic_width(f64::INFINITY);

// BETTER: Use constraints-based layout when possible
```

## sizedByParent Optimization

When a box's size depends **only** on constraints (not children), set `sized_by_parent = true`:

```rust
impl RenderColoredBox {
    fn sized_by_parent(&self) -> bool {
        true  // Size is just constraints.biggest()
    }
    
    fn perform_resize(&mut self) {
        // Called instead of performLayout for sizing
        self.set_size(self.constraints().biggest());
    }
    
    fn perform_layout(&mut self) {
        // Size already set by performResize
        // Only need to position children (if any)
    }
}
```

Benefits:
- Separates sizing from child layout
- Enables parallel layout in some cases
- Clearer code when size doesn't depend on children

## Dry Layout

`computeDryLayout` calculates size without side effects:

```rust
fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
    // Same logic as performLayout, but:
    // - Don't modify state
    // - Don't call child.layout() (use child.getDryLayout())
    // - Just return the computed size
    
    if let Some(child) = &self.child {
        let child_size = child.get_dry_layout(constraints.loosen());
        constraints.constrain(child_size)
    } else {
        constraints.smallest()
    }
}
```

Used for:
- Intrinsic sizing
- Layout previews
- Constraint propagation analysis

## Source Reference

Based on analysis of:
- [RenderBox class](https://api.flutter.dev/flutter/rendering/RenderBox-class.html)
- [BoxConstraints class](https://api.flutter.dev/flutter/rendering/BoxConstraints-class.html)
- [BoxParentData class](https://api.flutter.dev/flutter/rendering/BoxParentData-class.html)
