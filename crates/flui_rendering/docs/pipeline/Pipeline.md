# Rendering Pipeline

**Frame production system and dirty node management**

---

## Overview

FLUI's rendering pipeline transforms a render tree into pixels on screen through four coordinated phases: Layout, Compositing Bits, Paint, and Semantics. The `PipelineOwner` manages dirty node lists and orchestrates these phases efficiently.

---

## Pipeline Phases

### Phase Flow

```
Frame Start
    │
    ▼
┌───────────────────────────────────────────┐
│ 1. LAYOUT PHASE                           │
│    • Process _nodes_needing_layout        │
│    • Sort by depth (shallow → deep)       │
│    • Call performLayout() on each         │
│    • Relayout boundaries contain changes  │
│    • Output: geometry for each node       │
└───────────────────────────────────────────┘
    │
    ▼
┌───────────────────────────────────────────┐
│ 2. COMPOSITING BITS PHASE                 │
│    • Process _nodes_needing_compositing   │
│    • Update _needs_compositing flags      │
│    • Determine layer requirements         │
│    • Output: compositing requirements     │
└───────────────────────────────────────────┘
    │
    ▼
┌───────────────────────────────────────────┐
│ 3. PAINT PHASE                            │
│    • Process _nodes_needing_paint         │
│    • Sort by depth (deep → shallow)       │
│    • Build layer tree                     │
│    • Record drawing commands              │
│    • Output: Layer tree with Pictures     │
└───────────────────────────────────────────┘
    │
    ▼
┌───────────────────────────────────────────┐
│ 4. COMPOSITE PHASE                        │
│    • Convert Layer tree to Scene          │
│    • Send Scene to GPU                    │
│    • Output: Rendered frame               │
└───────────────────────────────────────────┘
    │
    ▼
┌───────────────────────────────────────────┐
│ 5. SEMANTICS PHASE (optional)             │
│    • Process _nodes_needing_semantics     │
│    • Build SemanticsNode tree             │
│    • Send to platform                     │
│    • Output: Accessibility tree           │
└───────────────────────────────────────────┘
```

---

## PipelineOwner Structure

```rust
pub struct PipelineOwner {
    // Root of render tree
    root_node: Option<Box<dyn RenderObject>>,
    
    // Dirty node lists
    nodes_needing_layout: Vec<*mut dyn RenderObject>,
    nodes_needing_compositing_bits_update: Vec<*mut dyn RenderObject>,
    nodes_needing_paint: Vec<*mut dyn RenderObject>,
    nodes_needing_semantics: HashSet<*mut dyn RenderObject>,
    
    // Callbacks
    on_need_visual_update: Option<Box<dyn Fn()>>,
    on_semantics_update: Option<Box<dyn Fn(SemanticsUpdate)>>,
    
    // Semantics
    semantics_owner: Option<SemanticsOwner>,
    semantics_enabled: bool,
    
    // Child owners (for multi-window support)
    children: Vec<PipelineOwner>,
}

impl PipelineOwner {
    pub fn new() -> Self {
        Self {
            root_node: None,
            nodes_needing_layout: Vec::new(),
            nodes_needing_compositing_bits_update: Vec::new(),
            nodes_needing_paint: Vec::new(),
            nodes_needing_semantics: HashSet::new(),
            on_need_visual_update: None,
            on_semantics_update: None,
            semantics_owner: None,
            semantics_enabled: false,
            children: Vec::new(),
        }
    }
    
    pub fn set_root(&mut self, root: Box<dyn RenderObject>) {
        self.root_node = Some(root);
    }
    
    pub fn request_visual_update(&self) {
        if let Some(callback) = &self.on_need_visual_update {
            callback();
        }
    }
}
```

---

## Phase 1: Layout

### Purpose
Compute sizes and positions for all dirty nodes.

### Algorithm

```rust
impl PipelineOwner {
    pub fn flush_layout(&mut self) {
        // Sort by depth (shallow → deep)
        // Parents must layout before children
        self.nodes_needing_layout.sort_by_key(|&ptr| unsafe {
            (*ptr).depth()
        });
        
        // Process each node
        for &node_ptr in &self.nodes_needing_layout {
            unsafe {
                let node = &mut *node_ptr;
                
                // Skip if already clean or detached
                if !node.needs_layout() || node.owner().is_none() {
                    continue;
                }
                
                // Layout the node
                node.layout_without_resize();
            }
        }
        
        // Clear list
        self.nodes_needing_layout.clear();
        
        // Recursively flush children
        for child in &mut self.children {
            child.flush_layout();
        }
    }
}
```

### Relayout Boundaries

A node becomes a relayout boundary when:
1. Parent doesn't use child's size (`parentUsesSize = false`)
2. Size determined only by constraints (`sizedByParent = true`)
3. Constraints are tight (only one valid size)
4. Node is root (no parent)

**Benefit:** Changes within a relayout boundary don't propagate upward.

```rust
impl RenderObject {
    fn layout(&mut self, constraints: Self::Constraints, parent_uses_size: bool) {
        // Determine if this is a relayout boundary
        let is_relayout_boundary = 
            !parent_uses_size ||
            self.sized_by_parent() ||
            constraints.is_tight() ||
            self.parent().is_none();
        
        self.set_is_relayout_boundary(is_relayout_boundary);
        
        // Perform layout
        if self.sized_by_parent() {
            self.perform_resize();
        }
        self.perform_layout(constraints);
        
        // Mark clean
        self.set_needs_layout(false);
    }
}
```

### Layout Algorithm Example

```rust
impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // 1. Determine main/cross axis
        let main_axis = self.direction;
        let cross_axis = main_axis.perpendicular();
        
        // 2. Lay out inflexible children
        let mut allocated_size = 0.0;
        let mut total_flex = 0;
        
        for child in self.children.iter_mut() {
            let parent_data = child.parent_data::<FlexParentData>();
            
            if parent_data.flex.is_none() {
                // Inflexible child
                let child_size = child.perform_layout(constraints);
                allocated_size += main_axis.size_of(child_size);
            } else {
                // Flexible child - count flex
                total_flex += parent_data.flex.unwrap();
            }
        }
        
        // 3. Distribute remaining space to flexible children
        let free_space = constraints.max_main_size() - allocated_size;
        let space_per_flex = if total_flex > 0 {
            free_space / total_flex as f32
        } else {
            0.0
        };
        
        for child in self.children.iter_mut() {
            let parent_data = child.parent_data::<FlexParentData>();
            
            if let Some(flex) = parent_data.flex {
                let child_main_size = space_per_flex * flex as f32;
                let child_constraints = BoxConstraints {
                    min_width: if main_axis == Axis::Horizontal {
                        child_main_size
                    } else {
                        0.0
                    },
                    max_width: if main_axis == Axis::Horizontal {
                        child_main_size
                    } else {
                        constraints.max_width
                    },
                    // ... similar for height
                };
                
                child.perform_layout(child_constraints);
            }
        }
        
        // 4. Position children
        let mut position = 0.0;
        for child in self.children.iter() {
            let child_size = child.size();
            let child_main_size = main_axis.size_of(child_size);
            
            // Set offset in parent data
            let parent_data = child.parent_data_mut::<FlexParentData>();
            parent_data.offset = main_axis.offset(position, 0.0);
            
            position += child_main_size;
        }
        
        // 5. Return size
        Size {
            width: if main_axis == Axis::Horizontal {
                position
            } else {
                constraints.max_width
            },
            height: if main_axis == Axis::Vertical {
                position
            } else {
                constraints.max_height
            },
        }
    }
}
```

---

## Phase 2: Compositing Bits

### Purpose
Determine which subtrees need their own compositing layers.

### Algorithm

```rust
impl PipelineOwner {
    pub fn flush_compositing_bits(&mut self) {
        // Sort by depth (shallow → deep)
        self.nodes_needing_compositing_bits_update.sort_by_key(|&ptr| unsafe {
            (*ptr).depth()
        });
        
        for &node_ptr in &self.nodes_needing_compositing_bits_update {
            unsafe {
                let node = &mut *node_ptr;
                
                if !node.needs_compositing_bits_update() || node.owner().is_none() {
                    continue;
                }
                
                node.update_compositing_bits();
            }
        }
        
        self.nodes_needing_compositing_bits_update.clear();
        
        for child in &mut self.children {
            child.flush_compositing_bits();
        }
    }
}
```

### Compositing Rules

A node needs compositing if:
1. `always_needs_compositing()` returns true (e.g., opacity, transforms)
2. Any descendant needs compositing
3. Node is a repaint boundary

```rust
impl RenderObject {
    fn update_compositing_bits(&mut self) {
        // Check if always needs compositing
        let mut needs_compositing = self.always_needs_compositing();
        
        // Check children
        for child in self.children() {
            if child.needs_compositing() {
                needs_compositing = true;
                break;
            }
        }
        
        // Update state
        self.set_needs_compositing(needs_compositing);
        self.set_needs_compositing_bits_update(false);
        
        // If repaint boundary and compositing changed, mark for paint
        if self.is_repaint_boundary() && 
           needs_compositing != self.was_repaint_boundary() {
            self.mark_needs_paint();
        }
    }
}
```

---

## Phase 3: Paint

### Purpose
Generate display lists and build layer tree.

### Algorithm

```rust
impl PipelineOwner {
    pub fn flush_paint(&mut self) {
        // Sort by depth (deep → shallow)
        // Children paint before parents
        self.nodes_needing_paint.sort_by_key(|&ptr| unsafe {
            std::cmp::Reverse((*ptr).depth())
        });
        
        for &node_ptr in &self.nodes_needing_paint {
            unsafe {
                let node = &mut *node_ptr;
                
                if !node.needs_paint() || node.owner().is_none() {
                    continue;
                }
                
                // Repaint if needed
                if node.is_repaint_boundary() {
                    PaintingContext::repaint_composited_child(node);
                }
            }
        }
        
        self.nodes_needing_paint.clear();
        
        for child in &mut self.children {
            child.flush_paint();
        }
    }
}
```

### Painting Context

```rust
pub struct PaintingContext {
    container_layer: ContainerLayer,
    canvas: Option<Canvas>,
    current_layer: Option<PictureLayer>,
}

impl PaintingContext {
    pub fn repaint_composited_child(child: &mut dyn RenderObject) {
        // Create new layer
        let mut layer = if child.needs_compositing() {
            OffsetLayer::new()
        } else {
            PictureLayer::new(Rect::ZERO)
        };
        
        // Create context
        let mut context = PaintingContext::new(layer);
        
        // Paint child
        child.paint(&mut context, Offset::ZERO);
        
        // Stop recording
        context.stop_recording_if_needed();
    }
    
    pub fn paint_child(&mut self, child: &dyn RenderObject, offset: Offset) {
        if child.is_repaint_boundary() {
            // Child has its own layer, just append it
            self.append_layer(child.layer());
        } else {
            // Paint child into current canvas
            child.paint(self, offset);
        }
    }
    
    pub fn push_clip_rect(&mut self, 
                          needs_compositing: bool,
                          offset: Offset,
                          clip_rect: Rect,
                          painter: impl FnOnce(&mut Self)) {
        if needs_compositing {
            // Create clip layer
            let clip_layer = ClipRectLayer::new(clip_rect);
            self.push_layer(clip_layer);
            painter(self);
            self.pop_layer();
        } else {
            // Clip on canvas
            self.canvas().save();
            self.canvas().clip_rect(clip_rect);
            painter(self);
            self.canvas().restore();
        }
    }
}
```

### Paint Example

```rust
impl RenderBox for RenderOpacity {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.opacity == 0.0 {
            return;  // Invisible
        }
        
        if let Some(child) = self.child() {
            if self.opacity == 1.0 {
                // Fully opaque - paint directly
                context.paint_child(child, offset);
            } else {
                // Semi-transparent - need opacity layer
                context.push_opacity(
                    offset,
                    (self.opacity * 255.0) as u8,
                    |ctx| ctx.paint_child(child, offset)
                );
            }
        }
    }
}
```

---

## Phase 4: Composite

### Purpose
Send layer tree to GPU for final rendering.

### Algorithm

```rust
impl RenderView {
    pub fn composite_frame(&mut self) {
        // Build scene from layer tree
        let mut builder = SceneBuilder::new();
        self.layer().build_scene(&mut builder);
        let scene = builder.build();
        
        // Send to GPU
        self.flutter_view.render(scene);
        
        // Clean up
        scene.dispose();
    }
}
```

---

## Phase 5: Semantics

### Purpose
Build accessibility tree for screen readers.

### Algorithm

```rust
impl PipelineOwner {
    pub fn flush_semantics(&mut self) {
        if self.semantics_owner.is_none() {
            return;
        }
        
        // Filter nodes that are clean
        let mut nodes_to_process: Vec<_> = self.nodes_needing_semantics
            .iter()
            .filter(|&&ptr| unsafe {
                !(*ptr).needs_layout() && (*ptr).owner().is_some()
            })
            .copied()
            .collect();
        
        // Sort by depth
        nodes_to_process.sort_by_key(|&ptr| unsafe { (*ptr).depth() });
        
        self.nodes_needing_semantics.clear();
        
        // Update semantics
        for &node_ptr in &nodes_to_process {
            unsafe {
                let node = &mut *node_ptr;
                node.update_semantics();
            }
        }
        
        // Send update to platform
        self.semantics_owner.as_ref().unwrap().send_semantics_update();
        
        for child in &mut self.children {
            child.flush_semantics();
        }
    }
}
```

---

## Dirty Tracking

### Mark Methods

```rust
impl RenderObject {
    pub fn mark_needs_layout(&mut self) {
        if self.needs_layout {
            return;  // Already dirty
        }
        
        self.needs_layout = true;
        
        if self.is_relayout_boundary {
            // Add self to owner's list
            if let Some(owner) = self.owner() {
                owner.add_needs_layout(self);
                owner.request_visual_update();
            }
        } else {
            // Propagate to parent
            if let Some(parent) = self.parent() {
                parent.mark_needs_layout();
            }
        }
    }
    
    pub fn mark_needs_paint(&mut self) {
        if self.needs_paint {
            return;
        }
        
        self.needs_paint = true;
        
        // Find repaint boundary
        let mut node = self;
        while let Some(parent) = node.parent() {
            if node.is_repaint_boundary {
                break;
            }
            node = parent;
        }
        
        // Add repaint boundary to owner's list
        if let Some(owner) = node.owner() {
            owner.add_needs_paint(node);
            owner.request_visual_update();
        }
    }
    
    pub fn mark_needs_compositing_bits_update(&mut self) {
        if self.needs_compositing_bits_update {
            return;
        }
        
        self.needs_compositing_bits_update = true;
        
        if let Some(owner) = self.owner() {
            owner.add_needs_compositing_bits_update(self);
            owner.request_visual_update();
        }
    }
}
```

### Dirty Propagation Rules

| Change | Propagates To | Stops At |
|--------|---------------|----------|
| Layout | Parent | Relayout boundary |
| Paint | Parent | Repaint boundary |
| Compositing | Children | Leaf nodes |
| Semantics | None | Current node only |

---

## Frame Timeline

```
User Action / Animation Tick
         │
         ▼
    markNeedsLayout() / markNeedsPaint()
         │
         ▼
    requestVisualUpdate()
         │
         ▼
    scheduleFrame() (SchedulerBinding)
         │
         ▼
┌────────────────────────────────┐
│  Engine signals "drawFrame"    │
└────────────────────────────────┘
         │
         ▼
┌────────────────────────────────┐
│  1. flushLayout()              │
│     • Sort by depth            │
│     • layoutWithoutResize()    │
│     • nodes_needing_layout=[]  │
└────────────────────────────────┘
         │
         ▼
┌────────────────────────────────┐
│  2. flushCompositingBits()     │
│     • updateCompositingBits()  │
│     • nodes_needing_...=[]     │
└────────────────────────────────┘
         │
         ▼
┌────────────────────────────────┐
│  3. flushPaint()               │
│     • repaintCompositedChild() │
│     • nodes_needing_paint=[]   │
└────────────────────────────────┘
         │
         ▼
┌────────────────────────────────┐
│  4. compositeFrame()           │
│     • Build Scene              │
│     • Send to GPU              │
└────────────────────────────────┘
         │
         ▼
┌────────────────────────────────┐
│  5. flushSemantics()           │
│     • Build SemanticsNode tree │
│     • Send to platform         │
└────────────────────────────────┘
```

---

## Performance Optimizations

### 1. Relayout Boundaries
- Contain layout changes
- Prevent upward propagation
- ~90% of layouts stay local

### 2. Repaint Boundaries
- Cache unchanged subtrees
- Reduce painting work
- Critical for scrolling performance

### 3. Compositing Layers
- GPU-accelerated transforms
- Efficient opacity changes
- No repaint for simple animations

### 4. Dirty List Sorting
- Process shallow nodes first (layout)
- Process deep nodes first (paint)
- Ensures correct order

---

## Implementation Files

```
flui-rendering/src/pipeline/
├── mod.rs
├── pipeline_owner.rs          # PipelineOwner implementation
├── painting_context.rs        # PaintingContext
├── render_view.rs             # RenderView (root)
└── semantics_owner.rs         # SemanticsOwner
```

---

## Next Steps

- [[Layer System]] - Compositing layer types
- [[Object Catalog]] - How objects integrate with pipeline
- [[Implementation Guide]] - Creating pipeline-aware objects

---

**See Also:**
- [[Protocol]] - Layout constraints and geometry
- [[Trait Hierarchy]] - RenderObject methods
