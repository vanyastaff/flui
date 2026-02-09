# Hit Test System Architecture

## Overview

The hit test system determines which render objects are under a given pointer position. It traverses the render tree in **reverse paint order** (front-to-back) to find the topmost element at a position.

## Hit Test Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                      HIT TEST FLOW                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   Pointer Event (x, y)                                          │
│         │                                                       │
│         ▼                                                       │
│  ┌─────────────────┐                                            │
│  │  Root.hit_test  │ ◄── Start from root                        │
│  └────────┬────────┘                                            │
│           │                                                     │
│           ▼                                                     │
│  ┌─────────────────┐     ┌─────────────────┐                   │
│  │ Contains point? │──No─►│ Return false    │                   │
│  └────────┬────────┘     └─────────────────┘                   │
│           │ Yes                                                 │
│           ▼                                                     │
│  ┌─────────────────┐                                            │
│  │ Test children   │ ◄── Reverse paint order (last first)      │
│  │ (back to front) │                                            │
│  └────────┬────────┘                                            │
│           │                                                     │
│           ▼                                                     │
│  ┌─────────────────┐     ┌─────────────────┐                   │
│  │ Child hit?      │──No─►│ Add self to     │                   │
│  └────────┬────────┘     │ result & return │                   │
│           │ Yes          └─────────────────┘                   │
│           ▼                                                     │
│  ┌─────────────────┐                                            │
│  │ Return true     │ ◄── Child handles event                    │
│  └─────────────────┘                                            │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## HitTestCapability Trait

```rust
pub trait HitTestCapability: Send + Sync + 'static {
    /// Position type for hit testing
    type Position: Clone + Debug + Default;
    
    /// Result accumulator
    type Result: HitTestResultApi + Default;
    
    /// Individual hit entry
    type Entry: Clone + Debug;
    
    /// Context for hit test operations (GAT)
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}
```

## Box Hit Test

### Position Type

For box protocol, position is a simple 2D offset:

```rust
pub type BoxPosition = Offset;

#[derive(Clone, Copy, Debug, Default)]
pub struct Offset {
    pub x: f32,
    pub y: f32,
}

impl Offset {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    
    /// Translate by delta
    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
    
    /// Transform by matrix
    pub fn transform(&self, matrix: &Matrix4) -> Self {
        // Apply 2D affine transform
        let (x, y) = matrix.transform_point(self.x, self.y);
        Self { x, y }
    }
}
```

### Hit Test Result

Accumulates hit entries in order:

```rust
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    /// Path from root to deepest hit target
    path: Vec<BoxHitTestEntry>,
}

impl BoxHitTestResult {
    /// Add an entry to the hit path
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        self.path.push(entry);
    }
    
    /// Add entry with transform
    pub fn add_with_transform(
        &mut self,
        render_id: RenderId,
        transform: Matrix4,
    ) {
        self.path.push(BoxHitTestEntry {
            render_id,
            transform,
        });
    }
    
    /// Get the path (root to target)
    pub fn path(&self) -> &[BoxHitTestEntry] {
        &self.path
    }
    
    /// Get the deepest target
    pub fn target(&self) -> Option<&BoxHitTestEntry> {
        self.path.last()
    }
    
    /// Check if anything was hit
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}
```

### Hit Test Entry

Individual entry in the hit path:

```rust
#[derive(Clone, Debug)]
pub struct BoxHitTestEntry {
    /// ID of the hit render object
    pub render_id: RenderId,
    
    /// Transform from global to local coordinates
    pub transform: Matrix4,
}

impl BoxHitTestEntry {
    /// Create new entry
    pub fn new(render_id: RenderId) -> Self {
        Self {
            render_id,
            transform: Matrix4::IDENTITY,
        }
    }
    
    /// Create entry with transform
    pub fn with_transform(render_id: RenderId, transform: Matrix4) -> Self {
        Self { render_id, transform }
    }
    
    /// Transform a position to local coordinates
    pub fn global_to_local(&self, position: Offset) -> Offset {
        self.transform.inverse().transform_point(position)
    }
}
```

### BoxHitTest Capability

```rust
pub struct BoxHitTest;

impl HitTestCapability for BoxHitTest {
    type Position = Offset;
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData> = BoxHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}
```

### BoxHitTestCtx

The context provided during box hit testing:

```rust
pub struct BoxHitTestCtx<'a, A: Arity, P: ParentData = BoxParentData> {
    /// Position being tested (in local coordinates)
    pub position: Offset,
    
    /// Result accumulator
    pub result: &'a mut BoxHitTestResult,
    
    /// Access to children
    pub children: ChildrenAccess<'a, A, P, HitTestPhase>,
}

impl<'a, A: Arity, P: ParentData> BoxHitTestCtx<'a, A, P> {
    /// Check if position is within bounds
    pub fn contains(&self, bounds: Rect) -> bool {
        bounds.contains(self.position)
    }
    
    /// Add self to hit result
    pub fn add_hit(&mut self, render_id: RenderId) {
        self.result.add(BoxHitTestEntry::new(render_id));
    }
    
    /// Add self with transform
    pub fn add_hit_with_transform(&mut self, render_id: RenderId, transform: Matrix4) {
        self.result.add_with_transform(render_id, transform);
    }
    
    /// Transform position for child hit testing
    pub fn child_position(&self, child_offset: Offset) -> Offset {
        Offset {
            x: self.position.x - child_offset.x,
            y: self.position.y - child_offset.y,
        }
    }
}
```

## Sliver Hit Test

### Position Type

For slivers, position is along the main axis:

```rust
#[derive(Clone, Debug, Default)]
pub struct SliverHitTestPosition {
    /// Position along main axis
    pub main_axis_position: f64,
    
    /// Position along cross axis
    pub cross_axis_position: f64,
}

impl SliverHitTestPosition {
    /// Create from box offset for given axis direction
    pub fn from_offset(offset: Offset, axis_direction: AxisDirection) -> Self {
        match axis_direction {
            AxisDirection::Down => Self {
                main_axis_position: offset.y as f64,
                cross_axis_position: offset.x as f64,
            },
            AxisDirection::Up => Self {
                main_axis_position: -offset.y as f64,
                cross_axis_position: offset.x as f64,
            },
            AxisDirection::Right => Self {
                main_axis_position: offset.x as f64,
                cross_axis_position: offset.y as f64,
            },
            AxisDirection::Left => Self {
                main_axis_position: -offset.x as f64,
                cross_axis_position: offset.y as f64,
            },
        }
    }
}
```

### Sliver Hit Test Result

```rust
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    path: Vec<SliverHitTestEntry>,
}

#[derive(Clone, Debug)]
pub struct SliverHitTestEntry {
    pub render_id: RenderId,
    pub main_axis_position: f64,
    pub cross_axis_position: f64,
}
```

### SliverHitTest Capability

```rust
pub struct SliverHitTest;

impl HitTestCapability for SliverHitTest {
    type Position = SliverHitTestPosition;
    type Result = SliverHitTestResult;
    type Entry = SliverHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData> = SliverHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}
```

## Hit Test Context API

Common interface for all hit test contexts:

```rust
pub trait HitTestContextApi<'ctx, H: HitTestCapability, A: Arity, P: ParentData> {
    /// Get current position
    fn position(&self) -> &H::Position;
    
    /// Get mutable result
    fn result(&mut self) -> &mut H::Result;
    
    /// Access children
    fn children(&mut self) -> &mut ChildrenAccess<'ctx, A, P, HitTestPhase>;
}
```

## Child Hit Test Operations

During hit test phase, children can be tested:

```rust
impl<'a, P: ParentData> ChildHandle<'a, P, HitTestPhase> {
    /// Hit test child at position
    pub fn hit_test(&mut self, position: Offset) -> bool {
        self.render_object.hit_test(position, self.result)
    }
    
    /// Hit test child at offset position
    pub fn hit_test_at(&mut self, parent_position: Offset) -> bool {
        let child_position = Offset {
            x: parent_position.x - self.parent_data.offset.x,
            y: parent_position.y - self.parent_data.offset.y,
        };
        self.hit_test(child_position)
    }
    
    /// Get child's bounds
    pub fn bounds(&self) -> Rect {
        Rect::from_offset_size(self.parent_data.offset, self.render_object.size())
    }
}
```

## Usage Examples

### Simple Box (Leaf)

```rust
impl RenderBox for RenderColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    
    fn hit_test(&self, ctx: BoxHitTestCtx<Leaf>) -> bool {
        let bounds = Rect::from_size(self.size);
        
        if bounds.contains(ctx.position) {
            ctx.add_hit(self.id);
            true
        } else {
            false
        }
    }
}
```

### Container with Child (Single)

```rust
impl RenderBox for RenderPadding {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn hit_test(&self, mut ctx: BoxHitTestCtx<Single>) -> bool {
        let bounds = Rect::from_size(self.size);
        
        if !bounds.contains(ctx.position) {
            return false;
        }
        
        // Test child first (it's on top)
        let child_hit = ctx.children.single(|child| {
            child.hit_test_at(ctx.position)
        });
        
        if child_hit {
            return true;
        }
        
        // Child didn't handle, add ourselves
        ctx.add_hit(self.id);
        true
    }
}
```

### Stack (Variable Children, Z-Order)

```rust
impl RenderBox for RenderStack {
    type Arity = Variable;
    type ParentData = StackParentData;
    
    fn hit_test(&self, mut ctx: BoxHitTestCtx<Variable, StackParentData>) -> bool {
        let bounds = Rect::from_size(self.size);
        
        if !bounds.contains(ctx.position) {
            return false;
        }
        
        // Test children in reverse order (last painted = on top)
        let hit = ctx.children.reverse_for_each(|child| {
            if child.hit_test_at(ctx.position) {
                return ControlFlow::Break(true);
            }
            ControlFlow::Continue(())
        });
        
        if matches!(hit, ControlFlow::Break(true)) {
            return true;
        }
        
        // No child hit, add ourselves
        ctx.add_hit(self.id);
        true
    }
}
```

### Transformed Container

```rust
impl RenderBox for RenderTransform {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn hit_test(&self, mut ctx: BoxHitTestCtx<Single>) -> bool {
        // Transform position to child's coordinate space
        let local_position = self.transform.inverse().transform_point(ctx.position);
        
        // Test child with transformed position
        let child_hit = ctx.children.single(|child| {
            child.hit_test(local_position)
        });
        
        if child_hit {
            // Add ourselves with transform
            ctx.add_hit_with_transform(self.id, self.transform);
            return true;
        }
        
        false
    }
}
```

### Clipped Container

```rust
impl RenderBox for RenderClip {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn hit_test(&self, mut ctx: BoxHitTestCtx<Single>) -> bool {
        // Check if position is within clip bounds
        if !self.clip_path.contains(ctx.position) {
            return false;
        }
        
        // Test child normally
        ctx.children.single(|child| {
            child.hit_test_at(ctx.position)
        })
    }
}
```

## Hit Test Behaviors

Different widgets have different hit test behaviors:

### HitTestBehavior

```rust
#[derive(Clone, Copy, Debug, Default)]
pub enum HitTestBehavior {
    /// Defer to children, never absorb
    #[default]
    DeferToChild,
    
    /// Absorb hits within bounds, even if transparent
    Opaque,
    
    /// Pass through transparent areas
    Translucent,
}
```

### GestureDetector Example

```rust
impl RenderBox for RenderGestureDetector {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn hit_test(&self, mut ctx: BoxHitTestCtx<Single>) -> bool {
        let bounds = Rect::from_size(self.size);
        
        if !bounds.contains(ctx.position) {
            return false;
        }
        
        match self.behavior {
            HitTestBehavior::DeferToChild => {
                // Only hit if child is hit
                ctx.children.single(|child| child.hit_test_at(ctx.position))
            }
            HitTestBehavior::Opaque => {
                // Test child, but always absorb
                ctx.children.single(|child| child.hit_test_at(ctx.position));
                ctx.add_hit(self.id);
                true
            }
            HitTestBehavior::Translucent => {
                // Test child, add self if child hit
                let child_hit = ctx.children.single(|child| {
                    child.hit_test_at(ctx.position)
                });
                if child_hit {
                    ctx.add_hit(self.id);
                }
                child_hit
            }
        }
    }
}
```

## Event Dispatch

After hit testing, events are dispatched along the path:

```rust
pub struct PointerEvent {
    pub kind: PointerEventKind,
    pub position: Offset,
    pub delta: Offset,
    pub buttons: u32,
    pub timestamp: Duration,
}

pub fn dispatch_pointer_event(
    event: PointerEvent,
    result: &BoxHitTestResult,
) {
    // Dispatch from deepest to root (bubble up)
    for entry in result.path().iter().rev() {
        let local_position = entry.global_to_local(event.position);
        let local_event = PointerEvent {
            position: local_position,
            ..event
        };
        
        // Get render object and dispatch
        if let Some(handler) = get_event_handler(entry.render_id) {
            let handled = handler.handle_event(&local_event);
            if handled {
                break; // Event consumed
            }
        }
    }
}
```

## Performance Considerations

### Bounding Box Optimization

Skip hit testing if position is outside bounding box:

```rust
fn hit_test(&self, ctx: BoxHitTestCtx<Variable>) -> bool {
    // Quick reject with bounding box
    if !self.bounding_box().contains(ctx.position) {
        return false;
    }
    
    // Detailed hit testing...
}
```

### Spatial Partitioning

For many children, use spatial data structures:

```rust
pub struct SpatialIndex {
    grid: HashMap<(i32, i32), Vec<RenderId>>,
    cell_size: f32,
}

impl SpatialIndex {
    pub fn query(&self, position: Offset) -> impl Iterator<Item = RenderId> {
        let cell = self.position_to_cell(position);
        self.grid.get(&cell).into_iter().flatten().copied()
    }
}
```

### Caching Hit Test Results

For static content:

```rust
pub struct CachedHitTestResult {
    position: Offset,
    result: BoxHitTestResult,
    valid: bool,
}

impl CachedHitTestResult {
    pub fn get_or_compute(
        &mut self,
        position: Offset,
        compute: impl FnOnce() -> BoxHitTestResult,
    ) -> &BoxHitTestResult {
        if !self.valid || self.position != position {
            self.position = position;
            self.result = compute();
            self.valid = true;
        }
        &self.result
    }
    
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
}
```

## Debugging Hit Tests

```rust
#[cfg(debug_assertions)]
pub fn debug_hit_test(result: &BoxHitTestResult, position: Offset) {
    tracing::debug!(
        "Hit test at ({}, {}): {} entries",
        position.x, position.y,
        result.path().len()
    );
    
    for (i, entry) in result.path().iter().enumerate() {
        tracing::debug!(
            "  [{}] {:?} transform={:?}",
            i, entry.render_id, entry.transform
        );
    }
}
```
