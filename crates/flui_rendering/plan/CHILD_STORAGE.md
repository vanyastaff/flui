# Child Storage Types

Flutter-подобные типы для хранения детей в RenderObject.

## Overview

| Flutter Mixin | Rust Type | Описание |
|---------------|-----------|----------|
| `RenderObjectWithChildMixin<T>` | `Child<P>` | Single child |
| `ContainerRenderObjectMixin<T, PD>` | `Children<P, PD>` | Multiple children |
| `SlottedContainerRenderObjectMixin<T, S>` | `Slots<P, S>` | Named slots |

---

## RenderHandle

Обёртка над `Box<dyn RenderObject>` с typestate и `Deref`:

```rust
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use flui_tree::{Depth, Mounted, Unmounted, NodeState};

/// Handle для render object с Protocol + NodeState typestate
pub struct RenderHandle<P: Protocol, S: NodeState> {
    render_object: Box<dyn RenderProtocol<P>>,
    depth: Depth,
    parent: Option<RenderId>,
    _marker: PhantomData<(P, S)>,
}

// Deref — вызываем методы RenderObject напрямую
impl<P: Protocol, S: NodeState> Deref for RenderHandle<P, S> {
    type Target = dyn RenderProtocol<P>;
    fn deref(&self) -> &Self::Target {
        self.render_object.as_ref()
    }
}

impl<P: Protocol, S: NodeState> DerefMut for RenderHandle<P, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.render_object.as_mut()
    }
}

// === Unmounted State ===

impl<P: Protocol> RenderHandle<P, Unmounted> {
    pub fn new<R: RenderProtocol<P> + 'static>(render_object: R) -> Self {
        Self {
            render_object: Box::new(render_object),
            depth: Depth::root(),
            parent: None,
            _marker: PhantomData,
        }
    }
    
    pub fn mount(self, parent: Option<RenderId>, depth: Depth) -> RenderHandle<P, Mounted> {
        RenderHandle {
            render_object: self.render_object,
            depth,
            parent,
            _marker: PhantomData,
        }
    }
}

// === Mounted State ===

impl<P: Protocol> RenderHandle<P, Mounted> {
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }
    
    pub fn depth(&self) -> Depth {
        self.depth
    }
    
    pub fn unmount(self) -> RenderHandle<P, Unmounted> {
        RenderHandle {
            render_object: self.render_object,
            depth: Depth::root(),
            parent: None,
            _marker: PhantomData,
        }
    }
    
    pub fn attach(&mut self) {
        self.render_object.attach();
    }
    
    pub fn detach(&mut self) {
        self.render_object.detach();
    }
}

// === Type Aliases ===

pub type BoxHandle<S> = RenderHandle<BoxProtocol, S>;
pub type SliverHandle<S> = RenderHandle<SliverProtocol, S>;
```

---

## Child<P> — Single Child Storage

Flutter's `RenderObjectWithChildMixin`:

```rust
/// Single child storage
/// 
/// # Example
/// 
/// ```rust
/// pub struct RenderPadding {
///     child: Child<BoxProtocol>,
///     padding: EdgeInsets,
/// }
/// ```
pub struct Child<P: Protocol> {
    inner: Option<RenderHandle<P, Mounted>>,
}

impl<P: Protocol> Child<P> {
    /// Create empty child slot
    pub fn new() -> Self {
        Self { inner: None }
    }
    
    /// Create with child
    pub fn with(child: RenderHandle<P, Mounted>) -> Self {
        Self { inner: Some(child) }
    }
    
    /// Get child reference
    pub fn get(&self) -> Option<&RenderHandle<P, Mounted>> {
        self.inner.as_ref()
    }
    
    /// Get child mutable reference
    pub fn get_mut(&mut self) -> Option<&mut RenderHandle<P, Mounted>> {
        self.inner.as_mut()
    }
    
    /// Set child (replaces existing)
    pub fn set(&mut self, child: Option<RenderHandle<P, Mounted>>) {
        self.inner = child;
    }
    
    /// Take child out
    pub fn take(&mut self) -> Option<RenderHandle<P, Mounted>> {
        self.inner.take()
    }
    
    /// Check if has child
    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }
    
    /// Check if empty
    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }
    
    // === Lifecycle Helpers ===
    
    /// Attach child to pipeline owner
    pub fn attach(&mut self) {
        if let Some(child) = &mut self.inner {
            child.attach();
        }
    }
    
    /// Detach child from pipeline owner
    pub fn detach(&mut self) {
        if let Some(child) = &mut self.inner {
            child.detach();
        }
    }
    
    /// Visit child
    pub fn visit(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.inner {
            visitor(child.as_ref());
        }
    }
    
    /// Redepth child
    pub fn redepth(&mut self, parent_depth: Depth) {
        if let Some(child) = &mut self.inner {
            // child.redepth_from_parent(parent_depth);
        }
    }
}

impl<P: Protocol> Default for Child<P> {
    fn default() -> Self {
        Self::new()
    }
}

// Deref для удобного доступа: self.child.get() -> self.child (если single)
impl<P: Protocol> Deref for Child<P> {
    type Target = Option<RenderHandle<P, Mounted>>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P: Protocol> DerefMut for Child<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
```

---

## Children<P, PD> — Multiple Children Storage

Flutter's `ContainerRenderObjectMixin`:

```rust
/// Parent data for container children
#[derive(Debug, Clone)]
pub struct ContainerParentData<PD: ParentData = ()> {
    /// Child offset (set during layout)
    pub offset: Offset,
    /// Custom parent data
    pub data: PD,
}

impl<PD: ParentData + Default> Default for ContainerParentData<PD> {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            data: PD::default(),
        }
    }
}

/// Multiple children storage
/// 
/// # Type Parameters
/// 
/// - `P: Protocol` — BoxProtocol or SliverProtocol
/// - `PD: ParentData` — Custom parent data (e.g., FlexParentData)
/// 
/// # Example
/// 
/// ```rust
/// pub struct RenderFlex {
///     children: Children<BoxProtocol, FlexParentData>,
///     direction: Axis,
/// }
/// ```
pub struct Children<P: Protocol, PD: ParentData = ()> {
    items: Vec<(RenderHandle<P, Mounted>, ContainerParentData<PD>)>,
}

impl<P: Protocol, PD: ParentData + Default> Children<P, PD> {
    /// Create empty children list
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    
    /// Create with capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self { items: Vec::with_capacity(capacity) }
    }
    
    // === Flutter's ContainerRenderObjectMixin Properties ===
    
    /// Number of children (childCount)
    pub fn len(&self) -> usize {
        self.items.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    
    /// First child (firstChild)
    pub fn first(&self) -> Option<&RenderHandle<P, Mounted>> {
        self.items.first().map(|(child, _)| child)
    }
    
    /// First child mutable
    pub fn first_mut(&mut self) -> Option<&mut RenderHandle<P, Mounted>> {
        self.items.first_mut().map(|(child, _)| child)
    }
    
    /// Last child (lastChild)
    pub fn last(&self) -> Option<&RenderHandle<P, Mounted>> {
        self.items.last().map(|(child, _)| child)
    }
    
    /// Last child mutable
    pub fn last_mut(&mut self) -> Option<&mut RenderHandle<P, Mounted>> {
        self.items.last_mut().map(|(child, _)| child)
    }
    
    // === Flutter's ContainerRenderObjectMixin Methods ===
    
    /// Add child to end (add)
    pub fn add(&mut self, child: RenderHandle<P, Mounted>) {
        self.items.push((child, ContainerParentData::default()));
    }
    
    /// Add child with custom parent data
    pub fn add_with_data(&mut self, child: RenderHandle<P, Mounted>, data: PD) {
        self.items.push((child, ContainerParentData { offset: Offset::ZERO, data }));
    }
    
    /// Add multiple children (addAll)
    pub fn add_all(&mut self, children: impl IntoIterator<Item = RenderHandle<P, Mounted>>) {
        for child in children {
            self.add(child);
        }
    }
    
    /// Insert child at index (insert)
    pub fn insert(&mut self, index: usize, child: RenderHandle<P, Mounted>) {
        self.items.insert(index, (child, ContainerParentData::default()));
    }
    
    /// Insert after specific child
    pub fn insert_after(&mut self, after_index: usize, child: RenderHandle<P, Mounted>) {
        self.items.insert(after_index + 1, (child, ContainerParentData::default()));
    }
    
    /// Remove child at index (remove)
    pub fn remove(&mut self, index: usize) -> Option<RenderHandle<P, Mounted>> {
        if index < self.items.len() {
            Some(self.items.remove(index).0)
        } else {
            None
        }
    }
    
    /// Remove all children (removeAll)
    pub fn clear(&mut self) {
        self.items.clear();
    }
    
    /// Move child to new position (move)
    pub fn move_child(&mut self, from: usize, to: usize) {
        if from < self.items.len() && to < self.items.len() {
            let item = self.items.remove(from);
            self.items.insert(to, item);
        }
    }
    
    // === Navigation (childBefore, childAfter) ===
    
    /// Get child at index
    pub fn get(&self, index: usize) -> Option<&RenderHandle<P, Mounted>> {
        self.items.get(index).map(|(child, _)| child)
    }
    
    /// Get child at index mutable
    pub fn get_mut(&mut self, index: usize) -> Option<&mut RenderHandle<P, Mounted>> {
        self.items.get_mut(index).map(|(child, _)| child)
    }
    
    // === Parent Data Access ===
    
    /// Get parent data for child at index
    pub fn parent_data(&self, index: usize) -> Option<&ContainerParentData<PD>> {
        self.items.get(index).map(|(_, pd)| pd)
    }
    
    /// Get parent data mutable
    pub fn parent_data_mut(&mut self, index: usize) -> Option<&mut ContainerParentData<PD>> {
        self.items.get_mut(index).map(|(_, pd)| pd)
    }
    
    /// Set offset for child (convenience)
    pub fn set_offset(&mut self, index: usize, offset: Offset) {
        if let Some((_, pd)) = self.items.get_mut(index) {
            pd.offset = offset;
        }
    }
    
    /// Get offset for child
    pub fn offset(&self, index: usize) -> Option<Offset> {
        self.items.get(index).map(|(_, pd)| pd.offset)
    }
    
    // === Iteration ===
    
    /// Iterate children
    pub fn iter(&self) -> impl Iterator<Item = &RenderHandle<P, Mounted>> {
        self.items.iter().map(|(child, _)| child)
    }
    
    /// Iterate children mutable
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut RenderHandle<P, Mounted>> {
        self.items.iter_mut().map(|(child, _)| child)
    }
    
    /// Iterate with parent data
    pub fn iter_with_data(&self) -> impl Iterator<Item = (&RenderHandle<P, Mounted>, &ContainerParentData<PD>)> {
        self.items.iter().map(|(child, pd)| (child, pd))
    }
    
    /// Iterate with parent data mutable
    pub fn iter_with_data_mut(&mut self) -> impl Iterator<Item = (&mut RenderHandle<P, Mounted>, &mut ContainerParentData<PD>)> {
        self.items.iter_mut().map(|(child, pd)| (child, pd))
    }
    
    /// Iterate with index
    pub fn iter_indexed(&self) -> impl Iterator<Item = (usize, &RenderHandle<P, Mounted>)> {
        self.items.iter().enumerate().map(|(i, (child, _))| (i, child))
    }
    
    // === Lifecycle Helpers ===
    
    /// Attach all children
    pub fn attach_all(&mut self) {
        for (child, _) in &mut self.items {
            child.attach();
        }
    }
    
    /// Detach all children
    pub fn detach_all(&mut self) {
        for (child, _) in &mut self.items {
            child.detach();
        }
    }
    
    /// Visit all children
    pub fn visit_all(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        for (child, _) in &self.items {
            visitor(child.as_ref());
        }
    }
    
    /// Redepth all children
    pub fn redepth_all(&mut self, parent_depth: Depth) {
        for (child, _) in &mut self.items {
            // child.redepth_from_parent(parent_depth);
        }
    }
}

impl<P: Protocol, PD: ParentData + Default> Default for Children<P, PD> {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Slots<P, S> — Named Slots Storage

Flutter's `SlottedContainerRenderObjectMixin`:

```rust
use std::collections::HashMap;
use std::hash::Hash;

/// Marker trait for slot keys (usually enums)
pub trait SlotKey: Eq + Hash + Copy + 'static {}

// Auto-impl for common types
impl<T: Eq + Hash + Copy + 'static> SlotKey for T {}

/// Named slots storage
/// 
/// # Type Parameters
/// 
/// - `P: Protocol` — BoxProtocol or SliverProtocol
/// - `S: SlotKey` — Slot enum type
/// 
/// # Example
/// 
/// ```rust
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// pub enum ListTileSlot {
///     Leading,
///     Title,
///     Subtitle,
///     Trailing,
/// }
/// 
/// pub struct RenderListTile {
///     slots: Slots<BoxProtocol, ListTileSlot>,
///     // ...
/// }
/// ```
pub struct Slots<P: Protocol, S: SlotKey> {
    items: HashMap<S, (RenderHandle<P, Mounted>, Offset)>,
}

impl<P: Protocol, S: SlotKey> Slots<P, S> {
    /// Create empty slots
    pub fn new() -> Self {
        Self { items: HashMap::new() }
    }
    
    // === Flutter's SlottedContainerRenderObjectMixin Methods ===
    
    /// Get child for slot (childForSlot)
    pub fn get(&self, slot: S) -> Option<&RenderHandle<P, Mounted>> {
        self.items.get(&slot).map(|(child, _)| child)
    }
    
    /// Get child for slot mutable
    pub fn get_mut(&mut self, slot: S) -> Option<&mut RenderHandle<P, Mounted>> {
        self.items.get_mut(&slot).map(|(child, _)| child)
    }
    
    /// Set child for slot
    pub fn set(&mut self, slot: S, child: Option<RenderHandle<P, Mounted>>) {
        match child {
            Some(c) => { self.items.insert(slot, (c, Offset::ZERO)); }
            None => { self.items.remove(&slot); }
        }
    }
    
    /// Check if slot has child
    pub fn has(&self, slot: S) -> bool {
        self.items.contains_key(&slot)
    }
    
    /// Remove child from slot
    pub fn remove(&mut self, slot: S) -> Option<RenderHandle<P, Mounted>> {
        self.items.remove(&slot).map(|(child, _)| child)
    }
    
    /// Clear all slots
    pub fn clear(&mut self) {
        self.items.clear();
    }
    
    // === Offset Access ===
    
    /// Get offset for slot
    pub fn offset(&self, slot: S) -> Option<Offset> {
        self.items.get(&slot).map(|(_, offset)| *offset)
    }
    
    /// Set offset for slot
    pub fn set_offset(&mut self, slot: S, offset: Offset) {
        if let Some((_, o)) = self.items.get_mut(&slot) {
            *o = offset;
        }
    }
    
    // === Iteration ===
    
    /// Number of filled slots
    pub fn len(&self) -> usize {
        self.items.len()
    }
    
    /// Check if all slots empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    
    /// Iterate all children (children getter)
    pub fn children(&self) -> impl Iterator<Item = &RenderHandle<P, Mounted>> {
        self.items.values().map(|(child, _)| child)
    }
    
    /// Iterate all children mutable
    pub fn children_mut(&mut self) -> impl Iterator<Item = &mut RenderHandle<P, Mounted>> {
        self.items.values_mut().map(|(child, _)| child)
    }
    
    /// Iterate slots with children
    pub fn iter(&self) -> impl Iterator<Item = (&S, &RenderHandle<P, Mounted>)> {
        self.items.iter().map(|(slot, (child, _))| (slot, child))
    }
    
    /// Iterate slots with children and offsets
    pub fn iter_with_offset(&self) -> impl Iterator<Item = (&S, &RenderHandle<P, Mounted>, Offset)> {
        self.items.iter().map(|(slot, (child, offset))| (slot, child, *offset))
    }
    
    // === Lifecycle Helpers ===
    
    /// Attach all children
    pub fn attach_all(&mut self) {
        for (child, _) in self.items.values_mut() {
            child.attach();
        }
    }
    
    /// Detach all children
    pub fn detach_all(&mut self) {
        for (child, _) in self.items.values_mut() {
            child.detach();
        }
    }
    
    /// Visit all children
    pub fn visit_all(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        for (child, _) in self.items.values() {
            visitor(child.as_ref());
        }
    }
}

impl<P: Protocol, S: SlotKey> Default for Slots<P, S> {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Usage Examples

### RenderPadding (Single Child)

```rust
pub struct RenderPadding {
    child: Child<BoxProtocol>,
    padding: EdgeInsets,
    size: Size,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            child: Child::new(),
            padding,
            size: Size::ZERO,
        }
    }
}

impl RenderObject for RenderPadding {
    fn attach(&mut self) { self.child.attach(); }
    fn detach(&mut self) { self.child.detach(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) { self.child.visit(v); }
}

impl RenderBox for RenderPadding {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child.get_mut() {
            let inner = constraints.deflate(&self.padding);
            let child_size = child.perform_layout(&inner);
            self.size = child_size + self.padding.size();
        }
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.get() {
            child.paint(ctx, offset + Offset::new(self.padding.left, self.padding.top));
        }
    }
}
```

### RenderFlex (Multiple Children)

```rust
/// Flex-specific parent data
#[derive(Debug, Clone, Default)]
pub struct FlexParentData {
    pub flex: f32,
    pub fit: FlexFit,
}

impl ParentData for FlexParentData {}

pub struct RenderFlex {
    children: Children<BoxProtocol, FlexParentData>,
    direction: Axis,
    size: Size,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> Self {
        Self {
            children: Children::new(),
            direction,
            size: Size::ZERO,
        }
    }
    
    pub fn add_child(&mut self, child: RenderHandle<BoxProtocol, Mounted>, flex: f32) {
        self.children.add_with_data(child, FlexParentData { flex, fit: FlexFit::Loose });
    }
}

impl RenderObject for RenderFlex {
    fn attach(&mut self) { self.children.attach_all(); }
    fn detach(&mut self) { self.children.detach_all(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) { self.children.visit_all(v); }
}

impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        // Phase 1: Calculate flex
        let mut total_flex = 0.0;
        for i in 0..self.children.len() {
            if let Some(pd) = self.children.parent_data(i) {
                total_flex += pd.data.flex;
            }
        }
        
        // Phase 2: Layout children
        let mut offset = 0.0;
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = child.perform_layout(&child_constraints);
            self.children.set_offset(i, Offset::new(offset, 0.0));
            offset += child_size.width;
        }
        
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (i, child) in self.children.iter().enumerate() {
            let child_offset = self.children.offset(i).unwrap_or(Offset::ZERO);
            child.paint(ctx, offset + child_offset);
        }
    }
}
```

### RenderListTile (Named Slots)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListTileSlot {
    Leading,
    Title,
    Subtitle,
    Trailing,
}

pub struct RenderListTile {
    slots: Slots<BoxProtocol, ListTileSlot>,
    size: Size,
}

impl RenderListTile {
    pub fn new() -> Self {
        Self {
            slots: Slots::new(),
            size: Size::ZERO,
        }
    }
    
    // Convenience accessors
    pub fn leading(&self) -> Option<&RenderHandle<BoxProtocol, Mounted>> {
        self.slots.get(ListTileSlot::Leading)
    }
    
    pub fn set_leading(&mut self, child: Option<RenderHandle<BoxProtocol, Mounted>>) {
        self.slots.set(ListTileSlot::Leading, child);
    }
    
    pub fn title(&self) -> Option<&RenderHandle<BoxProtocol, Mounted>> {
        self.slots.get(ListTileSlot::Title)
    }
    
    pub fn set_title(&mut self, child: Option<RenderHandle<BoxProtocol, Mounted>>) {
        self.slots.set(ListTileSlot::Title, child);
    }
    
    // ... subtitle, trailing ...
}

impl RenderObject for RenderListTile {
    fn attach(&mut self) { self.slots.attach_all(); }
    fn detach(&mut self) { self.slots.detach_all(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) { self.slots.visit_all(v); }
}

impl RenderBox for RenderListTile {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let mut x = 0.0;
        
        // Layout leading
        if let Some(leading) = self.slots.get_mut(ListTileSlot::Leading) {
            let size = leading.perform_layout(&leading_constraints);
            self.slots.set_offset(ListTileSlot::Leading, Offset::new(x, 0.0));
            x += size.width + 16.0;
        }
        
        // Layout title
        if let Some(title) = self.slots.get_mut(ListTileSlot::Title) {
            let size = title.perform_layout(&title_constraints);
            self.slots.set_offset(ListTileSlot::Title, Offset::new(x, 0.0));
        }
        
        // ... etc ...
        
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (slot, child, child_offset) in self.slots.iter_with_offset() {
            child.paint(ctx, offset + child_offset);
        }
    }
}
```

---

## Summary

| Type | Generic Params | Flutter Equivalent | Use Case |
|------|---------------|-------------------|----------|
| `Child<P>` | Protocol | `RenderObjectWithChildMixin` | Padding, Align, Transform |
| `Children<P, PD>` | Protocol, ParentData | `ContainerRenderObjectMixin` | Flex, Stack, Wrap |
| `Slots<P, S>` | Protocol, SlotKey | `SlottedContainerRenderObjectMixin` | ListTile, InputDecorator |

Все типы предоставляют:
- `attach_all()` / `detach_all()` — lifecycle
- `visit_all()` — tree traversal
- Offset storage — для paint
- `Deref` на `RenderHandle` — вызываем методы напрямую
