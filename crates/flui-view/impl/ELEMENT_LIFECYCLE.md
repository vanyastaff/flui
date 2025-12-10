# Flutter Element Lifecycle

This document analyzes the Element lifecycle from Flutter's `framework.dart`.

## Element Lifecycle States

**Source:** `framework.dart:3850-3900`

```dart
enum _ElementLifecycle {
  /// Created but not yet in tree
  initial,
  
  /// Incorporated into tree via mount() or activate()
  active,
  
  /// Removed from tree via deactivate()
  /// May become active again via GlobalKey
  inactive,
  
  /// Encountered unrecoverable error
  /// Subtree is inconsistent, cannot be reincorporated
  failed,
  
  /// Disposed and should not be interacted with
  /// Final state, not reversible
  defunct,
}
```

## Lifecycle Diagram

```
         ┌──────────────────────────────────────────────┐
         │                                              │
         v                                              │
     ┌────────┐                                         │
     │initial │                                         │
     └───┬────┘                                         │
         │ mount()                                      │
         v                                              │
     ┌────────┐  deactivate()   ┌──────────┐           │
     │ active │ ─────────────> │ inactive │           │
     └───┬────┘                 └────┬─────┘           │
         │                           │                  │
         │ error                     │ activate()      │
         v                           └─────────────────┘
     ┌────────┐                           │
     │ failed │                           │ unmount()
     └───┬────┘                           v
         │                           ┌─────────┐
         │ unmount()                 │ defunct │
         └─────────────────────────> └─────────┘
```

## Element Base Class

**Source:** `framework.dart:3900-4500`

### Core Fields

```dart
abstract class Element extends DiagnosticableTree implements BuildContext {
  Widget _widget;
  
  Element? _parent;
  Object? _slot;
  int _depth = 0;
  
  _ElementLifecycle _lifecycleState = _ElementLifecycle.initial;
  
  BuildOwner? _owner;
  
  Map<Type, InheritedElement>? _inheritedElements;
  Set<InheritedElement>? _dependencies;
  
  bool _dirty = true;
  bool _inDirtyList = false;
}
```

### Lifecycle Methods

#### mount()

**Called when:** Element is added to tree for first time.

```dart
@mustCallSuper
void mount(Element? parent, Object? newSlot) {
  assert(_lifecycleState == _ElementLifecycle.initial);
  assert(_parent == null);
  assert(_slot == null);
  assert(_depth == 0);
  
  _parent = parent;
  _slot = newSlot;
  _lifecycleState = _ElementLifecycle.active;
  _depth = parent != null ? parent.depth + 1 : 1;
  
  if (parent != null) {
    _owner = parent.owner;
  }
  
  // Register GlobalKey if present
  final Key? key = widget.key;
  if (key is GlobalKey) {
    owner!._registerGlobalKey(key, this);
  }
  
  _updateInheritance();
}
```

#### update()

**Called when:** Widget is replaced with compatible widget (same type/key).

```dart
@mustCallSuper
void update(covariant Widget newWidget) {
  assert(_lifecycleState == _ElementLifecycle.active);
  assert(Widget.canUpdate(widget, newWidget));
  
  _widget = newWidget;
}
```

#### deactivate()

**Called when:** Element is removed from tree (may be reactivated).

```dart
@mustCallSuper
void deactivate() {
  assert(_lifecycleState == _ElementLifecycle.active);
  
  // Clear dependencies
  if (_dependencies != null && _dependencies!.isNotEmpty) {
    for (final dependency in _dependencies!) {
      dependency._dependents.remove(this);
    }
  }
  _inheritedElements = null;
  
  _lifecycleState = _ElementLifecycle.inactive;
}
```

#### activate()

**Called when:** Inactive element is reinserted (via GlobalKey).

```dart
@mustCallSuper
void activate() {
  assert(_lifecycleState == _ElementLifecycle.inactive);
  
  _lifecycleState = _ElementLifecycle.active;
  
  // Rebuild inherited elements cache
  _updateInheritance();
  
  // Mark for rebuild
  markNeedsBuild();
}
```

#### unmount()

**Called when:** Element is permanently removed from tree.

```dart
@mustCallSuper
void unmount() {
  assert(_lifecycleState == _ElementLifecycle.inactive || 
         _lifecycleState == _ElementLifecycle.failed);
  
  // Unregister GlobalKey
  final Key? key = widget.key;
  if (key is GlobalKey) {
    owner!._unregisterGlobalKey(key, this);
  }
  
  _widget = null; // Breaking reference for GC
  _lifecycleState = _ElementLifecycle.defunct;
}
```

## Build Process

### markNeedsBuild()

```dart
void markNeedsBuild() {
  assert(_lifecycleState != _ElementLifecycle.defunct);
  
  if (_lifecycleState != _ElementLifecycle.active) {
    return; // Inactive elements don't rebuild
  }
  
  if (_dirty) return; // Already marked
  
  _dirty = true;
  owner!.scheduleBuildFor(this);
}
```

### rebuild()

```dart
void rebuild({bool force = false}) {
  assert(_lifecycleState != _ElementLifecycle.initial);
  
  if (_lifecycleState != _ElementLifecycle.active || (!_dirty && !force)) {
    return;
  }
  
  performRebuild();
  assert(!_dirty);
}

@protected
void performRebuild();  // Abstract - implemented by subclasses
```

### ComponentElement.performRebuild()

```dart
@override
void performRebuild() {
  Widget? built;
  try {
    built = build();
  } catch (e, stack) {
    built = ErrorWidget.builder(...);
  }
  
  try {
    _child = updateChild(_child, built, slot);
  } catch (e, stack) {
    built = ErrorWidget.builder(...);
    _child = updateChild(null, built, slot);
  }
  
  _dirty = false;
}
```

## Child Management

### updateChild()

**Core reconciliation algorithm:**

```dart
Element? updateChild(Element? child, Widget? newWidget, Object? newSlot) {
  // Case 1: Remove child
  if (newWidget == null) {
    if (child != null) deactivateChild(child);
    return null;
  }
  
  // Case 2: Update or create
  if (child != null) {
    // Case 2a: Same widget instance
    if (child.widget == newWidget) {
      if (child.slot != newSlot) {
        updateSlotForChild(child, newSlot);
      }
      return child;
    }
    
    // Case 2b: Compatible widget (same type and key)
    if (Widget.canUpdate(child.widget, newWidget)) {
      if (child.slot != newSlot) {
        updateSlotForChild(child, newSlot);
      }
      child.update(newWidget);
      return child;
    }
    
    // Case 2c: Incompatible widget
    deactivateChild(child);
  }
  
  // Case 3: Inflate new widget
  return inflateWidget(newWidget, newSlot);
}
```

### inflateWidget()

```dart
Element inflateWidget(Widget newWidget, Object? newSlot) {
  // Check for GlobalKey reparenting
  final Key? key = newWidget.key;
  if (key is GlobalKey) {
    final Element? newChild = _retakeInactiveElement(key, newWidget);
    if (newChild != null) {
      // Reparent existing element
      newChild._activateWithParent(this, newSlot);
      final Element? updatedChild = updateChild(newChild, newWidget, newSlot);
      return updatedChild!;
    }
  }
  
  // Create new element
  final Element newChild = newWidget.createElement();
  newChild.mount(this, newSlot);
  return newChild;
}
```

## InheritedWidget Dependencies

### dependOnInheritedWidgetOfExactType<T>()

```dart
@override
T? dependOnInheritedWidgetOfExactType<T extends InheritedWidget>({Object? aspect}) {
  final InheritedElement? ancestor = _inheritedElements?[T];
  
  if (ancestor != null) {
    // Register dependency
    _dependencies ??= HashSet<InheritedElement>();
    _dependencies!.add(ancestor);
    ancestor._dependents.add(this);
    return ancestor.widget as T;
  }
  
  return null;
}
```

### _updateInheritance()

```dart
void _updateInheritance() {
  _inheritedElements = _parent?._inheritedElements;
}
```

### InheritedElement._updateInheritance()

```dart
@override
void _updateInheritance() {
  final Map<Type, InheritedElement>? incomingWidgets = _parent?._inheritedElements;
  if (incomingWidgets != null) {
    _inheritedElements = HashMap<Type, InheritedElement>.of(incomingWidgets);
  } else {
    _inheritedElements = HashMap<Type, InheritedElement>();
  }
  _inheritedElements![widget.runtimeType] = this;
}
```

## FLUI Element Lifecycle

### Proposed Structure

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementLifecycle {
    /// Created but not mounted
    Initial,
    /// Mounted in tree
    Active,
    /// Removed, may be reactivated
    Inactive,
    /// Permanently removed
    Defunct,
}

pub struct Element {
    view_object: Box<dyn ViewObject>,
    
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    slot: Option<Slot>,
    depth: u32,
    
    lifecycle: ElementLifecycle,
    
    // Build state
    dirty: bool,
    
    // Inherited widgets cache
    inherited_cache: Option<InheritedCache>,
    dependencies: HashSet<ElementId>,
}
```

### Lifecycle Methods

```rust
impl Element {
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        debug_assert!(self.lifecycle == ElementLifecycle::Initial);
        
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        
        // Register global key if present
        if let Some(key) = self.view_object.key() {
            if let ViewKey::Global(id) = key {
                GlobalKeyRegistry::register(id, self.id);
            }
        }
        
        self.update_inheritance();
    }
    
    pub fn update(&mut self, new_view: Box<dyn ViewObject>) {
        debug_assert!(self.lifecycle == ElementLifecycle::Active);
        debug_assert!(Self::can_update(&*self.view_object, &*new_view));
        
        self.view_object = new_view;
    }
    
    pub fn deactivate(&mut self) {
        debug_assert!(self.lifecycle == ElementLifecycle::Active);
        
        // Clear dependencies
        self.dependencies.clear();
        self.inherited_cache = None;
        
        self.lifecycle = ElementLifecycle::Inactive;
    }
    
    pub fn activate(&mut self) {
        debug_assert!(self.lifecycle == ElementLifecycle::Inactive);
        
        self.lifecycle = ElementLifecycle::Active;
        self.update_inheritance();
        self.mark_needs_build();
    }
    
    pub fn unmount(&mut self) {
        debug_assert!(matches!(
            self.lifecycle, 
            ElementLifecycle::Inactive | ElementLifecycle::Failed
        ));
        
        // Unregister global key
        if let Some(ViewKey::Global(id)) = self.view_object.key() {
            GlobalKeyRegistry::unregister(id);
        }
        
        self.lifecycle = ElementLifecycle::Defunct;
    }
}
```

### Build Methods

```rust
impl Element {
    pub fn mark_needs_build(&mut self) {
        if self.lifecycle != ElementLifecycle::Active {
            return;
        }
        
        if self.dirty {
            return;
        }
        
        self.dirty = true;
        BuildOwner::schedule_build(self.id);
    }
    
    pub fn rebuild(&mut self, tree: &mut ElementTree, force: bool) {
        if self.lifecycle != ElementLifecycle::Active {
            return;
        }
        
        if !self.dirty && !force {
            return;
        }
        
        self.perform_rebuild(tree);
        self.dirty = false;
    }
    
    fn perform_rebuild(&mut self, tree: &mut ElementTree) {
        // Implemented by ViewObject
        self.view_object.perform_rebuild(self.id, tree);
    }
}
```

## Summary: Flutter → FLUI Lifecycle

| Flutter | FLUI | Description |
|---------|------|-------------|
| `_ElementLifecycle.initial` | `ElementLifecycle::Initial` | Pre-mount |
| `_ElementLifecycle.active` | `ElementLifecycle::Active` | Mounted |
| `_ElementLifecycle.inactive` | `ElementLifecycle::Inactive` | Deactivated |
| `_ElementLifecycle.failed` | (Handled differently) | Error state |
| `_ElementLifecycle.defunct` | `ElementLifecycle::Defunct` | Unmounted |
| `mount()` | `mount()` | Add to tree |
| `update()` | `update()` | Replace widget |
| `deactivate()` | `deactivate()` | Remove from tree |
| `activate()` | `activate()` | Reinsert |
| `unmount()` | `unmount()` | Permanent removal |
| `markNeedsBuild()` | `mark_needs_build()` | Schedule rebuild |
| `rebuild()` | `rebuild()` | Execute rebuild |
| `updateChild()` | `update_child()` | Reconciliation |
| `inflateWidget()` | `inflate_view()` | Create element |
| `_inheritedElements` | `inherited_cache` | Ancestor cache |
| `_dependencies` | `dependencies` | Dependency tracking |
