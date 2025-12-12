# Render Tree

**Tree structure, lifecycle, and parent-child relationships**

---

## Overview

The render tree is a tree of `RenderObject` nodes that defines the visual structure of the application. Each node knows its parent, children, depth, and owner. The tree supports dynamic updates through attach/detach operations.

---

## Tree Structure

### Node Properties

Every `RenderObject` maintains tree relationships:

```rust
pub struct RenderObjectBase {
    // Tree structure
    parent: Option<*mut dyn RenderObject>,
    depth: usize,
    
    // Owner
    owner: Option<Rc<RefCell<PipelineOwner>>>,
    
    // Parent data (stored on this node by parent)
    parent_data: Option<Box<dyn ParentData>>,
}

impl RenderObject for RenderObjectBase {
    fn parent(&self) -> Option<&dyn RenderObject> {
        self.parent.map(|ptr| unsafe { &*ptr })
    }
    
    fn depth(&self) -> usize {
        self.depth
    }
    
    fn owner(&self) -> Option<Rc<RefCell<PipelineOwner>>> {
        self.owner.clone()
    }
    
    fn parent_data<T: ParentData>(&self) -> &T {
        self.parent_data.as_ref()
            .expect("No parent data")
            .as_any()
            .downcast_ref::<T>()
            .expect("Wrong parent data type")
    }
}
```

### Tree Diagram

```
                    RenderView (depth=0, root)
                         │
                         │ owner: PipelineOwner
                         ▼
                  RenderPadding (depth=1)
                         │
                         │ parent: RenderView
                         │ parent_data: BoxParentData
                         ▼
                   RenderFlex (depth=2)
                    ╱    │    ╲
                   ╱     │     ╲
                  ╱      │      ╲
                 ▼       ▼       ▼
        RenderOpacity  RenderImage  RenderText
         (depth=3)     (depth=3)    (depth=3)
              │
              │ parent: RenderFlex
              │ parent_data: FlexParentData
              ▼
         RenderImage
          (depth=4)
```

---

## Tree Operations

### Adopting Children

When a parent adds a child:

```rust
impl RenderObject {
    pub fn adopt_child(&mut self, child: &mut dyn RenderObject) {
        // 1. Setup parent data
        self.setup_parent_data(child);
        
        // 2. Mark parent's layout dirty
        self.mark_needs_layout();
        
        // 3. Mark parent's compositing bits dirty
        self.mark_needs_compositing_bits_update();
        
        // 4. Set child's parent
        child.set_parent(self);
        
        // 5. Set child's depth
        child.set_depth(self.depth() + 1);
        
        // 6. If parent is attached, attach child
        if let Some(owner) = self.owner() {
            child.attach(owner);
        }
    }
}
```

### Dropping Children

When a parent removes a child:

```rust
impl RenderObject {
    pub fn drop_child(&mut self, child: &mut dyn RenderObject) {
        // 1. Detach child if attached
        if child.owner().is_some() {
            child.detach();
        }
        
        // 2. Clear child's parent
        child.set_parent(None);
        
        // 3. Mark parent's layout dirty
        self.mark_needs_layout();
        
        // 4. Mark parent's compositing bits dirty
        self.mark_needs_compositing_bits_update();
    }
}
```

---

## Lifecycle: Attach and Detach

### Attach

Attaching connects a node to the render tree and pipeline owner:

```rust
impl RenderObject {
    pub fn attach(&mut self, owner: Rc<RefCell<PipelineOwner>>) {
        // 1. Set owner
        self.owner = Some(owner.clone());
        
        // 2. Mark needs layout (new node always needs layout)
        if self.parent().is_some() {
            self.mark_needs_layout();
        }
        
        // 3. Mark needs compositing bits update
        self.mark_needs_compositing_bits_update();
        
        // 4. Attach all children recursively
        self.visit_children(|child| {
            child.attach(owner.clone());
        });
    }
}
```

**Effects of attaching:**
- ✅ Node becomes part of the tree
- ✅ Node can schedule layouts/paints
- ✅ Node receives frame callbacks
- ✅ Children are recursively attached

### Detach

Detaching disconnects a node from the pipeline:

```rust
impl RenderObject {
    pub fn detach(&mut self) {
        // 1. Detach all children recursively
        self.visit_children(|child| {
            child.detach();
        });
        
        // 2. Clear owner
        self.owner = None;
        
        // 3. Clear any pending operations
        // (owner will no longer process this node)
    }
}
```

**Effects of detaching:**
- ✅ Node removed from dirty lists
- ✅ No longer receives frame callbacks
- ✅ Cannot schedule layouts/paints
- ✅ Children are recursively detached

### Attach/Detach Flow

```
User Action: Insert child
         │
         ▼
    parent.adopt_child(child)
         │
         ├─ setup_parent_data()
         ├─ mark_needs_layout()
         ├─ set_parent()
         └─ if parent.attached:
                 │
                 ▼
            child.attach(owner)
                 │
                 ├─ set owner
                 ├─ mark_needs_layout()
                 ├─ mark_needs_compositing_bits()
                 └─ recursively attach children
         
User Action: Remove child
         │
         ▼
    parent.drop_child(child)
         │
         ├─ if child.attached:
         │       │
         │       ▼
         │  child.detach()
         │       │
         │       ├─ recursively detach children
         │       └─ clear owner
         │
         ├─ clear_parent()
         └─ mark_needs_layout()
```

---

## Depth Management

### Depth Rules

- **Root node**: depth = 0
- **Child node**: depth = parent.depth + 1
- **Depth must increase** down the tree
- **No cycles allowed** (enforced by depth)

### Depth Updates

When a subtree is moved:

```rust
impl RenderObject {
    pub fn redepth_children(&mut self) {
        let new_depth = self.depth() + 1;
        
        self.visit_children(|child| {
            if child.depth() <= new_depth {
                child.set_depth(new_depth);
                child.redepth_children();  // Recursive
            }
        });
    }
}
```

### Depth Usage

Depth is used for:
1. **Layout sorting**: Shallow nodes layout first
2. **Paint sorting**: Deep nodes paint first
3. **Cycle detection**: Ensure tree structure
4. **Debugging**: Visualize tree hierarchy

---

## Parent Data

### Parent Data Lifecycle

Parent data is metadata stored on children by their parent:

```rust
impl RenderObject {
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        // Override in each render object type
        if !child.has_parent_data_of_type::<MyParentData>() {
            child.set_parent_data(Box::new(MyParentData::default()));
        }
    }
}
```

**When parent data is set:**
1. Child is adopted by parent
2. Parent calls `setup_parent_data(child)`
3. Child stores parent-specific metadata

**Example: Flex Parent Data**

```rust
impl RenderFlex {
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        if !child.has_parent_data_of_type::<FlexParentData>() {
            child.set_parent_data(Box::new(FlexParentData {
                offset: Offset::ZERO,
                flex: None,
                fit: FlexFit::Tight,
            }));
        }
    }
}
```

---

## Tree Traversal

### Visit Children

```rust
pub trait RenderObject {
    fn visit_children<F>(&self, visitor: F)
    where
        F: FnMut(&dyn RenderObject);
    
    fn visit_children_mut<F>(&mut self, visitor: F)
    where
        F: FnMut(&mut dyn RenderObject);
}
```

### Traversal Patterns

**Single Child:**
```rust
impl RenderObject for RenderOpacity {
    fn visit_children<F>(&self, mut visitor: F)
    where
        F: FnMut(&dyn RenderObject)
    {
        if let Some(child) = self.proxy.child() {
            visitor(child);
        }
    }
}
```

**Multiple Children:**
```rust
impl RenderObject for RenderFlex {
    fn visit_children<F>(&self, mut visitor: F)
    where
        F: FnMut(&dyn RenderObject)
    {
        for child in self.children.iter() {
            visitor(child);
        }
    }
}
```

### Tree Walking

**Depth-First Pre-Order:**
```rust
fn walk_tree_preorder(node: &dyn RenderObject, visitor: &mut impl FnMut(&dyn RenderObject)) {
    visitor(node);
    node.visit_children(|child| {
        walk_tree_preorder(child, visitor);
    });
}
```

**Depth-First Post-Order:**
```rust
fn walk_tree_postorder(node: &dyn RenderObject, visitor: &mut impl FnMut(&dyn RenderObject)) {
    node.visit_children(|child| {
        walk_tree_postorder(child, visitor);
    });
    visitor(node);
}
```

**Breadth-First:**
```rust
fn walk_tree_breadth_first(root: &dyn RenderObject, visitor: &mut impl FnMut(&dyn RenderObject)) {
    let mut queue = VecDeque::new();
    queue.push_back(root as *const dyn RenderObject);
    
    while let Some(node_ptr) = queue.pop_front() {
        let node = unsafe { &*node_ptr };
        visitor(node);
        
        node.visit_children(|child| {
            queue.push_back(child as *const dyn RenderObject);
        });
    }
}
```

---

## Tree Invariants

### Must Always Hold

1. **Parent pointer matches**: If A.parent = B, then B has A as child
2. **Depth increases**: child.depth = parent.depth + 1
3. **No cycles**: Following parent pointers eventually reaches None
4. **Owner consistency**: All nodes in subtree have same owner or None
5. **Attached consistency**: If parent.attached, all children.attached

### Validation

```rust
pub fn validate_tree(node: &dyn RenderObject) -> Result<(), String> {
    // Check depth consistency
    node.visit_children(|child| {
        if child.depth() != node.depth() + 1 {
            return Err(format!(
                "Depth inconsistency: parent={}, child={}",
                node.depth(),
                child.depth()
            ));
        }
        
        // Check parent pointer
        if child.parent().map(|p| p as *const _) != Some(node as *const _) {
            return Err("Parent pointer mismatch".to_string());
        }
        
        // Check owner consistency
        match (node.owner(), child.owner()) {
            (Some(po), Some(co)) if Rc::ptr_eq(&po, &co) => {},
            (None, None) => {},
            _ => return Err("Owner mismatch".to_string()),
        }
        
        // Recursively validate children
        validate_tree(child)?;
    });
    
    Ok(())
}
```

---

## Tree Modification Patterns

### Pattern 1: Single Child Replacement

```rust
impl RenderOpacity {
    pub fn set_child(&mut self, new_child: Option<Box<dyn RenderBox>>) {
        // 1. Drop old child if exists
        if let Some(old_child) = self.proxy.take_child() {
            self.drop_child(&mut *old_child);
        }
        
        // 2. Adopt new child if provided
        if let Some(mut new_child) = new_child {
            self.adopt_child(&mut *new_child);
            self.proxy.set_child(new_child);
        }
    }
}
```

### Pattern 2: Multi-Child Insert

```rust
impl RenderFlex {
    pub fn insert(&mut self, index: usize, child: Box<dyn RenderBox>) {
        // 1. Adopt child
        self.adopt_child(&mut *child);
        
        // 2. Insert into container
        self.children.insert(index, child);
    }
}
```

### Pattern 3: Multi-Child Remove

```rust
impl RenderFlex {
    pub fn remove(&mut self, index: usize) -> Box<dyn RenderBox> {
        // 1. Remove from container
        let mut child = self.children.remove(index);
        
        // 2. Drop child
        self.drop_child(&mut *child);
        
        child
    }
}
```

### Pattern 4: Multi-Child Move

```rust
impl RenderFlex {
    pub fn move_child(&mut self, from: usize, to: usize) {
        // No adopt/drop needed - child stays in same parent
        let child = self.children.remove(from);
        self.children.insert(to, child);
        
        // But still need to relayout
        self.mark_needs_layout();
    }
}
```

---

## Tree Ownership

### Ownership Rules

- **Parent owns children**: Children are `Box<dyn RenderObject>`
- **PipelineOwner owns root**: Root is `Box<dyn RenderObject>`
- **No shared ownership**: No `Rc<RenderObject>`

### Memory Management

```rust
// Children are dropped when parent is dropped
impl Drop for RenderFlex {
    fn drop(&mut self) {
        // Children (Vec<Box<dyn RenderBox>>) automatically dropped
        // Detach not needed - parent being destroyed
    }
}

// Root is dropped when PipelineOwner is dropped
impl Drop for PipelineOwner {
    fn drop(&mut self) {
        // root_node (Option<Box<dyn RenderObject>>) automatically dropped
    }
}
```

---

## Tree Debugging

### Debug Print

```rust
pub fn debug_tree(node: &dyn RenderObject, indent: usize) {
    println!(
        "{:indent$}{:?} (depth={}, attached={})",
        "",
        node,
        node.depth(),
        node.owner().is_some(),
        indent = indent * 2
    );
    
    node.visit_children(|child| {
        debug_tree(child, indent + 1);
    });
}
```

**Output:**
```
RenderView (depth=0, attached=true)
  RenderPadding (depth=1, attached=true)
    RenderFlex (depth=2, attached=true)
      RenderOpacity (depth=3, attached=true)
        RenderImage (depth=4, attached=true)
      RenderImage (depth=3, attached=true)
      RenderText (depth=3, attached=true)
```

### Dump Tree to String

```rust
pub fn dump_tree(node: &dyn RenderObject) -> String {
    let mut output = String::new();
    dump_tree_impl(node, 0, &mut output);
    output
}

fn dump_tree_impl(node: &dyn RenderObject, indent: usize, output: &mut String) {
    output.push_str(&format!(
        "{:indent$}{} depth={}\n",
        "",
        node.describe_self(),
        node.depth(),
        indent = indent * 2
    ));
    
    node.visit_children(|child| {
        dump_tree_impl(child, indent + 1, output);
    });
}
```

---

## Performance Considerations

### Minimize Adopt/Drop

Adopt and drop operations are expensive:
- ✅ Reuse nodes when possible
- ✅ Use move operations within same parent
- ❌ Avoid recreating entire subtrees every frame

### Cache Subtrees

For static content:
```rust
pub struct RenderCachedSubtree {
    cached_child: Option<Box<dyn RenderBox>>,
}

impl RenderCachedSubtree {
    pub fn update(&mut self, need_rebuild: bool) {
        if need_rebuild {
            // Rebuild child
            let new_child = self.build_child();
            self.set_child(Some(new_child));
        }
        // Otherwise reuse cached_child
    }
}
```

### Efficient Traversal

Use `visit_children` instead of collecting into Vec:
```rust
// ❌ Inefficient
let children: Vec<&dyn RenderObject> = node.collect_children();
for child in children {
    // ...
}

// ✅ Efficient
node.visit_children(|child| {
    // ...
});
```

---

## File Organization

```
flui-rendering/src/
├── render_object.rs       # RenderObject trait + base implementation
├── tree/
│   ├── mod.rs
│   ├── attach.rs          # Attach/detach logic
│   ├── depth.rs           # Depth management
│   ├── parent_data.rs     # Parent data setup
│   └── traversal.rs       # Tree walking algorithms
└── pipeline/
    └── pipeline_owner.rs  # Owns root of tree
```

---

## Summary

| Concept | Key Points |
|---------|------------|
| **Structure** | Parent pointer, depth, owner |
| **Lifecycle** | Attach (connect to pipeline) / Detach (disconnect) |
| **Operations** | adopt_child, drop_child, visit_children |
| **Depth** | Increases down tree, used for sorting |
| **Parent Data** | Metadata stored on child by parent |
| **Invariants** | No cycles, depth consistency, owner consistency |
| **Ownership** | Parent owns children (Box), no shared ownership |

---

## Next Steps

- [[Pipeline]] - How tree integrates with rendering pipeline
- [[Object Catalog]] - Tree structure of different objects
- [[Parent Data]] - Metadata types

---

**See Also:**
- [[Protocol]] - Type system for tree nodes
- [[Implementation Guide]] - Creating tree-aware objects
