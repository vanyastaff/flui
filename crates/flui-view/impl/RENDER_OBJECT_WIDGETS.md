# RenderObjectWidget Hierarchy

This document analyzes Flutter's RenderObjectWidget class hierarchy - the bridge between Widget and RenderObject trees with different child management strategies.

## Source Files
- `packages/flutter/lib/src/widgets/framework.dart` (Widget & Element classes)
- `packages/flutter/lib/src/rendering/object.dart` (RenderObject mixins)

## Overview

Flutter provides four types of RenderObjectWidgets based on child count:

```
RenderObjectWidget (abstract)
├── LeafRenderObjectWidget        - 0 children
├── SingleChildRenderObjectWidget - 0 or 1 child
├── MultiChildRenderObjectWidget  - list of children
└── SlottedMultiChildRenderObjectWidget - named slots
```

Each type has a matching:
1. **Widget class** - holds child widget(s)
2. **Element class** - manages child element(s)
3. **RenderObject mixin** - manages child render object(s) in rendering layer

---

## 1. LeafRenderObjectWidget (0 children)

### Widget

```dart
abstract class LeafRenderObjectWidget extends RenderObjectWidget {
  const LeafRenderObjectWidget({super.key});

  @override
  LeafRenderObjectElement createElement() => LeafRenderObjectElement(this);
}
```

**Use cases:**
- `RichText` / `RenderParagraph` - text rendering
- `RawImage` - image display
- `Texture` - video/camera textures
- `ErrorWidget` - error display
- Any custom leaf node with no children

### Element

```dart
class LeafRenderObjectElement extends RenderObjectElement {
  LeafRenderObjectElement(LeafRenderObjectWidget super.widget);

  // All child methods assert(false) - no children allowed
  @override
  void forgetChild(Element child) {
    assert(false);
    super.forgetChild(child);
  }

  @override
  void insertRenderObjectChild(RenderObject child, Object? slot) {
    assert(false);
  }

  @override
  void moveRenderObjectChild(RenderObject child, Object? oldSlot, Object? newSlot) {
    assert(false);
  }

  @override
  void removeRenderObjectChild(RenderObject child, Object? slot) {
    assert(false);
  }
}
```

### RenderObject

No special mixin needed - just a regular RenderObject with no child management.

---

## 2. SingleChildRenderObjectWidget (0-1 child)

### Widget

```dart
abstract class SingleChildRenderObjectWidget extends RenderObjectWidget {
  const SingleChildRenderObjectWidget({super.key, this.child});

  /// The widget below this widget in the tree (optional).
  final Widget? child;

  @override
  SingleChildRenderObjectElement createElement() => SingleChildRenderObjectElement(this);
}
```

**Use cases:**
- `Opacity`, `Transform`, `ClipRect`, `ClipRRect` - visual effects
- `Padding`, `Align`, `Center`, `SizedBox` - layout modifiers
- `DecoratedBox`, `ColoredBox` - decoration
- `ConstrainedBox`, `LimitedBox` - constraints
- Most single-child layout widgets

### Element

```dart
class SingleChildRenderObjectElement extends RenderObjectElement {
  SingleChildRenderObjectElement(SingleChildRenderObjectWidget super.widget);

  Element? _child;

  @override
  void visitChildren(ElementVisitor visitor) {
    if (_child != null) {
      visitor(_child!);
    }
  }

  @override
  void forgetChild(Element child) {
    assert(child == _child);
    _child = null;
    super.forgetChild(child);
  }

  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    _child = updateChild(_child, (widget as SingleChildRenderObjectWidget).child, null);
  }

  @override
  void update(SingleChildRenderObjectWidget newWidget) {
    super.update(newWidget);
    assert(widget == newWidget);
    _child = updateChild(_child, newWidget.child, null);
  }

  @override
  void insertRenderObjectChild(RenderObject child, Object? slot) {
    final renderObject = this.renderObject as RenderObjectWithChildMixin<RenderObject>;
    assert(slot == null);  // Single child has no slot
    assert(renderObject.debugValidateChild(child));
    renderObject.child = child;
  }

  @override
  void moveRenderObjectChild(RenderObject child, Object? oldSlot, Object? newSlot) {
    assert(false);  // Single child can't move
  }

  @override
  void removeRenderObjectChild(RenderObject child, Object? slot) {
    final renderObject = this.renderObject as RenderObjectWithChildMixin<RenderObject>;
    assert(slot == null);
    assert(renderObject.child == child);
    renderObject.child = null;
  }
}
```

### RenderObject Mixin: `RenderObjectWithChildMixin`

```dart
mixin RenderObjectWithChildMixin<ChildType extends RenderObject> on RenderObject {
  
  /// Validates child type at runtime (debug only)
  bool debugValidateChild(RenderObject child) {
    assert(() {
      if (child is! ChildType) {
        throw FlutterError.fromParts(<DiagnosticsNode>[
          ErrorSummary(
            'A $runtimeType expected a child of type $ChildType but received a '
            'child of type ${child.runtimeType}.',
          ),
          // ... detailed error with debugCreator info
        ]);
      }
      return true;
    }());
    return true;
  }

  ChildType? _child;

  /// The render object's unique child.
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
  void redepthChildren() {
    if (_child != null) {
      redepthChild(_child!);
    }
  }

  @override
  void visitChildren(RenderObjectVisitor visitor) {
    if (_child != null) {
      visitor(_child!);
    }
  }

  @override
  List<DiagnosticsNode> debugDescribeChildren() {
    return child != null
        ? <DiagnosticsNode>[child!.toDiagnosticsNode(name: 'child')]
        : <DiagnosticsNode>[];
  }
}
```

---

## 3. MultiChildRenderObjectWidget (N children)

### Widget

```dart
abstract class MultiChildRenderObjectWidget extends RenderObjectWidget {
  const MultiChildRenderObjectWidget({super.key, this.children = const <Widget>[]});

  /// The widgets below this widget in the tree.
  /// 
  /// IMPORTANT: 
  /// - If mutating, use Keys on children for proper reconciliation
  /// - Always create a NEW list (Widget is immutable)
  /// - Never modify the list in place
  final List<Widget> children;

  @override
  MultiChildRenderObjectElement createElement() => MultiChildRenderObjectElement(this);
}
```

**Use cases:**
- `Row`, `Column`, `Flex` - flex layouts
- `Stack` - layered positioning  
- `Wrap` - flow layout
- `CustomMultiChildLayout` - custom positioning
- `Flow` - animated positioning

### Element

```dart
class MultiChildRenderObjectElement extends RenderObjectElement {
  MultiChildRenderObjectElement(MultiChildRenderObjectWidget super.widget)
    : assert(!debugChildrenHaveDuplicateKeys(widget, widget.children));

  @override
  ContainerRenderObjectMixin<RenderObject, ContainerParentDataMixin<RenderObject>>
  get renderObject {
    return super.renderObject
        as ContainerRenderObjectMixin<RenderObject, ContainerParentDataMixin<RenderObject>>;
  }

  late List<Element> _children;
  
  // Lazy removal optimization - avoid O(n²) list modifications
  final Set<Element> _forgottenChildren = HashSet<Element>();

  @protected
  @visibleForTesting
  Iterable<Element> get children =>
      _children.where((child) => !_forgottenChildren.contains(child));

  @override
  void visitChildren(ElementVisitor visitor) {
    for (final child in _children) {
      if (!_forgottenChildren.contains(child)) {
        visitor(child);
      }
    }
  }

  @override
  void forgetChild(Element child) {
    assert(_children.contains(child));
    assert(!_forgottenChildren.contains(child));
    _forgottenChildren.add(child);
    super.forgetChild(child);
  }

  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    final multiChildWidget = widget as MultiChildRenderObjectWidget;
    final children = List<Element>.filled(
      multiChildWidget.children.length,
      _NullElement.instance,
    );
    
    Element? previousChild;
    for (var i = 0; i < children.length; i++) {
      final newChild = inflateWidget(
        multiChildWidget.children[i],
        IndexedSlot<Element?>(i, previousChild),  // Slot = (index, previousSibling)
      );
      children[i] = newChild;
      previousChild = newChild;
    }
    _children = children;
  }

  @override
  void update(MultiChildRenderObjectWidget newWidget) {
    super.update(newWidget);
    assert(!debugChildrenHaveDuplicateKeys(widget, newWidget.children));
    _children = updateChildren(
      _children,
      newWidget.children,
      forgottenChildren: _forgottenChildren,
    );
    _forgottenChildren.clear();
  }

  @override
  void insertRenderObjectChild(RenderObject child, IndexedSlot<Element?> slot) {
    final renderObject = this.renderObject;
    assert(renderObject.debugValidateChild(child));
    renderObject.insert(child, after: slot.value?.renderObject);
  }

  @override
  void moveRenderObjectChild(
    RenderObject child,
    IndexedSlot<Element?> oldSlot,
    IndexedSlot<Element?> newSlot,
  ) {
    final renderObject = this.renderObject;
    assert(child.parent == renderObject);
    renderObject.move(child, after: newSlot.value?.renderObject);
  }

  @override
  void removeRenderObjectChild(RenderObject child, Object? slot) {
    final renderObject = this.renderObject;
    assert(child.parent == renderObject);
    renderObject.remove(child);
  }
}
```

### IndexedSlot

```dart
/// Slot that identifies a child by index and previous sibling.
/// 
/// The previous sibling reference enables O(1) insertion in the 
/// render object's linked list (insert after).
@immutable
class IndexedSlot<T extends Element?> {
  const IndexedSlot(this.index, this.value);

  final T value;      // Previous sibling element (null for first child)
  final int index;    // Index in children list

  @override
  bool operator ==(Object other) {
    if (other.runtimeType != runtimeType) return false;
    return other is IndexedSlot && index == other.index && value == other.value;
  }

  @override
  int get hashCode => Object.hash(index, value);
}
```

### ParentData Mixin: `ContainerParentDataMixin`

```dart
/// Parent data to support a doubly-linked list of children.
///
/// Children can be traversed using [nextSibling] or [previousSibling].
mixin ContainerParentDataMixin<ChildType extends RenderObject> on ParentData {
  /// The previous sibling in the parent's child list.
  ChildType? previousSibling;

  /// The next sibling in the parent's child list.
  ChildType? nextSibling;

  @override
  void detach() {
    assert(previousSibling == null, 'Pointers must be nulled before detaching.');
    assert(nextSibling == null, 'Pointers must be nulled before detaching.');
    super.detach();
  }
}
```

### RenderObject Mixin: `ContainerRenderObjectMixin`

```dart
/// Generic mixin for render objects with a doubly-linked list of children.
mixin ContainerRenderObjectMixin<
  ChildType extends RenderObject,
  ParentDataType extends ContainerParentDataMixin<ChildType>
> on RenderObject {
  
  int _childCount = 0;
  int get childCount => _childCount;

  ChildType? _firstChild;
  ChildType? _lastChild;
  
  ChildType? get firstChild => _firstChild;
  ChildType? get lastChild => _lastChild;

  /// Validates child type (debug only)
  bool debugValidateChild(RenderObject child) {
    assert(() {
      if (child is! ChildType) {
        throw FlutterError(/* type mismatch error */);
      }
      return true;
    }());
    return true;
  }

  /// Insert child after the given sibling (null = insert at start)
  void insert(ChildType child, {ChildType? after}) {
    assert(child != this, 'Cannot insert into itself');
    assert(after != this);
    assert(child != after);
    assert(child != _firstChild);
    assert(child != _lastChild);
    adoptChild(child);
    _insertIntoChildList(child, after: after);
  }

  /// Append child to end of list
  void add(ChildType child) {
    insert(child, after: _lastChild);
  }

  /// Add all children to end
  void addAll(List<ChildType>? children) {
    children?.forEach(add);
  }

  /// Remove child from list
  void remove(ChildType child) {
    _removeFromChildList(child);
    dropChild(child);
  }

  /// Remove all children
  void removeAll() {
    ChildType? child = _firstChild;
    while (child != null) {
      final childParentData = child.parentData! as ParentDataType;
      final next = childParentData.nextSibling;
      childParentData.previousSibling = null;
      childParentData.nextSibling = null;
      dropChild(child);
      child = next;
    }
    _firstChild = null;
    _lastChild = null;
    _childCount = 0;
  }

  /// Move child to new position (more efficient than remove+insert)
  void move(ChildType child, {ChildType? after}) {
    assert(child != this);
    assert(after != this);
    assert(child != after);
    assert(child.parent == this);
    final childParentData = child.parentData! as ParentDataType;
    if (childParentData.previousSibling == after) {
      return;  // Already in correct position
    }
    _removeFromChildList(child);
    _insertIntoChildList(child, after: after);
    markNeedsLayout();
  }

  // === Internal linked list operations ===

  void _insertIntoChildList(ChildType child, {ChildType? after}) {
    final childParentData = child.parentData! as ParentDataType;
    assert(childParentData.nextSibling == null);
    assert(childParentData.previousSibling == null);
    _childCount += 1;
    
    if (after == null) {
      // Insert at start
      childParentData.nextSibling = _firstChild;
      if (_firstChild != null) {
        final firstChildParentData = _firstChild!.parentData! as ParentDataType;
        firstChildParentData.previousSibling = child;
      }
      _firstChild = child;
      _lastChild ??= child;
    } else {
      final afterParentData = after.parentData! as ParentDataType;
      if (afterParentData.nextSibling == null) {
        // Insert at end
        assert(after == _lastChild);
        childParentData.previousSibling = after;
        afterParentData.nextSibling = child;
        _lastChild = child;
      } else {
        // Insert in middle
        childParentData.nextSibling = afterParentData.nextSibling;
        childParentData.previousSibling = after;
        
        final prevParentData = childParentData.previousSibling!.parentData! as ParentDataType;
        final nextParentData = childParentData.nextSibling!.parentData! as ParentDataType;
        prevParentData.nextSibling = child;
        nextParentData.previousSibling = child;
      }
    }
  }

  void _removeFromChildList(ChildType child) {
    final childParentData = child.parentData! as ParentDataType;
    
    if (childParentData.previousSibling == null) {
      assert(_firstChild == child);
      _firstChild = childParentData.nextSibling;
    } else {
      final prevParentData = childParentData.previousSibling!.parentData! as ParentDataType;
      prevParentData.nextSibling = childParentData.nextSibling;
    }
    
    if (childParentData.nextSibling == null) {
      assert(_lastChild == child);
      _lastChild = childParentData.previousSibling;
    } else {
      final nextParentData = childParentData.nextSibling!.parentData! as ParentDataType;
      nextParentData.previousSibling = childParentData.previousSibling;
    }
    
    childParentData.previousSibling = null;
    childParentData.nextSibling = null;
    _childCount -= 1;
  }

  // === Lifecycle delegation ===

  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    ChildType? child = _firstChild;
    while (child != null) {
      child.attach(owner);
      final childParentData = child.parentData! as ParentDataType;
      child = childParentData.nextSibling;
    }
  }

  @override
  void detach() {
    super.detach();
    ChildType? child = _firstChild;
    while (child != null) {
      child.detach();
      final childParentData = child.parentData! as ParentDataType;
      child = childParentData.nextSibling;
    }
  }

  @override
  void visitChildren(RenderObjectVisitor visitor) {
    ChildType? child = _firstChild;
    while (child != null) {
      visitor(child);
      final childParentData = child.parentData! as ParentDataType;
      child = childParentData.nextSibling;
    }
  }

  // === Navigation helpers ===

  ChildType? childBefore(ChildType child) {
    assert(child.parent == this);
    final childParentData = child.parentData! as ParentDataType;
    return childParentData.previousSibling;
  }

  ChildType? childAfter(ChildType child) {
    assert(child.parent == this);
    final childParentData = child.parentData! as ParentDataType;
    return childParentData.nextSibling;
  }
}
```

---

## 4. updateChildren Algorithm

The core O(n) reconciliation algorithm for multi-child widgets:

```dart
List<Element> updateChildren(
  List<Element> oldChildren,
  List<Widget> newWidgets, {
  Set<Element>? forgottenChildren,
}) {
  // Optimization: scan from top and bottom to find unchanged sections
  
  int newChildrenTop = 0;
  int oldChildrenTop = 0;
  int newChildrenBottom = newWidgets.length - 1;
  int oldChildrenBottom = oldChildren.length - 1;
  
  final newChildren = List<Element?>.filled(newWidgets.length, null);
  Element? previousChild;

  // === Phase 1: Scan from TOP while widgets match ===
  while ((oldChildrenTop <= oldChildrenBottom) && (newChildrenTop <= newChildrenBottom)) {
    final oldChild = replaceWithNullIfForgotten(oldChildren[oldChildrenTop], forgottenChildren);
    final newWidget = newWidgets[newChildrenTop];
    if (oldChild == null || !Widget.canUpdate(oldChild.widget, newWidget)) break;
    
    final newChild = updateChild(oldChild, newWidget, slotFor(newChildrenTop, previousChild))!;
    newChildren[newChildrenTop] = newChild;
    previousChild = newChild;
    newChildrenTop++;
    oldChildrenTop++;
  }

  // === Phase 2: Scan from BOTTOM while widgets match ===
  while ((oldChildrenTop <= oldChildrenBottom) && (newChildrenTop <= newChildrenBottom)) {
    final oldChild = replaceWithNullIfForgotten(oldChildren[oldChildrenBottom], forgottenChildren);
    final newWidget = newWidgets[newChildrenBottom];
    if (oldChild == null || !Widget.canUpdate(oldChild.widget, newWidget)) break;
    // Don't update yet - wait for slot to be known
    oldChildrenBottom--;
    newChildrenBottom--;
  }

  // === Phase 3: Build key->element map for MIDDLE section ===
  final bool haveOldChildren = oldChildrenTop <= oldChildrenBottom;
  Map<Key, Element>? oldKeyedChildren;
  if (haveOldChildren) {
    oldKeyedChildren = <Key, Element>{};
    while (oldChildrenTop <= oldChildrenBottom) {
      final oldChild = replaceWithNullIfForgotten(oldChildren[oldChildrenTop], forgottenChildren);
      if (oldChild != null) {
        if (oldChild.widget.key != null) {
          oldKeyedChildren[oldChild.widget.key!] = oldChild;
        } else {
          deactivateChild(oldChild);  // No key = can't reuse
        }
      }
      oldChildrenTop++;
    }
  }

  // === Phase 4: Update MIDDLE section using key matching ===
  while (newChildrenTop <= newChildrenBottom) {
    Element? oldChild;
    final newWidget = newWidgets[newChildrenTop];
    if (haveOldChildren) {
      final key = newWidget.key;
      if (key != null) {
        oldChild = oldKeyedChildren![key];
        if (oldChild != null) {
          if (Widget.canUpdate(oldChild.widget, newWidget)) {
            oldKeyedChildren.remove(key);
          } else {
            oldChild = null;
          }
        }
      }
    }
    final newChild = updateChild(oldChild, newWidget, slotFor(newChildrenTop, previousChild))!;
    newChildren[newChildrenTop] = newChild;
    previousChild = newChild;
    newChildrenTop++;
  }

  // === Phase 5: Update BOTTOM section (already matched in phase 2) ===
  newChildrenBottom = newWidgets.length - 1;
  oldChildrenBottom = oldChildren.length - 1;
  while (newChildrenTop <= newChildrenBottom) {
    final oldChild = oldChildren[oldChildrenBottom];
    final newWidget = newWidgets[newChildrenBottom];
    final newChild = updateChild(oldChild, newWidget, slotFor(newChildrenBottom, previousChild))!;
    newChildren[newChildrenBottom] = newChild;
    newChildrenBottom--;
    oldChildrenBottom--;
  }

  // === Cleanup: Deactivate unused keyed children ===
  if (haveOldChildren && oldKeyedChildren!.isNotEmpty) {
    for (final oldChild in oldKeyedChildren.values) {
      deactivateChild(oldChild);
    }
  }

  return newChildren.cast<Element>();
}
```

**Algorithm Complexity: O(n)**
- Phase 1 & 2: Linear scan from ends
- Phase 3: Build key map O(m) where m = unmatched middle
- Phase 4: Key lookup O(1) each
- Phase 5: Linear update of bottom

---

## FLUI Design

### Arity-Based Type System (Existing)

FLUI already uses compile-time arity markers:

```rust
/// Child arity marker types
pub struct Leaf;       // 0 children
pub struct Single;     // exactly 1 child  
pub struct Optional;   // 0 or 1 child
pub struct Variable;   // 0..N children
```

### View Traits by Arity

```rust
/// View with no children (Leaf)
pub trait LeafView: View {
    // No child-related methods
}

/// View with exactly one child
pub trait SingleChildView: View {
    fn child(&self) -> &dyn View;
}

/// View with optional child  
pub trait OptionalChildView: View {
    fn child(&self) -> Option<&dyn View>;
}

/// View with list of children
pub trait MultiChildView: View {
    fn children(&self) -> &[Box<dyn View>];
}
```

### Element Types

```rust
/// Element for leaf views
pub struct LeafElement {
    view_object: Box<dyn ViewObject>,
    render_id: Option<RenderObjectId>,
}

/// Element for single-child views  
pub struct SingleChildElement {
    view_object: Box<dyn ViewObject>,
    render_id: Option<RenderObjectId>,
    child: Option<ElementId>,
}

/// Element for multi-child views
pub struct MultiChildElement {
    view_object: Box<dyn ViewObject>,
    render_id: Option<RenderObjectId>,
    children: Vec<ElementId>,
    forgotten_children: HashSet<ElementId>,
}
```

### RenderObject Child Mixins

```rust
/// Render object with single optional child
pub trait SingleChildRender: RenderObject {
    fn child(&self) -> Option<RenderObjectId>;
    fn set_child(&mut self, child: Option<RenderObjectId>);
}

/// Parent data for linked-list children
#[derive(Default)]
pub struct ContainerParentData {
    pub previous_sibling: Option<RenderObjectId>,
    pub next_sibling: Option<RenderObjectId>,
}

/// Render object with linked-list children
pub trait ContainerRender: RenderObject {
    fn first_child(&self) -> Option<RenderObjectId>;
    fn last_child(&self) -> Option<RenderObjectId>;
    fn child_count(&self) -> usize;
    
    fn insert(&mut self, child: RenderObjectId, after: Option<RenderObjectId>);
    fn add(&mut self, child: RenderObjectId);
    fn remove(&mut self, child: RenderObjectId);
    fn move_child(&mut self, child: RenderObjectId, after: Option<RenderObjectId>);
    fn remove_all(&mut self);
    
    fn child_before(&self, child: RenderObjectId) -> Option<RenderObjectId>;
    fn child_after(&self, child: RenderObjectId) -> Option<RenderObjectId>;
}
```

### IndexedSlot

```rust
/// Slot for multi-child element reconciliation
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct IndexedSlot {
    pub index: usize,
    pub previous: Option<ElementId>,
}

impl IndexedSlot {
    pub fn new(index: usize, previous: Option<ElementId>) -> Self {
        Self { index, previous }
    }
}
```

### updateChildren in Rust

```rust
impl MultiChildElement {
    pub fn update_children(
        &mut self,
        new_widgets: &[Box<dyn View>],
        ctx: &mut UpdateContext,
    ) -> Vec<ElementId> {
        let old_children = std::mem::take(&mut self.children);
        let forgotten = std::mem::take(&mut self.forgotten_children);
        
        let mut new_children = vec![None; new_widgets.len()];
        let mut previous_child: Option<ElementId> = None;
        
        let mut new_top = 0usize;
        let mut old_top = 0usize;
        let mut new_bottom = new_widgets.len().saturating_sub(1);
        let mut old_bottom = old_children.len().saturating_sub(1);
        
        // Phase 1: Match from top
        while old_top <= old_bottom && new_top <= new_bottom {
            let old_child = old_children.get(old_top).copied();
            if old_child.map(|c| forgotten.contains(&c)).unwrap_or(true) {
                old_top += 1;
                continue;
            }
            
            let old_id = old_child.unwrap();
            if !ctx.can_update(old_id, &*new_widgets[new_top]) {
                break;
            }
            
            let slot = IndexedSlot::new(new_top, previous_child);
            let new_child = ctx.update_child(Some(old_id), Some(&*new_widgets[new_top]), slot);
            new_children[new_top] = new_child;
            previous_child = new_child;
            new_top += 1;
            old_top += 1;
        }
        
        // Phase 2: Match from bottom
        while old_top <= old_bottom && new_top <= new_bottom {
            let old_child = old_children.get(old_bottom).copied();
            if old_child.map(|c| forgotten.contains(&c)).unwrap_or(true) {
                if old_bottom == 0 { break; }
                old_bottom -= 1;
                continue;
            }
            
            let old_id = old_child.unwrap();
            if !ctx.can_update(old_id, &*new_widgets[new_bottom]) {
                break;
            }
            old_bottom = old_bottom.saturating_sub(1);
            new_bottom = new_bottom.saturating_sub(1);
        }
        
        // Phase 3: Build key map for middle
        let mut keyed_children: HashMap<ViewKey, ElementId> = HashMap::new();
        for i in old_top..=old_bottom {
            if let Some(old_id) = old_children.get(i).copied() {
                if forgotten.contains(&old_id) { continue; }
                if let Some(key) = ctx.element_key(old_id) {
                    keyed_children.insert(key, old_id);
                } else {
                    ctx.deactivate_child(old_id);
                }
            }
        }
        
        // Phase 4: Update middle with key matching
        while new_top <= new_bottom {
            let new_widget = &*new_widgets[new_top];
            let old_child = new_widget.key()
                .and_then(|k| keyed_children.remove(&k))
                .filter(|&id| ctx.can_update(id, new_widget));
            
            let slot = IndexedSlot::new(new_top, previous_child);
            let new_child = ctx.update_child(old_child, Some(new_widget), slot);
            new_children[new_top] = new_child;
            previous_child = new_child;
            new_top += 1;
        }
        
        // Phase 5: Update bottom (matched in phase 2)
        // ... similar logic
        
        // Deactivate unused keyed children
        for (_, old_id) in keyed_children {
            ctx.deactivate_child(old_id);
        }
        
        new_children.into_iter().flatten().collect()
    }
}
```

---

## Summary

| Type | Children | Widget | Element | RenderObject Mixin |
|------|----------|--------|---------|-------------------|
| Leaf | 0 | `LeafRenderObjectWidget` | `LeafRenderObjectElement` | (none) |
| Single | 0-1 | `SingleChildRenderObjectWidget` | `SingleChildRenderObjectElement` | `RenderObjectWithChildMixin` |
| Multi | 0-N | `MultiChildRenderObjectWidget` | `MultiChildRenderObjectElement` | `ContainerRenderObjectMixin` |
| Slotted | named | `SlottedMultiChildRenderObjectWidget` | `SlottedRenderObjectElement` | `SlottedContainerRenderObjectMixin` |

**Key Patterns:**
1. Widget stores child reference(s)
2. Element manages child element(s) via `updateChild`/`updateChildren`
3. RenderObject mixin manages render child(ren) as linked list
4. `IndexedSlot` enables O(1) linked-list operations
5. `updateChildren` algorithm is O(n) with key-based reuse

FLUI uses compile-time arity (`Leaf`, `Single`, `Optional`, `Variable`) for stronger type safety while implementing equivalent runtime algorithms.
