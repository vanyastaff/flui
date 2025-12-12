# Adapter Container

**Cross-protocol wrapper for embedding objects of one protocol within another**

---

## Overview

The `Adapter` container enables cross-protocol composition by wrapping a container of one protocol and exposing a different protocol interface. This allows embedding Box objects within Sliver contexts and vice versa, matching Flutter's adapter pattern.

**Key insight:** Protocol conversion is encoded in the *type* (`Adapter<Inner, TargetProtocol>`), not in separate methods. This keeps the API simple - there's only one `add()` method, and the type system handles protocol compatibility.

---

## Architecture

### Basic Structure

```rust
/// Protocol adapter container.
///
/// Wraps a container of one protocol and exposes a different protocol.
///
/// # Type Parameters
///
/// - `C`: Inner container type (e.g., `Single<BoxProtocol>`)
/// - `ToProtocol`: Target protocol that this adapter exposes
///
/// # Memory Layout
///
/// - Size: Same as inner container (zero-cost wrapper)
/// - `PhantomData<ToProtocol>` is zero-sized
pub struct Adapter<C, ToProtocol> {
    inner: C,
    _to_protocol: PhantomData<ToProtocol>,
}

impl<C, ToProtocol> Adapter<C, ToProtocol> {
    /// Create new adapter
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            _to_protocol: PhantomData,
        }
    }
    
    /// Get inner container
    pub fn inner(&self) -> &C {
        &self.inner
    }
    
    /// Get mutable inner container
    pub fn inner_mut(&mut self) -> &mut C {
        &mut self.inner
    }
    
    /// Unwrap into inner container
    pub fn into_inner(self) -> C {
        self.inner
    }
}
```

### Zero-Cost Abstraction

```rust
// Adapter adds no runtime overhead
assert_eq!(
    std::mem::size_of::<Adapter<Single<BoxProtocol>, SliverProtocol>>(),
    std::mem::size_of::<Single<BoxProtocol>>()
);
// Both are the same size!

// PhantomData<ToProtocol> is zero-sized
assert_eq!(std::mem::size_of::<PhantomData<SliverProtocol>>(), 0);
```

---

## Type Aliases

Common adapter patterns have ergonomic type aliases:

```rust
/// Single Box child exposed as Sliver protocol
pub type BoxToSliver = Adapter<Single<BoxProtocol>, SliverProtocol>;

/// Multiple Sliver children exposed as Box protocol
pub type SliverToBox = Adapter<Children<SliverProtocol>, BoxProtocol>;

/// Optional Box child exposed as Sliver protocol
pub type OptionalBoxToSliver = Adapter<Optional<BoxProtocol>, SliverProtocol>;

/// Single Sliver child exposed as Box protocol
pub type SliverSingleToBox = Adapter<Single<SliverProtocol>, BoxProtocol>;
```

---

## Usage Examples

### Example 1: SliverToBoxAdapter (Box → Sliver)

Wraps a single Box child and exposes it as a Sliver:

```rust
use flui_rendering::{BoxToSliver, RenderBox, RenderSliver};

/// Sliver that contains a single Box child.
///
/// Flutter equivalent: `SliverToBoxAdapter`
#[derive(Debug)]
pub struct RenderSliverToBoxAdapter {
    // Type encodes: Box child, Sliver protocol
    child: BoxToSliver,
    geometry: SliverGeometry,
}

impl RenderSliverToBoxAdapter {
    pub fn new(child: Box<dyn RenderBox>) -> Self {
        // Create Single container
        let mut single = Single::new();
        single.set_child(child);
        
        Self {
            child: Adapter::new(single),
            geometry: SliverGeometry::zero(),
        }
    }
    
    pub fn child(&self) -> Option<&dyn RenderBox> {
        self.child.inner().child()
    }
    
    pub fn set_child(&mut self, child: Box<dyn RenderBox>) {
        self.child.inner_mut().set_child(child);
    }
}

// Implements target protocol (Sliver)
impl RenderSliver for RenderSliverToBoxAdapter {
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        if let Some(child) = self.child.inner_mut().child_mut() {
            // Convert sliver constraints → box constraints
            let box_constraints = BoxConstraints::new(
                0.0,
                constraints.cross_axis_extent,
                0.0,
                f32::INFINITY,
            );
            
            // Layout box child
            let size = child.perform_layout(box_constraints);
            
            // Convert box size → sliver geometry
            self.geometry = SliverGeometry {
                scroll_extent: size.height,
                paint_extent: size.height.min(constraints.remaining_paint_extent),
                max_paint_extent: size.height,
                hit_test_extent: size.height,
                visible: true,
                ..Default::default()
            };
            
            self.geometry
        } else {
            SliverGeometry::zero()
        }
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.inner().child() {
            context.paint_child(child, offset);
        }
    }
}

// Usage - simple add()!
let box_widget = RenderPadding::new(
    EdgeInsets::all(16.0),
    Box::new(RenderText::new("Hello")),
);

let adapter = RenderSliverToBoxAdapter::new(Box::new(box_widget));

let mut sliver_list = RenderSliverList::new();
sliver_list.add(Box::new(adapter));  // ✅ Just add() - type system handles it!
```

### Example 2: ShrinkWrappingViewport (Sliver → Box)

Wraps multiple Sliver children and exposes them as a Box:

```rust
use flui_rendering::{SliverToBox, RenderBox, RenderSliver};

/// Box that contains Sliver children.
///
/// Flutter equivalent: `ShrinkWrappingViewport`
#[derive(Debug)]
pub struct RenderShrinkWrappingViewport {
    // Type encodes: Sliver children, Box protocol
    children: SliverToBox,
    offset: ViewportOffset,
    size: Size,
}

impl RenderShrinkWrappingViewport {
    pub fn new() -> Self {
        Self {
            children: Adapter::new(Children::new()),
            offset: ViewportOffset::zero(),
            size: Size::zero(),
        }
    }
    
    pub fn add(&mut self, sliver: Box<dyn RenderSliver>) {
        self.children.inner_mut().add(sliver);
    }
    
    pub fn children(&self) -> impl Iterator<Item = &dyn RenderSliver> {
        self.children.inner().children()
    }
}

// Implements target protocol (Box)
impl RenderBox for RenderShrinkWrappingViewport {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let mut total_height = 0.0;
        let mut scroll_offset = 0.0;
        
        for sliver in self.children.inner_mut().children_mut() {
            // Convert box constraints → sliver constraints
            let sliver_constraints = SliverConstraints {
                axis_direction: AxisDirection::Down,
                scroll_offset,
                overlap: 0.0,
                remaining_paint_extent: constraints.max_height - total_height,
                cross_axis_extent: constraints.max_width,
                cross_axis_direction: AxisDirection::Right,
                viewport_main_axis_extent: constraints.max_height,
                remaining_cache_extent: 0.0,
                cache_origin: 0.0,
            };
            
            // Layout sliver child
            let geometry = sliver.perform_layout(sliver_constraints);
            
            total_height += geometry.scroll_extent;
            scroll_offset += geometry.scroll_extent;
        }
        
        self.size = Size::new(
            constraints.max_width,
            total_height.min(constraints.max_height),
        );
        
        self.size
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let mut y = offset.dy;
        
        for sliver in self.children.inner().children() {
            sliver.paint(context, Offset::new(offset.dx, y));
            // Advance offset (simplified - real impl tracks geometry)
            y += 100.0;
        }
    }
}

// Usage - simple add()!
let mut viewport = RenderShrinkWrappingViewport::new();
viewport.add(Box::new(RenderSliverList::new()));
viewport.add(Box::new(RenderSliverGrid::new()));

// Viewport is Box protocol - can add to column
let mut column = RenderFlex::new(Axis::Vertical);
column.add(Box::new(viewport));  // ✅ Just add() - no special method!
```

### Example 3: Complex Mixed Protocol Tree

```rust
fn build_mixed_protocol_ui() -> Box<dyn RenderBox> {
    // Root: Box protocol
    let mut column = RenderFlex::new(Axis::Vertical);
    
    // Add regular box children
    column.add(Box::new(RenderPadding::new(
        EdgeInsets::all(16.0),
        Box::new(RenderText::new("Header")),
    )));
    
    // Add sliver content via viewport adapter
    let mut viewport = RenderShrinkWrappingViewport::new();
    //                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //                 Adapter<Children<Sliver>, Box>
    
    viewport.add(Box::new(RenderSliverList::new()));
    viewport.add(Box::new(RenderSliverGrid::new()));
    
    // Add box widget inside sliver via sliver adapter
    let box_widget = RenderContainer::new(
        Color::blue(),
        Size::new(100.0, 100.0),
    );
    let sliver_adapter = RenderSliverToBoxAdapter::new(Box::new(box_widget));
    //                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //                   Adapter<Single<Box>, Sliver>
    
    viewport.add(Box::new(sliver_adapter));
    
    // Add viewport to column (it's Box protocol!)
    column.add(Box::new(viewport));
    
    // Add footer
    column.add(Box::new(RenderText::new("Footer")));
    
    Box::new(column)
}

// ✅ Simple API throughout - just add()
// ✅ Types encode protocol conversion
// ✅ Compile-time safety
```

---

## Protocol Conversion Flow

### Visual Diagram

```
┌─────────────────────────────────────────────────────────┐
│  RenderSliverToBoxAdapter                                │
│  (implements SliverProtocol)                             │
│                                                           │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Adapter<Single<BoxProtocol>, SliverProtocol>    │    │
│  │                                                  │    │
│  │  ┌────────────────────────────────────────┐    │    │
│  │  │ Single<BoxProtocol>                    │    │    │
│  │  │                                         │    │    │
│  │  │  ┌──────────────────────────────────┐ │    │    │
│  │  │  │ Box<dyn RenderBox>               │ │    │    │
│  │  │  │ (e.g., RenderPadding)            │ │    │    │
│  │  │  └──────────────────────────────────┘ │    │    │
│  │  │                                         │    │    │
│  │  └────────────────────────────────────────┘    │    │
│  │                                                  │    │
│  └─────────────────────────────────────────────────┘    │
│                                                           │
│  ┌─────────────────────────────────────────────────┐    │
│  │ impl RenderSliver                               │    │
│  │  - perform_layout()                             │    │
│  │  - paint()                                      │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
                        │
                        │ Can be added to any SliverList
                        ▼
```

### Type Flow

```rust
// Step 1: Create box widget
let box_widget: Box<dyn RenderBox> = Box::new(RenderPadding::new(/* ... */));
//              ^^^^^^^^^^^^^^^^^^^
//              BoxProtocol

// Step 2: Wrap in adapter
let adapter: RenderSliverToBoxAdapter = RenderSliverToBoxAdapter::new(box_widget);
//           ^^^^^^^^^^^^^^^^^^^^^^^^
//           Implements RenderSliver (SliverProtocol)
//
//           Internal structure:
//           child: Adapter<Single<BoxProtocol>, SliverProtocol>
//                  ├── Inner: Single<BoxProtocol> (contains box_widget)
//                  └── Outer: SliverProtocol (what adapter exposes)

// Step 3: Add to sliver container
let mut sliver_list: RenderSliverList = RenderSliverList::new();
//                   ^^^^^^^^^^^^^^^^
//                   Accepts Box<dyn RenderSliver>

sliver_list.add(Box::new(adapter));
//              ^^^^^^^^^^^^^^^^^^
//              Box<dyn RenderSliver> ✅ Type matches!
```

---

## API Design Benefits

### Single Method API

```rust
// ✅ GOOD: One method - simple and clear
trait Container<P: Protocol> {
    fn add(&mut self, child: Box<P::Object>);
}

// Usage:
container.add(same_protocol_child);  // ✅ Direct add
container.add(adapter);              // ✅ Adapter's type handles conversion

// ❌ BAD: Multiple methods - confusing
trait Container<P: Protocol> {
    fn add(&mut self, child: Box<P::Object>);
    fn add_adapter<A: ProtocolAdapter<To = P>>(&mut self, adapter: Box<A>);
}

// User confusion:
container.add(child)?              // Which one?
container.add_adapter(child)?      // When to use this?
```

### Self-Documenting Types

```rust
// ✅ Type signature tells the whole story
pub struct RenderSliverToBoxAdapter {
    child: Adapter<Single<BoxProtocol>, SliverProtocol>,
    //             ^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^
    //             Inner: Box child     Outer: Sliver protocol
}

// vs less clear alternative
pub struct RenderSliverToBoxAdapter {
    child: Option<Box<dyn RenderBox>>,  // ❓ Doesn't show it's an adapter
}
```

### Compile-Time Safety

```rust
// ❌ Type mismatch caught at compile time
let mut sliver_list = RenderSliverList::new();
sliver_list.add(Box::new(RenderPadding::new(/* ... */)));
// Compile error:
// expected `Box<dyn RenderSliver>`, found `Box<RenderPadding>`
// RenderPadding implements BoxProtocol, not SliverProtocol

// ✅ Fix: Use adapter (explicit in type)
let adapter = RenderSliverToBoxAdapter::new(
    Box::new(RenderPadding::new(/* ... */))
);
sliver_list.add(Box::new(adapter));  // ✅ OK
```

---

## Implementation Patterns

### Pattern 1: Single Child Adapter

```rust
pub struct AdapterObject<FromProtocol, ToProtocol> {
    // Inner container with source protocol
    child: Adapter<Single<FromProtocol>, ToProtocol>,
    // Additional state...
}

// Example: Box → Sliver
pub struct RenderSliverToBoxAdapter {
    child: BoxToSliver,  // = Adapter<Single<BoxProtocol>, SliverProtocol>
    geometry: SliverGeometry,
}
```

### Pattern 2: Multi-Child Adapter

```rust
pub struct AdapterObject<FromProtocol, ToProtocol> {
    // Inner container with source protocol
    children: Adapter<Children<FromProtocol>, ToProtocol>,
    // Additional state...
}

// Example: Sliver → Box
pub struct RenderShrinkWrappingViewport {
    children: SliverToBox,  // = Adapter<Children<SliverProtocol>, BoxProtocol>
    size: Size,
}
```

### Pattern 3: Optional Child Adapter

```rust
pub struct AdapterObject<FromProtocol, ToProtocol> {
    // Inner container with source protocol
    child: Adapter<Optional<FromProtocol>, ToProtocol>,
    // Additional state...
}

// Example: Optional box in sliver
pub struct RenderSliverOptionalBox {
    child: OptionalBoxToSliver,  // = Adapter<Optional<BoxProtocol>, SliverProtocol>
    geometry: SliverGeometry,
}
```

---

## Comparison with Flutter

### Flutter Approach

```dart
// Flutter: Runtime type checks
class SliverToBoxAdapter extends RenderSliver {
  RenderBox? child;  // Nullable - runtime null checks
  
  @override
  void performLayout() {
    if (child != null) {
      child!.layout(constraints);  // Runtime null assertion
    }
  }
}

// Flutter: Separate classes for adapters
class SliverToBoxAdapter extends RenderSliver { /* ... */ }
class RenderShrinkWrappingViewport extends RenderBox { /* ... */ }
```

### FLUI Approach

```rust
// FLUI: Compile-time type safety
pub struct RenderSliverToBoxAdapter {
    child: BoxToSliver,  // Type encodes protocol conversion
    geometry: SliverGeometry,
}

impl RenderSliver for RenderSliverToBoxAdapter {
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        // No null checks needed - handled by inner Single container
        if let Some(child) = self.child.inner_mut().child_mut() {
            child.perform_layout(/* ... */);
        }
        // ...
    }
}

// FLUI: Generic Adapter type with specialization
pub type BoxToSliver = Adapter<Single<BoxProtocol>, SliverProtocol>;
pub type SliverToBox = Adapter<Children<SliverProtocol>, BoxProtocol>;
```

### Key Differences

| Aspect | Flutter | FLUI |
|--------|---------|------|
| **Type Safety** | Runtime checks | Compile-time |
| **Protocol Mixing** | Implicit | Explicit in type |
| **Null Safety** | Nullable pointers | Option<T> + inner container |
| **API Methods** | Varies by adapter | Uniform (add, child) |
| **Memory Overhead** | Pointer + null bit | Same + zero-cost PhantomData |

---

## Advanced Usage

### Generic Adapter Trait (Optional)

```rust
/// Trait for querying adapter information at compile-time.
pub trait AdapterType {
    type FromProtocol: Protocol;
    type ToProtocol: Protocol;
}

impl<C, ToProtocol> AdapterType for Adapter<C, ToProtocol>
where
    C: Container,
    C::Protocol: Protocol,
    ToProtocol: Protocol,
{
    type FromProtocol = C::Protocol;
    type ToProtocol = ToProtocol;
}

// Query adapter info
fn print_adapter_info<A: AdapterType>() {
    println!(
        "Adapter: {} → {}",
        std::any::type_name::<A::FromProtocol>(),
        std::any::type_name::<A::ToProtocol>()
    );
}

print_adapter_info::<BoxToSliver>();
// Output: "Adapter: BoxProtocol → SliverProtocol"
```

### Nested Adapters (Rare but Possible)

```rust
// Triple nesting: Box → Sliver → Box
type TripleAdapter = Adapter<
    Adapter<Single<BoxProtocol>, SliverProtocol>,
    BoxProtocol
>;

// While technically possible, this is rarely useful in practice
// Usually indicates a design issue if you need this
```

---

## Best Practices

### 1. Use Type Aliases

```rust
// ✅ Good: Clear intent
pub struct RenderSliverToBoxAdapter {
    child: BoxToSliver,
    geometry: SliverGeometry,
}

// ❌ Avoid: Verbose
pub struct RenderSliverToBoxAdapter {
    child: Adapter<Single<BoxProtocol>, SliverProtocol>,
    geometry: SliverGeometry,
}
```

### 2. Document Protocol Conversion

```rust
/// Sliver that wraps a single Box child.
///
/// **Protocol conversion:** Box → Sliver
///
/// This adapter allows embedding box-protocol widgets within
/// sliver-protocol contexts (e.g., inside a CustomScrollView).
pub struct RenderSliverToBoxAdapter {
    child: BoxToSliver,
}
```

### 3. Provide Helper Constructors

```rust
impl RenderSliverToBoxAdapter {
    /// Create adapter with child.
    pub fn new(child: Box<dyn RenderBox>) -> Self {
        let mut single = Single::new();
        single.set_child(child);
        
        Self {
            child: Adapter::new(single),
            geometry: SliverGeometry::zero(),
        }
    }
    
    /// Create adapter without child.
    pub fn empty() -> Self {
        Self {
            child: Adapter::new(Single::new()),
            geometry: SliverGeometry::zero(),
        }
    }
}
```

### 4. Implement Standard Methods

```rust
impl RenderSliverToBoxAdapter {
    /// Get child reference.
    pub fn child(&self) -> Option<&dyn RenderBox> {
        self.child.inner().child()
    }
    
    /// Get mutable child reference.
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.inner_mut().child_mut()
    }
    
    /// Set child (replaces existing).
    pub fn set_child(&mut self, child: Box<dyn RenderBox>) {
        self.child.inner_mut().set_child(child);
    }
    
    /// Take child out (removes from tree).
    pub fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.inner_mut().take_child()
    }
}
```

---

## Performance Characteristics

### Memory Layout

```rust
// Adapter adds no overhead
struct RenderSliverToBoxAdapter {
    child: Adapter<Single<BoxProtocol>, SliverProtocol>,  // 24 bytes
    geometry: SliverGeometry,                              // 32 bytes
}
// Total: 56 bytes

// Same as without adapter:
struct Hypothetical {
    child: Single<BoxProtocol>,  // 24 bytes
    geometry: SliverGeometry,    // 32 bytes
}
// Total: 56 bytes

// ✅ Zero-cost abstraction!
```

### Runtime Overhead

```rust
// No virtual dispatch overhead for adapter itself
let adapter: BoxToSliver = Adapter::new(single);

// inner() is #[inline] - compiles to direct field access
let inner = adapter.inner();

// Same performance as direct access
let inner = adapter.inner;  // (if field were public)
```

---

## Summary

| Feature | Details |
|---------|---------|
| **Purpose** | Cross-protocol composition |
| **Type Parameters** | `Adapter<InnerContainer, TargetProtocol>` |
| **Memory Cost** | Zero (PhantomData is ZST) |
| **Runtime Cost** | Zero (inlined accessors) |
| **API Simplicity** | Single `add()` method |
| **Type Safety** | Compile-time protocol checking |
| **Self-Documenting** | Type shows protocol conversion |

**Core benefit:** Protocol conversion is encoded in the *type system*, not in runtime methods. This keeps the API simple while providing strong compile-time guarantees.

---

## Next Steps

- Implement `RenderSliverToBoxAdapter` using `BoxToSliver`
- Implement `RenderShrinkWrappingViewport` using `SliverToBox`
- Add adapter support to all relevant render objects
- Write tests for cross-protocol composition
- Benchmark adapter overhead (should be zero)

---

**See Also:**
- [[Containers]] - Base container types
- [[Protocol]] - Protocol system overview
- [[Arity Integration]] - Arity validation with containers
- [[Object Catalog]] - All render objects including adapters

---

**References:**
- Flutter's `SliverToBoxAdapter`
- Flutter's `RenderShrinkWrappingViewport`
- Rust's zero-cost abstraction principles
