# SlottedRenderObjectWidget Pattern

This document analyzes Flutter's SlottedRenderObjectWidget - a pattern for widgets with named child slots instead of a list of children.

## Source Files
- `packages/flutter/lib/src/widgets/slotted_render_object_widget.dart`

## The Problem

Some widgets have a fixed set of named children, not a dynamic list:

```dart
// ListTile has specific named slots:
ListTile(
  leading: Icon(Icons.star),     // Slot: leading
  title: Text('Title'),          // Slot: title  
  subtitle: Text('Subtitle'),    // Slot: subtitle
  trailing: Icon(Icons.arrow),   // Slot: trailing
)
```

Using `MultiChildRenderObjectWidget` with a list doesn't express this structure well.

## Core Architecture

### SlottedMultiChildRenderObjectWidget

```dart
abstract class SlottedMultiChildRenderObjectWidget<SlotType, ChildType extends RenderObject>
    extends RenderObjectWidget
    with SlottedMultiChildRenderObjectWidgetMixin<SlotType, ChildType> {
  
  const SlottedMultiChildRenderObjectWidget({super.key});
}
```

**Type Parameters:**
- `SlotType` - Type identifying slots (typically an enum)
- `ChildType` - Type of render object children (typically `RenderBox`)

### The Mixin Interface

```dart
mixin SlottedMultiChildRenderObjectWidgetMixin<SlotType, ChildType extends RenderObject>
    on RenderObjectWidget {
  
  /// Returns list of all available slots (must be static/constant)
  @protected
  Iterable<SlotType> get slots;

  /// Returns the widget for a given slot (null if empty)
  @protected
  Widget? childForSlot(SlotType slot);

  @override
  SlottedContainerRenderObjectMixin<SlotType, ChildType> createRenderObject(BuildContext context);

  @override
  SlottedRenderObjectElement<SlotType, ChildType> createElement() =>
      SlottedRenderObjectElement<SlotType, ChildType>(this);
}
```

**Key Requirements:**
1. `slots` must return the same list every time (static)
2. `slots` must have unique values
3. `childForSlot` returns current widget for each slot

### SlottedContainerRenderObjectMixin

```dart
mixin SlottedContainerRenderObjectMixin<SlotType, ChildType extends RenderObject> 
    on RenderObject {
  
  final Map<SlotType, ChildType> _slotToChild = <SlotType, ChildType>{};

  /// Get child for a slot
  @protected
  ChildType? childForSlot(SlotType slot) => _slotToChild[slot];

  /// Iterate all non-null children
  @protected
  Iterable<ChildType> get children => _slotToChild.values;

  /// Debug name for slot (uses enum.name if enum)
  @protected
  String debugNameForSlot(SlotType slot) {
    if (slot is Enum) return slot.name;
    return slot.toString();
  }

  // Lifecycle methods delegate to all children
  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    for (final child in children) {
      child.attach(owner);
    }
  }

  @override
  void detach() {
    super.detach();
    for (final child in children) {
      child.detach();
    }
  }

  @override
  void visitChildren(RenderObjectVisitor visitor) {
    children.forEach(visitor);
  }

  // Internal slot management
  void _setChild(ChildType? child, SlotType slot) {
    final oldChild = _slotToChild[slot];
    if (oldChild != null) {
      dropChild(oldChild);
      _slotToChild.remove(slot);
    }
    if (child != null) {
      _slotToChild[slot] = child;
      adoptChild(child);
    }
  }

  void _moveChild(ChildType child, SlotType slot, SlotType oldSlot) {
    assert(slot != oldSlot);
    final oldChild = _slotToChild[oldSlot];
    if (oldChild == child) {
      _setChild(null, oldSlot);
    }
    _setChild(child, slot);
  }
}
```

### SlottedRenderObjectElement

```dart
class SlottedRenderObjectElement<SlotType, ChildType extends RenderObject>
    extends RenderObjectElement {
  
  Map<SlotType, Element> _slotToChild = <SlotType, Element>{};
  Map<Key, Element> _keyedChildren = <Key, Element>{};

  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    _updateChildren();
  }

  @override
  void update(SlottedMultiChildRenderObjectWidgetMixin<SlotType, ChildType> newWidget) {
    super.update(newWidget);
    _updateChildren();
  }

  void _updateChildren() {
    final widget = this.widget as SlottedMultiChildRenderObjectWidgetMixin<SlotType, ChildType>;
    
    // Verify slots are constant
    assert(listEquals(_debugPreviousSlots, widget.slots.toList()),
      '${widget.runtimeType}.slots must not change.');
    
    final oldKeyedElements = _keyedChildren;
    _keyedChildren = <Key, Element>{};
    final oldSlotToChild = _slotToChild;
    _slotToChild = <SlotType, Element>{};

    for (final slot in widget.slots) {
      final Widget? widgetForSlot = widget.childForSlot(slot);
      final Key? newWidgetKey = widgetForSlot?.key;

      // Key-based matching for element reuse
      final Element? oldSlotChild = oldSlotToChild[slot];
      final Element? oldKeyChild = oldKeyedElements[newWidgetKey];

      // Find correct element to update
      final Element? fromElement;
      if (oldKeyChild != null) {
        fromElement = oldSlotToChild.remove(oldKeyChild.slot as SlotType);
      } else if (oldSlotChild?.widget.key == null) {
        fromElement = oldSlotToChild.remove(slot);
      } else {
        fromElement = null;
      }
      
      final newChild = updateChild(fromElement, widgetForSlot, slot);

      if (newChild != null) {
        _slotToChild[slot] = newChild;
        if (newWidgetKey != null) {
          _keyedChildren[newWidgetKey] = newChild;
        }
      }
    }
    
    // Deactivate removed children
    oldSlotToChild.values.forEach(deactivateChild);
  }

  @override
  void insertRenderObjectChild(ChildType child, SlotType slot) {
    renderObject._setChild(child, slot);
  }

  @override
  void removeRenderObjectChild(ChildType child, SlotType slot) {
    if (renderObject._slotToChild[slot] == child) {
      renderObject._setChild(null, slot);
    }
  }

  @override
  void moveRenderObjectChild(ChildType child, SlotType oldSlot, SlotType newSlot) {
    renderObject._moveChild(child, newSlot, oldSlot);
  }
}
```

## Usage Example (from Flutter)

### Widget Definition

```dart
enum _ListTileSlot {
  leading,
  title,
  subtitle,
  trailing,
}

class _ListTile extends SlottedMultiChildRenderObjectWidget<_ListTileSlot, RenderBox> {
  const _ListTile({
    this.leading,
    required this.title,
    this.subtitle,
    this.trailing,
  });

  final Widget? leading;
  final Widget title;
  final Widget? subtitle;
  final Widget? trailing;

  @override
  Iterable<_ListTileSlot> get slots => _ListTileSlot.values;

  @override
  Widget? childForSlot(_ListTileSlot slot) {
    return switch (slot) {
      _ListTileSlot.leading => leading,
      _ListTileSlot.title => title,
      _ListTileSlot.subtitle => subtitle,
      _ListTileSlot.trailing => trailing,
    };
  }

  @override
  _RenderListTile createRenderObject(BuildContext context) => _RenderListTile();
}
```

### RenderObject Definition

```dart
class _RenderListTile extends RenderBox
    with SlottedContainerRenderObjectMixin<_ListTileSlot, RenderBox> {
  
  RenderBox? get leading => childForSlot(_ListTileSlot.leading);
  RenderBox? get title => childForSlot(_ListTileSlot.title);
  RenderBox? get subtitle => childForSlot(_ListTileSlot.subtitle);
  RenderBox? get trailing => childForSlot(_ListTileSlot.trailing);

  @override
  void performLayout() {
    // Layout each slot with appropriate constraints
    final leadingSize = leading?.getDryLayout(constraints) ?? Size.zero;
    
    final titleConstraints = BoxConstraints(
      maxWidth: constraints.maxWidth - leadingSize.width - trailingWidth,
    );
    title?.layout(titleConstraints, parentUsesSize: true);
    
    // Position children using ParentData
    // ...
  }

  @override
  void paint(PaintingContext context, Offset offset) {
    // Paint each slot
    if (leading != null) {
      context.paintChild(leading!, offset + leadingOffset);
    }
    if (title != null) {
      context.paintChild(title!, offset + titleOffset);
    }
    // ...
  }
}
```

---

## FLUI Design

### Slot Trait

```rust
/// Marker trait for slot types
pub trait Slot: Copy + Eq + Hash + 'static {
    /// All possible slot values (must be constant)
    fn all_slots() -> &'static [Self];
    
    /// Debug name for the slot
    fn debug_name(&self) -> &'static str;
}

/// Derive macro for enum slots
#[derive(Slot)]
pub enum ListTileSlot {
    Leading,
    Title,
    Subtitle,
    Trailing,
}

// Generated:
impl Slot for ListTileSlot {
    fn all_slots() -> &'static [Self] {
        &[Self::Leading, Self::Title, Self::Subtitle, Self::Trailing]
    }
    
    fn debug_name(&self) -> &'static str {
        match self {
            Self::Leading => "leading",
            Self::Title => "title",
            Self::Subtitle => "subtitle",
            Self::Trailing => "trailing",
        }
    }
}
```

### SlottedView Trait

```rust
/// View with named child slots
pub trait SlottedView: View {
    type Slot: Slot;
    
    /// Get the view for a specific slot
    fn child_for_slot(&self, slot: Self::Slot) -> Option<&dyn View>;
}
```

### Slotted Element

```rust
pub struct SlottedElement<S: Slot> {
    slot_to_child: HashMap<S, ElementId>,
    keyed_children: HashMap<ViewKey, ElementId>,
}

impl<S: Slot> SlottedElement<S> {
    fn update_children(&mut self, view: &dyn SlottedView<Slot = S>, ctx: &mut UpdateContext) {
        let old_slot_to_child = std::mem::take(&mut self.slot_to_child);
        let old_keyed = std::mem::take(&mut self.keyed_children);
        
        for slot in S::all_slots() {
            let widget = view.child_for_slot(*slot);
            let key = widget.and_then(|w| w.key());
            
            // Key-based element reuse
            let from_element = if let Some(key) = key {
                old_keyed.get(&key).and_then(|id| old_slot_to_child.get(/* slot for id */))
            } else {
                old_slot_to_child.get(slot)
            };
            
            let new_child = ctx.update_child(from_element.copied(), widget, *slot);
            
            if let Some(child_id) = new_child {
                self.slot_to_child.insert(*slot, child_id);
                if let Some(key) = key {
                    self.keyed_children.insert(key, child_id);
                }
            }
        }
        
        // Deactivate removed children
        for (_, child_id) in old_slot_to_child {
            if !self.slot_to_child.values().any(|id| *id == child_id) {
                ctx.deactivate_child(child_id);
            }
        }
    }
}
```

### Slotted RenderObject Mixin

```rust
/// Mixin for render objects with slotted children
pub trait SlottedRenderMixin<S: Slot>: RenderObject {
    /// Get child render object for slot
    fn child_for_slot(&self, slot: S) -> Option<RenderObjectId>;
    
    /// Set child for slot
    fn set_child(&mut self, slot: S, child: Option<RenderObjectId>);
    
    /// Iterate all children (for attach/detach/visit)
    fn children(&self) -> impl Iterator<Item = RenderObjectId>;
}

/// Storage for slotted children
pub struct SlottedChildren<S: Slot> {
    slot_to_child: HashMap<S, RenderObjectId>,
}

impl<S: Slot> SlottedChildren<S> {
    pub fn new() -> Self {
        Self { slot_to_child: HashMap::new() }
    }
    
    pub fn get(&self, slot: S) -> Option<RenderObjectId> {
        self.slot_to_child.get(&slot).copied()
    }
    
    pub fn set(&mut self, slot: S, child: Option<RenderObjectId>) {
        match child {
            Some(id) => { self.slot_to_child.insert(slot, id); }
            None => { self.slot_to_child.remove(&slot); }
        }
    }
    
    pub fn iter(&self) -> impl Iterator<Item = RenderObjectId> + '_ {
        self.slot_to_child.values().copied()
    }
}
```

### Complete Example

```rust
#[derive(Slot)]
pub enum ListTileSlot {
    Leading,
    Title,
    Subtitle,
    Trailing,
}

pub struct ListTile {
    pub leading: Option<Box<dyn View>>,
    pub title: Box<dyn View>,
    pub subtitle: Option<Box<dyn View>>,
    pub trailing: Option<Box<dyn View>>,
}

impl SlottedView for ListTile {
    type Slot = ListTileSlot;
    
    fn child_for_slot(&self, slot: ListTileSlot) -> Option<&dyn View> {
        match slot {
            ListTileSlot::Leading => self.leading.as_deref(),
            ListTileSlot::Title => Some(self.title.as_ref()),
            ListTileSlot::Subtitle => self.subtitle.as_deref(),
            ListTileSlot::Trailing => self.trailing.as_deref(),
        }
    }
}

impl View for ListTile {
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        SlottedElement::<ListTileSlot>::new(self)
    }
}

// RenderObject
pub struct ListTileRender {
    children: SlottedChildren<ListTileSlot>,
    // layout state...
}

impl ListTileRender {
    fn leading(&self) -> Option<RenderObjectId> {
        self.children.get(ListTileSlot::Leading)
    }
    
    fn title(&self) -> Option<RenderObjectId> {
        self.children.get(ListTileSlot::Title)
    }
    
    fn subtitle(&self) -> Option<RenderObjectId> {
        self.children.get(ListTileSlot::Subtitle)
    }
    
    fn trailing(&self) -> Option<RenderObjectId> {
        self.children.get(ListTileSlot::Trailing)
    }
}

impl RenderBox for ListTileRender {
    fn perform_layout(&mut self, constraints: &BoxConstraints, ctx: &mut LayoutContext) {
        // Layout each slot
        let leading_size = if let Some(leading) = self.leading() {
            ctx.layout_child(leading, &BoxConstraints::loose(constraints.max_size()))
        } else {
            Size::ZERO
        };
        
        let title_constraints = BoxConstraints {
            max_width: constraints.max_width - leading_size.width - trailing_width,
            ..Default::default()
        };
        
        if let Some(title) = self.title() {
            ctx.layout_child(title, &title_constraints);
        }
        
        // ... position children
    }
    
    fn paint(&self, ctx: &mut PaintContext) {
        for child_id in self.children.iter() {
            ctx.paint_child(child_id);
        }
    }
}
```

### Derive Macro for Convenience

```rust
/// Derive SlottedView implementation
#[derive(SlottedView)]
#[slots(ListTileSlot)]
pub struct ListTile {
    #[slot(Leading)]
    pub leading: Option<Box<dyn View>>,
    
    #[slot(Title)]
    pub title: Box<dyn View>,
    
    #[slot(Subtitle)]
    pub subtitle: Option<Box<dyn View>>,
    
    #[slot(Trailing)]
    pub trailing: Option<Box<dyn View>>,
}

// Generates SlottedView implementation automatically
```

## Benefits of Slotted Pattern

1. **Type Safety** - Slots are compile-time checked
2. **Named Access** - `title()` instead of `children[1]`
3. **Partial Updates** - Can update single slot efficiently
4. **Key Matching** - Keys work across slots for element reuse
5. **Clear Intent** - Widget structure is explicit

## Comparison with MultiChild

| Aspect | MultiChild | Slotted |
|--------|------------|---------|
| Children | Dynamic list | Fixed named slots |
| Access | By index | By slot name |
| Type safety | Runtime | Compile-time |
| Use case | Unknown count | Known structure |
| Examples | Column, Row | ListTile, AppBar |

## Summary

SlottedRenderObjectWidget provides:
1. **Named slots** - Children identified by type-safe slot enum
2. **Key-based reuse** - Elements can move between slots
3. **Render mixin** - Easy child management in render objects
4. **Type safety** - Compile-time slot validation

FLUI can leverage Rust's stronger type system with derive macros to make this pattern even more ergonomic while maintaining full type safety.
