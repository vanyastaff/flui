# FLUI Tree Architecture Diagram

## Полная архитектура с flui-tree

```
┌─────────────────────────────────────────────────────────────────────┐
│                          APPLICATION LAYER                           │
│                         (flui_app/main.rs)                          │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          WIDGET LAYER                                │
│  flui_widgets: Column, Row, Text, Button, Container, etc.          │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      COORDINATION LAYER                              │
│                      (flui-pipeline)                                 │
│  ┌──────────────┬─────────────────┬──────────────────┐             │
│  │ BuildPipeline│  LayoutPipeline │   PaintPipeline  │             │
│  │              │                 │                  │             │
│  │ build()      │  layout_flex()  │   paint_box()    │             │
│  │ rebuild()    │  layout_box()   │   paint_layer()  │             │
│  └──────────────┴─────────────────┴──────────────────┘             │
│                        ↓↓↓  uses traits  ↓↓↓                       │
└─────────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     ABSTRACTION LAYER                                │
│                       (flui-tree)                                    │
│  ┌──────────────────────────────────────────────────────────┐      │
│  │ TreeRead      TreeNav      TreeWrite    TreeWriteNav     │      │
│  │   get()        parent()      insert()      set_parent()  │      │
│  │   contains()   children()    remove()      add_child()   │      │
│  │   len()        ancestors()   get_mut()     detach()      │      │
│  └──────────────────────────────────────────────────────────┘      │
│  ┌──────────────────────────────────────────────────────────┐      │
│  │ RenderTreeAccess           DirtyTracking                 │      │
│  │   render_object()            mark_needs_layout()         │      │
│  │   render_state()             mark_needs_paint()          │      │
│  │   render_children()          clear_needs_layout()        │      │
│  │   is_render_element()        needs_layout()              │      │
│  └──────────────────────────────────────────────────────────┘      │
│                    ↑↑↑  implemented by  ↑↑↑                        │
└─────────────────────────────────────────────────────────────────────┘
                   ↑                              ↑
                   │                              │
        implements │                              │ uses (read-only)
                   │                              │
   ┌───────────────┴────────────┐   ┌────────────┴───────────────┐
   │   ELEMENT MANAGEMENT       │   │   RENDERING ALGORITHMS      │
   │   (flui-element)           │   │   (flui-rendering)          │
   │                            │   │                             │
   │ ElementTree                │   │ Layout algorithms:          │
   │   nodes: Slab<Element>     │   │   layout_flex()             │
   │   dirty_sets: DirtySets    │   │   layout_box()              │
   │                            │   │   layout_stack()            │
   │ impl TreeRead              │   │                             │
   │ impl TreeNav               │   │ Paint algorithms:           │
   │ impl TreeWrite             │   │   paint_box()               │
   │ impl TreeWriteNav          │   │   paint_decoration()        │
   │ impl RenderTreeAccess      │   │   paint_text()              │
   │ impl DirtyTracking         │   │                             │
   └────────────────────────────┘   └─────────────────────────────┘
                   ↓                              ↓
                   └──────────────┬───────────────┘
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                       FOUNDATION LAYER                               │
│                     (flui-foundation)                               │
│   ElementId, Slot, Key, ChangeNotifier, AtomicElementFlags         │
└─────────────────────────────────────────────────────────────────────┘
```

## Data Flow: Layout Pipeline

```
1. User interaction triggers rebuild
   │
   ▼
2. BuildPipeline marks elements dirty
   │
   │  tree.mark_needs_layout(element_id)  ← DirtyTracking trait
   │
   ▼
3. LayoutPipeline collects dirty elements
   │
   │  let dirty = tree.elements_needing_layout()  ← DirtyTrackingExt
   │
   ▼
4. Sort by depth (parents first)
   │
   │  dirty.sort_by_key(|id| tree.depth(id))  ← TreeNav trait
   │
   ▼
5. For each dirty element:
   │
   │  a) Get render object type
   │     render_obj = tree.render_object_typed::<RenderFlex>(id)
   │                  ↑ RenderTreeAccessExt
   │
   │  b) Get render children
   │     children = tree.render_children(id)
   │                ↑ RenderTreeAccess
   │
   │  c) Call layout algorithm
   │     size = layout_flex(tree, id, constraints)
   │            ↑ Takes &T: RenderTreeAccess generic
   │
   │  d) Store result in RenderState
   │     state = tree.render_state_typed_mut::<RenderState>(id)
   │             ↑ RenderTreeAccessExt
   │     state.set_size(size)
   │
   │  e) Clear dirty flag
   │     tree.clear_needs_layout(id)
   │     ↑ DirtyTracking
   │
   ▼
6. Layout complete
```

## Component Interaction Example

```
User Code:
┌─────────────────────────────────────┐
│ Column(                             │
│   children: vec![                   │
│     Text("Hello"),                  │
│     Container(                      │
│       child: Button("Click me"),    │
│     ),                              │
│   ]                                 │
│ )                                   │
└─────────────────────────────────────┘
          │
          ▼ Build phase
┌─────────────────────────────────────┐
│ Element Tree (flui-element):        │
│                                     │
│  RenderFlex (Column)                │
│    ├─ RenderParagraph (Text)       │
│    └─ RenderPadding (Container)    │
│         └─ RenderBox (Button)      │
│              └─ RenderParagraph    │
└─────────────────────────────────────┘
          │
          ▼ Layout phase
┌─────────────────────────────────────┐
│ Layout Algorithm (flui-rendering):  │
│                                     │
│ fn layout_flex<T: RenderTreeAccess>│
│ {                                   │
│   // Get render children            │
│   let children = tree               │
│     .render_children(element_id);   │
│   // ^ Uses trait, not concrete type│
│                                     │
│   for child in children {           │
│     // Recursive layout             │
│     layout_child(tree, child, ..)   │
│   }                                 │
│ }                                   │
└─────────────────────────────────────┘
          │
          ▼ Paint phase
┌─────────────────────────────────────┐
│ Paint Algorithm (flui-rendering):   │
│                                     │
│ fn paint_box<T: RenderTreeAccess>  │
│ {                                   │
│   let offset = tree.get_offset(id); │
│   // ^ Uses trait method            │
│                                     │
│   canvas.translate(offset);         │
│   render_box.paint(canvas);         │
│                                     │
│   // Paint children                 │
│   for child in tree.render_children│
│     (id) {                          │
│     paint_box(tree, child, canvas); │
│   }                                 │
│ }                                   │
└─────────────────────────────────────┘
```

## Dependency Graph (No Cycles!)

```
Before flui-tree (with cycles):
═════════════════════════════════
flui_core ──┐
    │       │
    ▼       │
flui_rendering  │
    │           │
    ▼           │
flui_pipeline ──┘
    ↑
    └─ CYCLE! ❌

After flui-tree (clean):
═══════════════════════════
flui-foundation
    │
    ▼
flui-tree (traits only)
    │
    ├──────────────┐
    │              │
    ▼              ▼
flui-element   flui-rendering
    │              │
    └──────┬───────┘
           ▼
    flui-pipeline
    
    ✅ No cycles!
```

## Testing Architecture

```
Production:
───────────
LayoutPipeline → ElementTree → RenderFlex
                  (concrete)    (stored in Element)

Testing:
────────
layout_flex_test → MockTree → RenderFlex
                    (test impl)  (standalone)

┌──────────────────────────────────────┐
│ MockTree (test-only)                 │
│                                      │
│ impl TreeRead {                      │
│   fn get(&self, id) -> &MockNode     │
│ }                                    │
│                                      │
│ impl RenderTreeAccess {              │
│   fn render_object(&self, id)        │
│     -> &dyn Any {                    │
│       &self.mock_renders[id]         │
│   }                                  │
│ }                                    │
└──────────────────────────────────────┘
         ↓
    layout_flex(&mock_tree, ...)
         ↓
    ✅ Tests layout WITHOUT ElementTree!
```

## Iterator Usage Example

```rust
// Find all render elements in subtree
use flui_tree::iter::RenderDescendants;

let render_elements: Vec<_> = 
    RenderDescendants::new(&tree, root)
        .collect();

// Find path to render ancestor
use flui_tree::iter::RenderAncestors;

let render_path: Vec<_> = 
    RenderAncestors::new(&tree, leaf)
        .collect();

// Traverse depth-first
use flui_tree::visitor::{visit_depth_first, TreeVisitor};

struct DebugPrinter;
impl TreeVisitor for DebugPrinter {
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        println!("{:indent$}{:?}", "", id, indent = depth * 2);
        VisitorResult::Continue
    }
}

visit_depth_first(&tree, root, &mut DebugPrinter);
```

## Performance Characteristics

```
Operation                Time         Space          Notes
────────────────────────────────────────────────────────────
tree.get(id)             O(1)        O(1)           Direct slab access
tree.parent(id)          O(1)        O(1)           Stored in Element
tree.children(id)        O(1)        O(1)           Vec slice
tree.ancestors(id)       O(d)        O(1)*          d = depth, *inline stack
tree.descendants(id)     O(n)        O(log n)*      n = subtree, *inline stack
tree.is_descendant(a,b)  O(d)        O(1)           Early termination
render_children(id)      O(k)        O(k)           k = children count
mark_needs_layout(id)    O(1)        O(1)           Atomic flag
needs_layout(id)         O(1)        O(1)           Atomic load

* Uses 32-element inline stack, only allocates for deeper trees
```

## Memory Layout

```
ElementTree:
┌────────────────────────────────────┐
│ nodes: Slab<ElementNode>           │
│   ├─ [0]: Element (RenderFlex)    │
│   │    ├─ base: ElementBase       │
│   │    ├─ view_object: Box<..>    │
│   │    └─ children: Vec<ElementId>│
│   ├─ [1]: Element (RenderBox)     │
│   └─ [2]: Element (RenderText)    │
│                                    │
│ dirty_sets: Arc<DirtySets>        │
│   ├─ layout: RwLock<HashSet>      │
│   └─ paint: RwLock<HashSet>       │
└────────────────────────────────────┘

Element:
┌────────────────────────────────────┐
│ base: ElementBase                  │
│   ├─ parent: Option<ElementId>    │ 8 bytes
│   ├─ slot: Option<Slot>           │ 8 bytes
│   ├─ lifecycle: ElementLifecycle  │ 1 byte
│   └─ flags: AtomicElementFlags    │ 1 byte
│                                    │
│ view_object: Box<dyn ViewObject>  │ 16 bytes
│   └─ RenderViewWrapper             │
│       ├─ render: Box<RenderFlex>   │
│       └─ state: RenderState        │
│           ├─ size: Size            │ 8 bytes
│           ├─ offset: Offset        │ 8 bytes
│           ├─ constraints: Box<..>  │ 16 bytes
│           └─ flags: AtomicU8       │ 1 byte (lock-free!)
│                                    │
│ children: Vec<ElementId>           │ 24 bytes
└────────────────────────────────────┘
Total: ~90 bytes per Element (cache-friendly!)
```

## Summary

flui-tree enables:

✅ **Separation of Concerns**
   - ElementTree: manages tree structure
   - Layout algorithms: work with any tree via traits
   - Pipeline: coordinates without tight coupling

✅ **No Circular Dependencies**
   - Clean dependency graph
   - Each crate has clear responsibility
   - Traits break the cycle

✅ **Testability**
   - Mock trees for testing layout
   - No need for full ElementTree in tests
   - Fast, isolated unit tests

✅ **Performance**
   - O(1) access through slab
   - Lock-free dirty flags
   - Zero-allocation iterators
   - Cache-friendly memory layout

✅ **Maintainability**
   - Clear interfaces (traits)
   - Easy to add new render types
   - Easy to test new algorithms
   - Incremental migration path
