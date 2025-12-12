# Safety Features

**Compile-time, type-safe, and runtime safety guarantees in FLUI**

---

## Overview

FLUI leverages Rust's type system to provide **multiple layers of safety** that catch errors at different stages:

| Safety Level | When | How | Examples |
|--------------|------|-----|----------|
| **Type Safety** | Compile-time | Type system prevents mixing incompatible types | Branded IDs, Protocol bounds |
| **Compile Safety** | Compile-time | Const generics and assertions validate at compile | Const assertions, Typestate |
| **Runtime Safety** | Runtime | Checks with clear error messages | Depth limits, Arity validation |
| **Panic Safety** | Recovery | Maintain consistency even on panic | Transaction log, Checkpoints |

**Philosophy:** Make invalid states **unrepresentable** at compile-time, and catch runtime issues with **clear, actionable errors**.

---

## 1. Branded IDs

**Problem:** In systems with multiple tree types (Element tree, Render tree), IDs can be accidentally mixed, causing crashes or undefined behavior.

**Solution:** Brand IDs with phantom types so the compiler prevents mixing.

### Implementation

```rust
use std::marker::PhantomData;

/// Brand marker trait (sealed to prevent external implementation)
pub trait Brand: sealed::Sealed {}

mod sealed {
    pub trait Sealed {}
}

/// Element tree brand
pub struct ElementBrand;
impl sealed::Sealed for ElementBrand {}
impl Brand for ElementBrand {}

/// Render tree brand
pub struct RenderBrand;
impl sealed::Sealed for RenderBrand {}
impl Brand for RenderBrand {}

/// Branded ID tied to a specific tree type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrandedId<B: Brand> {
    index: usize,
    generation: u32,
    _brand: PhantomData<B>,
}

impl<B: Brand> BrandedId<B> {
    pub fn new(index: usize, generation: u32) -> Self {
        Self {
            index,
            generation,
            _brand: PhantomData,
        }
    }
    
    pub fn index(&self) -> usize {
        self.index
    }
    
    pub fn generation(&self) -> u32 {
        self.generation
    }
}

// Type aliases for each tree
pub type ElementId = BrandedId<ElementBrand>;
pub type RenderId = BrandedId<RenderBrand>;
```

### Usage

```rust
pub struct RenderTree {
    nodes: Slab<RenderNode>,
}

impl RenderTree {
    /// Get node by ID - only accepts RenderId
    pub fn get(&self, id: RenderId) -> &RenderNode {
        &self.nodes[id.index()]
    }
    
    /// Get mutable node
    pub fn get_mut(&mut self, id: RenderId) -> &mut RenderNode {
        &mut self.nodes[id.index()]
    }
}

// ✅ Compile-time safety
let render_id: RenderId = RenderId::new(0, 1);
let element_id: ElementId = ElementId::new(0, 1);

render_tree.get(render_id);      // ✅ OK
// render_tree.get(element_id);  // ❌ Compile error: wrong brand!
```

### Benefits

- **Zero runtime cost** - PhantomData is zero-sized
- **Compile-time prevention** - Can't mix IDs from different trees
- **Clear error messages** - Compiler tells you exactly what's wrong
- **Refactoring safety** - Change tree types without fear

### Flutter Comparison

**Flutter:**
```dart
// No type safety - all IDs are just ints
class RenderTree {
  Map<int, RenderNode> nodes = {};
  
  RenderNode? get(int id) => nodes[id];  // Can pass any int!
}

// Runtime error if you mix element and render IDs
final elementId = 42;
renderTree.get(elementId);  // Might return wrong object!
```

**FLUI:**
```rust
// Type safety - IDs are branded
render_tree.get(element_id);  // ❌ Compile error immediately
```

---

## 2. Typestate Pattern for Lifecycle

**Problem:** Flutter allows calling methods in invalid states (e.g., paint before layout), causing runtime crashes.

**Solution:** Encode lifecycle state in the type system using typestate pattern.

### Implementation

```rust
/// Typestate markers (zero-sized)
pub struct Detached;
pub struct Attached;
pub struct LaidOut;
pub struct Painted;

/// RenderNode with compile-time state tracking
pub struct RenderNode<State = Detached> {
    render_object: Box<dyn RenderObject>,
    parent: Option<RenderId>,
    depth: usize,
    constraints: Option<RenderConstraints>,
    geometry: Option<Geometry>,
    _state: PhantomData<State>,
}

impl RenderNode<Detached> {
    /// Create new detached node
    pub fn new(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object,
            parent: None,
            depth: 0,
            constraints: None,
            geometry: None,
            _state: PhantomData,
        }
    }
    
    /// Attach to tree (state transition: Detached → Attached)
    pub fn attach(
        self,
        owner: Rc<RefCell<PipelineOwner>>,
        parent: Option<RenderId>,
        depth: usize,
    ) -> RenderNode<Attached> {
        self.render_object.attach(owner);
        
        RenderNode {
            render_object: self.render_object,
            parent,
            depth,
            constraints: self.constraints,
            geometry: self.geometry,
            _state: PhantomData,
        }
    }
}

impl RenderNode<Attached> {
    /// Layout (state transition: Attached → LaidOut)
    pub fn layout(
        mut self,
        constraints: RenderConstraints
    ) -> RenderNode<LaidOut> {
        let geometry = self.render_object.perform_layout(constraints);
        
        RenderNode {
            render_object: self.render_object,
            parent: self.parent,
            depth: self.depth,
            constraints: Some(constraints),
            geometry: Some(geometry),
            _state: PhantomData,
        }
    }
    
    /// Can also mark needs layout (transition back to Attached)
    pub fn mark_needs_layout(self) -> RenderNode<Attached> {
        self  // Already Attached
    }
}

impl RenderNode<LaidOut> {
    /// Paint (state transition: LaidOut → Painted)
    pub fn paint(
        self,
        context: &mut PaintingContext,
        offset: Offset
    ) -> RenderNode<Painted> {
        self.render_object.paint(context, offset);
        
        RenderNode {
            render_object: self.render_object,
            parent: self.parent,
            depth: self.depth,
            constraints: self.constraints,
            geometry: self.geometry,
            _state: PhantomData,
        }
    }
    
    /// Relayout (transition back to Attached)
    pub fn mark_needs_layout(self) -> RenderNode<Attached> {
        RenderNode {
            render_object: self.render_object,
            parent: self.parent,
            depth: self.depth,
            constraints: None,
            geometry: None,
            _state: PhantomData,
        }
    }
}

impl RenderNode<Painted> {
    /// Repaint (transition back to LaidOut)
    pub fn mark_needs_paint(self) -> RenderNode<LaidOut> {
        RenderNode {
            render_object: self.render_object,
            parent: self.parent,
            depth: self.depth,
            constraints: self.constraints,
            geometry: self.geometry,
            _state: PhantomData,
        }
    }
}
```

### Usage

```rust
// Create detached node
let node = RenderNode::new(Box::new(RenderOpacity::new(0.5)));

// ❌ Can't paint yet - not attached!
// node.paint(context, offset);  // Compile error

// Attach
let attached = node.attach(owner, None, 0);

// ❌ Can't paint yet - not laid out!
// attached.paint(context, offset);  // Compile error

// Layout
let laid_out = attached.layout(constraints);

// ✅ NOW we can paint!
let painted = laid_out.paint(context, offset);
```

### State Diagram

```
Detached ──attach()──> Attached ──layout()──> LaidOut ──paint()──> Painted
                           ↑                      ↑
                           └─── mark_needs_layout() ─┘
```

### Benefits

- **Impossible states** - Can't paint before layout at compile-time
- **State transitions** - Explicit methods for each transition
- **Zero cost** - PhantomData compiles away completely
- **Self-documenting** - Type signature shows required state

### Limitations

For dynamic trees, we need runtime state tracking (enum). Typestate works best for:
- Builder patterns (fluent API)
- Single-threaded pipelines
- Known state sequences

For production, use **hybrid approach**: typestate for builders, enum for tree nodes.

---

## 3. Protocol Compatibility

**Problem:** Box protocol objects shouldn't be mixed with Sliver protocol objects, but nothing prevents it at compile-time in naive implementations.

**Solution:** Use trait bounds to enforce protocol compatibility.

### Implementation

```rust
/// Marker trait for protocol compatibility
pub trait Compatible<P: Protocol>: Protocol {
    /// Check if protocols are compatible
    fn is_compatible() -> bool {
        true
    }
}

// Box protocol is compatible with itself
impl Compatible<BoxProtocol> for BoxProtocol {}

// Sliver protocol is compatible with itself
impl Compatible<SliverProtocol> for SliverProtocol {}

// Box and Sliver are NOT compatible (no cross-impl)

/// Container with protocol checking
pub struct TypedChildren<P: Protocol, A: Arity = Variable> {
    storage: ArityStorage<Box<P::Object>, A>,
    _protocol: PhantomData<P>,
}

impl<P: Protocol, A: Arity> TypedChildren<P, A> {
    /// Add child with protocol compatibility check
    pub fn add<C>(&mut self, child: Box<C>)
    where
        C: RenderObject,
        C::Protocol: Compatible<P>,  // ✅ Compile-time check!
    {
        // Safe to cast - protocols are compatible
        let child_obj = child as Box<P::Object>;
        self.storage.try_push(child_obj)
            .expect("Arity violation");
    }
    
    /// Type-safe child access
    pub fn child_at(&self, index: usize) -> Option<&P::Object> {
        self.storage.get(index).map(|b| &**b)
    }
}
```

### Usage

```rust
// Box children container
let mut box_children: TypedChildren<BoxProtocol> = TypedChildren::new();

// ✅ OK - Box protocol object
box_children.add(Box::new(RenderOpacity::new(0.5)));
box_children.add(Box::new(RenderPadding::new(padding)));

// ❌ Compile error - Sliver protocol object!
// box_children.add(Box::new(RenderSliverList::new()));
// Error: SliverProtocol does not implement Compatible<BoxProtocol>

// Sliver children container
let mut sliver_children: TypedChildren<SliverProtocol> = TypedChildren::new();

// ✅ OK - Sliver protocol object
sliver_children.add(Box::new(RenderSliverList::new()));

// ❌ Compile error - Box protocol object!
// sliver_children.add(Box::new(RenderOpacity::new(0.5)));
```

### Error Messages

```rust
// When trying to mix protocols:
error[E0277]: the trait bound `SliverProtocol: Compatible<BoxProtocol>` is not satisfied
  --> src/main.rs:10:20
   |
10 |     box_children.add(Box::new(RenderSliverList::new()));
   |                  ^^^ the trait `Compatible<BoxProtocol>` is not implemented for `SliverProtocol`
```

### Benefits

- **Type safety** - Can't mix incompatible protocols
- **Clear errors** - Compiler explains exactly what's wrong
- **Zero runtime cost** - All checking at compile-time
- **Extensible** - Easy to add new protocol types

---

## 4. Depth Limits

**Problem:** Deeply nested trees can cause stack overflow during recursive operations.

**Solution:** Const generic depth limits with runtime validation.

### Implementation

```rust
use std::marker::PhantomData;

/// Maximum tree depth (configurable via const generic)
pub const DEFAULT_MAX_DEPTH: usize = 1000;

/// Const generic marker for depth limits
pub struct ConstUsize<const N: usize>;

/// Tree with compile-time depth configuration
pub struct RenderTree<const MAX_DEPTH: usize = DEFAULT_MAX_DEPTH> {
    nodes: Slab<RenderNode>,
    root: Option<RenderId>,
    _max_depth: PhantomData<ConstUsize<MAX_DEPTH>>,
}

impl<const MAX_DEPTH: usize> RenderTree<MAX_DEPTH> {
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            _max_depth: PhantomData,
        }
    }
    
    /// Adopt child with depth validation
    pub fn adopt_child(
        &mut self,
        parent_id: RenderId,
        child_id: RenderId
    ) -> Result<(), TreeError> {
        let parent_depth = self.nodes[parent_id.index()].depth;
        let new_child_depth = parent_depth + 1;
        
        // ✅ Validate against compile-time limit
        if new_child_depth > MAX_DEPTH {
            return Err(TreeError::MaxDepthExceeded {
                max: MAX_DEPTH,
                attempted: new_child_depth,
            });
        }
        
        // Update child
        let child = &mut self.nodes[child_id.index()];
        child.depth = new_child_depth;
        child.parent = Some(parent_id);
        
        Ok(())
    }
    
    /// Get max depth for this tree (const)
    pub const fn max_depth() -> usize {
        MAX_DEPTH
    }
}

/// Error type for tree operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    MaxDepthExceeded { max: usize, attempted: usize },
    NodeNotFound(RenderId),
    CycleDetected,
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxDepthExceeded { max, attempted } => {
                write!(
                    f,
                    "Maximum tree depth exceeded: limit={max}, attempted={attempted}"
                )
            }
            Self::NodeNotFound(id) => {
                write!(f, "Node not found: {:?}", id)
            }
            Self::CycleDetected => {
                write!(f, "Cycle detected in tree structure")
            }
        }
    }
}

impl std::error::Error for TreeError {}
```

### Usage

```rust
// Default depth limit (1000)
let mut tree: RenderTree = RenderTree::new();

// Custom shallow tree (100 levels max)
let mut shallow_tree: RenderTree<100> = RenderTree::new();

// Custom deep tree (10000 levels max)
let mut deep_tree: RenderTree<10000> = RenderTree::new();

// Try to adopt child
match tree.adopt_child(parent_id, child_id) {
    Ok(_) => {
        // Success - depth within limit
    }
    Err(TreeError::MaxDepthExceeded { max, attempted }) => {
        eprintln!("Tree too deep: max={max}, attempted={attempted}");
        // Handle error - maybe flatten tree
    }
    Err(e) => {
        eprintln!("Tree error: {}", e);
    }
}
```

### Benefits

- **Stack overflow prevention** - Catch deep nesting early
- **Configurable limits** - Different limits for different use cases
- **Clear errors** - Know exactly how deep you tried to go
- **Compile-time knowledge** - Optimizer can use MAX_DEPTH

### Use Cases

| Use Case | Depth Limit | Reason |
|----------|-------------|--------|
| **UI Trees** | 100-200 | UIs rarely go deeper than 100 levels |
| **Test Trees** | 10-50 | Tests use small trees |
| **Document Trees** | 1000-5000 | Documents can be deeply nested |
| **Debug Trees** | 10 | Prevent runaway recursion in debug |

---

## 5. Immutable Paint

**Problem:** Flutter allows mutation during paint, which can cause inconsistent state and hard-to-debug issues.

**Solution:** Make paint methods take `&self` instead of `&mut self`.

### Implementation

```rust
/// Paint context - only provides immutable access to tree
pub struct PaintingContext<'a> {
    canvas: &'a mut Canvas,
    layer_tree: &'a LayerTree,
    _no_send: PhantomData<*const ()>,  // Not Send - local to thread
}

impl<'a> PaintingContext<'a> {
    /// Paint child - only accepts immutable reference
    pub fn paint_child(&mut self, child: &dyn RenderBox, offset: Offset) {
        // ✅ child is immutable - can't mutate during paint
        child.paint(self, offset);
    }
    
    /// Push layer (creates new layer, doesn't mutate tree)
    pub fn push_layer(&mut self, layer: Layer) -> LayerHandle {
        self.layer_tree.push(layer)
    }
}

/// RenderBox trait - paint takes immutable self
pub trait RenderBox: RenderObject {
    /// Paint takes IMMUTABLE self
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    /// Layout takes mutable self (can mutate)
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;
}

/// RenderProxyBox implementation
pub trait RenderProxyBox: SingleChildRenderBox {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // ✅ self is immutable
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
}
```

### Usage

```rust
impl RenderBox for RenderOpacity {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // ❌ Can't mutate self during paint!
        // self.opacity = 0.5;  // Compile error: cannot mutate immutable self
        
        // ✅ Can read state
        let opacity = self.opacity;
        
        // ✅ Can paint child
        if let Some(child) = self.child() {
            // Save layer with opacity
            context.push_layer(Layer::Opacity { opacity });
            context.paint_child(child, offset);
            context.pop_layer();
        }
    }
    
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // ✅ Can mutate during layout
        if let Some(child) = self.child_mut() {
            let size = child.perform_layout(constraints);
            self.cached_size = Some(size);  // ✅ OK - mutable self
            size
        } else {
            constraints.smallest()
        }
    }
}
```

### Benefits

- **Consistency guarantee** - Tree can't change during paint
- **Fearless concurrency** - Multiple paint passes could run in parallel (future)
- **Better optimization** - Compiler knows tree is immutable
- **Catch bugs early** - Mutation attempts caught at compile-time

### Flutter Comparison

**Flutter:**
```dart
class RenderOpacity extends RenderProxyBox {
  @override
  void paint(PaintingContext context, Offset offset) {
    // ❌ Can mutate during paint - potentially dangerous
    this.opacity = someCalculation();  // Allowed but risky
    
    if (child != null) {
      context.paintChild(child!, offset);
    }
  }
}
```

**FLUI:**
```rust
impl RenderBox for RenderOpacity {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // ✅ Immutable self - can't mutate
        // self.opacity = 0.5;  // ❌ Compile error!
        
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
}
```

---

## 6. Thread Safety

**Problem:** Not all render objects are safe to share across threads, but this isn't enforced.

**Solution:** Explicit `Send + Sync` bounds where needed.

### Implementation

```rust
/// Trait for thread-safe render objects
pub trait ThreadSafeRenderObject: RenderObject + Send + Sync {}

/// Auto-implement for types that satisfy bounds
impl<T> ThreadSafeRenderObject for T 
where 
    T: RenderObject + Send + Sync 
{}

/// Thread-safe tree using Arc + RwLock
pub struct ThreadSafeTree {
    nodes: Arc<RwLock<Slab<RenderNode>>>,
    root: AtomicOption<RenderId>,
}

impl ThreadSafeTree {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(Slab::new())),
            root: AtomicOption::new(None),
        }
    }
    
    /// Add node - requires Send + Sync
    pub fn add_node<R>(&self, object: R) -> RenderId
    where
        R: ThreadSafeRenderObject + 'static,
    {
        let mut nodes = self.nodes.write().unwrap();
        let index = nodes.insert(RenderNode::new(Box::new(object)));
        RenderId::new(index, 0)
    }
    
    /// Get node (read lock)
    pub fn get_node<F, T>(&self, id: RenderId, f: F) -> T
    where
        F: FnOnce(&RenderNode) -> T,
    {
        let nodes = self.nodes.read().unwrap();
        f(&nodes[id.index()])
    }
    
    /// Modify node (write lock)
    pub fn modify_node<F>(&self, id: RenderId, f: F)
    where
        F: FnOnce(&mut RenderNode),
    {
        let mut nodes = self.nodes.write().unwrap();
        f(&mut nodes[id.index()])
    }
}

// For single-threaded, use simpler non-thread-safe tree
pub struct SingleThreadTree {
    nodes: Slab<RenderNode>,
    root: Option<RenderId>,
    _not_send: PhantomData<*const ()>,  // Not Send
}
```

### Usage

```rust
// Thread-safe tree
let tree = ThreadSafeTree::new();

// ✅ RenderOpacity is Send + Sync
let opacity_id = tree.add_node(RenderOpacity::new(0.5));

// Can share tree across threads
let tree_clone = Arc::clone(&tree.nodes);
std::thread::spawn(move || {
    tree_clone.get_node(opacity_id, |node| {
        // Read node from another thread
    });
});

// ❌ Non-Send type won't compile
struct NotSend {
    _marker: PhantomData<*const ()>,
}

impl RenderObject for NotSend {
    // ...
}

// tree.add_node(NotSend { _marker: PhantomData });  
// ❌ Compile error: NotSend doesn't implement Send
```

### Benefits

- **Explicit requirements** - Thread safety requirements in type signature
- **Compile-time checking** - Can't accidentally use non-thread-safe objects
- **Documentation** - Type signature documents thread safety
- **Future-proof** - Ready for parallel rendering

---

## 7. Typestate Builder

**Problem:** Builders can be incomplete (missing required fields) causing runtime panics.

**Solution:** Use typestate pattern to enforce all required fields at compile-time.

### Implementation

```rust
/// Builder states
pub struct NeedsOpacity;
pub struct NeedsChild;
pub struct Complete;

/// Builder with typestate progression
pub struct RenderOpacityBuilder<State = NeedsOpacity> {
    opacity: Option<f32>,
    child: Option<Box<dyn RenderBox>>,
    _state: PhantomData<State>,
}

impl RenderOpacityBuilder<NeedsOpacity> {
    pub fn new() -> Self {
        Self {
            opacity: None,
            child: None,
            _state: PhantomData,
        }
    }
    
    /// Set opacity (transition: NeedsOpacity → NeedsChild)
    pub fn opacity(self, opacity: f32) -> RenderOpacityBuilder<NeedsChild> {
        assert!(opacity >= 0.0 && opacity <= 1.0, "Opacity must be 0.0-1.0");
        
        RenderOpacityBuilder {
            opacity: Some(opacity),
            child: self.child,
            _state: PhantomData,
        }
    }
}

impl RenderOpacityBuilder<NeedsChild> {
    /// Set child (transition: NeedsChild → Complete)
    pub fn child(self, child: Box<dyn RenderBox>) -> RenderOpacityBuilder<Complete> {
        RenderOpacityBuilder {
            opacity: self.opacity,
            child: Some(child),
            _state: PhantomData,
        }
    }
}

impl RenderOpacityBuilder<Complete> {
    /// Build - only available when Complete
    pub fn build(self) -> RenderOpacity {
        RenderOpacity {
            proxy: ProxyBox::with_child(self.child.unwrap()),
            opacity: self.opacity.unwrap(),
        }
    }
}
```

### Usage

```rust
// ✅ Complete builder - compiles
let opacity = RenderOpacityBuilder::new()
    .opacity(0.5)
    .child(Box::new(some_child))
    .build();

// ❌ Incomplete builder - compile error
// let opacity = RenderOpacityBuilder::new()
//     .opacity(0.5)
//     .build();  // Error: method `build` not found for `RenderOpacityBuilder<NeedsChild>`

// ❌ Missing opacity - compile error
// let opacity = RenderOpacityBuilder::new()
//     .child(Box::new(some_child))
//     .build();  // Error: no method `child` on `RenderOpacityBuilder<NeedsOpacity>`

// ❌ Wrong order - compile error
// let opacity = RenderOpacityBuilder::new()
//     .child(Box::new(some_child))  // Error: NeedsOpacity doesn't have `child` method
//     .opacity(0.5)
//     .build();
```

### Benefits

- **Compile-time completeness** - Can't build incomplete objects
- **Guided API** - IDE shows only valid next methods
- **No runtime panics** - All validation at compile-time
- **Self-documenting** - Type shows what's needed next

---

## 8. NonZero Types

**Problem:** Depth and counts are often non-zero, but we check for zero at runtime.

**Solution:** Use `NonZeroUsize` to eliminate zero checks.

### Implementation

```rust
use std::num::NonZeroUsize;

/// Depth that cannot be zero (root is depth 1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Depth(NonZeroUsize);

impl Depth {
    /// Root depth (1)
    pub const ROOT: Self = unsafe { 
        Self(NonZeroUsize::new_unchecked(1)) 
    };
    
    /// Create from usize (returns None if zero)
    pub fn new(value: usize) -> Option<Self> {
        NonZeroUsize::new(value).map(Self)
    }
    
    /// Create from usize (panics if zero)
    pub fn from_usize(value: usize) -> Self {
        Self(NonZeroUsize::new(value).expect("Depth cannot be zero"))
    }
    
    /// Increment depth
    pub fn increment(self) -> Self {
        Self(self.0.saturating_add(1))
    }
    
    /// Decrement depth (saturates at 1)
    pub fn decrement(self) -> Self {
        let value = self.0.get();
        if value > 1 {
            Self(NonZeroUsize::new(value - 1).unwrap())
        } else {
            Self::ROOT
        }
    }
    
    /// Get raw value (guaranteed non-zero)
    pub fn get(self) -> usize {
        self.0.get()
    }
    
    /// Get parent depth (always safe)
    pub fn parent_depth(self) -> Option<Self> {
        if self.0.get() > 1 {
            Some(Self(NonZeroUsize::new(self.0.get() - 1).unwrap()))
        } else {
            None  // Root has no parent
        }
    }
}

/// RenderNode with NonZero depth
pub struct RenderNode {
    render_object: Box<dyn RenderObject>,
    parent: Option<RenderId>,
    depth: Depth,  // ✅ Cannot be zero!
}

impl RenderNode {
    pub fn new_root(object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object: object,
            parent: None,
            depth: Depth::ROOT,  // ✅ Always >= 1
        }
    }
    
    pub fn new_child(object: Box<dyn RenderObject>, parent_depth: Depth) -> Self {
        Self {
            render_object: object,
            parent: None,
            depth: parent_depth.increment(),  // ✅ Always > parent
        }
    }
}
```

### Usage

```rust
// Create root
let root = RenderNode::new_root(Box::new(RenderView::new()));
assert_eq!(root.depth, Depth::ROOT);  // depth = 1

// Create child
let child = RenderNode::new_child(
    Box::new(RenderOpacity::new(0.5)),
    root.depth
);
assert_eq!(child.depth.get(), 2);

// Parent depth is always safe
if let Some(parent_depth) = child.depth.parent_depth() {
    assert_eq!(parent_depth, root.depth);
}

// Compiler can optimize: no zero checks needed
fn process_depth(depth: Depth) {
    // depth.get() is guaranteed non-zero
    let parent_depth = depth.get() - 1;  // ✅ Safe: always > 0
    
    // Compiler can optimize division by depth
    let x = 100 / depth.get();  // ✅ No zero-check needed
}
```

### Benefits

- **Eliminate runtime checks** - Compiler knows value is non-zero
- **Better optimization** - Division, modulo don't need zero checks
- **Type safety** - Can't accidentally create zero depth
- **Clear intent** - Type documents that value is non-zero

---

## 9. Const Assertions

**Problem:** Invariants are checked at runtime, wasting CPU cycles.

**Solution:** Validate invariants at compile-time using const assertions.

### Implementation

```rust
/// Const assertion macro
macro_rules! const_assert {
    ($condition:expr) => {
        const _: () = assert!($condition);
    };
    ($condition:expr, $message:expr) => {
        const _: () = assert!($condition, $message);
    };
}

// =============================================================================
// SIZE ASSERTIONS
// =============================================================================

// Lifecycle should be 1 byte
const_assert!(std::mem::size_of::<RenderLifecycle>() == 1);

// RenderId should be <= 16 bytes (usize + u32 + phantom)
const_assert!(std::mem::size_of::<RenderId>() <= 16);

// Depth should be <= 8 bytes (NonZeroUsize)
const_assert!(std::mem::size_of::<Depth>() <= 8);

// RenderNode should be reasonably sized
const_assert!(std::mem::size_of::<RenderNode>() <= 128);

// =============================================================================
// ALIGNMENT ASSERTIONS
// =============================================================================

// Critical types should be well-aligned
const_assert!(std::mem::align_of::<RenderNode>() <= 8);
const_assert!(std::mem::align_of::<RenderId>() <= 8);

// =============================================================================
// PROTOCOL ASSERTIONS
// =============================================================================

// BoxConstraints should be exactly 16 bytes (4 f32s)
const_assert!(std::mem::size_of::<BoxConstraints>() == 16);

// SliverConstraints should be reasonable
const_assert!(std::mem::size_of::<SliverConstraints>() <= 32);

// =============================================================================
// ARITY ASSERTIONS
// =============================================================================

pub struct Exact<const N: usize>;

impl<const N: usize> Exact<N> {
    // Exact<0> is invalid - use Leaf
    const _ASSERT_NON_ZERO: () = assert!(
        N > 0,
        "Exact<0> is invalid, use Leaf instead"
    );
    
    // Exact<N> with huge N is impractical
    const _ASSERT_REASONABLE: () = assert!(
        N <= 1000,
        "Exact<N> with N > 1000 is impractical"
    );
}

pub struct Range<const MIN: usize, const MAX: usize>;

impl<const MIN: usize, const MAX: usize> Range<MIN, MAX> {
    // MIN must be <= MAX
    const _ASSERT_VALID_RANGE: () = assert!(
        MIN <= MAX,
        "Range<MIN, MAX>: MIN must be <= MAX"
    );
    
    // MIN must be > 0
    const _ASSERT_MIN_NON_ZERO: () = assert!(
        MIN > 0,
        "Range<0, MAX> is invalid, use Optional or Variable"
    );
}
```

### Usage

```rust
// ✅ Valid types compile
type ValidExact = Exact<5>;
type ValidRange = Range<2, 10>;

// ❌ Invalid types don't compile
// type InvalidExact = Exact<0>;     // Compile error: N must be > 0
// type HugeExact = Exact<10000>;    // Compile error: N > 1000
// type InvalidRange = Range<10, 5>; // Compile error: MIN > MAX
```

### Error Messages

```
error[E0080]: evaluation of `<Exact<0> as Exact>::_ASSERT_NON_ZERO` failed
  --> src/arity.rs:142:5
   |
142|     const _ASSERT_NON_ZERO: () = assert!(N > 0, "Exact<0> is invalid, use Leaf instead");
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the assertion failed
```

### Benefits

- **Zero runtime cost** - All checks at compile-time
- **Catch bugs early** - Invalid types won't compile
- **Documentation** - Const assertions document invariants
- **Refactoring safety** - Changes that violate invariants won't compile

---

## 10. Panic Safety

**Problem:** If layout or paint panics, tree can be left in inconsistent state.

**Solution:** Transaction log + checkpoints for panic recovery.

### Implementation

```rust
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Tree operation for transaction log
#[derive(Debug, Clone)]
enum TreeOperation {
    AttachNode { id: RenderId, parent: Option<RenderId>, depth: usize },
    DetachNode { id: RenderId },
    MarkNeedsLayout { id: RenderId },
    MarkNeedsPaint { id: RenderId },
}

/// Checkpoint for rollback
struct Checkpoint {
    operation_count: usize,
    node_states: Vec<(RenderId, NodeState)>,
}

#[derive(Debug, Clone)]
struct NodeState {
    parent: Option<RenderId>,
    depth: usize,
    lifecycle: RenderLifecycle,
}

/// Panic-safe tree
pub struct PanicSafeTree {
    nodes: Slab<RenderNode>,
    transaction_log: Vec<TreeOperation>,
}

impl PanicSafeTree {
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            transaction_log: Vec::new(),
        }
    }
    
    /// Layout with panic recovery
    pub fn layout_with_recovery(
        &mut self,
        id: RenderId,
        constraints: RenderConstraints
    ) -> Result<Size, LayoutError> {
        // Create checkpoint
        let checkpoint = self.create_checkpoint();
        
        // Try layout (catch panic)
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.layout_node(id, constraints)
        }));
        
        match result {
            Ok(Ok(size)) => {
                // Success - commit transaction
                self.commit_checkpoint(checkpoint);
                Ok(size)
            }
            Ok(Err(e)) => {
                // Layout error - rollback
                self.rollback_to_checkpoint(checkpoint);
                Err(e)
            }
            Err(panic_err) => {
                // Panic occurred - rollback
                self.rollback_to_checkpoint(checkpoint);
                eprintln!("Layout panic recovered: {:?}", panic_err);
                Err(LayoutError::Panic)
            }
        }
    }
    
    fn create_checkpoint(&self) -> Checkpoint {
        // Save current state
        let node_states = self.nodes.iter()
            .map(|(idx, node)| {
                let id = RenderId::new(idx, 0);
                let state = NodeState {
                    parent: node.parent,
                    depth: node.depth,
                    lifecycle: node.lifecycle,
                };
                (id, state)
            })
            .collect();
        
        Checkpoint {
            operation_count: self.transaction_log.len(),
            node_states,
        }
    }
    
    fn rollback_to_checkpoint(&mut self, checkpoint: Checkpoint) {
        // Restore node states
        for (id, state) in checkpoint.node_states {
            if let Some(node) = self.nodes.get_mut(id.index()) {
                node.parent = state.parent;
                node.depth = state.depth;
                node.lifecycle = state.lifecycle;
            }
        }
        
        // Truncate transaction log
        self.transaction_log.truncate(checkpoint.operation_count);
    }
    
    fn commit_checkpoint(&mut self, _checkpoint: Checkpoint) {
        // Checkpoint successful - no action needed
    }
    
    fn layout_node(
        &mut self,
        id: RenderId,
        constraints: RenderConstraints
    ) -> Result<Size, LayoutError> {
        // Layout implementation...
        // If this panics, checkpoint will restore state
        Ok(Size::ZERO)
    }
}

#[derive(Debug)]
pub enum LayoutError {
    InvalidConstraints,
    NodeNotFound,
    Panic,
}
```

### Usage

```rust
let mut tree = PanicSafeTree::new();

// Layout with automatic panic recovery
match tree.layout_with_recovery(id, constraints) {
    Ok(size) => {
        // Success - tree is consistent
        println!("Layout succeeded: {:?}", size);
    }
    Err(LayoutError::Panic) => {
        // Panic occurred but tree is still consistent!
        eprintln!("Layout panicked but tree recovered");
    }
    Err(e) => {
        // Other error
        eprintln!("Layout error: {:?}", e);
    }
}
```

### Benefits

- **Consistency guarantee** - Tree never left in invalid state
- **Debug friendly** - Can continue after panic
- **Production safety** - Graceful degradation
- **Audit trail** - Transaction log for debugging

---

## Comparison with Flutter

| Feature | Flutter (Dart) | FLUI (Rust) | Safety Level |
|---------|----------------|-------------|--------------|
| **ID Mixing** | Runtime error | Compile error (Branded IDs) | Compile-time |
| **Lifecycle State** | Runtime enum | Typestate pattern (optional) | Compile-time |
| **Protocol Mix** | Runtime crash | Trait bounds | Compile-time |
| **Depth Overflow** | Stack overflow | Configurable limits | Runtime |
| **Paint Mutation** | Allowed | Compile error (immutable) | Compile-time |
| **Thread Safety** | Manual checks | Send/Sync bounds | Compile-time |
| **Builder Errors** | Runtime panic | Typestate builder | Compile-time |
| **Zero Values** | Runtime checks | NonZero types | Compile-time |
| **Invariants** | Runtime asserts | Const assertions | Compile-time |
| **Panic Safety** | Inconsistent state | Transaction log | Runtime |

---

## Priority Implementation

### High Priority (Implement First)

| Feature | Effort | Impact | Why |
|---------|--------|--------|-----|
| **Branded IDs** | Low | High | Prevents common bug (ID mixing) |
| **Immutable Paint** | Low | High | Prevents state corruption |
| **Protocol Bounds** | Low | High | Type safety for free |
| **Const Assertions** | Low | Medium | Documents + validates invariants |

### Medium Priority (Implement Later)

| Feature | Effort | Impact | Why |
|---------|--------|--------|-----|
| **NonZero Depth** | Low | Medium | Performance optimization |
| **Depth Limits** | Medium | Medium | Prevents stack overflow |
| **Thread Safety** | Medium | Low | Future-proofing |

### Low Priority (Optional)

| Feature | Effort | Impact | Why |
|---------|--------|--------|-----|
| **Typestate Lifecycle** | High | Low | Complex, limited benefit |
| **Typestate Builder** | Medium | Low | Nice-to-have for API |
| **Panic Safety** | High | Low | Edge case protection |

---

## Best Practices

### 1. Choose Safety Level

```rust
// Development: Maximum safety (debug assertions)
#[cfg(debug_assertions)]
fn validate_tree(tree: &RenderTree) {
    for (id, node) in tree.nodes.iter() {
        assert!(node.depth > 0, "Invalid depth");
        assert!(node.lifecycle != RenderLifecycle::Disposed, "Disposed node in tree");
    }
}

// Production: Minimal overhead
#[cfg(not(debug_assertions))]
fn validate_tree(_tree: &RenderTree) {
    // No-op in release
}
```

### 2. Layer Safety Features

```rust
// Layer 1: Type safety (always)
fn add_box_child(children: &mut BoxChildren, child: Box<dyn RenderBox>) {
    children.add(child);  // ✅ Compile-time protocol check
}

// Layer 2: Runtime validation (debug only)
fn adopt_child(tree: &mut RenderTree, parent: RenderId, child: RenderId) {
    debug_assert!(tree.get(parent).depth < tree.max_depth());
    tree.adopt_child(parent, child).unwrap();
}

// Layer 3: Panic recovery (production)
fn layout_safe(tree: &mut PanicSafeTree, id: RenderId, constraints: RenderConstraints) {
    match tree.layout_with_recovery(id, constraints) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Layout error: {:?}", e);
            // Fallback behavior
        }
    }
}
```

### 3. Document Safety Guarantees

```rust
/// Add child to tree.
///
/// # Safety Guarantees
/// - Compile-time: Protocol compatibility checked
/// - Runtime: Arity validated
/// - Panic-safe: Tree remains consistent
pub fn add_child<R>(
    &mut self,
    parent_id: RenderId,
    child: R
) -> Result<RenderId, TreeError>
where
    R: RenderObject<Protocol = BoxProtocol>,
{
    // Implementation...
}
```

---

## Summary

| Safety Level | Features | When Checked | Cost |
|--------------|----------|--------------|------|
| **Type Safety** | Branded IDs, Protocol bounds | Compile-time | Zero |
| **Compile Safety** | Typestate, Const assertions | Compile-time | Zero |
| **Runtime Safety** | Depth limits, Arity validation | Runtime | Debug only |
| **Panic Safety** | Transaction log, Checkpoints | Recovery | Production |

**Key Insight:** Rust's type system allows us to catch errors **at compile-time** that Flutter catches **at runtime** (or not at all). This is a fundamental advantage.

---

## Next Steps

1. **Implement Branded IDs** - Highest ROI safety feature
2. **Make Paint Immutable** - Prevents entire class of bugs
3. **Add Protocol Bounds** - Type safety for container APIs
4. **Add Const Assertions** - Document + validate invariants
5. **Consider Typestate** - For builder APIs where it makes sense

---

**See Also:**
- [[Lifecycle]] - Lifecycle state management
- [[Arity Integration]] - Compile-time arity validation
- [[Protocol]] - Protocol system design
- [[Containers]] - Type-safe container implementations

---

**References:**
- Rust Book: [Unsafe Rust](https://doc.rust-lang.org/book/ch19-01-unsafe-rust.html)
- Typestate Pattern: [Session Types](https://docs.rs/session-types/)
- NonZero Types: [std::num](https://doc.rust-lang.org/std/num/index.html)
