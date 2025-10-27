# Chapter 4: Layout Engine

## 📋 Overview

Layout Engine - это система вычисления размеров и позиций виджетов. FLUI использует **constraint-based layout** (как Flutter), где constraints текут вниз по дереву, а sizes возвращаются вверх. Ключевая особенность - **aggressive caching** и **relayout boundaries** для оптимальной производительности.

## 📐 BoxConstraints - Layout Constraints

### Core Definition

```rust
/// BoxConstraints - rectangular layout constraints
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width
    pub min_width: f32,
    
    /// Maximum width
    pub max_width: f32,
    
    /// Minimum height
    pub min_height: f32,
    
    /// Maximum height
    pub max_height: f32,
}

impl BoxConstraints {
    /// Create constraints with min and max size
    pub fn new(min: Size, max: Size) -> Self {
        Self {
            min_width: min.width,
            max_width: max.width,
            min_height: min.height,
            max_height: max.height,
        }
    }
    
    /// Tight constraints (exact size required)
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }
    
    /// Loose constraints (any size from 0 to max)
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }
    
    /// Expand to fill parent (tight to max)
    pub fn expand() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }
    
    /// Constrain to finite size
    pub fn biggest(&self) -> Size {
        Size::new(
            self.max_width.min(f32::MAX),
            self.max_height.min(f32::MAX),
        )
    }
    
    /// Constrain to smallest possible size
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }
    
    /// Check if constraints require exact size
    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width
            && self.min_height == self.max_height
    }
    
    /// Check if constraints are unbounded
    pub fn is_unbounded(&self) -> bool {
        self.max_width.is_infinite() || self.max_height.is_infinite()
    }
    
    /// Constrain size to fit within these constraints
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }
    
    /// Deflate constraints by padding
    pub fn deflate(&self, insets: EdgeInsets) -> Self {
        let horizontal = insets.horizontal();
        let vertical = insets.vertical();
        
        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }
    
    /// Tighten constraints
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.unwrap_or(self.min_width),
            max_width: width.unwrap_or(self.max_width),
            min_height: height.unwrap_or(self.min_height),
            max_height: height.unwrap_or(self.max_height),
        }
    }
    
    /// Loosen constraints (remove minimum)
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }
}

// Hash implementation for caching
impl std::hash::Hash for BoxConstraints {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Use bits representation for deterministic hashing
        self.min_width.to_bits().hash(state);
        self.max_width.to_bits().hash(state);
        self.min_height.to_bits().hash(state);
        self.max_height.to_bits().hash(state);
    }
}

impl Eq for BoxConstraints {}
```

### Constraint Examples

```rust
// ═══════════════════════════════════════════════════════════════
// Tight - exact size required
// ═══════════════════════════════════════════════════════════════

let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
// min_width = 100, max_width = 100
// min_height = 50, max_height = 50
// Child MUST be exactly 100x50

// ═══════════════════════════════════════════════════════════════
// Loose - any size up to max
// ═══════════════════════════════════════════════════════════════

let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
// min_width = 0, max_width = 200
// min_height = 0, max_height = 100
// Child can be 0x0 to 200x100

// ═══════════════════════════════════════════════════════════════
// Bounded - range of sizes
// ═══════════════════════════════════════════════════════════════

let bounded = BoxConstraints::new(
    Size::new(50.0, 25.0),  // min
    Size::new(200.0, 100.0), // max
);
// Child must be between 50x25 and 200x100

// ═══════════════════════════════════════════════════════════════
// Unbounded - infinite in one dimension
// ═══════════════════════════════════════════════════════════════

let unbounded = BoxConstraints::new(
    Size::ZERO,
    Size::new(f32::INFINITY, 100.0),
);
// Width unbounded, height max 100
// Used in scrolling contexts
```

---

## 🔄 Layout Algorithm

### The Rule: Constraints Down, Sizes Up

```
                Parent
                  │
      constraints │ (e.g., 0-500 × 0-300)
                  ↓
                Child
                  │
           size   │ (e.g., 200 × 100)
                  ↑
                Parent
```

**Key Rules:**
1. Parent passes constraints down
2. Child returns size up (within constraints!)
3. Single pass (with relayout boundaries)
4. Child MUST respect parent's constraints

### Layout Flow Example

```rust
// ═══════════════════════════════════════════════════════════════
// Example: Padding → Container → Text
// ═══════════════════════════════════════════════════════════════

// Step 1: Root gets screen constraints
// Root: BoxConstraints::tight(Size::new(800, 600))

// Step 2: Padding receives constraints
impl RenderObject for RenderPadding {
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        // Received: 0-800 × 0-600
        println!("Padding received: {:?}", cx.constraints());
        
        // Deflate by padding (16px all sides)
        let child_constraints = cx.constraints().deflate(
            EdgeInsets::all(16.0)
        );
        // Pass down: 0-768 × 0-568
        println!("Padding passes to child: {:?}", child_constraints);
        
        let child = cx.child();
        let child_size = cx.layout_child(child, child_constraints);
        // Received back: 200 × 100
        println!("Padding received from child: {:?}", child_size);
        
        // Return: child size + padding
        let size = Size::new(
            child_size.width + 32.0,  // 200 + 32 = 232
            child_size.height + 32.0,  // 100 + 32 = 132
        );
        println!("Padding returns: {:?}", size);
        
        size  // 232 × 132
    }
}

// Step 3: Container receives constraints
impl RenderObject for RenderContainer {
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        // Received: 0-768 × 0-568
        println!("Container received: {:?}", cx.constraints());
        
        // Has fixed width=200, height=100
        let child_constraints = BoxConstraints::tight(
            Size::new(self.width, self.height)
        );
        // Pass down: 200-200 × 100-100 (tight!)
        println!("Container passes to child: {:?}", child_constraints);
        
        let child = cx.child();
        let child_size = cx.layout_child(child, child_constraints);
        // Received back: 200 × 100 (must match tight constraints)
        
        Size::new(self.width, self.height)  // 200 × 100
    }
}

// Step 4: Text receives tight constraints
impl RenderObject for RenderText {
    fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
        // Received: 200-200 × 100-100 (tight!)
        println!("Text received: {:?}", cx.constraints());
        
        // Layout text within max width
        let paragraph = layout_text(
            &self.text,
            &self.style,
            cx.constraints().max_width(),
        );
        
        // Constrain to fit constraints
        let size = cx.constraints().constrain(
            Size::new(paragraph.width(), paragraph.height())
        );
        // Returns: 200 × 100 (forced by tight constraints)
        
        size
    }
}

// Final result:
// Root -> Padding (232×132) -> Container (200×100) -> Text (200×100)
```

### Layout Violations

```rust
// ❌ Child violates constraints
impl RenderObject for BadWidget {
    fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
        let constraints = cx.constraints();
        // Received: 0-100 × 0-100
        
        // ❌ Returns size larger than max!
        Size::new(200.0, 200.0)  // VIOLATION!
    }
}

// ✅ Correct - respect constraints
impl RenderObject for GoodWidget {
    fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
        let constraints = cx.constraints();
        let desired = Size::new(200.0, 200.0);
        
        // ✅ Constrain to fit
        constraints.constrain(desired)  // Returns 100×100
    }
}
```

---

## 🗂️ Layout Cache

### Cache Key

```rust
use std::hash::{Hash, Hasher};

/// Key for layout cache
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayoutCacheKey {
    /// Element ID
    element_id: ElementId,
    
    /// Constraints
    constraints: BoxConstraints,
    
    /// Optional: child count (for MultiArity)
    child_count: Option<usize>,
    
    /// Optional: intrinsic dimensions
    intrinsic_width: Option<u32>,
    intrinsic_height: Option<u32>,
}

impl LayoutCacheKey {
    pub fn new(
        element_id: ElementId,
        constraints: BoxConstraints,
    ) -> Self {
        Self {
            element_id,
            constraints,
            child_count: None,
            intrinsic_width: None,
            intrinsic_height: None,
        }
    }
    
    pub fn with_child_count(mut self, count: usize) -> Self {
        self.child_count = Some(count);
        self
    }
}
```

### Cache Implementation

```rust
use moka::sync::Cache;
use std::time::Duration;

/// Layout cache with LRU + TTL eviction
pub struct LayoutCache {
    cache: Cache<LayoutCacheKey, LayoutResult>,
    stats: CacheStats,
}

#[derive(Debug, Clone)]
pub struct LayoutResult {
    pub size: Size,
    pub baseline: Option<f32>,
}

#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub invalidations: AtomicU64,
}

impl LayoutCache {
    pub fn new(max_capacity: usize, ttl: Duration) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_capacity as u64)
                .time_to_live(ttl)
                .build(),
            stats: CacheStats::default(),
        }
    }
    
    pub fn get(&self, key: &LayoutCacheKey) -> Option<LayoutResult> {
        let result = self.cache.get(key);
        
        if result.is_some() {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
        }
        
        result
    }
    
    pub fn insert(&self, key: LayoutCacheKey, result: LayoutResult) {
        self.cache.insert(key, result);
    }
    
    pub fn invalidate(&self, element_id: ElementId) {
        self.stats.invalidations.fetch_add(1, Ordering::Relaxed);
        
        // Remove all entries for this element
        self.cache.invalidate_entries_if(move |key, _| {
            key.element_id == element_id
        }).expect("Invalidation failed");
    }
    
    pub fn invalidate_tree(&self, root_id: ElementId, tree: &ElementTree) {
        // Invalidate element and all descendants
        let mut stack = vec![root_id];
        
        while let Some(id) = stack.pop() {
            self.invalidate(id);
            
            // Add children to stack
            if let Some(children) = tree.children(id) {
                stack.extend(children);
            }
        }
    }
    
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
    
    pub fn hit_rate(&self) -> f32 {
        let hits = self.stats.hits.load(Ordering::Relaxed) as f32;
        let misses = self.stats.misses.load(Ordering::Relaxed) as f32;
        let total = hits + misses;
        
        if total == 0.0 {
            0.0
        } else {
            hits / total
        }
    }
}
```

### Using the Cache

```rust
impl RenderPipeline {
    fn layout_node(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Build cache key
        let key = LayoutCacheKey::new(id, constraints);
        
        // Check cache
        if let Some(cached) = self.layout_cache.get(&key) {
            return cached.size;  // ✅ Cache hit!
        }
        
        // ❌ Cache miss - compute layout
        let size = self.layout_node_uncached(id, constraints);
        
        // Store in cache
        self.layout_cache.insert(
            key,
            LayoutResult { size, baseline: None },
        );
        
        size
    }
    
    fn mark_needs_layout(&mut self, id: ElementId) {
        // Invalidate cache for this element
        self.layout_cache.invalidate(id);
        
        // Mark dirty
        self.nodes_needing_layout.push(id);
        
        // Propagate to parent (unless relayout boundary)
        if !self.is_relayout_boundary(id) {
            if let Some(parent) = self.parent(id) {
                self.mark_needs_layout(parent);
            }
        }
    }
}
```

---

## 🚧 Relayout Boundaries

### What is a Relayout Boundary?

**Relayout Boundary** - это RenderObject который **изолирует layout изменения**, предотвращая propagation к родителям.

### When to Use

```rust
// ✅ Good - ScrollView is relayout boundary
impl RenderObject for RenderScrollView {
    type Arity = SingleArity;
    
    fn is_relayout_boundary(&self) -> bool {
        true  // Content size changes don't affect container
    }
    
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        let child = cx.child();
        
        // Give child unbounded constraints (can be any size)
        let child_constraints = BoxConstraints::new(
            Size::ZERO,
            Size::new(cx.constraints().max_width(), f32::INFINITY),
        );
        
        let child_size = cx.layout_child(child, child_constraints);
        
        // Store content size for scrolling
        self.content_size = child_size;
        
        // Return our fixed size (not child's!)
        cx.constraints().biggest()
    }
}

// ✅ Good - Dialog is relayout boundary
impl RenderObject for RenderDialog {
    fn is_relayout_boundary(&self) -> bool {
        true  // Dialog layout independent of background
    }
}

// ❌ Bad - don't use unnecessarily
impl RenderObject for RenderPadding {
    fn is_relayout_boundary(&self) -> bool {
        false  // Padding always depends on child size
    }
}
```

### Boundary Benefits

```
Without relayout boundary:
  Text changes → Column relayout → Scaffold relayout → Root relayout
  (Expensive! Cascades up entire tree)

With relayout boundary (ScrollView):
  Text changes → Column relayout → ScrollView (STOP)
  (Cheap! Isolated to scrollable area)
```

### Implementation

```rust
impl RenderPipeline {
    pub fn mark_needs_layout(&mut self, id: ElementId) {
        // Add to dirty list
        self.nodes_needing_layout.push(id);
        
        // Check if relayout boundary
        if self.is_relayout_boundary(id) {
            // ✅ Stop propagation here!
            return;
        }
        
        // ❌ Not a boundary - propagate to parent
        if let Some(parent) = self.parent(id) {
            self.mark_needs_layout(parent);
        }
    }
    
    fn is_relayout_boundary(&self, id: ElementId) -> bool {
        self.relayout_boundaries.contains(&id)
    }
}
```

---

## 📏 Intrinsic Dimensions

### What are Intrinsics?

**Intrinsic dimensions** - это "natural" размеры widget без constraints.

```rust
pub trait IntrinsicDimensions {
    /// Minimum width for given height
    fn min_intrinsic_width(&self, height: f32) -> f32;
    
    /// Maximum width for given height
    fn max_intrinsic_width(&self, height: f32) -> f32;
    
    /// Minimum height for given width
    fn min_intrinsic_height(&self, width: f32) -> f32;
    
    /// Maximum height for given width
    fn max_intrinsic_height(&self, width: f32) -> f32;
}
```

### Example: Text Intrinsics

```rust
impl IntrinsicDimensions for RenderText {
    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        // Minimum width = longest word
        self.paragraph.longest_line_width()
    }
    
    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        // Maximum width = full text on one line
        self.paragraph.max_intrinsic_width()
    }
    
    fn min_intrinsic_height(&self, width: f32) -> f32 {
        // Minimum height given width constraint
        let paragraph = layout_text(&self.text, &self.style, width);
        paragraph.height()
    }
    
    fn max_intrinsic_height(&self, width: f32) -> f32 {
        // Same as min for text
        self.min_intrinsic_height(width)
    }
}
```

### Use Case: IntrinsicWidth Widget

```rust
/// Widget that sizes child to its intrinsic width
#[derive(Debug, Clone)]
pub struct IntrinsicWidth {
    child: BoxedWidget,
}

impl RenderObjectWidget for IntrinsicWidth {
    type Arity = SingleArity;
    type Render = RenderIntrinsicWidth;
    
    fn create_render_object(&self) -> Self::Render {
        RenderIntrinsicWidth
    }
}

#[derive(Debug)]
pub struct RenderIntrinsicWidth;

impl RenderObject for RenderIntrinsicWidth {
    type Arity = SingleArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        let child = cx.child();
        
        // Get child's intrinsic width
        let intrinsic_width = cx.child_min_intrinsic_width(child, f32::INFINITY);
        
        // Layout child with tight width
        let child_constraints = cx.constraints().tighten(
            Some(intrinsic_width),
            None,
        );
        
        cx.layout_child(child, child_constraints)
    }
    
    fn paint(&self, cx: &PaintCx<SingleArity>) -> BoxedLayer {
        let child = cx.child();
        cx.capture_child_layer(child)
    }
}
```

---

## 🎯 ParentData - Layout Metadata

### What is ParentData?

**ParentData** - это metadata которую родитель прикрепляет к детям для layout.

### Example: Flex ParentData

```rust
#[derive(Debug, Clone)]
pub struct FlexParentData {
    /// Flex factor (0 = fixed size, >0 = flexible)
    pub flex: i32,
    
    /// How to fit child
    pub fit: FlexFit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexFit {
    /// Child can be smaller than flex space
    Loose,
    
    /// Child must fill flex space
    Tight,
}

impl ParentData for FlexParentData {}
```

### Using ParentData in Layout

```rust
impl RenderObject for RenderFlex {
    type Arity = MultiArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
        let children = cx.children();
        let constraints = cx.constraints();
        
        // Calculate flex space
        let mut total_flex = 0;
        let mut allocated_space = 0.0;
        
        // First pass: layout non-flex children
        for &child_id in children {
            if let Some(parent_data) = cx.parent_data::<FlexParentData>(child_id) {
                if parent_data.flex == 0 {
                    // Fixed size child
                    let child_size = cx.layout_child(
                        child_id,
                        constraints.loosen(),
                    );
                    allocated_space += child_size.width;
                } else {
                    // Flexible child
                    total_flex += parent_data.flex;
                }
            }
        }
        
        // Calculate space per flex unit
        let remaining_space = (constraints.max_width() - allocated_space).max(0.0);
        let space_per_flex = if total_flex > 0 {
            remaining_space / total_flex as f32
        } else {
            0.0
        };
        
        // Second pass: layout flex children
        for &child_id in children {
            if let Some(parent_data) = cx.parent_data::<FlexParentData>(child_id) {
                if parent_data.flex > 0 {
                    let child_space = space_per_flex * parent_data.flex as f32;
                    
                    let child_constraints = match parent_data.fit {
                        FlexFit::Tight => {
                            // Must fill space
                            BoxConstraints::tight(
                                Size::new(child_space, constraints.max_height())
                            )
                        }
                        FlexFit::Loose => {
                            // Can be smaller
                            BoxConstraints::new(
                                Size::ZERO,
                                Size::new(child_space, constraints.max_height()),
                            )
                        }
                    };
                    
                    cx.layout_child(child_id, child_constraints);
                }
            }
        }
        
        // Return total size
        Size::new(constraints.max_width(), constraints.max_height())
    }
    
    // ... paint implementation
}
```

---

## ⚡ Performance Optimizations

### 1. Layout Batching

```rust
impl RenderPipeline {
    pub fn flush_layout(&mut self, root_constraints: BoxConstraints) -> Size {
        // Sort dirty nodes by depth (parents first)
        self.nodes_needing_layout.sort_by_key(|&id| {
            self.depth(id)
        });
        
        // Batch layout operations
        let mut batch = Vec::new();
        
        for &node_id in &self.nodes_needing_layout {
            if self.can_batch_layout(node_id) {
                batch.push(node_id);
            } else {
                // Flush batch before non-batchable operation
                self.layout_batch(&batch);
                batch.clear();
                
                // Layout single node
                self.layout_node(node_id, root_constraints);
            }
        }
        
        // Flush remaining batch
        if !batch.is_empty() {
            self.layout_batch(&batch);
        }
        
        self.nodes_needing_layout.clear();
        
        self.root_size()
    }
}
```

### 2. Parallel Layout (Future)

```rust
// Future: parallel layout for independent subtrees
use rayon::prelude::*;

impl RenderPipeline {
    fn layout_children_parallel(
        &mut self,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Vec<Size> {
        children
            .par_iter()
            .map(|&child_id| {
                // Each child layout is independent
                self.layout_node(child_id, constraints)
            })
            .collect()
    }
}
```

### 3. Dry Layout (Measure Without Commit)

```rust
impl LayoutCx<MultiArity> {
    /// Layout child without committing (for measurement)
    pub fn dry_layout_child(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Save current state
        let saved_state = self.save_state();
        
        // Perform layout
        let size = self.layout_child(child_id, constraints);
        
        // Restore state (don't commit)
        self.restore_state(saved_state);
        
        size
    }
}

// Use case: Row needs to measure children before final layout
impl RenderObject for RenderRow {
    fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
        let children = cx.children();
        
        // First pass: measure all children
        let mut total_width = 0.0;
        for &child_id in children {
            let size = cx.dry_layout_child(
                child_id,
                cx.constraints().loosen(),
            );
            total_width += size.width;
        }
        
        // Second pass: final layout with adjusted constraints
        // ...
    }
}
```

---

## 📊 Layout Performance Metrics

```rust
pub struct LayoutMetrics {
    /// Total layout operations
    pub layout_count: AtomicU64,
    
    /// Cache hits
    pub cache_hits: AtomicU64,
    
    /// Cache misses
    pub cache_misses: AtomicU64,
    
    /// Average layout time
    pub avg_layout_time_us: AtomicU64,
    
    /// Max layout depth
    pub max_depth: AtomicU32,
}

impl LayoutMetrics {
    pub fn cache_hit_rate(&self) -> f32 {
        let hits = self.cache_hits.load(Ordering::Relaxed) as f32;
        let misses = self.cache_misses.load(Ordering::Relaxed) as f32;
        let total = hits + misses;
        
        if total == 0.0 {
            0.0
        } else {
            hits / total
        }
    }
}
```

---

## 🎯 Best Practices

### 1. Avoid Unbounded Constraints

```rust
// ❌ Bad - unbounded constraints in Row
row![
    text("Left"),
    scroll_view(/* unbounded width! */),  // ERROR!
]

// ✅ Good - use Expanded to bound
row![
    text("Left"),
    expanded(scroll_view(...)),  // ✅ Bounded
]
```

### 2. Use Relayout Boundaries

```rust
// ✅ Good - isolate expensive layouts
column![
    app_bar(),
    relayout_boundary(
        expensive_scrollable_content()
    ),
    bottom_nav(),
]
```

### 3. Cache Expensive Measurements

```rust
impl RenderObject for RenderCustomLayout {
    fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
        // ✅ Cache expensive computation
        if self.cached_constraints != Some(cx.constraints()) {
            self.expensive_computation(cx);
            self.cached_constraints = Some(cx.constraints());
        }
        
        self.cached_size
    }
}
```

## 🔗 Cross-References

- **Previous:** [Chapter 3: RenderObject System](03_render_objects.md)
- **Next:** [Chapter 5: Layers & Compositing](05_layers_and_painters.md)
- **Related:** [Appendix C: Performance Guide](appendix_c_performance.md)

---

**Key Takeaway:** FLUI's layout engine uses constraint-based layout with aggressive caching and relayout boundaries to achieve optimal performance. Understanding constraints and boundaries is key to building fast UIs!
