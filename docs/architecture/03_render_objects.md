# Chapter 3: RenderObject System

## 📋 Overview

RenderObject система - это performance-critical слой FLUI, который отвечает за **layout** (вычисление размеров и позиций) и **paint** (генерацию визуального контента). Главная особенность - **type-safe Arity system**, который гарантирует на compile-time корректность работы с детьми.

## 🎯 RenderObject Trait

### Core Definition

```rust
/// RenderObject - performs layout and paint operations
pub trait RenderObject: Debug + Send + Sync + 'static {
    /// Arity - compile-time child count guarantee
    type Arity: Arity;
    
    /// Compute size given constraints
    /// Called during layout phase
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size;
    
    /// Generate layer tree for rendering
    /// Called during paint phase
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer;
    
    /// Optional: hit test for input events
    fn hit_test(&self, position: Offset, cx: &HitTestCx<Self::Arity>) -> bool {
        // Default: check if point in bounds
        self.bounds().contains(position)
    }
    
    /// Optional: is this a relayout boundary?
    fn is_relayout_boundary(&self) -> bool {
        false
    }
    
    /// Optional: is this a repaint boundary?
    fn is_repaint_boundary(&self) -> bool {
        false
    }
}
```

### Key Concepts

1. **Arity** - child count type (LeafArity, SingleArity, MultiArity)
2. **Layout** - compute sizes from constraints
3. **Paint** - generate visual output as layers
4. **Boundaries** - optimization points

---

## 🔢 Arity System - Type-Safe Child Count

### Why Arity?

**Problem (Flutter approach):**
```dart
// ❌ Runtime crashes possible
class RenderOpacity extends RenderBox {
  void performLayout() {
    if (child != null) {  // Runtime check!
      child.layout(constraints);
    }
  }
}
```

**Solution (FLUI approach):**
```rust
// ✅ Compile-time guarantee!
impl RenderObject for RenderOpacity {
    type Arity = SingleArity;  // Exactly one child
    
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        let child = cx.child();  // ✅ Always exists!
        cx.layout_child(child, cx.constraints())
    }
}
```

### Arity Types

```rust
/// Arity trait - sealed, three implementations
pub trait Arity: sealed::Sealed + Debug + Send + Sync + 'static {
    /// Associated context types
    type LayoutContext<'a>: LayoutContext<'a>;
    type PaintContext<'a>: PaintContext<'a>;
    type HitTestContext<'a>: HitTestContext<'a>;
}

mod sealed {
    pub trait Sealed {}
}

/// LeafArity - zero children
#[derive(Debug, Clone, Copy)]
pub struct LeafArity;

/// SingleArity - exactly one child
#[derive(Debug, Clone, Copy)]
pub struct SingleArity;

/// MultiArity - zero or more children
#[derive(Debug, Clone, Copy)]
pub struct MultiArity;

impl Arity for LeafArity {
    type LayoutContext<'a> = LeafLayoutCx<'a>;
    type PaintContext<'a> = LeafPaintCx<'a>;
    type HitTestContext<'a> = LeafHitTestCx<'a>;
}

impl Arity for SingleArity {
    type LayoutContext<'a> = SingleLayoutCx<'a>;
    type PaintContext<'a> = SinglePaintCx<'a>;
    type HitTestContext<'a> = SingleHitTestCx<'a>;
}

impl Arity for MultiArity {
    type LayoutContext<'a> = MultiLayoutCx<'a>;
    type PaintContext<'a> = MultiPaintCx<'a>;
    type HitTestContext<'a> = MultiHitTestCx<'a>;
}
```

### Example: All Three Arities

```rust
// ═══════════════════════════════════════════════════════════════
// LeafArity - No Children (e.g., Text, Image)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct RenderText {
    text: String,
    style: TextStyle,
    paragraph: Option<Paragraph>,
}

impl RenderObject for RenderText {
    type Arity = LeafArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
        // No children to layout!
        // Just compute text size
        
        let constraints = cx.constraints();
        let max_width = constraints.max_width();
        
        // Layout text paragraph
        self.paragraph = Some(
            layout_text(&self.text, &self.style, max_width)
        );
        
        let paragraph = self.paragraph.as_ref().unwrap();
        Size::new(paragraph.width(), paragraph.height())
    }
    
    fn paint(&self, cx: &PaintCx<LeafArity>) -> BoxedLayer {
        // No children to paint!
        
        let mut picture = PictureLayer::new();
        let paragraph = self.paragraph.as_ref().unwrap();
        picture.draw_paragraph(paragraph, Offset::ZERO);
        Box::new(picture)
    }
}

// ═══════════════════════════════════════════════════════════════
// SingleArity - Exactly One Child (e.g., Padding, Opacity)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct RenderPadding {
    padding: EdgeInsets,
}

impl RenderObject for RenderPadding {
    type Arity = SingleArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        // ✅ cx.child() guaranteed to exist!
        let child = cx.child();
        
        // Deflate constraints by padding
        let child_constraints = cx.constraints().deflate(self.padding);
        
        // Layout child
        let child_size = cx.layout_child(child, child_constraints);
        
        // Our size = child size + padding
        Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical(),
        )
    }
    
    fn paint(&self, cx: &PaintCx<SingleArity>) -> BoxedLayer {
        // ✅ cx.child() guaranteed to exist!
        let child = cx.child();
        
        // Capture child layer
        let child_layer = cx.capture_child_layer(child);
        
        // Wrap in offset layer (for padding)
        let mut offset_layer = OffsetLayer::new(
            Offset::new(self.padding.left, self.padding.top)
        );
        offset_layer.add_child(child_layer);
        
        Box::new(offset_layer)
    }
}

// ═══════════════════════════════════════════════════════════════
// MultiArity - Multiple Children (e.g., Row, Column, Stack)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct RenderFlex {
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
}

impl RenderObject for RenderFlex {
    type Arity = MultiArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
        // ✅ cx.children() returns Vec<ElementId>
        let children = cx.children();
        
        if children.is_empty() {
            return Size::ZERO;
        }
        
        let constraints = cx.constraints();
        let mut main_axis_size = 0.0;
        let mut cross_axis_size = 0.0;
        
        // Layout each child
        for &child_id in children {
            let child_constraints = BoxConstraints::new(
                Size::ZERO,
                Size::new(f32::INFINITY, constraints.max_height()),
            );
            
            let child_size = cx.layout_child(child_id, child_constraints);
            
            main_axis_size += child_size.width;
            cross_axis_size = cross_axis_size.max(child_size.height);
        }
        
        Size::new(main_axis_size, cross_axis_size)
    }
    
    fn paint(&self, cx: &PaintCx<MultiArity>) -> BoxedLayer {
        let children = cx.children();
        
        let mut container = ContainerLayer::new();
        let mut offset = 0.0;
        
        // Paint each child
        for &child_id in children {
            let child_layer = cx.capture_child_layer(child_id);
            
            let mut offset_layer = OffsetLayer::new(
                Offset::new(offset, 0.0)
            );
            offset_layer.add_child(child_layer);
            container.add_child(Box::new(offset_layer));
            
            // Get child size for next offset
            let child_size = cx.child_size(child_id);
            offset += child_size.width;
        }
        
        Box::new(container)
    }
}
```

---

## 🎨 LayoutCx - Layout Context (Typed by Arity)

### Common Operations (All Arities)

```rust
/// Base layout context operations (available for all arities)
pub trait LayoutContext<'a> {
    /// Get incoming constraints
    fn constraints(&self) -> BoxConstraints;
    
    /// Get element ID
    fn element_id(&self) -> ElementId;
    
    /// Mark needs repaint (e.g., after animation frame)
    fn mark_needs_repaint(&mut self);
    
    /// Access render pipeline
    fn pipeline(&self) -> &RenderPipeline;
}
```

### LeafLayoutCx (No Children)

```rust
pub struct LeafLayoutCx<'a> {
    element_id: ElementId,
    constraints: BoxConstraints,
    pipeline: &'a RenderPipeline,
}

impl<'a> LayoutContext<'a> for LeafLayoutCx<'a> {
    fn constraints(&self) -> BoxConstraints {
        self.constraints
    }
    
    fn element_id(&self) -> ElementId {
        self.element_id
    }
    
    // ... other methods
}

// No additional methods - no children!
```

### SingleLayoutCx (One Child)

```rust
pub struct SingleLayoutCx<'a> {
    element_id: ElementId,
    constraints: BoxConstraints,
    child_id: ElementId,
    pipeline: &'a mut RenderPipeline,
}

impl<'a> LayoutContext<'a> for SingleLayoutCx<'a> {
    // ... base methods
}

impl<'a> SingleLayoutCx<'a> {
    /// Get child ID (guaranteed to exist!)
    pub fn child(&self) -> ElementId {
        self.child_id
    }
    
    /// Layout child with given constraints
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        self.pipeline.layout_node(child_id, constraints)
    }
    
    /// Get child's computed size (after layout)
    pub fn child_size(&self, child_id: ElementId) -> Size {
        self.pipeline.get_size(child_id)
    }
}
```

### MultiLayoutCx (Multiple Children)

```rust
pub struct MultiLayoutCx<'a> {
    element_id: ElementId,
    constraints: BoxConstraints,
    children: Vec<ElementId>,
    pipeline: &'a mut RenderPipeline,
}

impl<'a> LayoutContext<'a> for MultiLayoutCx<'a> {
    // ... base methods
}

impl<'a> MultiLayoutCx<'a> {
    /// Get all children
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }
    
    /// Layout specific child
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        self.pipeline.layout_node(child_id, constraints)
    }
    
    /// Get child's parent data
    pub fn parent_data<T: ParentData>(&self, child_id: ElementId) -> Option<&T> {
        self.pipeline.get_parent_data::<T>(child_id)
    }
    
    /// Get child size
    pub fn child_size(&self, child_id: ElementId) -> Size {
        self.pipeline.get_size(child_id)
    }
}
```

---

## 🖌️ PaintCx - Paint Context (Typed by Arity)

### Common Operations

```rust
pub trait PaintContext<'a> {
    /// Get element ID
    fn element_id(&self) -> ElementId;
    
    /// Get size (result of layout)
    fn size(&self) -> Size;
    
    /// Get offset from parent
    fn offset(&self) -> Offset;
}
```

### LeafPaintCx

```rust
pub struct LeafPaintCx<'a> {
    element_id: ElementId,
    size: Size,
    offset: Offset,
}

// No additional methods - no children to paint!
```

### SinglePaintCx

```rust
pub struct SinglePaintCx<'a> {
    element_id: ElementId,
    size: Size,
    offset: Offset,
    child_id: ElementId,
    pipeline: &'a RenderPipeline,
}

impl<'a> SinglePaintCx<'a> {
    /// Get child ID
    pub fn child(&self) -> ElementId {
        self.child_id
    }
    
    /// Capture child's layer tree
    pub fn capture_child_layer(&self, child_id: ElementId) -> BoxedLayer {
        self.pipeline.paint_node(child_id)
    }
}
```

### MultiPaintCx

```rust
pub struct MultiPaintCx<'a> {
    element_id: ElementId,
    size: Size,
    offset: Offset,
    children: Vec<ElementId>,
    pipeline: &'a RenderPipeline,
}

impl<'a> MultiPaintCx<'a> {
    /// Get all children
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }
    
    /// Capture child's layer tree
    pub fn capture_child_layer(&self, child_id: ElementId) -> BoxedLayer {
        self.pipeline.paint_node(child_id)
    }
    
    /// Get child size
    pub fn child_size(&self, child_id: ElementId) -> Size {
        self.pipeline.get_size(child_id)
    }
    
    /// Get child offset
    pub fn child_offset(&self, child_id: ElementId) -> Offset {
        self.pipeline.get_offset(child_id)
    }
}
```

---

## 🔄 RenderPipeline - Orchestrates Rendering

### Core Structure

```rust
pub struct RenderPipeline {
    /// Element tree
    tree: Rc<RefCell<ElementTree>>,
    
    /// Dirty tracking
    nodes_needing_layout: Vec<ElementId>,
    nodes_needing_paint: Vec<ElementId>,
    
    /// Cached results
    layout_cache: LayoutCache,
    size_cache: HashMap<ElementId, Size>,
    offset_cache: HashMap<ElementId, Offset>,
    
    /// Relayout boundaries (optimization)
    relayout_boundaries: HashSet<ElementId>,
    
    /// Layer cache
    layer_cache: HashMap<ElementId, BoxedLayer>,
}

impl RenderPipeline {
    /// Mark node as needing layout
    pub fn mark_needs_layout(&mut self, id: ElementId) {
        self.nodes_needing_layout.push(id);
        
        // Propagate to parent (unless relayout boundary)
        if !self.is_relayout_boundary(id) {
            if let Some(parent) = self.tree.borrow().parent(id) {
                self.mark_needs_layout(parent);
            }
        }
    }
    
    /// Mark node as needing paint
    pub fn mark_needs_paint(&mut self, id: ElementId) {
        self.nodes_needing_paint.push(id);
        
        // Propagate to parent (unless repaint boundary)
        if !self.is_repaint_boundary(id) {
            if let Some(parent) = self.tree.borrow().parent(id) {
                self.mark_needs_paint(parent);
            }
        }
    }
    
    /// Flush layout phase
    pub fn flush_layout(&mut self, root_constraints: BoxConstraints) -> Size {
        // Sort by depth (parents first)
        self.sort_by_depth(&mut self.nodes_needing_layout);
        
        // Layout each dirty node
        let dirty_nodes = std::mem::take(&mut self.nodes_needing_layout);
        
        for node_id in dirty_nodes {
            self.layout_node_internal(node_id, root_constraints);
        }
        
        // Return root size
        self.size_cache.get(&ElementId::root()).copied().unwrap_or(Size::ZERO)
    }
    
    /// Flush paint phase
    pub fn flush_paint(&mut self) -> BoxedLayer {
        // Sort by depth
        self.sort_by_depth(&mut self.nodes_needing_paint);
        
        // Paint each dirty node
        let dirty_nodes = std::mem::take(&mut self.nodes_needing_paint);
        
        for node_id in dirty_nodes {
            self.paint_node_internal(node_id);
        }
        
        // Return root layer
        self.layer_cache.get(&ElementId::root())
            .cloned()
            .unwrap_or_else(|| Box::new(ContainerLayer::new()))
    }
    
    /// Layout individual node
    fn layout_node_internal(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Check cache
        let cache_key = LayoutCacheKey::new(id, constraints);
        if let Some(cached) = self.layout_cache.get(&cache_key) {
            return cached.size;
        }
        
        // Get element and render object
        let tree = self.tree.borrow();
        let element = tree.get(id).expect("Element not found");
        
        // Call RenderObject::layout() with appropriate context
        let size = match element.arity() {
            ArityType::Leaf => {
                let mut cx = LeafLayoutCx::new(id, constraints, self);
                element.render_object().layout(&mut cx)
            }
            ArityType::Single => {
                let child = tree.first_child(id).expect("Single child expected");
                let mut cx = SingleLayoutCx::new(id, constraints, child, self);
                element.render_object().layout(&mut cx)
            }
            ArityType::Multi => {
                let children = tree.children(id);
                let mut cx = MultiLayoutCx::new(id, constraints, children, self);
                element.render_object().layout(&mut cx)
            }
        };
        
        // Cache result
        self.size_cache.insert(id, size);
        self.layout_cache.insert(cache_key, LayoutResult { size });
        
        size
    }
    
    /// Paint individual node
    fn paint_node_internal(&mut self, id: ElementId) -> BoxedLayer {
        // Check cache
        if let Some(cached) = self.layer_cache.get(&id) {
            return cached.clone();
        }
        
        // Get element
        let tree = self.tree.borrow();
        let element = tree.get(id).expect("Element not found");
        
        let size = self.size_cache.get(&id).copied().unwrap_or(Size::ZERO);
        let offset = self.offset_cache.get(&id).copied().unwrap_or(Offset::ZERO);
        
        // Call RenderObject::paint() with appropriate context
        let layer = match element.arity() {
            ArityType::Leaf => {
                let cx = LeafPaintCx::new(id, size, offset);
                element.render_object().paint(&cx)
            }
            ArityType::Single => {
                let child = tree.first_child(id).expect("Single child expected");
                let cx = SinglePaintCx::new(id, size, offset, child, self);
                element.render_object().paint(&cx)
            }
            ArityType::Multi => {
                let children = tree.children(id);
                let cx = MultiPaintCx::new(id, size, offset, children, self);
                element.render_object().paint(&cx)
            }
        };
        
        // Cache layer
        self.layer_cache.insert(id, layer.clone());
        
        layer
    }
}
```

---

## ⚡ Performance Optimizations

### 1. Relayout Boundaries

```rust
impl RenderObject for RenderScrollView {
    type Arity = SingleArity;
    
    // ✅ This is a relayout boundary!
    fn is_relayout_boundary(&self) -> bool {
        true  // Children's layout doesn't affect parents
    }
    
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        // ScrollView sets its own constraints for child
        let child = cx.child();
        
        let child_constraints = BoxConstraints::new(
            Size::ZERO,
            Size::new(cx.constraints().max_width(), f32::INFINITY),
        );
        
        let child_size = cx.layout_child(child, child_constraints);
        
        // Store for scrolling
        self.content_size = child_size;
        
        // Return our size (not child's!)
        cx.constraints().biggest()
    }
}
```

**Benefits:**
- Prevents layout propagation to ancestors
- Enables local relayout
- Reduces cascade during scrolling/animation

### 2. Repaint Boundaries

```rust
impl RenderObject for RenderOpacity {
    type Arity = SingleArity;
    
    // ✅ This is a repaint boundary!
    fn is_repaint_boundary(&self) -> bool {
        true  // Opacity changes don't affect parents
    }
    
    fn paint(&self, cx: &PaintCx<SingleArity>) -> BoxedLayer {
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);
        
        // Wrap in opacity layer
        let mut opacity_layer = OpacityLayer::new(self.opacity);
        opacity_layer.add_child(child_layer);
        Box::new(opacity_layer)
    }
}
```

**Benefits:**
- Prevents repaint propagation
- Enables layer caching
- GPU can composite independently

### 3. Layout Cache

```rust
use moka::sync::Cache;

pub struct LayoutCache {
    cache: Cache<LayoutCacheKey, LayoutResult>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct LayoutCacheKey {
    element_id: ElementId,
    constraints: BoxConstraints,
    // Could add: child count, child sizes hash
}

pub struct LayoutResult {
    pub size: Size,
}

impl LayoutCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
        }
    }
    
    pub fn get(&self, key: &LayoutCacheKey) -> Option<LayoutResult> {
        self.cache.get(key)
    }
    
    pub fn insert(&self, key: LayoutCacheKey, result: LayoutResult) {
        self.cache.insert(key, result);
    }
    
    pub fn invalidate(&self, element_id: ElementId) {
        // Remove all entries for this element
        self.cache.invalidate_entries_if(move |key, _| {
            key.element_id == element_id
        }).expect("Invalidation failed");
    }
}
```

---

## 🎯 Custom RenderObject Example

### Creating Custom Circular Layout

```rust
// ═══════════════════════════════════════════════════════════════
// Widget
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CircularLayout {
    radius: f32,
    children: Vec<BoxedWidget>,
}

impl CircularLayout {
    pub fn new(radius: f32) -> Self {
        Self {
            radius,
            children: Vec::new(),
        }
    }
    
    pub fn add_child(mut self, child: BoxedWidget) -> Self {
        self.children.push(child);
        self
    }
}

impl RenderObjectWidget for CircularLayout {
    type Arity = MultiArity;
    type Render = RenderCircularLayout;
    
    fn create_render_object(&self) -> Self::Render {
        RenderCircularLayout {
            radius: self.radius,
        }
    }
    
    fn update_render_object(&self, render: &mut Self::Render) {
        render.radius = self.radius;
    }
}

// ═══════════════════════════════════════════════════════════════
// RenderObject
// ═══════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct RenderCircularLayout {
    radius: f32,
}

impl RenderObject for RenderCircularLayout {
    type Arity = MultiArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
        let children = cx.children();
        
        if children.is_empty() {
            return Size::ZERO;
        }
        
        // Layout each child with loose constraints
        let child_constraints = BoxConstraints::loose(Size::new(100.0, 100.0));
        
        for &child_id in children {
            cx.layout_child(child_id, child_constraints);
        }
        
        // Our size is diameter
        let diameter = self.radius * 2.0;
        Size::new(diameter, diameter)
    }
    
    fn paint(&self, cx: &PaintCx<MultiArity>) -> BoxedLayer {
        let children = cx.children();
        let child_count = children.len();
        
        let mut container = ContainerLayer::new();
        
        // Position children in circle
        for (i, &child_id) in children.iter().enumerate() {
            let angle = (i as f32 / child_count as f32) * 2.0 * std::f32::consts::PI;
            
            let x = self.radius + self.radius * angle.cos();
            let y = self.radius + self.radius * angle.sin();
            
            let child_layer = cx.capture_child_layer(child_id);
            
            let mut offset_layer = OffsetLayer::new(Offset::new(x, y));
            offset_layer.add_child(child_layer);
            container.add_child(Box::new(offset_layer));
        }
        
        Box::new(container)
    }
}
```

---

## 📊 Arity Comparison Table

| Arity | Children | LayoutCx Methods | PaintCx Methods | Use Cases |
|-------|----------|------------------|-----------------|-----------|
| **LeafArity** | 0 | `constraints()` | `size()` | Text, Image, Placeholder |
| **SingleArity** | 1 | `child()`, `layout_child()` | `child()`, `capture_child_layer()` | Padding, Opacity, Transform |
| **MultiArity** | 0+ | `children()`, `layout_child()`, `parent_data()` | `children()`, `capture_child_layer()` | Row, Column, Stack, Wrap |

## 🎯 Best Practices

### 1. Choose Right Arity

```rust
// ✅ Use LeafArity for no children
impl RenderObject for RenderImage {
    type Arity = LeafArity;
}

// ✅ Use SingleArity for exactly one
impl RenderObject for RenderPadding {
    type Arity = SingleArity;
}

// ✅ Use MultiArity for variable children
impl RenderObject for RenderFlex {
    type Arity = MultiArity;
}
```

### 2. Set Boundaries Wisely

```rust
// ✅ Good - boundary for independent layout
impl RenderObject for RenderScrollView {
    fn is_relayout_boundary(&self) -> bool {
        true  // Content can relayout without affecting scroll container
    }
}

// ✅ Good - boundary for layer caching
impl RenderObject for RenderOpacity {
    fn is_repaint_boundary(&self) -> bool {
        true  // GPU can cache and composite
    }
}
```

### 3. Cache When Possible

```rust
fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
    // ✅ Check if constraints changed
    if self.cached_constraints == Some(cx.constraints()) {
        return self.cached_size;
    }
    
    // Compute layout
    let size = self.compute_layout(cx);
    
    // Cache for next time
    self.cached_constraints = Some(cx.constraints());
    self.cached_size = size;
    
    size
}
```

## 🔗 Cross-References

- **Previous:** [Chapter 2: Widget/Element System](02_widget_element_system.md)
- **Next:** [Chapter 4: Layout Engine](04_layout_engine.md)
- **Related:** [Chapter 5: Layers & Compositing](05_layers_and_painters.md)

---

**Key Takeaway:** FLUI's RenderObject system with Arity provides compile-time safety, type-driven APIs, and zero-cost abstractions for high-performance rendering. The typed context pattern eliminates runtime checks and enables aggressive optimization!
