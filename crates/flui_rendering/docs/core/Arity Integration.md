# Arity Integration

**How flui-tree's arity system integrates into flui-rendering**

---

## Overview

FLUI integrates compile-time arity validation from `flui-tree` into `flui-rendering` while maintaining a clean, Flutter-like API. The arity system provides type safety at compile-time while keeping the user-facing API simple and familiar.

**Key principles:**
- **Arity is hidden** - validation happens internally, not exposed in API
- **Flutter-like API** - simple methods like `set_child()`, `add()`, `remove()`
- **Zero boilerplate** - Ambassador delegates everything automatically
- **Compile-time safety** - `Exact<1>`, `Exact<2>`, `Variable` in type signatures
- **Runtime validation** - debug assertions catch violations early

---

## Architecture

### Arity as Type Parameter

Containers accept an `Arity` generic parameter that defines valid child counts:

```rust
pub struct Proxy<P: Protocol, A: Arity = Exact<1>> {
    children: TypedChildren<P, A>,
    geometry: P::Geometry,
}

pub struct MultiChildren<P: Protocol, PD: ParentData, A: Arity = Variable> {
    storage: ArityStorage<Box<P::Object>, A>,
    _parent_data: PhantomData<PD>,
}
```

**Key insight:** The container knows its arity through the generic parameter, but this is hidden from the public API.

### Type Aliases

Common arity patterns have ergonomic aliases:

```rust
// Single child (exactly 1)
pub type ProxyBox<A = Exact<1>> = Proxy<BoxProtocol, A>;
pub type Single<P> = Proxy<P, Exact<1>>;

// Optional child (0 or 1)
pub type OptionalChild<P> = Proxy<P, Optional>;

// Exactly N children
pub type ExactN<P, const N: usize> = MultiChildren<P, BoxParentData, Exact<N>>;

// Variable children (any number)
pub type BoxChildren<PD> = MultiChildren<BoxProtocol, PD, Variable>;

// Bounded children (MIN to MAX)
pub type BoundedChildren<P, PD, const MIN: usize, const MAX: usize> = 
    MultiChildren<P, PD, Range<MIN, MAX>>;
```

---

## Flutter-Like API

### Single-Child API

The `RenderProxyBox` trait provides Flutter-like methods for single-child nodes:

```rust
#[delegatable_trait]
pub trait RenderProxyBox: SingleChildRenderBox {
    /// Get the child (if any).
    fn child(&self) -> Option<&dyn RenderBox>;
    
    /// Get mutable child (if any).
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;
    
    /// Set the child (replaces existing if any).
    fn set_child(&mut self, child: Box<dyn RenderBox>);
    
    /// Take the child out (removes from tree).
    fn take_child(&mut self) -> Option<Box<dyn RenderBox>>;
    
    /// Layout (delegates to child).
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;
    
    /// Paint (delegates to child).
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
}
```

**Note:** No arity-specific methods exposed. Validation happens internally.

### Multi-Child API

The `MultiChildRenderBox` trait provides Flutter-like collection methods:

```rust
#[delegatable_trait]
pub trait MultiChildRenderBox: RenderBox {
    /// Add child to the end.
    fn add(&mut self, child: Box<dyn RenderBox>);
    
    /// Insert child at index.
    fn insert(&mut self, index: usize, child: Box<dyn RenderBox>);
    
    /// Remove child at index.
    fn remove(&mut self, index: usize) -> Box<dyn RenderBox>;
    
    /// Remove all children.
    fn clear(&mut self);
    
    /// Get child count.
    fn child_count(&self) -> usize;
    
    /// Get child by index.
    fn child_at(&self, index: usize) -> Option<&dyn RenderBox>;
    
    /// Get mutable child by index.
    fn child_at_mut(&mut self, index: usize) -> Option<&mut dyn RenderBox>;
    
    /// Iterate over children.
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox>;
    
    /// Iterate mutably over children.
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox>;
}
```

**Note:** Methods panic in debug builds on arity violations (like Flutter's assertions).

---

## Container Implementation

### ProxyBox Implementation

The `Proxy` container implements `RenderProxyBox` with internal arity validation:

```rust
impl<A: Arity> RenderProxyBox for Proxy<BoxProtocol, A> {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.children.iter().next()
    }
    
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.children.iter_mut().next()
    }
    
    fn set_child(&mut self, child: Box<dyn RenderBox>) {
        // Arity validation happens here (hidden from user!)
        debug_assert!(
            A::validate_count(1),
            "Cannot set child: arity {:?} doesn't allow children",
            A::runtime_arity()
        );
        
        // Clear existing children
        self.children.clear();
        
        // Add new child
        self.children.storage.try_push(child)
            .expect("Arity violation: cannot set child");
    }
    
    fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.children.storage.pop()
    }
    
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = self.child_mut() {
            let child_size = child.perform_layout(constraints);
            self.geometry = child_size;
            child_size
        } else {
            constraints.smallest()
        }
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
}
```

**Key points:**
- Arity validation uses `A::validate_count()` internally
- Debug assertions catch violations early
- Public API remains clean (no arity methods exposed)

### MultiChildren Implementation

The `MultiChildren` container implements `MultiChildRenderBox`:

```rust
impl<PD: ParentData, A: Arity> MultiChildRenderBox 
    for MultiChildren<BoxProtocol, PD, A> 
{
    fn add(&mut self, child: Box<dyn RenderBox>) {
        // Arity validation hidden inside
        self.storage.try_push(child)
            .expect("Cannot add child: arity limit reached");
    }
    
    fn insert(&mut self, index: usize, child: Box<dyn RenderBox>) {
        debug_assert!(
            index <= self.child_count(),
            "Index out of bounds: {index} > {}",
            self.child_count()
        );
        
        self.storage.validate_can_add()
            .expect("Cannot insert child: arity limit reached");
        
        self.storage.insert(index, child);
    }
    
    fn remove(&mut self, index: usize) -> Box<dyn RenderBox> {
        debug_assert!(
            index < self.child_count(),
            "Index out of bounds: {index} >= {}",
            self.child_count()
        );
        
        self.storage.validate_can_remove()
            .expect("Cannot remove child: minimum arity not met");
        
        self.storage.remove(index)
    }
    
    fn clear(&mut self) {
        if !A::validate_count(0) {
            panic!(
                "Cannot clear: arity {:?} requires at least {} children",
                A::runtime_arity(),
                A::runtime_arity().min_count()
            );
        }
        
        self.storage.clear();
    }
    
    fn child_count(&self) -> usize {
        self.storage.len()
    }
    
    fn child_at(&self, index: usize) -> Option<&dyn RenderBox> {
        self.storage.get(index).map(|b| &**b)
    }
    
    fn child_at_mut(&mut self, index: usize) -> Option<&mut dyn RenderBox> {
        self.storage.get_mut(index).map(|b| &mut **b)
    }
    
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
        self.storage.iter().map(|b| &**b)
    }
    
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox> {
        self.storage.iter_mut().map(|b| &mut **b)
    }
}
```

---

## Usage Examples

### Example 1: RenderOpacity (Single Child)

Exactly 1 child required:

```rust
use ambassador::Delegate;

#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox<Exact<1>>,  // ✅ Arity = Exact<1>
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

// Usage - Flutter-like API
let mut opacity = RenderOpacity::new(0.5);

// Set child
opacity.set_child(Box::new(some_child));

// Get child
if let Some(child) = opacity.child() {
    // Use child
}

// Take child out
let child = opacity.take_child();
```

**Arity validation:**
```rust
// ✅ OK: setting 1 child
opacity.set_child(child);

// ❌ Debug panic: "Exact<1> doesn't allow 0 children"
let empty_opacity = RenderOpacity::new(0.5);
empty_opacity.perform_layout(constraints);  // Panics if no child
```

### Example 2: RenderSizedBox (Optional Child)

0 or 1 child allowed:

```rust
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderSizedBox {
    proxy: ProxyBox<Optional>,  // ✅ Arity = Optional (0 or 1)
    width: Option<f32>,
    height: Option<f32>,
}

impl RenderProxyBox for RenderSizedBox {}

// Usage
let mut sized_box = RenderSizedBox::new(Some(100.0), Some(100.0));

// Can work without child
sized_box.perform_layout(constraints);  // ✅ OK: returns fixed size

// Can have child
sized_box.set_child(Box::new(child));
```

### Example 3: RenderSwitcher (Exactly 2 Children)

Must have exactly 2 children:

```rust
#[derive(Debug)]
pub struct RenderSwitcher {
    children: ExactN<BoxProtocol, 2>,  // ✅ Arity = Exact<2>
    active_index: usize,
}

impl RenderSwitcher {
    pub fn new(child_a: Box<dyn RenderBox>, child_b: Box<dyn RenderBox>) -> Self {
        let mut children = ExactN::new();
        children.add(child_a);
        children.add(child_b);
        
        Self {
            children,
            active_index: 0,
        }
    }
    
    pub fn switch(&mut self) {
        self.active_index = 1 - self.active_index;
    }
    
    pub fn active_child(&self) -> &dyn RenderBox {
        self.children.child_at(self.active_index).unwrap()
    }
}

impl MultiChildRenderBox for RenderSwitcher {
    fn add(&mut self, child: Box<dyn RenderBox>) {
        self.children.add(child);
    }
    
    // Delegate other methods...
    fn child_count(&self) -> usize {
        self.children.child_count()
    }
    
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
        self.children.children()
    }
    
    // ... etc
}

// Usage
let mut switcher = RenderSwitcher::new(child_a, child_b);
switcher.switch();  // Toggle between children

// ❌ Debug panic: "Exact<2> doesn't allow 3 children"
switcher.add(child_c);  // Panic in debug build
```

### Example 4: RenderFlex (Variable Children)

Any number of children:

```rust
#[derive(Debug)]
pub struct RenderFlex {
    children: BoxChildren<FlexParentData>,  // ✅ Arity = Variable
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
}

impl MultiChildRenderBox for RenderFlex {
    fn add(&mut self, child: Box<dyn RenderBox>) {
        self.children.add(child);
    }
    
    fn insert(&mut self, index: usize, child: Box<dyn RenderBox>) {
        self.children.insert(index, child);
    }
    
    fn remove(&mut self, index: usize) -> Box<dyn RenderBox> {
        self.children.remove(index)
    }
    
    fn child_count(&self) -> usize {
        self.children.child_count()
    }
    
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox> {
        self.children.children()
    }
    
    // ... other methods
}

// Usage - Flutter-like API
let mut flex = RenderFlex::new(Axis::Vertical);

// Add any number of children
flex.add(Box::new(child1));
flex.add(Box::new(child2));
flex.add(Box::new(child3));
// ... can add unlimited children

// Access by index
if let Some(first) = flex.child_at(0) {
    // Use first child
}

// Remove
let removed = flex.remove(1);

// Iterate
for child in flex.children() {
    // Process each child
}
```

### Example 5: RenderTabBar (Bounded Children)

2 to 10 children (tabs):

```rust
#[derive(Debug)]
pub struct RenderTabBar {
    children: BoundedChildren<
        BoxProtocol,
        BoxParentData,
        2,   // MIN = 2 tabs
        10   // MAX = 10 tabs
    >,
    selected_index: usize,
}

impl MultiChildRenderBox for RenderTabBar {
    fn add(&mut self, child: Box<dyn RenderBox>) {
        self.children.add(child);
    }
    
    // ... delegate other methods
}

// Usage
let mut tab_bar = RenderTabBar::new();

// Must add at least 2 tabs
tab_bar.add(tab1);
tab_bar.add(tab2);  // ✅ Now valid (>= 2)

// Can add up to 10 tabs
tab_bar.add(tab3);
tab_bar.add(tab4);
// ... up to tab10

// ❌ Debug panic: "Range<2, 10> doesn't allow 11 children"
tab_bar.add(tab11);  // Panic in debug build
```

---

## ArityStorage (Internal)

The `ArityStorage` enum bridges the arity system with container storage:

```rust
pub enum ArityStorage<T, A: Arity> {
    /// No children (Leaf).
    Leaf(PhantomData<(T, A)>),
    
    /// Optional child (0 or 1).
    Optional(Option<T>),
    
    /// Exactly N children (stack-allocated for N <= 8).
    Exact(SmallVec<[T; 8]>),
    
    /// Variable children (heap-allocated).
    Variable(Vec<T>),
    
    /// Range-bounded children.
    Range(Vec<T>),
}
```

### Internal Validation Methods

```rust
impl<T, A: Arity> ArityStorage<T, A> {
    /// Check if can add another child (internal).
    fn validate_can_add(&self) -> Result<(), ArityError> {
        let count = self.len();
        if !A::validate_count(count + 1) {
            return Err(ArityError::TooManyChildren {
                arity: A::runtime_arity(),
                attempted: count + 1,
            });
        }
        Ok(())
    }
    
    /// Check if can remove a child (internal).
    fn validate_can_remove(&self) -> Result<(), ArityError> {
        let count = self.len();
        if !A::validate_count(count - 1) {
            return Err(ArityError::TooFewChildren {
                arity: A::runtime_arity(),
                attempted: count - 1,
            });
        }
        Ok(())
    }
}
```

### Storage Strategies

| Arity | Storage | Reason |
|-------|---------|--------|
| `Leaf` | `PhantomData` | Zero size (no children) |
| `Optional` | `Option<T>` | Minimal overhead for 0-1 child |
| `Exact<N>` | `SmallVec<[T; 8]>` | Stack allocation for small N |
| `Variable` | `Vec<T>` | Heap allocation for dynamic size |
| `Range<MIN, MAX>` | `Vec<T>` | Heap with bounds checking |

**Performance:**
- `Leaf`: Zero cost (ZST)
- `Optional`: Single pointer size
- `Exact<N>` where N ≤ 8: Stack-allocated (no heap)
- `Exact<N>` where N > 8: Falls back to heap
- `Variable`/`Range`: Standard `Vec` performance

---

## Delegation Pattern

Ambassador automatically delegates all trait methods:

```rust
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox<Exact<1>>,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}
```

**Ambassador generates:**

```rust
impl RenderProxyBox for RenderOpacity {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.proxy.child()  // Delegates to proxy
    }
    
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.proxy.child_mut()
    }
    
    fn set_child(&mut self, child: Box<dyn RenderBox>) {
        self.proxy.set_child(child)  // Arity validation inside proxy
    }
    
    fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.proxy.take_child()
    }
    
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.proxy.perform_layout(constraints)
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        self.proxy.paint(context, offset)
    }
}
```

**Benefits:**
- **Zero boilerplate**: One `#[delegate]` line
- **Type-safe**: Arity from proxy's generic parameter
- **Clean API**: No arity methods exposed
- **Automatic**: Ambassador handles all forwarding

---

## Error Handling

### Debug Assertions

In debug builds, arity violations panic with clear messages:

```rust
// Exact<1> with 0 children
let opacity = RenderOpacity::new(0.5);
opacity.perform_layout(constraints);
// Panic: "Exact<1> expects 1 child, got 0"

// Exact<2> with 3 children
let mut switcher = RenderSwitcher::new(child_a, child_b);
switcher.add(child_c);
// Panic: "Exact<2> expects 2 children, attempting 3"
```

### Release Builds

In release builds, assertions are removed but logic remains:

```rust
// Option 1: Graceful handling (recommended)
if let Some(child) = opacity.child() {
    child.perform_layout(constraints);
} else {
    // Handle missing child case
    constraints.smallest()
}

// Option 2: Result-based API (future enhancement)
opacity.try_set_child(child)?;  // Returns Result
```

### ArityError Type

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArityError {
    TooManyChildren {
        arity: RuntimeArity,
        attempted: usize,
    },
    TooFewChildren {
        arity: RuntimeArity,
        attempted: usize,
    },
    InvalidChildCount {
        arity: RuntimeArity,
        actual: usize,
    },
}
```

**Display:**
```rust
let error = ArityError::TooManyChildren {
    arity: RuntimeArity::Exact(2),
    attempted: 3,
};

println!("{}", error);
// Output: "Too many children: arity Exact(2 children) does not allow 3 children"
```

---

## Performance Characteristics

### Compile-Time Optimization

```rust
// Exact<1>: Compiler can optimize away bounds checks
let child = opacity.child().unwrap();  
// In release: Direct pointer dereference (no check)

// Variable: Standard Vec performance
flex.add(child);  
// Vec::push with amortized O(1)
```

### Memory Layout

```rust
// Exact<1> with Proxy
struct RenderOpacity {
    proxy: ProxyBox<Exact<1>>,  // Size: ~24 bytes
    opacity: f32,                // Size: 4 bytes
}
// Total: ~32 bytes (with padding)

// Variable with MultiChildren
struct RenderFlex {
    children: BoxChildren<FlexParentData>,  // Size: 24 bytes (Vec)
    direction: Axis,                         // Size: 1 byte
    // ...
}
```

### Arity Validation Cost

| Operation | Debug Build | Release Build |
|-----------|-------------|---------------|
| `set_child()` | Check + panic | No check |
| `add()` | Check + panic | No check |
| `remove()` | Check + panic | No check |
| `child()` | No check | No check |
| `child_at()` | Bounds check | Bounds check (Vec) |

**Zero overhead in release builds** for arity validation (debug assertions removed).

---

## Comparison with Flutter

### Flutter (Dart)

```dart
class RenderOpacity extends RenderProxyBox {
  RenderOpacity({
    required double opacity,
    RenderBox? child,
  }) : _opacity = opacity, super(child);

  double _opacity;
  
  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    child?.attach(owner);  // Runtime null check
  }
  
  @override
  void performLayout() {
    if (child != null) {
      child!.layout(constraints, parentUsesSize: true);
      size = child!.size;
    } else {
      size = constraints.smallest;
    }
  }
}
```

### FLUI (Rust)

```rust
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox<Exact<1>>,  // ✅ Compile-time arity
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

// Usage identical to Flutter
opacity.set_child(child);
opacity.perform_layout(constraints);
```

### Key Differences

| Aspect | Flutter | FLUI |
|--------|---------|------|
| **Arity Check** | Runtime (null checks) | Compile-time (generic) |
| **Memory** | 8 bytes (nullable pointer) | 24 bytes (Vec with capacity) |
| **Safety** | Runtime errors | Debug assertions |
| **Performance** | Null checks always | Zero checks in release |
| **Type Safety** | No (any child count) | Yes (Exact<1> in type) |

---

## Best Practices

### Choose the Right Arity

```rust
// ✅ Single child wrapper (Opacity, Transform, etc.)
proxy: ProxyBox<Exact<1>>

// ✅ Optional child (SizedBox, Container)
proxy: ProxyBox<Optional>

// ✅ Multiple fixed children (Switcher = 2, TabBarView = N)
children: ExactN<BoxProtocol, 2>

// ✅ Dynamic list (Flex, Stack, Column)
children: BoxChildren<FlexParentData>

// ✅ Bounded list (TabBar = 2-10 tabs)
children: BoundedChildren<BoxProtocol, BoxParentData, 2, 10>

// ✅ Leaf nodes (Text, Image)
// No children field at all
```

### Handle Missing Children Gracefully

```rust
// ✅ Good: Check before use
if let Some(child) = self.child() {
    child.perform_layout(constraints);
}

// ✅ Good: Provide fallback
let size = self.child()
    .map(|c| c.perform_layout(constraints))
    .unwrap_or_else(|| constraints.smallest());

// ❌ Bad: Assume child exists
let child = self.child().unwrap();  // May panic at runtime
```

### Use Type Aliases

```rust
// ✅ Good: Use semantic type alias
pub type ProxyBox<A = Exact<1>> = Proxy<BoxProtocol, A>;

struct RenderOpacity {
    proxy: ProxyBox,  // Clear intent
}

// ❌ Avoid: Raw generic everywhere
struct RenderOpacity {
    proxy: Proxy<BoxProtocol, Exact<1>>,  // Verbose
}
```

### Leverage Debug Assertions

```rust
// ✅ Good: Let debug assertions catch errors
flex.add(child1);
flex.add(child2);
// Debug build will panic if arity violated

// ❌ Avoid: Manual validation
if flex.child_count() < max {
    flex.add(child);
}
// Duplicates internal arity validation
```

---

## Migration Guide

### From Old Container System

**Before (old system):**
```rust
struct RenderOpacity {
    child: Option<Box<dyn RenderBox>>,
    opacity: f32,
}

impl RenderOpacity {
    pub fn set_child(&mut self, child: Box<dyn RenderBox>) {
        self.child = Some(child);
    }
}
```

**After (with arity):**
```rust
#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
struct RenderOpacity {
    proxy: ProxyBox<Exact<1>>,  // ✅ Arity integrated
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}
```

### Benefits of Migration

1. **Type safety**: Arity in type signature (`Exact<1>`)
2. **Less code**: Ambassador handles delegation
3. **Better errors**: Clear arity violation messages
4. **Zero overhead**: No runtime cost in release builds
5. **Flexibility**: Easy to change arity (just change generic)

---

## Advanced Patterns

### Custom Arity for Specialized Objects

```rust
// Grid with exactly MxN children
pub struct RenderGrid<const ROWS: usize, const COLS: usize> {
    children: ExactN<BoxProtocol, { ROWS * COLS }>,
}

// Arity = Exact<ROWS * COLS> (compile-time validated!)
```

### Arity as Constraint

```rust
pub trait GridLayout {
    type Arity: Arity;
    
    fn layout_grid(&mut self, constraints: BoxConstraints);
}

impl GridLayout for RenderGrid<2, 3> {
    type Arity = Exact<6>;  // 2x3 = 6 children required
}
```

### Dynamic Arity Switching

```rust
pub enum RenderDynamic {
    Single(ProxyBox<Exact<1>>),
    Multi(BoxChildren<BoxParentData>),
}

impl RenderDynamic {
    pub fn as_single(&self) -> Option<&ProxyBox<Exact<1>>> {
        match self {
            Self::Single(proxy) => Some(proxy),
            _ => None,
        }
    }
}
```

---

## Debugging

### Arity Validation Errors

```rust
// Enable arity debug info (internal API)
#[cfg(debug_assertions)]
impl<A: Arity> ProxyBox<A> {
    pub fn debug_arity(&self) {
        println!("Arity: {:?}", A::runtime_arity());
        println!("Current count: {}", self.children.len());
        println!("Valid: {}", A::validate_count(self.children.len()));
    }
}

// Usage in tests
opacity.debug_arity();
// Output:
// Arity: Exact(1)
// Current count: 0
// Valid: false
```

### Tracing Integration

```rust
use tracing::debug;

impl<A: Arity> ProxyBox<A> {
    pub fn set_child(&mut self, child: Box<dyn RenderBox>) {
        debug!(
            arity = ?A::runtime_arity(),
            current = self.children.len(),
            "Setting child"
        );
        
        // ... validation and set logic
    }
}
```

---

## Summary

| Aspect | Details |
|--------|---------|
| **API Style** | Flutter-like (set_child, add, remove) |
| **Arity Location** | Type parameter in container |
| **Validation** | Internal (not exposed to user) |
| **Delegation** | Automatic via Ambassador |
| **Error Handling** | Debug assertions (panic in debug) |
| **Performance** | Zero overhead in release builds |
| **Type Safety** | Compile-time arity validation |
| **Memory** | Optimized storage (SmallVec for small N) |

---

## Next Steps

1. **Implement ArityStorage** - Create the bridge enum
2. **Update Containers** - Add Arity generic parameter
3. **Refactor Objects** - Use new container types
4. **Add Tests** - Verify arity validation
5. **Benchmark** - Measure performance impact

---

**See Also:**
- [[Lifecycle]] - Render object lifecycle states
- [[Containers]] - Container implementations
- [[Delegation Pattern]] - Ambassador usage
- [[Render Tree]] - Tree structure and relationships

---

**References:**
- `flui-tree/src/arity/mod.rs` - Arity trait and types
- `flui-rendering/src/containers/` - Container implementations
- Flutter's `RenderObject` - Original API design
