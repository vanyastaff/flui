# Implementation Example: RenderAlign (Flutter-like in Rust)

This document shows how to implement RenderObjects following the plan
in `RENDER_STATE_PROTOCOL.md` and `TRAITS_OVERVIEW.md`.

## Flutter Hierarchy

```
RenderObject
    └── RenderBox
            └── RenderShiftedBox              // Single child + offset
                    └── RenderAligningShiftedBox  // + alignment logic
                            └── RenderPositionedBox   // + width/height factors
```

## Rust Translation Strategy

Flutter uses **class inheritance**. Rust uses **composition + traits**.

### Key Insight

Flutter's hierarchy is about **reusing code**:
- `RenderShiftedBox` = single child + paint at offset
- `RenderAligningShiftedBox` = + `alignChild()` method
- `RenderPositionedBox` = + width/height factors

In Rust we achieve the same with:
1. **Structs** for data storage
2. **Traits** for behavior contracts
3. **Blanket impls** for shared behavior
4. **Composition** for code reuse

---

## Step 1: Base Structs (Data Storage)

```rust
// ============================================================================
// flui_rendering/src/box/shifted_box.rs
// ============================================================================

use crate::{BoxProtocol, RenderNodeId, Single};
use flui_types::{Offset, Size};

/// Base data for single-child render objects that position child at offset.
/// 
/// Equivalent to Flutter's RenderShiftedBox fields.
#[derive(Debug)]
pub struct ShiftedBoxData {
    /// Cached child offset (set during layout, used during paint)
    pub child_offset: Offset,
    
    /// Cached size from layout
    pub size: Size,
}

impl Default for ShiftedBoxData {
    fn default() -> Self {
        Self {
            child_offset: Offset::ZERO,
            size: Size::ZERO,
        }
    }
}

impl ShiftedBoxData {
    pub fn new() -> Self {
        Self::default()
    }
}
```

```rust
// ============================================================================
// flui_rendering/src/box/aligning_shifted_box.rs
// ============================================================================

use flui_types::{Alignment, Offset, Size, TextDirection};

/// Base data for aligning single-child render objects.
/// 
/// Equivalent to Flutter's RenderAligningShiftedBox fields.
#[derive(Debug)]
pub struct AligningShiftedBoxData {
    /// Base shifted box data
    pub base: ShiftedBoxData,
    
    /// Alignment within available space
    pub alignment: Alignment,
    
    /// Text direction for resolving alignment
    pub text_direction: Option<TextDirection>,
}

impl AligningShiftedBoxData {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            base: ShiftedBoxData::new(),
            alignment,
            text_direction: None,
        }
    }
    
    /// Resolve alignment to concrete Alignment (handles RTL).
    pub fn resolved_alignment(&self) -> Alignment {
        // For now, just return alignment
        // Full impl would handle AlignmentDirectional + TextDirection
        self.alignment
    }
    
    /// Calculate child offset for given child and container sizes.
    /// 
    /// This is Flutter's `alignChild()` method.
    pub fn align_child(&mut self, child_size: Size, container_size: Size) {
        let alignment = self.resolved_alignment();
        self.base.child_offset = alignment.compute_offset(child_size, container_size);
    }
}
```

---

## Step 2: Behavior Traits

```rust
// ============================================================================
// flui_rendering/src/box/traits.rs
// ============================================================================

use crate::{BoxConstraints, BoxProtocol, PaintingContext, RenderNodeId, Size};
use flui_types::Offset;

/// Trait for render objects with a single child.
/// 
/// Provides child access methods.
pub trait HasSingleChild {
    fn child(&self) -> Option<RenderNodeId>;
    fn child_mut(&mut self) -> Option<&mut RenderNodeId>;
}

/// Trait for render objects that position child at an offset.
/// 
/// Equivalent to Flutter's RenderShiftedBox behavior.
pub trait ShiftedBox: HasSingleChild {
    /// Get child offset (set during layout).
    fn child_offset(&self) -> Offset;
    
    /// Set child offset (during layout).
    fn set_child_offset(&mut self, offset: Offset);
    
    /// Get cached size.
    fn size(&self) -> Size;
    
    /// Set size (during layout).
    fn set_size(&mut self, size: Size);
}

/// Trait for render objects that align child within available space.
/// 
/// Equivalent to Flutter's RenderAligningShiftedBox behavior.
pub trait AligningShiftedBox: ShiftedBox {
    /// Get alignment.
    fn alignment(&self) -> Alignment;
    
    /// Set alignment (marks needs layout).
    fn set_alignment(&mut self, alignment: Alignment);
    
    /// Align child within container.
    /// 
    /// Calculates and stores child_offset based on alignment.
    fn align_child(&mut self, child_size: Size, container_size: Size) {
        let offset = self.alignment().compute_offset(child_size, container_size);
        self.set_child_offset(offset);
    }
}
```

---

## Step 3: Concrete Implementation (RenderPositionedBox / RenderAlign)

```rust
// ============================================================================
// flui_objects/src/layout/positioned_box.rs (or align.rs)
// ============================================================================

use flui_rendering::{
    AligningShiftedBox, AligningShiftedBoxData, BoxConstraints, BoxProtocol,
    HasSingleChild, LayoutProtocol, PaintProtocol, PaintingContext,
    RenderBox, RenderNodeId, RenderObject, ShiftedBox, Single,
};
use flui_types::{Alignment, Offset, Size};

/// Positions its child using an Alignment.
/// 
/// Equivalent to Flutter's RenderPositionedBox.
/// 
/// # Layout Behavior
/// 
/// - If `width_factor` is set: width = child_width × factor
/// - If `width_factor` is None: expand to fill available width
/// - Same for height
/// 
/// # Example
/// 
/// ```rust
/// // Center child, expand to fill
/// let center = RenderPositionedBox::new(Alignment::CENTER);
/// 
/// // Center child, size = 2× child size
/// let doubled = RenderPositionedBox::with_factors(
///     Alignment::CENTER,
///     Some(2.0),
///     Some(2.0),
/// );
/// ```
#[derive(Debug)]
pub struct RenderPositionedBox {
    /// Base aligning data (alignment + child offset)
    data: AligningShiftedBoxData,
    
    /// Optional width factor (None = expand to fill)
    width_factor: Option<f32>,
    
    /// Optional height factor (None = expand to fill)
    height_factor: Option<f32>,
    
    /// Child node ID (managed by tree)
    child: Option<RenderNodeId>,
}

// ============================================================================
// Constructors
// ============================================================================

impl RenderPositionedBox {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            data: AligningShiftedBoxData::new(alignment),
            width_factor: None,
            height_factor: None,
            child: None,
        }
    }
    
    pub fn with_factors(
        alignment: Alignment,
        width_factor: Option<f32>,
        height_factor: Option<f32>,
    ) -> Self {
        Self {
            data: AligningShiftedBoxData::new(alignment),
            width_factor,
            height_factor,
            child: None,
        }
    }
    
    /// Create centered (most common case).
    pub fn centered() -> Self {
        Self::new(Alignment::CENTER)
    }
}

// ============================================================================
// Property Accessors (Flutter-style setters that mark needs layout)
// ============================================================================

impl RenderPositionedBox {
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }
    
    pub fn set_width_factor(&mut self, value: Option<f32>) {
        if self.width_factor != value {
            self.width_factor = value;
            // mark_needs_layout() would be called by the tree/owner
        }
    }
    
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }
    
    pub fn set_height_factor(&mut self, value: Option<f32>) {
        if self.height_factor != value {
            self.height_factor = value;
        }
    }
}

// ============================================================================
// Trait Implementations (Delegation to data)
// ============================================================================

impl HasSingleChild for RenderPositionedBox {
    fn child(&self) -> Option<RenderNodeId> {
        self.child
    }
    
    fn child_mut(&mut self) -> Option<&mut RenderNodeId> {
        self.child.as_mut()
    }
}

impl ShiftedBox for RenderPositionedBox {
    fn child_offset(&self) -> Offset {
        self.data.base.child_offset
    }
    
    fn set_child_offset(&mut self, offset: Offset) {
        self.data.base.child_offset = offset;
    }
    
    fn size(&self) -> Size {
        self.data.base.size
    }
    
    fn set_size(&mut self, size: Size) {
        self.data.base.size = size;
    }
}

impl AligningShiftedBox for RenderPositionedBox {
    fn alignment(&self) -> Alignment {
        self.data.alignment
    }
    
    fn set_alignment(&mut self, alignment: Alignment) {
        if self.data.alignment != alignment {
            self.data.alignment = alignment;
        }
    }
}

// ============================================================================
// RenderObject Base Implementation
// ============================================================================

impl RenderObject for RenderPositionedBox {
    fn debug_name(&self) -> &'static str {
        "RenderPositionedBox"
    }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(RenderNodeId)) {
        if let Some(child) = self.child {
            visitor(child);
        }
    }
    
    fn child_count(&self) -> usize {
        if self.child.is_some() { 1 } else { 0 }
    }
}

// ============================================================================
// Layout Implementation
// ============================================================================

impl LayoutProtocol<BoxProtocol> for RenderPositionedBox {
    fn sized_by_parent(&self) -> bool {
        // Only sized by parent if BOTH factors are None (expand to fill)
        self.width_factor.is_none() && self.height_factor.is_none()
    }
    
    fn perform_layout(
        &mut self,
        constraints: &BoxConstraints,
        children: &mut dyn ChildLayouter<BoxProtocol>,
    ) -> Size {
        let shrink_wrap_width = self.width_factor.is_some() || !constraints.has_bounded_width();
        let shrink_wrap_height = self.height_factor.is_some() || !constraints.has_bounded_height();
        
        if let Some(child_id) = self.child {
            // Layout child with loosened constraints
            let child_size = children.layout_child(child_id, constraints.loosen());
            
            // Compute our size based on factors
            let width = if shrink_wrap_width {
                let factor = self.width_factor.unwrap_or(1.0);
                child_size.width * factor
            } else {
                constraints.max_width
            };
            
            let height = if shrink_wrap_height {
                let factor = self.height_factor.unwrap_or(1.0);
                child_size.height * factor
            } else {
                constraints.max_height
            };
            
            let size = constraints.constrain(Size::new(width, height));
            
            // Align child within our size
            self.align_child(child_size, size);
            
            // Store and return size
            self.set_size(size);
            size
        } else {
            // No child - compute size from constraints
            let size = constraints.constrain(Size::new(
                if shrink_wrap_width { 0.0 } else { constraints.max_width },
                if shrink_wrap_height { 0.0 } else { constraints.max_height },
            ));
            self.set_size(size);
            size
        }
    }
}

// ============================================================================
// Paint Implementation
// ============================================================================

impl PaintProtocol for RenderPositionedBox {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child_id) = self.child {
            // Paint child at aligned offset
            ctx.paint_child(child_id, offset + self.child_offset());
        }
    }
    
    fn paint_bounds(&self) -> Rect {
        Rect::from_size(self.size())
    }
}

// ============================================================================
// Hit Test Implementation (default from ShiftedBox pattern)
// ============================================================================

impl HitTestProtocol<BoxProtocol> for RenderPositionedBox {
    fn hit_test_self(&self, position: Offset) -> bool {
        let size = self.size();
        position.x >= 0.0 && position.x < size.width &&
        position.y >= 0.0 && position.y < size.height
    }
    
    fn hit_test_children(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child_id) = self.child {
            // Transform position to child's coordinate space
            let child_position = position - self.child_offset();
            result.add_with_paint_offset(
                Some(self.child_offset()),
                position,
                |result, pos| {
                    // Delegate to child's hit test
                    // This would be handled by the tree
                    false
                },
            )
        } else {
            false
        }
    }
}

// ============================================================================
// Combined RenderBox trait (optional convenience)
// ============================================================================

impl RenderBox for RenderPositionedBox {
    // All methods delegated to individual traits above
}
```

---

## Step 4: Type Alias for Convenience

```rust
// flui_objects/src/layout/mod.rs

/// Alias: RenderAlign = RenderPositionedBox
/// 
/// Common name used in Flutter widgets.
pub type RenderAlign = RenderPositionedBox;
```

---

## Comparison: Flutter vs Rust

### Flutter (Dart)

```dart
class RenderPositionedBox extends RenderAligningShiftedBox {
  double? _widthFactor;
  double? _heightFactor;
  
  @override
  void performLayout() {
    final shrinkWrapWidth = _widthFactor != null || constraints.maxWidth == double.infinity;
    final shrinkWrapHeight = _heightFactor != null || constraints.maxHeight == double.infinity;
    
    if (child != null) {
      child!.layout(constraints.loosen(), parentUsesSize: true);
      size = constraints.constrain(Size(
        shrinkWrapWidth ? child!.size.width * (_widthFactor ?? 1.0) : double.infinity,
        shrinkWrapHeight ? child!.size.height * (_heightFactor ?? 1.0) : double.infinity,
      ));
      alignChild();  // inherited from RenderAligningShiftedBox
    } else {
      size = constraints.constrain(Size(
        shrinkWrapWidth ? 0.0 : double.infinity,
        shrinkWrapHeight ? 0.0 : double.infinity,
      ));
    }
  }
}
```

### Rust (FLUI)

```rust
impl LayoutProtocol<BoxProtocol> for RenderPositionedBox {
    fn perform_layout(&mut self, constraints: &BoxConstraints, children: &mut dyn ChildLayouter<BoxProtocol>) -> Size {
        let shrink_wrap_width = self.width_factor.is_some() || !constraints.has_bounded_width();
        let shrink_wrap_height = self.height_factor.is_some() || !constraints.has_bounded_height();
        
        if let Some(child_id) = self.child {
            let child_size = children.layout_child(child_id, constraints.loosen());
            let size = constraints.constrain(Size::new(
                if shrink_wrap_width { child_size.width * self.width_factor.unwrap_or(1.0) } else { constraints.max_width },
                if shrink_wrap_height { child_size.height * self.height_factor.unwrap_or(1.0) } else { constraints.max_height },
            ));
            self.align_child(child_size, size);  // from AligningShiftedBox trait
            self.set_size(size);
            size
        } else {
            let size = constraints.constrain(Size::new(
                if shrink_wrap_width { 0.0 } else { constraints.max_width },
                if shrink_wrap_height { 0.0 } else { constraints.max_height },
            ));
            self.set_size(size);
            size
        }
    }
}
```

---

## Key Differences

| Aspect | Flutter | Rust FLUI |
|--------|---------|-----------|
| Code reuse | Class inheritance | Composition + traits |
| Base data | Fields in superclass | Embedded struct (data: AligningShiftedBoxData) |
| Methods | Inherited methods | Trait methods + blanket impls |
| Type safety | Runtime (dynamic dispatch) | Compile-time (generics + traits) |
| Arity | Runtime child count check | Compile-time (Single, Optional, Variable) |

---

## Pattern Summary

1. **Data structs** (`ShiftedBoxData`, `AligningShiftedBoxData`) - hold state
2. **Behavior traits** (`ShiftedBox`, `AligningShiftedBox`) - define interface
3. **Concrete struct** (`RenderPositionedBox`) - compose data + implement traits
4. **Protocol traits** (`LayoutProtocol`, `PaintProtocol`, `HitTestProtocol`) - rendering behavior
5. **Combined trait** (`RenderBox`) - optional convenience super-trait

This achieves Flutter's code reuse through Rust idioms!
