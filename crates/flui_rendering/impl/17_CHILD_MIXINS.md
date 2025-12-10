# Child Management Mixins

This document describes the patterns for managing children in RenderObjects.

## Overview

Flutter provides two main mixins for child management:
- **RenderObjectWithChildMixin** - single child
- **ContainerRenderObjectMixin** - multiple children (linked list)

These are architectural patterns, not concrete implementations.

## RenderObjectWithChildMixin

For RenderObjects with exactly one optional child.

### Flutter Definition

```dart
mixin RenderObjectWithChildMixin<ChildType extends RenderObject> on RenderObject {
  ChildType? _child;
  
  ChildType? get child => _child;
  
  set child(ChildType? value) {
    if (_child != null) {
      dropChild(_child!);
    }
    _child = value;
    if (_child != null) {
      adoptChild(_child!);
    }
  }
  
  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    _child?.attach(owner);
  }
  
  @override
  void detach() {
    super.detach();
    _child?.detach();
  }
  
  @override
  void visitChildren(RenderObjectVisitor visitor) {
    if (_child != null) {
      visitor(_child!);
    }
  }
}
```

### Key Behaviors

1. **Adopt/Drop**: When child changes, properly adopt new and drop old
2. **Attach/Detach**: Propagate to child
3. **Visit**: Include child in tree traversals

### Rust Translation

```rust
/// Mixin-like struct for single child management.
pub struct SingleChild<Child> {
    child: Option<Child>,
}

impl<Child: RenderObject> SingleChild<Child> {
    pub fn new() -> Self {
        Self { child: None }
    }
    
    pub fn child(&self) -> Option<&Child> {
        self.child.as_ref()
    }
    
    pub fn child_mut(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }
    
    /// Set child with proper adopt/drop handling.
    pub fn set_child(&mut self, value: Option<Child>, owner: &mut dyn RenderOwner) {
        // Drop old child
        if let Some(old) = self.child.take() {
            owner.drop_child(&old);
        }
        
        // Adopt new child
        if let Some(ref new) = value {
            owner.adopt_child(new);
        }
        
        self.child = value;
    }
    
    /// Attach child to pipeline.
    pub fn attach(&mut self, owner: &PipelineOwner) {
        if let Some(child) = &mut self.child {
            child.attach(owner);
        }
    }
    
    /// Detach child from pipeline.
    pub fn detach(&mut self) {
        if let Some(child) = &mut self.child {
            child.detach();
        }
    }
    
    /// Visit child if present.
    pub fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(child);
        }
    }
}
```

### Alternative: Trait-based

```rust
/// Trait for RenderObjects with a single child.
pub trait HasSingleChild {
    type Child: RenderObject;
    
    fn child(&self) -> Option<&Self::Child>;
    fn child_mut(&mut self) -> Option<&mut Self::Child>;
    fn set_child(&mut self, child: Option<Self::Child>);
}

/// Blanket implementation for visiting
impl<T: HasSingleChild> VisitChildren for T {
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = self.child() {
            visitor(child);
        }
    }
}
```

## ContainerRenderObjectMixin

For RenderObjects with multiple children in a linked list.

### Flutter Definition

```dart
mixin ContainerRenderObjectMixin<
    ChildType extends RenderObject, 
    ParentDataType extends ContainerParentDataMixin<ChildType>
> on RenderObject {
  
  int _childCount = 0;
  ChildType? _firstChild;
  ChildType? _lastChild;
  
  // === Accessors ===
  
  int get childCount => _childCount;
  ChildType? get firstChild => _firstChild;
  ChildType? get lastChild => _lastChild;
  
  // === Navigation ===
  
  ChildType? childBefore(ChildType child) {
    final ParentDataType childParentData = child.parentData as ParentDataType;
    return childParentData.previousSibling;
  }
  
  ChildType? childAfter(ChildType child) {
    final ParentDataType childParentData = child.parentData as ParentDataType;
    return childParentData.nextSibling;
  }
  
  // === Mutations ===
  
  void insert(ChildType child, {ChildType? after});
  void add(ChildType child);
  void remove(ChildType child);
  void removeAll();
  void move(ChildType child, {ChildType? after});
  
  // === Lifecycle ===
  
  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    ChildType? child = _firstChild;
    while (child != null) {
      child.attach(owner);
      final ParentDataType childParentData = child.parentData as ParentDataType;
      child = childParentData.nextSibling;
    }
  }
  
  @override
  void visitChildren(RenderObjectVisitor visitor) {
    ChildType? child = _firstChild;
    while (child != null) {
      visitor(child);
      final ParentDataType childParentData = child.parentData as ParentDataType;
      child = childParentData.nextSibling;
    }
  }
}
```

### ContainerParentDataMixin

Children store links to siblings:

```dart
mixin ContainerParentDataMixin<ChildType extends RenderObject> on ParentData {
  ChildType? previousSibling;
  ChildType? nextSibling;
}
```

### Rust Translation (Arena-based)

Using an arena/slotmap for storage:

```rust
use slotmap::{SlotMap, new_key_type};

new_key_type! {
    pub struct ChildId;
}

/// Parent data with sibling links.
#[derive(Debug, Default)]
pub struct ContainerParentData {
    pub previous_sibling: Option<ChildId>,
    pub next_sibling: Option<ChildId>,
    pub offset: Offset,
}

/// Container managing multiple children.
pub struct ChildContainer<Child> {
    children: SlotMap<ChildId, Child>,
    first_child: Option<ChildId>,
    last_child: Option<ChildId>,
    child_count: usize,
}

impl<Child: RenderObject> ChildContainer<Child> {
    pub fn new() -> Self {
        Self {
            children: SlotMap::with_key(),
            first_child: None,
            last_child: None,
            child_count: 0,
        }
    }
    
    // === Accessors ===
    
    pub fn child_count(&self) -> usize {
        self.child_count
    }
    
    pub fn first_child(&self) -> Option<ChildId> {
        self.first_child
    }
    
    pub fn last_child(&self) -> Option<ChildId> {
        self.last_child
    }
    
    pub fn get(&self, id: ChildId) -> Option<&Child> {
        self.children.get(id)
    }
    
    pub fn get_mut(&mut self, id: ChildId) -> Option<&mut Child> {
        self.children.get_mut(id)
    }
    
    // === Navigation ===
    
    pub fn child_before(&self, id: ChildId) -> Option<ChildId> {
        self.get(id)?.parent_data().previous_sibling
    }
    
    pub fn child_after(&self, id: ChildId) -> Option<ChildId> {
        self.get(id)?.parent_data().next_sibling
    }
    
    // === Mutations ===
    
    /// Insert child after given sibling (or at start if after is None).
    pub fn insert(&mut self, child: Child, after: Option<ChildId>, owner: &mut dyn RenderOwner) -> ChildId {
        let id = self.children.insert(child);
        owner.adopt_child(&self.children[id]);
        
        self.link_child(id, after);
        self.child_count += 1;
        
        owner.mark_needs_layout();
        id
    }
    
    /// Add child at the end.
    pub fn add(&mut self, child: Child, owner: &mut dyn RenderOwner) -> ChildId {
        self.insert(child, self.last_child, owner)
    }
    
    /// Remove child.
    pub fn remove(&mut self, id: ChildId, owner: &mut dyn RenderOwner) -> Option<Child> {
        self.unlink_child(id);
        
        if let Some(child) = self.children.remove(id) {
            owner.drop_child(&child);
            self.child_count -= 1;
            owner.mark_needs_layout();
            Some(child)
        } else {
            None
        }
    }
    
    /// Remove all children.
    pub fn remove_all(&mut self, owner: &mut dyn RenderOwner) {
        while let Some(id) = self.first_child {
            self.remove(id, owner);
        }
    }
    
    /// Move child to new position.
    pub fn move_child(&mut self, id: ChildId, after: Option<ChildId>, owner: &mut dyn RenderOwner) {
        // Unlink from current position
        self.unlink_child(id);
        // Link at new position
        self.link_child(id, after);
        owner.mark_needs_layout();
    }
    
    // === Private Helpers ===
    
    fn link_child(&mut self, id: ChildId, after: Option<ChildId>) {
        let child = &mut self.children[id];
        let parent_data = child.parent_data_mut();
        
        if let Some(after_id) = after {
            // Insert after specified sibling
            let after_next = self.children[after_id].parent_data().next_sibling;
            
            parent_data.previous_sibling = Some(after_id);
            parent_data.next_sibling = after_next;
            
            self.children[after_id].parent_data_mut().next_sibling = Some(id);
            
            if let Some(after_next_id) = after_next {
                self.children[after_next_id].parent_data_mut().previous_sibling = Some(id);
            } else {
                self.last_child = Some(id);
            }
        } else {
            // Insert at beginning
            parent_data.previous_sibling = None;
            parent_data.next_sibling = self.first_child;
            
            if let Some(first_id) = self.first_child {
                self.children[first_id].parent_data_mut().previous_sibling = Some(id);
            } else {
                self.last_child = Some(id);
            }
            self.first_child = Some(id);
        }
    }
    
    fn unlink_child(&mut self, id: ChildId) {
        let child = &self.children[id];
        let prev = child.parent_data().previous_sibling;
        let next = child.parent_data().next_sibling;
        
        // Update previous sibling's next
        if let Some(prev_id) = prev {
            self.children[prev_id].parent_data_mut().next_sibling = next;
        } else {
            self.first_child = next;
        }
        
        // Update next sibling's previous
        if let Some(next_id) = next {
            self.children[next_id].parent_data_mut().previous_sibling = prev;
        } else {
            self.last_child = prev;
        }
        
        // Clear this child's links
        let child = &mut self.children[id];
        let parent_data = child.parent_data_mut();
        parent_data.previous_sibling = None;
        parent_data.next_sibling = None;
    }
    
    // === Lifecycle ===
    
    pub fn attach(&mut self, owner: &PipelineOwner) {
        let mut current = self.first_child;
        while let Some(id) = current {
            let child = &mut self.children[id];
            child.attach(owner);
            current = child.parent_data().next_sibling;
        }
    }
    
    pub fn detach(&mut self) {
        let mut current = self.first_child;
        while let Some(id) = current {
            let child = &mut self.children[id];
            let next = child.parent_data().next_sibling;
            child.detach();
            current = next;
        }
    }
    
    // === Iteration ===
    
    pub fn visit_children(&self, mut visitor: impl FnMut(&Child)) {
        let mut current = self.first_child;
        while let Some(id) = current {
            let child = &self.children[id];
            visitor(child);
            current = child.parent_data().next_sibling;
        }
    }
    
    pub fn visit_children_mut(&mut self, mut visitor: impl FnMut(&mut Child)) {
        let mut current = self.first_child;
        while let Some(id) = current {
            let child = &mut self.children[id];
            let next = child.parent_data().next_sibling;
            visitor(child);
            current = next;
        }
    }
}

/// Iterator over children.
pub struct ChildIter<'a, Child> {
    container: &'a ChildContainer<Child>,
    current: Option<ChildId>,
}

impl<'a, Child: RenderObject> Iterator for ChildIter<'a, Child> {
    type Item = (ChildId, &'a Child);
    
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.current?;
        let child = self.container.get(id)?;
        self.current = child.parent_data().next_sibling;
        Some((id, child))
    }
}
```

## Proxy Pattern

For RenderObjects that wrap a single child and delegate most behavior.

### Concept

```rust
/// A RenderBox that delegates to its child.
/// Base for visual effects (opacity, transform, clip).
pub trait RenderProxyBox: RenderBox {
    fn child(&self) -> Option<&dyn RenderBox>;
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;
    
    // Default implementations delegate to child
    
    fn perform_layout(&mut self) {
        if let Some(child) = self.child_mut() {
            child.layout(self.constraints());
            self.set_size(child.size());
        } else {
            self.set_size(self.constraints().smallest());
        }
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
    
    fn hit_test_children(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position)
        } else {
            false
        }
    }
}
```

## Shifted Pattern

For RenderObjects that position child at non-zero offset.

### Concept

```rust
/// A RenderBox that positions child via BoxParentData.offset.
/// Base for padding, alignment, custom positioning.
pub trait RenderShiftedBox: RenderBox {
    fn child(&self) -> Option<&dyn RenderBox>;
    fn child_offset(&self) -> Offset;
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset + self.child_offset());
        }
    }
    
    fn hit_test_children(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            let child_position = position - self.child_offset();
            child.hit_test(result, child_position)
        } else {
            false
        }
    }
}
```

### Difference from Proxy

| RenderProxyBox | RenderShiftedBox |
|----------------|------------------|
| Child at (0, 0) | Child at variable offset |
| Size = child size | Size may differ from child |
| No position control | Full position control |
| For: effects | For: layout/positioning |

## Source Reference

Based on analysis of:
- [RenderObjectWithChildMixin](https://api.flutter.dev/flutter/rendering/RenderObjectWithChildMixin-mixin.html)
- [ContainerRenderObjectMixin](https://api.flutter.dev/flutter/rendering/ContainerRenderObjectMixin-mixin.html)
- [ContainerParentDataMixin](https://api.flutter.dev/flutter/rendering/ContainerParentDataMixin-mixin.html)
- [RenderProxyBox](https://api.flutter.dev/flutter/rendering/RenderProxyBox-class.html)
- [RenderShiftedBox](https://api.flutter.dev/flutter/rendering/RenderShiftedBox-class.html)
