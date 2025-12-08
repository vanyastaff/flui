# FLUI Four-Tree Architecture (Flutter-Aligned)

## Overview

FLUI implements Flutter's proven four-tree architecture with modern Rust patterns and Rust 1.91 features.

```
┌─────────────────────────────────────────────────────────┐
│  ViewTree (immutable, like Flutter's Widget tree)      │
│  - View definitions                                     │
│  - User-facing API                                      │
│  - Cheap to create/rebuild                             │
└─────────────────┬───────────────────────────────────────┘
                  │ build()
┌─────────────────▼───────────────────────────────────────┐
│  ElementTree (mutable, like Flutter's Element tree)    │
│  - Lifecycle management                                 │
│  - Holds references to RenderObjects                   │
│  - Manages tree structure                              │
└─────────────────┬───────────────────────────────────────┘
                  │ owns/references
┌─────────────────▼───────────────────────────────────────┐
│  RenderTree (layout & paint, like Flutter)             │
│  - RenderObjects for layout/paint                      │
│  - Box and Sliver protocols                            │
│  - Constraints & geometry                              │
└─────────────────┬───────────────────────────────────────┘
                  │ creates
┌─────────────────▼───────────────────────────────────────┐
│  LayerTree (compositing, like Flutter)                 │
│  - Layers for GPU compositing                          │
│  - Repaint boundaries                                  │
│  - Optimized rendering                                 │
└─────────────────────────────────────────────────────────┘
```

---

## Tree Relationships

### 1. ViewTree → ElementTree

**Flutter:**
```dart
Widget build(BuildContext context) {
  return Padding(
    padding: EdgeInsets.all(8.0),
    child: Text("Hello"),
  );
}

// Creates Element tree:
// PaddingElement → TextElement
```

**FLUI (proposed):**
```rust
pub trait View {
    type Element: Element;

    fn build(&self, ctx: &BuildContext) -> Self::Element;
}

pub struct PaddingView {
    padding: EdgeInsets,
    child: Box<dyn View>,
}

impl View for PaddingView {
    type Element = RenderElement<RenderPadding, BoxProtocol>;

    fn build(&self, ctx: &BuildContext) -> Self::Element {
        RenderElement::new(
            RenderPadding::new(self.padding),
            RuntimeArity::Single,
        )
    }
}
```

### 2. ElementTree → RenderTree

**Key Design Decision:** How should Element reference RenderObject?

#### Option A: Direct Ownership (Recommended)

```rust
pub struct ElementTree {
    elements: Slab<Element>,
}

pub enum Element {
    /// Element that owns a RenderObject
    Render(RenderElement),

    /// Element without RenderObject (e.g., InheritedElement)
    Widget(WidgetElement),
}

pub struct RenderElement {
    // ========== Element Data ==========
    id: ElementId,
    parent: Option<ElementId>,
    children: Vec<ElementId>,

    // ========== RenderObject (OWNED!) ==========
    /// Direct ownership - Element owns its RenderObject
    render_object: Box<dyn RenderObject>,

    // ========== Render State ==========
    state: RenderState,  // Using safe enum variant

    // ========== Lifecycle ==========
    lifecycle: ElementLifecycle,
}

impl RenderElement {
    /// Direct access like Flutter's element.renderObject
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }
}
```

**Benefits:**
- ✅ Matches Flutter: Element owns RenderObject
- ✅ No separate RenderTree storage
- ✅ Direct access (no ID lookup)
- ✅ Clear ownership semantics

#### Option B: Separate RenderTree with Safe References

```rust
pub struct ElementTree {
    elements: Slab<Element>,
    // Reference to RenderTree
    render_tree: Arc<RwLock<RenderTree>>,
}

pub struct RenderElement {
    id: ElementId,
    // Safe reference to RenderObject
    render_ref: RenderRef,  // Not raw RenderId!
}

/// Safe, type-erased reference to RenderObject
pub struct RenderRef {
    id: RenderId,
    tree: Weak<RwLock<RenderTree>>,  // Weak reference to tree
}

impl RenderRef {
    /// Safe access to RenderObject
    pub fn with_render_object<R>(
        &self,
        f: impl FnOnce(&dyn RenderObject) -> R,
    ) -> Option<R> {
        let tree = self.tree.upgrade()?;
        let tree = tree.read().unwrap();
        let object = tree.get(self.id)?;
        Some(f(object))
    }
}
```

**Benefits:**
- ✅ Keeps trees separate (existing architecture)
- ✅ Safe access (no raw pointers)
- ✅ Can share RenderTree across threads
- ⚠️ Requires lock acquisition for access

**Recommendation:** **Option A** (Direct Ownership) - simpler, faster, matches Flutter.

### 3. RenderTree → LayerTree

```rust
pub trait RenderObject {
    /// Paint into LayerTree
    fn paint(
        &self,
        ctx: &mut PaintContext,
    ) -> Result<(), PaintError>;

    /// Whether this creates a repaint boundary (new layer)
    fn is_repaint_boundary(&self) -> bool {
        false
    }
}

pub struct PaintContext<'a> {
    /// Current layer being painted into
    layer: &'a mut Layer,

    /// Offset from parent
    offset: Offset,

    /// Access to ElementTree for child painting
    element_tree: &'a ElementTree,

    /// Current element being painted
    element_id: ElementId,
}

impl<'a> PaintContext<'a> {
    /// Paint child (creates new layer if child is repaint boundary)
    pub fn paint_child(
        &mut self,
        child_id: ElementId,
        offset: Offset,
    ) -> Result<(), PaintError> {
        let child_element = self.element_tree.get(child_id)?;
        let child_render = child_element.render_object();

        // Check if child needs new layer
        if child_render.is_repaint_boundary() {
            // Create new layer
            let mut child_layer = Layer::new();
            let mut child_ctx = PaintContext {
                layer: &mut child_layer,
                offset,
                element_tree: self.element_tree,
                element_id: child_id,
            };

            child_render.paint(&mut child_ctx)?;

            // Add child layer to current layer
            self.layer.add_child(child_layer);
        } else {
            // Paint into same layer
            let mut child_ctx = PaintContext {
                layer: self.layer,
                offset: self.offset + offset,
                element_tree: self.element_tree,
                element_id: child_id,
            };

            child_render.paint(&mut child_ctx)?;
        }

        Ok(())
    }
}
```

---

## Modern Rust Patterns for 4-Tree Design

### 1. Context-Based API (No Callbacks!)

**Layout Context:**
```rust
pub struct LayoutContext<'tree, P: Protocol> {
    /// Immutable reference to ElementTree
    element_tree: &'tree ElementTree,

    /// Current element being laid out
    element_id: ElementId,

    /// Constraints for this layout
    constraints: P::Constraints,

    /// Mutable layout cache
    cache: &'tree mut LayoutCache,

    _phantom: PhantomData<P>,
}

impl<'tree, P: Protocol> LayoutContext<'tree, P> {
    /// Layout child through ElementTree (no callback!)
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: P::Constraints,
    ) -> Result<P::Geometry, LayoutError> {
        // Access child element
        let child_element = self.element_tree.get(child_id)?;
        let child_render = child_element.render_object();

        // Create child context
        let mut child_ctx = LayoutContext {
            element_tree: self.element_tree,
            element_id: child_id,
            constraints,
            cache: self.cache,
            _phantom: PhantomData,
        };

        // Layout child (recursively, but safely!)
        child_render.layout(&mut child_ctx)
    }

    /// Access to children (type-safe!)
    pub fn children(&self) -> ChildrenView<'_, P> {
        let element = self.element_tree.get(self.element_id).unwrap();
        ChildrenView {
            children: element.children(),
            element_tree: self.element_tree,
            _phantom: PhantomData,
        }
    }
}
```

**RenderObject Implementation:**
```rust
impl RenderObject for RenderPadding {
    type Protocol = BoxProtocol;

    fn layout(
        &mut self,
        ctx: &mut LayoutContext<'_, BoxProtocol>,
    ) -> Result<Size, LayoutError> {
        // Get child through context (no callback!)
        let child_id = ctx.children().single()?;

        // Deflate constraints
        let child_constraints = ctx.constraints.deflate(self.padding);

        // Layout child through context
        let child_size = ctx.layout_child(child_id, child_constraints)?;

        // Return our size
        Ok(Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        ))
    }
}
```

### 2. Safe RenderState (Fix UB)

**Problem:** Different-sized geometries cause UB in pointer casting.

**Solution:** Enum-based storage

```rust
/// Type-safe state for any protocol
pub enum RenderState {
    Box(BoxRenderState),
    Sliver(SliverRenderState),
}

pub struct BoxRenderState {
    flags: AtomicRenderFlags,
    geometry: OnceLock<Size>,           // 8 bytes
    constraints: OnceLock<BoxConstraints>,
    offset: AtomicOffset,
}

pub struct SliverRenderState {
    flags: AtomicRenderFlags,
    geometry: OnceLock<SliverGeometry>, // 56 bytes - OK in enum!
    constraints: OnceLock<SliverConstraints>,
    offset: AtomicOffset,
}

impl RenderState {
    /// Type-safe accessor
    pub fn as_box(&self) -> Option<&BoxRenderState> {
        match self {
            Self::Box(state) => Some(state),
            _ => None,
        }
    }

    pub fn as_sliver(&self) -> Option<&SliverRenderState> {
        match self {
            Self::Sliver(state) => Some(state),
            _ => None,
        }
    }
}
```

### 3. LayerTree Design

```rust
/// Layer in LayerTree (like Flutter)
pub struct Layer {
    /// Layer type
    kind: LayerKind,

    /// Transform applied to this layer
    transform: Option<Matrix4>,

    /// Clip applied to this layer
    clip: Option<Rect>,

    /// Child layers
    children: Vec<Layer>,

    /// Paint operations (for leaf layers)
    operations: Vec<PaintOp>,
}

pub enum LayerKind {
    /// Container layer (has children)
    Container,

    /// Picture layer (has paint operations)
    Picture,

    /// Transform layer
    Transform,

    /// Clip layer
    Clip,

    /// Opacity layer
    Opacity { alpha: f32 },

    /// Backdrop filter layer
    BackdropFilter { filter: ImageFilter },
}

impl Layer {
    /// Add child layer
    pub fn add_child(&mut self, child: Layer) {
        self.children.push(child);
    }

    /// Add paint operation (for Picture layers)
    pub fn add_operation(&mut self, op: PaintOp) {
        self.operations.push(op);
    }

    /// Composite into scene for GPU
    pub fn composite(&self, scene: &mut Scene) {
        match self.kind {
            LayerKind::Container => {
                // Composite children
                for child in &self.children {
                    child.composite(scene);
                }
            }
            LayerKind::Picture => {
                // Rasterize paint operations
                scene.add_picture(&self.operations);
            }
            LayerKind::Transform => {
                scene.push_transform(self.transform.unwrap());
                for child in &self.children {
                    child.composite(scene);
                }
                scene.pop_transform();
            }
            // ... other layer types
        }
    }
}
```

---

## Complete Pipeline Flow

### 1. Build Phase (ViewTree → ElementTree)

```rust
pub struct BuildContext<'tree> {
    element_tree: &'tree mut ElementTree,
    current_element: ElementId,
}

impl ViewTree {
    /// Build ElementTree from ViewTree
    pub fn build(&self, element_tree: &mut ElementTree) {
        for view in self.views() {
            let ctx = BuildContext {
                element_tree,
                current_element: ElementId::root(),
            };

            let element = view.build(&ctx);
            element_tree.insert(element);
        }
    }
}
```

### 2. Layout Phase (ElementTree → RenderTree)

```rust
impl ElementTree {
    /// Layout entire tree
    pub fn layout(
        &self,
        root_constraints: BoxConstraints,
    ) -> Result<(), LayoutError> {
        let mut cache = LayoutCache::new();

        // Layout root element
        let root_id = self.root_id();
        let root_element = self.get(root_id)?;
        let root_render = root_element.render_object();

        // Create layout context
        let mut ctx = LayoutContext {
            element_tree: self,
            element_id: root_id,
            constraints: root_constraints,
            cache: &mut cache,
            _phantom: PhantomData,
        };

        // Layout (recursively through context)
        root_render.layout(&mut ctx)?;

        Ok(())
    }
}
```

### 3. Paint Phase (RenderTree → LayerTree)

```rust
impl ElementTree {
    /// Paint into LayerTree
    pub fn paint(&self) -> Result<LayerTree, PaintError> {
        let mut root_layer = Layer::new();

        // Paint root element
        let root_id = self.root_id();
        let root_element = self.get(root_id)?;
        let root_render = root_element.render_object();

        // Create paint context
        let mut ctx = PaintContext {
            layer: &mut root_layer,
            offset: Offset::ZERO,
            element_tree: self,
            element_id: root_id,
        };

        // Paint (recursively through context)
        root_render.paint(&mut ctx)?;

        // Return LayerTree
        Ok(LayerTree::new(root_layer))
    }
}
```

### 4. Composite Phase (LayerTree → GPU)

```rust
impl LayerTree {
    /// Composite layers into GPU scene
    pub fn composite(&self, scene: &mut Scene) {
        self.root_layer.composite(scene);
    }
}

// Full pipeline
pub fn render_frame(view_tree: &ViewTree) -> Result<Scene, RenderError> {
    // 1. Build
    let mut element_tree = ElementTree::new();
    view_tree.build(&mut element_tree);

    // 2. Layout
    let constraints = BoxConstraints::tight(window_size);
    element_tree.layout(constraints)?;

    // 3. Paint
    let layer_tree = element_tree.paint()?;

    // 4. Composite
    let mut scene = Scene::new();
    layer_tree.composite(&mut scene);

    Ok(scene)
}
```

---

## Key Improvements for 4-Tree Design

### 1. Context-Based, Not Callback-Based
```rust
// ❌ Old: callback-based (unsafe)
fn layout(&mut self, callback: &mut dyn FnMut(...)) { ... }

// ✅ New: context-based (safe)
fn layout(&mut self, ctx: &mut LayoutContext) { ... }
```

### 2. Direct Element-Render Ownership
```rust
// ❌ Old: indirect via RenderId
struct Element {
    render_id: Option<RenderId>,  // Lookup needed
}

// ✅ New: direct ownership
struct RenderElement {
    render_object: Box<dyn RenderObject>,  // Direct access
}
```

### 3. Safe Protocol State
```rust
// ❌ Old: UB from pointer casting
RenderState<P>  // Different sizes = UB

// ✅ New: enum-based
enum RenderState {
    Box(BoxRenderState),
    Sliver(SliverRenderState),
}
```

### 4. Type-Safe Child Access
```rust
// Context provides type-safe children access
ctx.children().single()   // For Single arity
ctx.children().iter()     // For Variable arity
ctx.children().optional() // For Optional arity
```

---

## Implementation Priorities

### Phase 1: Fix Critical Issues (Week 1-2)
1. ✅ Implement safe `RenderState` enum
2. ✅ Create context types (Layout/Paint/HitTest)
3. ✅ Update `RenderObject` trait to use contexts
4. ✅ Test with core RenderObjects

### Phase 2: Element-Render Ownership (Week 3-4)
5. ✅ Decide: Direct ownership vs. separate tree
6. ✅ Implement chosen approach
7. ✅ Migrate existing code
8. ✅ Performance benchmarks

### Phase 3: Complete 4-Tree Pipeline (Week 5-6)
9. ✅ Finalize LayerTree design
10. ✅ Implement compositing
11. ✅ End-to-end testing
12. ✅ Documentation

---

## Conclusion

The 4-tree design is the right architecture! With these improvements:

- ✅ **Matches Flutter** - proven architecture
- ✅ **Modern Rust** - context-based, safe, idiomatic
- ✅ **No UB** - enum-based storage, safe references
- ✅ **Better ergonomics** - clear APIs, type safety
- ✅ **Maintainable** - clear separation of concerns

Ready to implement! 🚀
