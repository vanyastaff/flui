# Layout System Architecture

## Overview

The layout system in FLUI determines the size and position of every render object in the tree. It uses a **constraint-based** approach where parents pass constraints down and children return geometry up.

## Layout Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                      LAYOUT FLOW                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Parent                         Child                           │
│    │                              │                             │
│    │  ┌─────────────────────┐     │                             │
│    ├──│ 1. Pass Constraints │────►│                             │
│    │  └─────────────────────┘     │                             │
│    │                              │                             │
│    │                         ┌────┴────┐                        │
│    │                         │ 2. Layout│                       │
│    │                         │  Children│                       │
│    │                         └────┬────┘                        │
│    │                              │                             │
│    │  ┌─────────────────────┐     │                             │
│    │◄─│ 3. Return Geometry  │─────┤                             │
│    │  └─────────────────────┘     │                             │
│    │                              │                             │
│    │  ┌─────────────────────┐     │                             │
│    ├──│ 4. Position Child   │────►│                             │
│    │  └─────────────────────┘     │                             │
│    ▼                              ▼                             │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## LayoutCapability Trait

The `LayoutCapability` trait defines what types are used for layout:

```rust
pub trait LayoutCapability: Send + Sync + 'static {
    /// Input constraints from parent
    type Constraints: Clone + Debug + Send + Sync;
    
    /// Output geometry to parent
    type Geometry: Clone + Debug + Default + Send + Sync;
    
    /// Context for layout operations (GAT)
    type Context<'ctx, A: Arity, P: ParentData>: LayoutContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
    
    /// Default geometry when layout hasn't run
    fn default_geometry() -> Self::Geometry;
}
```

## Box Layout

### BoxConstraints

Defines min/max width and height bounds:

```rust
#[derive(Clone, Debug, Default)]
pub struct BoxConstraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl BoxConstraints {
    /// Tight constraints - exact size required
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }
    
    /// Loose constraints - size up to max
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }
    
    /// Unbounded constraints
    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }
    
    /// Constrain a size to fit within bounds
    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.clamp(self.min_width, self.max_width),
            height: size.height.clamp(self.min_height, self.max_height),
        }
    }
    
    /// Deflate by edge insets (for padding)
    pub fn deflate(&self, insets: EdgeInsets) -> Self {
        let horizontal = insets.left + insets.right;
        let vertical = insets.top + insets.bottom;
        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }
    
    /// Check if constraints are tight (exact size)
    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width && self.min_height == self.max_height
    }
    
    /// Check if constraints are bounded
    pub fn is_bounded(&self) -> bool {
        self.max_width.is_finite() && self.max_height.is_finite()
    }
}
```

### BoxLayout Capability

```rust
pub struct BoxLayout;

impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type Context<'ctx, A: Arity, P: ParentData> = BoxLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;
    
    fn default_geometry() -> Size {
        Size::ZERO
    }
}
```

### BoxLayoutCtx

The context provided during box layout:

```rust
pub struct BoxLayoutCtx<'a, A: Arity, P: ParentData = BoxParentData> {
    /// Constraints from parent
    pub constraints: BoxConstraints,
    
    /// Access to children
    pub children: ChildrenAccess<'a, A, P, LayoutPhase>,
}

impl<'a, A: Arity, P: ParentData> BoxLayoutCtx<'a, A, P> {
    /// Layout a single child with given constraints
    pub fn layout_child(&mut self, constraints: BoxConstraints) -> Size
    where
        A: SingleChild,
    {
        self.children.single(|child| child.layout(constraints))
    }
    
    /// Set child offset
    pub fn set_child_offset(&mut self, offset: Offset)
    where
        A: SingleChild,
    {
        self.children.single(|child| child.set_offset(offset))
    }
    
    /// Layout all children with same constraints
    pub fn layout_children(&mut self, constraints: BoxConstraints) -> Vec<Size>
    where
        A: MultiChild,
    {
        self.children.map(|child| child.layout(constraints))
    }
}
```

## Sliver Layout

### SliverConstraints

Defines scrolling viewport constraints:

```rust
#[derive(Clone, Debug)]
pub struct SliverConstraints {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,
    
    /// Direction items grow from leading edge
    pub growth_direction: GrowthDirection,
    
    /// Scroll offset relative to sliver start
    pub scroll_offset: f64,
    
    /// Amount of content consumed by preceding slivers
    pub preceding_scroll_extent: f64,
    
    /// Remaining space in viewport after preceding slivers
    pub remaining_paint_extent: f64,
    
    /// Extent in cross axis (width for vertical scroll)
    pub cross_axis_extent: f64,
    
    /// Direction of cross axis
    pub cross_axis_direction: AxisDirection,
    
    /// Remaining space in cache area
    pub remaining_cache_extent: f64,
    
    /// Cache origin relative to viewport
    pub cache_origin: f64,
    
    /// Full viewport extent in main axis
    pub viewport_main_axis_extent: f64,
}

impl SliverConstraints {
    /// Check if scrolling in main axis
    pub fn is_normalized(&self) -> bool {
        matches!(
            self.axis_direction,
            AxisDirection::Down | AxisDirection::Right
        )
    }
    
    /// Get main axis extent from box size
    pub fn main_axis_extent(&self, size: Size) -> f64 {
        match self.axis_direction {
            AxisDirection::Down | AxisDirection::Up => size.height as f64,
            AxisDirection::Left | AxisDirection::Right => size.width as f64,
        }
    }
    
    /// Convert to box constraints for a child
    pub fn as_box_constraints(&self, min_extent: f64, max_extent: f64) -> BoxConstraints {
        match self.axis_direction {
            AxisDirection::Down | AxisDirection::Up => BoxConstraints {
                min_width: self.cross_axis_extent as f32,
                max_width: self.cross_axis_extent as f32,
                min_height: min_extent as f32,
                max_height: max_extent as f32,
            },
            AxisDirection::Left | AxisDirection::Right => BoxConstraints {
                min_width: min_extent as f32,
                max_width: max_extent as f32,
                min_height: self.cross_axis_extent as f32,
                max_height: self.cross_axis_extent as f32,
            },
        }
    }
}
```

### SliverGeometry

Output geometry for slivers:

```rust
#[derive(Clone, Debug, Default)]
pub struct SliverGeometry {
    /// Total scroll extent of this sliver
    pub scroll_extent: f64,
    
    /// How much to paint (may exceed viewport for overscroll)
    pub paint_extent: f64,
    
    /// Origin of paint relative to layout position
    pub paint_origin: f64,
    
    /// Extent of content actually laid out
    pub layout_extent: f64,
    
    /// Maximum paint extent (for scroll physics)
    pub max_paint_extent: f64,
    
    /// How much of scroll extent was scrolled past
    pub max_scroll_obstruction_extent: f64,
    
    /// Whether this sliver has visual content
    pub visible: bool,
    
    /// Whether layout includes content past viewport
    pub has_visual_overflow: bool,
    
    /// Scroll offset correction (for dynamic content)
    pub scroll_offset_correction: Option<f64>,
    
    /// Extent in cache area
    pub cache_extent: f64,
}

impl SliverGeometry {
    /// Zero geometry (nothing to show)
    pub const ZERO: Self = Self {
        scroll_extent: 0.0,
        paint_extent: 0.0,
        paint_origin: 0.0,
        layout_extent: 0.0,
        max_paint_extent: 0.0,
        max_scroll_obstruction_extent: 0.0,
        visible: false,
        has_visual_overflow: false,
        scroll_offset_correction: None,
        cache_extent: 0.0,
    };
    
    /// Create geometry for a simple box sliver
    pub fn from_paint_extent(paint_extent: f64) -> Self {
        Self {
            scroll_extent: paint_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: paint_extent,
            max_scroll_obstruction_extent: 0.0,
            visible: paint_extent > 0.0,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent: paint_extent,
        }
    }
}
```

### SliverLayout Capability

```rust
pub struct SliverLayout;

impl LayoutCapability for SliverLayout {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type Context<'ctx, A: Arity, P: ParentData> = SliverLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;
    
    fn default_geometry() -> SliverGeometry {
        SliverGeometry::ZERO
    }
}
```

## Layout Context API

Common interface for all layout contexts:

```rust
pub trait LayoutContextApi<'ctx, L: LayoutCapability, A: Arity, P: ParentData> {
    /// Get current constraints
    fn constraints(&self) -> &L::Constraints;
    
    /// Access children
    fn children(&mut self) -> &mut ChildrenAccess<'ctx, A, P, LayoutPhase>;
}
```

## Child Layout Operations

During layout phase, children can be:

```rust
impl<'a, P: ParentData> ChildHandle<'a, P, LayoutPhase> {
    /// Layout child with constraints, returns geometry
    pub fn layout<L: LayoutCapability>(&mut self, constraints: L::Constraints) -> L::Geometry {
        // Recursively layout child
        self.render_object.layout(constraints)
    }
    
    /// Set child offset (position relative to parent)
    pub fn set_offset(&mut self, offset: Offset) {
        self.parent_data.offset = offset;
    }
    
    /// Access parent data for modification
    pub fn parent_data_mut(&mut self) -> &mut P {
        &mut self.parent_data
    }
    
    /// Get child's current size (after layout)
    pub fn size(&self) -> Size {
        self.render_object.size()
    }
}
```

## Usage Examples

### Simple Container (Leaf)

```rust
impl RenderBox for RenderColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    
    fn perform_layout(&mut self, ctx: BoxLayoutCtx<Leaf>) -> Size {
        // No children, just take max size
        Size {
            width: ctx.constraints.max_width,
            height: ctx.constraints.max_height,
        }
    }
}
```

### Padding (Single Child)

```rust
impl RenderBox for RenderPadding {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn perform_layout(&mut self, mut ctx: BoxLayoutCtx<Single>) -> Size {
        // Deflate constraints by padding
        let inner_constraints = ctx.constraints.deflate(self.padding);
        
        // Layout child
        let child_size = ctx.children.single(|child| {
            child.layout(inner_constraints)
        });
        
        // Position child
        ctx.children.single(|child| {
            child.set_offset(Offset {
                x: self.padding.left,
                y: self.padding.top,
            });
        });
        
        // Return padded size
        Size {
            width: child_size.width + self.padding.horizontal(),
            height: child_size.height + self.padding.vertical(),
        }
    }
}
```

### Flex Layout (Variable Children)

```rust
impl RenderBox for RenderFlex {
    type Arity = Variable;
    type ParentData = FlexParentData;
    
    fn perform_layout(&mut self, mut ctx: BoxLayoutCtx<Variable, FlexParentData>) -> Size {
        let mut total_flex = 0.0;
        let mut allocated_size = 0.0;
        
        // First pass: layout non-flex children
        ctx.children.for_each(|child| {
            let flex = child.parent_data().flex;
            if flex == 0.0 {
                let size = child.layout(ctx.constraints.loosen());
                allocated_size += self.main_axis_size(size);
            } else {
                total_flex += flex;
            }
        });
        
        // Calculate remaining space for flex children
        let free_space = self.main_axis_extent(ctx.constraints) - allocated_size;
        let space_per_flex = if total_flex > 0.0 {
            free_space / total_flex
        } else {
            0.0
        };
        
        // Second pass: layout flex children
        ctx.children.for_each(|child| {
            let flex = child.parent_data().flex;
            if flex > 0.0 {
                let extent = space_per_flex * flex;
                let constraints = self.child_constraints(ctx.constraints, extent);
                child.layout(constraints);
            }
        });
        
        // Third pass: position children
        let mut offset = 0.0;
        ctx.children.for_each(|child| {
            child.set_offset(self.main_axis_offset(offset));
            offset += self.main_axis_size(child.size());
        });
        
        // Return total size
        self.compute_size(ctx.constraints, offset)
    }
}
```

## Intrinsic Dimensions

For widgets that need to know child sizes before layout:

```rust
pub trait IntrinsicDimensions {
    /// Minimum width given a height constraint
    fn min_intrinsic_width(&self, height: f32) -> f32;
    
    /// Maximum width given a height constraint
    fn max_intrinsic_width(&self, height: f32) -> f32;
    
    /// Minimum height given a width constraint
    fn min_intrinsic_height(&self, width: f32) -> f32;
    
    /// Maximum height given a width constraint
    fn max_intrinsic_height(&self, width: f32) -> f32;
}
```

## Layout Optimization

### Relayout Boundary

Skip relayout when constraints haven't changed:

```rust
pub trait RelayoutBoundary {
    /// Check if this node is a relayout boundary
    fn is_relayout_boundary(&self) -> bool;
    
    /// Check if needs layout
    fn needs_layout(&self) -> bool;
    
    /// Mark as needing layout
    fn mark_needs_layout(&mut self);
}
```

### Sized by Parent

Optimization for widgets whose size is determined entirely by constraints:

```rust
pub trait SizedByParent {
    /// If true, size is computed from constraints alone
    fn sized_by_parent(&self) -> bool {
        false
    }
    
    /// Compute size from constraints (called before perform_layout)
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        Size::ZERO
    }
}
```

## Common Layout Patterns

| Pattern | Description | Example |
|---------|-------------|---------|
| **Tight** | Force exact size | `Container(width: 100, height: 100)` |
| **Loose** | Allow shrinking | `Align(child: ...)` |
| **Expand** | Take all available space | `Expanded(child: ...)` |
| **Shrink** | Minimum size | `IntrinsicWidth(child: ...)` |
| **Pass-through** | Same as child | `Opacity(child: ...)` |

## Debugging Layout

```rust
impl Debug for BoxConstraints {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.is_tight() {
            write!(f, "BoxConstraints.tight({}, {})", 
                   self.max_width, self.max_height)
        } else if self.min_width == 0.0 && self.min_height == 0.0 {
            write!(f, "BoxConstraints.loose({}, {})",
                   self.max_width, self.max_height)
        } else {
            write!(f, "BoxConstraints({}-{}, {}-{})",
                   self.min_width, self.max_width,
                   self.min_height, self.max_height)
        }
    }
}
```
