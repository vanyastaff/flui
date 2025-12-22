# Hit Test System Architecture

## Overview

The Hit Test system determines which render objects are located at a given position. This is essential for handling pointer events like taps, drags, and hovers.

## Core Concepts

### Hit Testing Model

```
User taps screen at (100, 200)
        │
        ↓
    RenderView
        │
        ├─ Transform to local: (100, 200)
        ├─ Hit test children
        │
        ├─ RenderBox A at offset (50, 50)
        │   │
        │   ├─ Transform: (100, 200) - (50, 50) = (50, 150)
        │   ├─ Hit? Rectangle test
        │   └─ Result: HIT ✅
        │
        └─ RenderBox B at offset (200, 100)
            │
            ├─ Transform: (100, 200) - (200, 100) = (-100, 100)
            ├─ Hit? Rectangle test
            └─ Result: MISS ❌

Result: [RenderView, RenderBox A]
```

### Hit Test Flow

1. **Transform**: Convert position to local coordinates
2. **Test**: Check if position hits this object
3. **Propagate**: Test children (if hit or translucent)
4. **Accumulate**: Add entries to result path

## HitTestCapability

### Minimal Protocol Contract

```rust
pub trait HitTestCapability: Send + Sync + 'static {
    /// Position type (Offset, AxisPosition, Point3D, etc.)
    type Position: Clone + Debug + Default + Send + Sync + 'static;
    
    /// Result accumulator
    type Result: Default + Send + Sync;
    
    /// Entry type in results
    type Entry: Clone + Debug;
    
    /// Hit test context type (GAT!)
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where Self: 'ctx;
}
```

**What it defines**: ONLY the position representation and result types.

**What it does NOT define**: How to test (rect, circle, path), filtering, propagation.

### Box Hit Test

```rust
pub struct BoxHitTest;

impl HitTestCapability for BoxHitTest {
    type Position = Offset;
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A, P> = RenderContext<'ctx, BoxProtocol, HitTestPhase, A, P>;
}
```

#### Offset Position

```rust
pub struct Offset {
    pub dx: f32,
    pub dy: f32,
}

impl Offset {
    pub const ZERO: Offset = Offset { dx: 0.0, dy: 0.0 };
    
    pub fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }
    
    pub fn translate(&self, other: Offset) -> Offset {
        Offset::new(self.dx + other.dx, self.dy + other.dy)
    }
    
    pub fn distance_to(&self, other: Offset) -> f32 {
        let dx = self.dx - other.dx;
        let dy = self.dy - other.dy;
        (dx * dx + dy * dy).sqrt()
    }
}
```

#### BoxHitTestResult

```rust
pub struct BoxHitTestResult {
    entries: Vec<BoxHitTestEntry>,
    transforms: Vec<Offset>,  // Transform stack
}

impl BoxHitTestResult {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            transforms: Vec::new(),
        }
    }
    
    /// Adds entry at current position
    pub fn add_with_position(&mut self, position: Offset) {
        let transform = self.current_transform();
        self.entries.push(BoxHitTestEntry {
            local_position: position,
            transform: Some(transform),
        });
    }
    
    /// Pushes offset transform
    pub fn push_offset(&mut self, offset: Offset) {
        self.transforms.push(offset);
    }
    
    /// Pops transform
    pub fn pop_transform(&mut self) {
        self.transforms.pop();
    }
    
    /// Returns accumulated transform
    fn current_transform(&self) -> Offset {
        self.transforms.iter().fold(Offset::ZERO, |acc, t| {
            acc.translate(*t)
        })
    }
    
    /// Returns hit test path (front to back)
    pub fn path(&self) -> &[BoxHitTestEntry] {
        &self.entries
    }
}
```

#### BoxHitTestEntry

```rust
pub struct BoxHitTestEntry {
    /// Position in local coordinates
    pub local_position: Offset,
    
    /// Transform from global to local
    pub transform: Option<Offset>,
}

impl BoxHitTestEntry {
    pub fn new(local_position: Offset) -> Self {
        Self {
            local_position,
            transform: None,
        }
    }
    
    /// Transforms global position to local
    pub fn global_to_local(&self, global: Offset) -> Offset {
        if let Some(transform) = self.transform {
            Offset::new(
                global.dx - transform.dx,
                global.dy - transform.dy,
            )
        } else {
            global
        }
    }
}
```

### Sliver Hit Test

```rust
pub struct SliverHitTest;

impl HitTestCapability for SliverHitTest {
    type Position = AxisPosition;
    type Result = SliverHitTestResult;
    type Entry = SliverHitTestEntry;
    type Context<'ctx, A, P> = RenderContext<'ctx, SliverProtocol, HitTestPhase, A, P>;
}
```

#### AxisPosition

```rust
pub struct AxisPosition {
    pub main_axis: f32,
    pub cross_axis: f32,
}

impl AxisPosition {
    pub fn new(main_axis: f32, cross_axis: f32) -> Self {
        Self { main_axis, cross_axis }
    }
    
    pub fn translate(&self, offset: AxisPosition) -> AxisPosition {
        AxisPosition::new(
            self.main_axis + offset.main_axis,
            self.cross_axis + offset.cross_axis,
        )
    }
    
    /// Converts to 2D position based on axis direction
    pub fn to_offset(&self, is_horizontal: bool) -> Offset {
        if is_horizontal {
            Offset::new(self.main_axis, self.cross_axis)
        } else {
            Offset::new(self.cross_axis, self.main_axis)
        }
    }
}
```

## Hit Test Context

### BoxHitTestContext

```rust
pub struct RenderContext<'ctx, BoxProtocol, HitTestPhase, A: Arity, P: ParentData> {
    phase_data: PhaseData::HitTest {
        position: Offset,
        result: &'ctx mut BoxHitTestResult,
    },
    children: ChildrenAccess<'ctx, A, P, HitTestPhase>,
}

impl<'ctx, A: Arity, P: ParentData> HitTestPhaseContext<BoxProtocol>
    for RenderContext<'ctx, BoxProtocol, HitTestPhase, A, P>
{
    fn position(&self) -> &Offset {
        // Returns position from phase_data
    }
    
    fn result(&mut self) -> &mut BoxHitTestResult {
        // Returns result from phase_data
    }
}
```

**Usage**:

```rust
impl RenderBoxImpl for MyWidget {
    type Arity = Optional;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Optional>) -> bool {
        let position = *ctx.position();
        
        if self.size.contains(position) {
            ctx.result().add_with_position(position);
            true
        } else {
            false
        }
    }
}
```

## Hit Test Behavior

Hit test behavior is a **widget property**, NOT part of the protocol type system.

### HitTestBehavior Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Only register hits on this object, block children if hit
    #[default]
    Opaque,
    
    /// Register hits on both this object and children
    Translucent,
    
    /// Defer to children, don't register self
    Defer,
}
```

### Behavior Examples

#### Opaque

```rust
impl RenderBoxImpl for RenderPointerListener {
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Single>) -> bool {
        let hit = self.size.contains(*ctx.position());
        
        match self.behavior {
            HitTestBehavior::Opaque => {
                if hit {
                    // Add self and stop
                    ctx.result().add_with_position(*ctx.position());
                    true
                } else {
                    false
                }
            }
            // ... other behaviors
        }
    }
}
```

#### Translucent

```rust
HitTestBehavior::Translucent => {
    // Always test children
    let mut child = ctx.children().get();
    let child_hit = child.hit_test(*ctx.position());
    
    // Add self if hit
    if self.size.contains(*ctx.position()) {
        ctx.result().add_with_position(*ctx.position());
        true
    } else {
        child_hit
    }
}
```

#### Defer

```rust
HitTestBehavior::Defer => {
    // Only test children, don't register self
    let mut child = ctx.children().get();
    child.hit_test(*ctx.position())
}
```

## Testing Utilities

Testing strategies are **helper functions**, NOT part of the protocol type system.

### Rectangle Testing (AABB)

```rust
/// Tests if position is within rectangular bounds.
pub fn test_rect_contains(position: Offset, bounds: Rect) -> bool {
    position.dx >= bounds.left()
        && position.dx < bounds.right()
        && position.dy >= bounds.top()
        && position.dy < bounds.bottom()
}
```

**Usage**:

```rust
impl RenderBoxImpl for RenderColoredBox {
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Optional>) -> bool {
        if test_rect_contains(*ctx.position(), self.size.as_rect()) {
            ctx.result().add_with_position(*ctx.position());
            true
        } else {
            false
        }
    }
}
```

### Circle Testing

```rust
/// Tests if position is within circular region.
pub fn test_circle_contains(
    position: Offset, 
    center: Offset, 
    radius: f32
) -> bool {
    let distance = position.distance_to(center);
    distance <= radius
}
```

**Usage**:

```rust
impl RenderBoxImpl for RenderCircleAvatar {
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Optional>) -> bool {
        let center = Offset::new(self.radius, self.radius);
        
        if test_circle_contains(*ctx.position(), center, self.radius) {
            ctx.result().add_with_position(*ctx.position());
            true
        } else {
            false
        }
    }
}
```

### Path Testing

```rust
/// Tests if position is within path.
pub fn test_path_contains(position: Offset, path: &Path) -> bool {
    path.contains(position)
}
```

**Usage**:

```rust
impl RenderBoxImpl for RenderCustomShape {
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Optional>) -> bool {
        if test_path_contains(*ctx.position(), &self.path) {
            ctx.result().add_with_position(*ctx.position());
            true
        } else {
            false
        }
    }
}
```

### Rounded Rectangle Testing

```rust
/// Tests if position is within rounded rectangle.
pub fn test_rrect_contains(
    position: Offset, 
    bounds: Rect, 
    radius: f32
) -> bool {
    // First check bounding box
    if !test_rect_contains(position, bounds) {
        return false;
    }
    
    // Check if in corner regions
    let in_left = position.dx < bounds.left() + radius;
    let in_right = position.dx > bounds.right() - radius;
    let in_top = position.dy < bounds.top() + radius;
    let in_bottom = position.dy > bounds.bottom() - radius;
    
    // If in corner, test circle
    if (in_left && in_top) {
        let corner = Offset::new(bounds.left() + radius, bounds.top() + radius);
        test_circle_contains(position, corner, radius)
    } else if (in_right && in_top) {
        let corner = Offset::new(bounds.right() - radius, bounds.top() + radius);
        test_circle_contains(position, corner, radius)
    } else if (in_left && in_bottom) {
        let corner = Offset::new(bounds.left() + radius, bounds.bottom() - radius);
        test_circle_contains(position, corner, radius)
    } else if (in_right && in_bottom) {
        let corner = Offset::new(bounds.right() - radius, bounds.bottom() - radius);
        test_circle_contains(position, corner, radius)
    } else {
        true  // In non-corner region
    }
}
```

## Transform Handling

Transforms are handled through the result's transform stack.

### Push/Pop Pattern

```rust
impl RenderBoxImpl for RenderTransform {
    type Arity = Single;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Single>) -> bool {
        // Push transform
        ctx.result().push_offset(self.offset);
        
        // Test child with transformed position
        let mut child = ctx.children().get();
        let transformed_pos = Offset::new(
            ctx.position().dx - self.offset.dx,
            ctx.position().dy - self.offset.dy,
        );
        let hit = child.hit_test(transformed_pos);
        
        // Pop transform
        ctx.result().pop_transform();
        
        hit
    }
}
```

### Helper Methods

```rust
impl BoxHitTestResult {
    /// Executes closure with offset transform
    pub fn add_with_paint_offset<F>(
        &mut self,
        offset: Option<Offset>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut Self, Offset) -> bool,
    {
        let transformed_position = if let Some(off) = offset {
            self.push_offset(off);
            Offset::new(position.dx - off.dx, position.dy - off.dy)
        } else {
            position
        };
        
        let result = hit_test(self, transformed_position);
        
        if offset.is_some() {
            self.pop_transform();
        }
        
        result
    }
}
```

**Usage**:

```rust
impl RenderBoxImpl for MyWidget {
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Single>) -> bool {
        let mut child = ctx.children().get();
        
        ctx.result().add_with_paint_offset(
            Some(child.offset()),
            *ctx.position(),
            |result, position| {
                child.hit_test(position)
            },
        )
    }
}
```

## Hit Test Examples

### Example 1: Simple Container

```rust
pub struct RenderColoredBox {
    color: Color,
    size: Size,
}

impl RenderBoxImpl for RenderColoredBox {
    type Arity = Optional;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Optional>) -> bool {
        // Test self (rectangle)
        if test_rect_contains(*ctx.position(), self.size.as_rect()) {
            // Add self to result
            ctx.result().add_with_position(*ctx.position());
            true
        } else {
            false
        }
    }
}
```

### Example 2: Container with Child

```rust
pub struct RenderPadding {
    padding: EdgeInsets,
    size: Size,
}

impl RenderBoxImpl for RenderPadding {
    type Arity = Single;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Single>) -> bool {
        // Test child with transformed position
        let mut child = ctx.children().get();
        let child_offset = Offset::new(self.padding.left, self.padding.top);
        
        ctx.result().add_with_paint_offset(
            Some(child_offset),
            *ctx.position(),
            |result, position| {
                child.hit_test(position)
            },
        )
    }
}
```

### Example 3: Stack (Multiple Children)

```rust
pub struct RenderStack {
    alignment: Alignment,
    size: Size,
}

impl RenderBoxImpl for RenderStack {
    type Arity = Variable;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Variable>) -> bool {
        // Test children in reverse paint order (front to back)
        let mut hit = false;
        
        // Iterate backwards through children
        for i in (0..ctx.children().len()).rev() {
            if let Some(mut child) = ctx.children().get_mut(i) {
                let child_offset = child.offset();
                
                let child_hit = ctx.result().add_with_paint_offset(
                    Some(child_offset),
                    *ctx.position(),
                    |result, position| {
                        child.hit_test(position)
                    },
                );
                
                if child_hit {
                    hit = true;
                    break;  // Stop at first hit (front-most)
                }
            }
        }
        
        hit
    }
}
```

### Example 4: Custom Shape

```rust
pub struct RenderCircleAvatar {
    radius: f32,
    size: Size,
}

impl RenderBoxImpl for RenderCircleAvatar {
    type Arity = Optional;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Optional>) -> bool {
        let center = Offset::new(self.radius, self.radius);
        
        // Test with circle
        if test_circle_contains(*ctx.position(), center, self.radius) {
            // Test child if any
            if let Some(mut child) = ctx.children().get() {
                // Child is clipped to circle - still test it
                child.hit_test(*ctx.position())
            } else {
                // No child - just register self
                ctx.result().add_with_position(*ctx.position());
                true
            }
        } else {
            false
        }
    }
}
```

### Example 5: Pointer Listener with Behavior

```rust
pub struct RenderPointerListener {
    behavior: HitTestBehavior,
    size: Size,
}

impl RenderBoxImpl for RenderPointerListener {
    type Arity = Single;
    
    fn hit_test(&self, mut ctx: BoxHitTestContext<'_, Single>) -> bool {
        let hit = test_rect_contains(*ctx.position(), self.size.as_rect());
        let mut child = ctx.children().get();
        
        match self.behavior {
            HitTestBehavior::Opaque => {
                // Block children if we're hit
                if hit {
                    ctx.result().add_with_position(*ctx.position());
                    true
                } else {
                    false
                }
            }
            HitTestBehavior::Translucent => {
                // Test children AND add self if hit
                let child_hit = child.hit_test(*ctx.position());
                
                if hit {
                    ctx.result().add_with_position(*ctx.position());
                }
                
                hit || child_hit
            }
            HitTestBehavior::Defer => {
                // Only test children, ignore self
                child.hit_test(*ctx.position())
            }
        }
    }
}
```

## Hit Test Best Practices

### 1. Transform Positions Correctly

```rust
// ✅ Good - subtract offset
let child_position = Offset::new(
    position.dx - child_offset.dx,
    position.dy - child_offset.dy,
);

// ❌ Bad - add offset (backwards!)
let child_position = Offset::new(
    position.dx + child_offset.dx,
    position.dy + child_offset.dy,
);
```

### 2. Use Helper Methods

```rust
// ✅ Good - use add_with_paint_offset
ctx.result().add_with_paint_offset(
    Some(child.offset()),
    *ctx.position(),
    |result, position| child.hit_test(position),
)

// ❌ Bad - manual transform tracking
ctx.result().push_offset(child.offset());
let hit = child.hit_test(transformed_pos);
ctx.result().pop_transform();
```

### 3. Test Children in Reverse Order

```rust
// ✅ Good - front to back (reverse paint order)
for i in (0..children.len()).rev() {
    if child.hit_test(position) {
        break;  // Stop at first hit
    }
}

// ❌ Bad - back to front
for child in children {
    child.hit_test(position);
}
```

### 4. Choose Appropriate Testing Strategy

```rust
// ✅ Good - match shape to testing
// Rectangle widget → test_rect_contains
// Circle widget → test_circle_contains
// Custom shape → test_path_contains

// ❌ Bad - always using AABB
// Circle widget but using rect test (imprecise!)
```

### 5. Respect Hit Test Behavior

```rust
// ✅ Good - check behavior property
match self.behavior {
    Opaque => /* block children */,
    Translucent => /* test both */,
    Defer => /* only children */,
}

// ❌ Bad - hardcoded behavior
if hit {
    return true;  // Always blocks!
}
```

## Summary

**Hit test is about position and results, NOT about strategies.**

| Aspect | Protocol Level | Widget Level |
|--------|---------------|--------------|
| **Types** | Position, Result, Entry | N/A |
| **Testing** | N/A | Rect, circle, path logic |
| **Transform** | N/A | Offset subtraction |
| **Behavior** | N/A | HitTestBehavior property |
| **Propagation** | N/A | Reverse paint order |
| **Utilities** | N/A | test_rect, test_circle, etc. |

**Key Principle**: Protocol defines position representation. Widgets define how to test their specific shape.
