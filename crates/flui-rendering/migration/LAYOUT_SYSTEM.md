# Layout System Architecture

## Overview

The Layout system is responsible for computing sizes and positions of render objects. It operates on a **constraints-down, size-up** model where constraints flow from parent to child, and sizes return from child to parent.

## Core Concepts

### Constraints-Down, Size-Up

```
Parent
  │
  ├─ Constraints (down) ──────▶ Child
  │                              │
  │                              │ [Compute size]
  │                              │
  │ ◀────────────── Size (up) ───┘
  │
  └─ [Position child at offset]
```

**Flow**:
1. Parent creates constraints for child
2. Child computes its size within those constraints
3. Child returns size to parent
4. Parent positions child at an offset

### Layout Phases

Layout happens in **three distinct phases**:

```
Phase 1: LAYOUT
├─ Parent calls child.layout(constraints)
├─ Child computes size
└─ Returns geometry (Size, SliverGeometry, etc.)

Phase 2: POSITIONING
├─ Parent computes child offsets
└─ Parent calls child.set_offset(offset)

Phase 3: FINALIZATION
├─ Parent finalizes its own size
└─ Returns size to its parent
```

## LayoutCapability

### Minimal Protocol Contract

```rust
pub trait LayoutCapability: Send + Sync + 'static {
    /// Input constraints type
    type Constraints: Clone + Debug + Send + Sync + 'static;
    
    /// Output geometry type
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;
    
    /// Layout context type (GAT!)
    type Context<'ctx, A: Arity, P: ParentData>: LayoutContextApi<'ctx, Self, A, P>
    where Self: 'ctx;
    
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }
}
```

**What it defines**: ONLY the input/output types.

**What it does NOT define**: How to compute size, how to position children.

### Box Layout

```rust
pub struct BoxLayout;

impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type Context<'ctx, A, P> = RenderContext<'ctx, BoxProtocol, LayoutPhase, A, P>;
}
```

#### BoxConstraints

```rust
pub struct BoxConstraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl BoxConstraints {
    /// Creates unconstrained constraints
    pub fn unconstrained() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }
    
    /// Creates tight constraints (exact size)
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }
    
    /// Creates loose constraints (0 to max)
    pub fn loose(max: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: max.width,
            min_height: 0.0,
            max_height: max.height,
        }
    }
    
    /// Returns smallest valid size
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }
    
    /// Returns largest valid size
    pub fn biggest(&self) -> Size {
        Size::new(self.max_width, self.max_height)
    }
    
    /// Checks if constraints are tight (min == max)
    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width && 
        self.min_height == self.max_height
    }
    
    /// Constrains a size to these constraints
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }
    
    /// Loosens constraints (sets min to 0)
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }
    
    /// Tightens constraints (sets max = min)
    pub fn tighten(&self) -> Self {
        Self {
            min_width: self.max_width,
            max_width: self.max_width,
            min_height: self.max_height,
            max_height: self.max_height,
        }
    }
}
```

#### Size

```rust
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size { width: 0.0, height: 0.0 };
    pub const INFINITY: Size = Size { 
        width: f32::INFINITY, 
        height: f32::INFINITY 
    };
    
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
    
    pub fn square(size: f32) -> Self {
        Self::new(size, size)
    }
    
    pub fn as_rect(&self) -> Rect {
        Rect::from_ltwh(0.0, 0.0, self.width, self.height)
    }
    
    pub fn contains(&self, offset: Offset) -> bool {
        offset.dx >= 0.0 && offset.dx < self.width &&
        offset.dy >= 0.0 && offset.dy < self.height
    }
}
```

### Sliver Layout

```rust
pub struct SliverLayout;

impl LayoutCapability for SliverLayout {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type Context<'ctx, A, P> = RenderContext<'ctx, SliverProtocol, LayoutPhase, A, P>;
}
```

#### SliverConstraints

```rust
pub struct SliverConstraints {
    /// How much scrolling has already occurred
    pub scroll_offset: f32,
    
    /// How much space is remaining in viewport
    pub remaining_paint_extent: f32,
    
    /// Cross axis extent (width for vertical, height for horizontal)
    pub cross_axis_extent: f32,
    
    /// Axis direction
    pub axis_direction: AxisDirection,
    
    /// Growth direction
    pub growth_direction: GrowthDirection,
}

pub enum AxisDirection {
    Up,
    Down,
    Left,
    Right,
}

pub enum GrowthDirection {
    Forward,  // Down or Right
    Reverse,  // Up or Left
}
```

#### SliverGeometry

```rust
pub struct SliverGeometry {
    /// How much of the sliver is visible in viewport
    pub paint_extent: f32,
    
    /// Total scrollable extent
    pub scroll_extent: f32,
    
    /// Maximum paint extent (when fully visible)
    pub max_paint_extent: f32,
    
    /// Whether this sliver has visual overflow
    pub has_visual_overflow: bool,
    
    /// Scroll offset correction (for changes during scroll)
    pub scroll_offset_correction: f32,
    
    /// Cache extent (how much to keep in memory)
    pub cache_extent: f32,
}
```

## Layout Context

### BoxLayoutContext

```rust
pub struct RenderContext<'ctx, BoxProtocol, LayoutPhase, A: Arity, P: ParentData> {
    phase_data: PhaseData::Layout {
        constraints: BoxConstraints,
    },
    children: ChildrenAccess<'ctx, A, P, LayoutPhase>,
}

impl<'ctx, A: Arity, P: ParentData> LayoutPhaseContext<BoxProtocol> 
    for RenderContext<'ctx, BoxProtocol, LayoutPhase, A, P>
{
    fn constraints(&self) -> &BoxConstraints {
        // Returns constraints from phase_data
    }
}
```

**Usage**:

```rust
impl RenderBoxImpl for MyWidget {
    type Arity = Optional;
    
    fn perform_layout(
        &mut self,
        mut ctx: BoxLayoutContext<'_, Optional>
    ) -> Size {
        let constraints = ctx.constraints();
        
        if let Some(mut child) = ctx.children().get() {
            let child_size = child.layout(constraints.loosen());
            child.set_offset(Offset::ZERO);
            child_size
        } else {
            constraints.smallest()
        }
    }
}
```

## Layout Utilities

Layout strategies are **helper utilities**, NOT part of the protocol type system. Widgets use them as needed.

### FlexLayout Utility

```rust
pub struct FlexLayout {
    pub direction: FlexDirection,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
}

impl FlexLayout {
    /// Computes child offsets for flex layout
    pub fn compute_offsets(
        &self,
        parent_size: Size,
        child_sizes: &[Size],
    ) -> Vec<Offset> {
        // Implementation...
    }
}

pub enum FlexDirection {
    Row,
    Column,
}

pub enum MainAxisAlignment {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

pub enum CrossAxisAlignment {
    Start,
    End,
    Center,
    Stretch,
}
```

**Usage in RenderFlex**:

```rust
impl RenderBoxImpl for RenderFlex {
    type Arity = Variable;
    type ParentData = FlexParentData;
    
    fn perform_layout(
        &mut self,
        mut ctx: BoxLayoutContext<'_, Variable, FlexParentData>
    ) -> Size {
        // 1. Layout children
        let mut child_sizes = Vec::new();
        ctx.children().for_each(|mut child| {
            let size = child.layout(constraints.loosen());
            child_sizes.push(size);
        });
        
        // 2. Use FlexLayout utility
        let flex_layout = FlexLayout {
            direction: self.direction,
            main_axis_alignment: self.main_axis_alignment,
            cross_axis_alignment: self.cross_axis_alignment,
        };
        
        let parent_size = constraints.biggest();
        let offsets = flex_layout.compute_offsets(parent_size, &child_sizes);
        
        // 3. Position children
        ctx.children().for_each_indexed(|i, mut child| {
            child.set_offset(offsets[i]);
        });
        
        parent_size
    }
}
```

### StackLayout Utility

```rust
pub struct StackLayout {
    pub fit: StackFit,
    pub alignment: Alignment,
}

impl StackLayout {
    pub fn compute_offsets(
        &self,
        parent_size: Size,
        child_sizes: &[Size],
    ) -> Vec<Offset> {
        child_sizes.iter().map(|child_size| {
            self.alignment.along_offset(
                parent_size.width - child_size.width,
                parent_size.height - child_size.height,
            )
        }).collect()
    }
}

pub enum StackFit {
    Loose,    // Children can be smaller
    Expand,   // Children must fill parent
    Passthrough, // Pass constraints unchanged
}

pub struct Alignment {
    pub x: f32,  // -1.0 (left) to 1.0 (right)
    pub y: f32,  // -1.0 (top) to 1.0 (bottom)
}

impl Alignment {
    pub const TOP_LEFT: Self = Self { x: -1.0, y: -1.0 };
    pub const CENTER: Self = Self { x: 0.0, y: 0.0 };
    pub const BOTTOM_RIGHT: Self = Self { x: 1.0, y: 1.0 };
    // ... more constants
}
```

### GridLayout Utility

```rust
pub struct GridLayout {
    pub columns: usize,
    pub row_spacing: f32,
    pub column_spacing: f32,
}

impl GridLayout {
    pub fn compute_offsets_and_sizes(
        &self,
        parent_size: Size,
        child_count: usize,
    ) -> (Vec<Offset>, Vec<Size>) {
        let column_width = (parent_size.width - 
            (self.columns - 1) as f32 * self.column_spacing) / 
            self.columns as f32;
        
        let mut offsets = Vec::new();
        let mut sizes = Vec::new();
        
        for i in 0..child_count {
            let col = i % self.columns;
            let row = i / self.columns;
            
            let x = col as f32 * (column_width + self.column_spacing);
            let y = row as f32 * (row_height + self.row_spacing);
            
            offsets.push(Offset::new(x, y));
            sizes.push(Size::new(column_width, row_height));
        }
        
        (offsets, sizes)
    }
}
```

## Intrinsics System

Intrinsics are **optional capabilities** that render objects can opt into.

### HasIntrinsics Trait

```rust
pub trait HasIntrinsics {
    /// Minimum intrinsic width for given height
    fn min_intrinsic_width(&self, height: f32) -> f32;
    
    /// Maximum intrinsic width for given height
    fn max_intrinsic_width(&self, height: f32) -> f32;
    
    /// Minimum intrinsic height for given width
    fn min_intrinsic_height(&self, width: f32) -> f32;
    
    /// Maximum intrinsic height for given width
    fn max_intrinsic_height(&self, width: f32) -> f32;
}
```

**Why Intrinsics?** They allow widgets to query their "natural size" before actual layout:

```rust
impl HasIntrinsics for RenderFlex {
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        match self.direction {
            FlexDirection::Row => {
                // Sum of min widths
                self.children.iter()
                    .map(|c| c.min_intrinsic_width(height))
                    .sum()
            }
            FlexDirection::Column => {
                // Max of min widths
                self.children.iter()
                    .map(|c| c.min_intrinsic_width(height))
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap_or(0.0)
            }
        }
    }
    
    // ... other methods
}
```

**Not all widgets need intrinsics!** Only implement when:
- Widget needs to know natural size
- Parent uses IntrinsicWidth/IntrinsicHeight
- Widget participates in baseline alignment

## Baseline System

Baseline alignment is another **optional capability**.

### HasBaseline Trait

```rust
pub trait HasBaseline {
    fn baseline(&self, baseline_type: BaselineType) -> Option<f32>;
}

pub enum BaselineType {
    /// Alphabetic baseline (for Latin text)
    Alphabetic,
    
    /// Ideographic baseline (for CJK text)
    Ideographic,
}
```

**Usage**:

```rust
impl HasBaseline for RenderParagraph {
    fn baseline(&self, baseline_type: BaselineType) -> Option<f32> {
        match baseline_type {
            BaselineType::Alphabetic => Some(self.alphabetic_baseline),
            BaselineType::Ideographic => Some(self.ideographic_baseline),
        }
    }
}

// RenderBaseline uses it:
impl RenderBoxImpl for RenderBaseline {
    fn perform_layout(&mut self, ctx: BoxLayoutContext) -> Size {
        let mut child = ctx.children().get();
        let child_size = child.layout(constraints);
        
        if let Some(baseline_offset) = child.baseline(BaselineType::Alphabetic) {
            // Position child so baseline is at desired position
            let y = self.baseline_position - baseline_offset;
            child.set_offset(Offset::new(0.0, y));
        }
        
        child_size
    }
}
```

## Layout Examples

### Example 1: RenderPadding (Single child)

```rust
pub struct RenderPadding {
    pub padding: EdgeInsets,
    size: Size,
}

impl RenderBoxImpl for RenderPadding {
    type Arity = Single;
    
    fn perform_layout(
        &mut self,
        mut ctx: BoxLayoutContext<'_, Single>
    ) -> Size {
        let constraints = ctx.constraints();
        
        // Shrink constraints by padding
        let child_constraints = BoxConstraints {
            min_width: (constraints.min_width - self.padding.horizontal).max(0.0),
            max_width: (constraints.max_width - self.padding.horizontal).max(0.0),
            min_height: (constraints.min_height - self.padding.vertical).max(0.0),
            max_height: (constraints.max_height - self.padding.vertical).max(0.0),
        };
        
        // Layout child
        let mut child = ctx.children().get();
        let child_size = child.layout(child_constraints);
        
        // Position child with padding offset
        child.set_offset(Offset::new(self.padding.left, self.padding.top));
        
        // Return size including padding
        Size::new(
            child_size.width + self.padding.horizontal,
            child_size.height + self.padding.vertical,
        )
    }
}
```

### Example 2: RenderAlign (Optional child)

```rust
pub struct RenderAlign {
    pub alignment: Alignment,
    pub width_factor: Option<f32>,
    pub height_factor: Option<f32>,
    size: Size,
}

impl RenderBoxImpl for RenderAlign {
    type Arity = Optional;
    
    fn perform_layout(
        &mut self,
        mut ctx: BoxLayoutContext<'_, Optional>
    ) -> Size {
        let constraints = ctx.constraints();
        
        if let Some(mut child) = ctx.children().get() {
            // Layout child with loose constraints
            let child_size = child.layout(constraints.loosen());
            
            // Compute parent size
            let width = self.width_factor
                .map(|f| child_size.width * f)
                .unwrap_or(constraints.max_width);
            let height = self.height_factor
                .map(|f| child_size.height * f)
                .unwrap_or(constraints.max_height);
            let parent_size = Size::new(width, height);
            
            // Position child according to alignment
            let offset = self.alignment.along_offset(
                parent_size.width - child_size.width,
                parent_size.height - child_size.height,
            );
            child.set_offset(offset);
            
            constraints.constrain(parent_size)
        } else {
            // No child - shrink to minimum
            constraints.smallest()
        }
    }
}
```

### Example 3: RenderFlex (Variable children)

```rust
pub struct RenderFlex {
    pub direction: FlexDirection,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
    size: Size,
}

impl RenderBoxImpl for RenderFlex {
    type Arity = Variable;
    type ParentData = FlexParentData;
    
    fn perform_layout(
        &mut self,
        mut ctx: BoxLayoutContext<'_, Variable, FlexParentData>
    ) -> Size {
        let constraints = ctx.constraints();
        
        // Phase 1: Determine flex total
        let mut total_flex = 0;
        ctx.children().for_each(|child| {
            total_flex += child.parent_data().flex;
        });
        
        // Phase 2: Layout inflexible children
        let mut allocated_size = 0.0;
        let mut child_sizes = Vec::new();
        
        ctx.children().for_each(|mut child| {
            if child.parent_data().flex == 0 {
                let size = child.layout(constraints.loosen());
                allocated_size += match self.direction {
                    FlexDirection::Row => size.width,
                    FlexDirection::Column => size.height,
                };
                child_sizes.push(size);
            } else {
                child_sizes.push(Size::ZERO);  // Placeholder
            }
        });
        
        // Phase 3: Layout flexible children
        let free_space = match self.direction {
            FlexDirection::Row => constraints.max_width - allocated_size,
            FlexDirection::Column => constraints.max_height - allocated_size,
        };
        let space_per_flex = if total_flex > 0 {
            free_space / total_flex as f32
        } else {
            0.0
        };
        
        ctx.children().for_each_indexed(|i, mut child| {
            if child.parent_data().flex > 0 {
                let flex_space = space_per_flex * child.parent_data().flex as f32;
                let flex_constraints = match self.direction {
                    FlexDirection::Row => BoxConstraints::tight(
                        Size::new(flex_space, constraints.max_height)
                    ),
                    FlexDirection::Column => BoxConstraints::tight(
                        Size::new(constraints.max_width, flex_space)
                    ),
                };
                let size = child.layout(flex_constraints);
                child_sizes[i] = size;
            }
        });
        
        // Phase 4: Position children
        let flex_layout = FlexLayout {
            direction: self.direction,
            main_axis_alignment: self.main_axis_alignment,
            cross_axis_alignment: self.cross_axis_alignment,
        };
        
        let parent_size = constraints.biggest();
        let offsets = flex_layout.compute_offsets(parent_size, &child_sizes);
        
        ctx.children().for_each_indexed(|i, mut child| {
            child.set_offset(offsets[i]);
        });
        
        parent_size
    }
}
```

## Layout Best Practices

### 1. Always Constrain Child Sizes

```rust
// ✅ Good
let child_size = child.layout(constraints);
constraints.constrain(child_size)

// ❌ Bad - might violate constraints
let child_size = child.layout(constraints);
child_size  // What if child ignored constraints?
```

### 2. Use Loose Constraints When Appropriate

```rust
// ✅ Good - let child choose its size
child.layout(constraints.loosen())

// ❌ Bad - forces child to fill parent
child.layout(constraints.tighten())
```

### 3. Position Children After Layout

```rust
// ✅ Good - layout first, position second
let size = child.layout(constraints);
child.set_offset(compute_offset(size));

// ❌ Bad - position before layout
child.set_offset(offset);  // Offset doesn't make sense yet!
let size = child.layout(constraints);
```

### 4. Cache Layout Results When Possible

```rust
// RenderObject has built-in caching
if !child.needs_layout() {
    return child.size();  // Use cached size
}
let size = child.layout(constraints);
```

### 5. Use Utilities for Complex Layout

```rust
// ✅ Good - use FlexLayout utility
let flex = FlexLayout::new(direction, alignment);
let offsets = flex.compute_offsets(parent_size, &child_sizes);

// ❌ Bad - reimplementing flex logic
let mut offset = 0.0;
for child_size in child_sizes {
    // ... manual spacing calculation ...
}
```

## Summary

**Layout is about constraints and geometry, NOT about strategies.**

| Aspect | Protocol Level | Widget Level |
|--------|---------------|--------------|
| **Types** | Constraints, Geometry | N/A |
| **Computation** | N/A | How to compute size |
| **Positioning** | N/A | How to position children |
| **Strategies** | N/A | FlexLayout, StackLayout, etc. |
| **Optional** | N/A | HasIntrinsics, HasBaseline |

**Key Principle**: Protocol defines what types flow (constraints → geometry). Widgets define how to compute those values.
