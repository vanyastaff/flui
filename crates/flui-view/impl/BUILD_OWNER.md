# Flutter BuildOwner

This document analyzes the BuildOwner class from Flutter's `framework.dart`.

## BuildOwner Overview

**Source:** `framework.dart:2200-2550`

```dart
/// Manager class for the widgets framework.
/// 
/// Tracks which widgets need rebuilding, handles global keys,
/// and drives the build process.
class BuildOwner {
  BuildOwner({
    this.onBuildScheduled,
    FocusManager? focusManager,
  }) : focusManager = focusManager ?? FocusManager();

  /// Called each time a widget is marked dirty
  VoidCallback? onBuildScheduled;

  /// Focus manager for this owner
  FocusManager focusManager;

  /// Dirty elements waiting for rebuild
  final List<Element> _dirtyElements = <Element>[];

  /// Whether we're currently building
  bool _scheduledFlushDirtyElements = false;

  /// Inactive elements waiting for disposal
  final _InactiveElements _inactiveElements = _InactiveElements();

  /// GlobalKey registry
  final Map<GlobalKey, Element> _globalKeyRegistry = <GlobalKey, Element>{};

  /// Whether we're in build phase
  bool? _dirtyElementsNeedsResorting;

  /// Debug: currently building element
  Element? _debugCurrentBuildTarget;
}
```

## Key Methods

### scheduleBuildFor()

**Called when:** `Element.markNeedsBuild()` is called.

```dart
void scheduleBuildFor(Element element) {
  assert(element.owner == this);
  assert(() {
    if (debugPrintScheduleBuildForStacks) {
      debugPrintStack(label: 'scheduleBuildFor() called for $element');
    }
    if (!element.dirty) {
      throw FlutterError.fromParts([...]);
    }
    return true;
  }());

  if (element._inDirtyList) {
    _dirtyElementsNeedsResorting = true;
    return;
  }

  if (!_scheduledFlushDirtyElements && onBuildScheduled != null) {
    _scheduledFlushDirtyElements = true;
    onBuildScheduled!();
  }

  _dirtyElements.add(element);
  element._inDirtyList = true;
}
```

### buildScope()

**Main build phase driver.**

```dart
void buildScope(Element context, [VoidCallback? callback]) {
  if (callback == null && _dirtyElements.isEmpty) {
    return;
  }

  Timeline.startSync('Build', arguments: timelineArgumentsIndicatingLandmarkEvent);
  try {
    _scheduledFlushDirtyElements = true;

    if (callback != null) {
      _dirtyElementsNeedsResorting = false;
      callback();
    }

    // Sort by depth - rebuild parents before children
    _dirtyElements.sort(Element._sort);
    _dirtyElementsNeedsResorting = false;

    int dirtyCount = _dirtyElements.length;
    int index = 0;

    while (index < dirtyCount) {
      final element = _dirtyElements[index];
      
      try {
        element.rebuild();
      } catch (e, stack) {
        _reportException(...);
      }

      index += 1;

      // Check if more elements were added during rebuild
      if (dirtyCount < _dirtyElements.length || _dirtyElementsNeedsResorting!) {
        _dirtyElements.sort(Element._sort);
        _dirtyElementsNeedsResorting = false;
        dirtyCount = _dirtyElements.length;
        
        // Skip already-rebuilt elements
        while (index > 0 && _dirtyElements[index - 1].dirty) {
          index -= 1;
        }
      }
    }

  } finally {
    for (final element in _dirtyElements) {
      element._inDirtyList = false;
    }
    _dirtyElements.clear();
    _scheduledFlushDirtyElements = false;
    Timeline.finishSync();
  }
}
```

### finalizeTree()

**Called after build phase to clean up inactive elements.**

```dart
void finalizeTree() {
  Timeline.startSync('Finalize tree');
  try {
    lockState(() {
      _inactiveElements._unmountAll();
    });
  } catch (e, stack) {
    _reportException(...);
  } finally {
    Timeline.finishSync();
  }
}
```

### lockState()

**Prevents state modifications during certain phases.**

```dart
void lockState(VoidCallback callback) {
  assert(_debugStateLockLevel >= 0);
  _debugStateLockLevel += 1;
  try {
    callback();
  } finally {
    _debugStateLockLevel -= 1;
  }
}
```

### GlobalKey Registration

```dart
void _registerGlobalKey(GlobalKey key, Element element) {
  assert(() {
    if (_globalKeyRegistry.containsKey(key)) {
      // Duplicate GlobalKey error
      throw FlutterError.fromParts([...]);
    }
    return true;
  }());
  _globalKeyRegistry[key] = element;
}

void _unregisterGlobalKey(GlobalKey key, Element element) {
  assert(() {
    if (_globalKeyRegistry[key] != element) {
      // Mismatched unregister
      throw FlutterError.fromParts([...]);
    }
    return true;
  }());
  _globalKeyRegistry.remove(key);
}
```

### Element Sorting

**Elements are sorted by depth for correct rebuild order.**

```dart
// In Element class
static int _sort(Element a, Element b) {
  if (a.depth < b.depth) return -1;
  if (b.depth < a.depth) return 1;
  // Same depth - keep stable order
  if (b.dirty && !a.dirty) return -1;
  if (a.dirty && !b.dirty) return 1;
  return 0;
}
```

## _InactiveElements

**Manages elements removed from tree.**

```dart
class _InactiveElements {
  bool _locked = false;
  final Set<Element> _elements = HashSet<Element>();

  static void _unmount(Element element) {
    element.visitChildren((child) {
      _unmount(child);
    });
    element.unmount();
  }

  void _unmountAll() {
    _locked = true;
    final elements = _elements.toList()..sort(Element._sort);
    _elements.clear();
    try {
      // Unmount in reverse depth order (children first)
      elements.reversed.forEach(_unmount);
    } finally {
      _locked = false;
    }
  }

  static void _deactivateRecursively(Element element) {
    element.deactivate();
    element.visitChildren(_deactivateRecursively);
  }

  void add(Element element) {
    assert(!_locked);
    if (element._lifecycleState == _ElementLifecycle.active) {
      _deactivateRecursively(element);
    }
    _elements.add(element);
  }

  void remove(Element element) {
    assert(!_locked);
    _elements.remove(element);
  }
}
```

## FLUI BuildOwner Design

### Structure

```rust
/// Manager for widget build process
pub struct BuildOwner {
    /// Dirty elements awaiting rebuild
    dirty_elements: Vec<ElementId>,
    
    /// Whether build is scheduled
    build_scheduled: bool,
    
    /// Inactive elements awaiting disposal
    inactive_elements: HashSet<ElementId>,
    
    /// GlobalKey registry
    global_keys: HashMap<GlobalKeyId, ElementId>,
    
    /// Whether dirty list needs resorting
    needs_resort: bool,
    
    /// Callback when build scheduled
    on_build_scheduled: Option<Box<dyn Fn()>>,
}

impl BuildOwner {
    pub fn new() -> Self {
        Self {
            dirty_elements: Vec::new(),
            build_scheduled: false,
            inactive_elements: HashSet::new(),
            global_keys: HashMap::new(),
            needs_resort: false,
            on_build_scheduled: None,
        }
    }
}
```

### schedule_build_for()

```rust
impl BuildOwner {
    pub fn schedule_build_for(&mut self, element_id: ElementId, tree: &ElementTree) {
        let element = tree.get(element_id).expect("Element must exist");
        
        debug_assert!(element.dirty, "Element must be marked dirty first");
        
        // Already in list - just mark for resort
        if element.in_dirty_list {
            self.needs_resort = true;
            return;
        }
        
        // Schedule callback if first dirty element
        if !self.build_scheduled {
            self.build_scheduled = true;
            if let Some(callback) = &self.on_build_scheduled {
                callback();
            }
        }
        
        self.dirty_elements.push(element_id);
        tree.get_mut(element_id).unwrap().in_dirty_list = true;
    }
}
```

### build_scope()

```rust
impl BuildOwner {
    pub fn build_scope(&mut self, tree: &mut ElementTree) {
        if self.dirty_elements.is_empty() {
            return;
        }
        
        tracing::debug!("Build scope starting with {} dirty elements", 
            self.dirty_elements.len());
        
        self.build_scheduled = true;
        
        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|id| tree.depth(*id));
        self.needs_resort = false;
        
        let mut index = 0;
        
        while index < self.dirty_elements.len() {
            let element_id = self.dirty_elements[index];
            
            if let Err(e) = tree.rebuild(element_id) {
                tracing::error!("Rebuild failed for {:?}: {}", element_id, e);
            }
            
            index += 1;
            
            // Check if more elements were added
            if self.needs_resort {
                self.dirty_elements.sort_by_key(|id| tree.depth(*id));
                self.needs_resort = false;
                
                // Find new position (skip already-rebuilt)
                while index > 0 {
                    let prev_id = self.dirty_elements[index - 1];
                    if tree.get(prev_id).map(|e| e.dirty).unwrap_or(false) {
                        index -= 1;
                    } else {
                        break;
                    }
                }
            }
        }
        
        // Clear dirty list
        for &id in &self.dirty_elements {
            if let Some(element) = tree.get_mut(id) {
                element.in_dirty_list = false;
            }
        }
        self.dirty_elements.clear();
        self.build_scheduled = false;
    }
}
```

### finalize_tree()

```rust
impl BuildOwner {
    pub fn finalize_tree(&mut self, tree: &mut ElementTree) {
        // Sort inactive elements by depth (deepest first for unmount)
        let mut elements: Vec<_> = self.inactive_elements.iter().copied().collect();
        elements.sort_by_key(|id| std::cmp::Reverse(tree.depth(*id)));
        
        self.inactive_elements.clear();
        
        for element_id in elements {
            Self::unmount_recursively(tree, element_id);
        }
    }
    
    fn unmount_recursively(tree: &mut ElementTree, element_id: ElementId) {
        // Unmount children first
        if let Some(element) = tree.get(element_id) {
            let children: Vec<_> = element.children.clone();
            for child_id in children {
                Self::unmount_recursively(tree, child_id);
            }
        }
        
        // Then unmount self
        if let Some(element) = tree.get_mut(element_id) {
            element.unmount();
        }
    }
}
```

### GlobalKey Management

```rust
impl BuildOwner {
    pub fn register_global_key(&mut self, key: GlobalKeyId, element: ElementId) {
        if cfg!(debug_assertions) {
            if let Some(&existing) = self.global_keys.get(&key) {
                panic!("Duplicate GlobalKey: {:?} already registered to {:?}", 
                    key, existing);
            }
        }
        self.global_keys.insert(key, element);
    }
    
    pub fn unregister_global_key(&mut self, key: GlobalKeyId, element: ElementId) {
        if cfg!(debug_assertions) {
            if self.global_keys.get(&key) != Some(&element) {
                panic!("GlobalKey mismatch during unregister: {:?}", key);
            }
        }
        self.global_keys.remove(&key);
    }
    
    pub fn lookup_global_key(&self, key: GlobalKeyId) -> Option<ElementId> {
        self.global_keys.get(&key).copied()
    }
}
```

### Inactive Element Management

```rust
impl BuildOwner {
    pub fn add_inactive(&mut self, element_id: ElementId, tree: &mut ElementTree) {
        // Deactivate recursively
        Self::deactivate_recursively(tree, element_id);
        
        self.inactive_elements.insert(element_id);
    }
    
    fn deactivate_recursively(tree: &mut ElementTree, element_id: ElementId) {
        if let Some(element) = tree.get_mut(element_id) {
            element.deactivate();
            
            let children: Vec<_> = element.children.clone();
            for child_id in children {
                Self::deactivate_recursively(tree, child_id);
            }
        }
    }
    
    pub fn remove_inactive(&mut self, element_id: ElementId) {
        self.inactive_elements.remove(&element_id);
    }
}
```

## Build Phase Timing

### Flutter Frame Pipeline

```
Frame Start
    ↓
1. Animation Phase (Ticker callbacks)
    ↓
2. Build Phase (buildScope)
    - Process dirty elements
    - Sort by depth
    - Rebuild parent→child order
    ↓
3. Layout Phase
    ↓
4. Paint Phase
    ↓
5. Compositing
    ↓
6. Finalize Phase (finalizeTree)
    - Unmount inactive elements
    ↓
Frame End
```

### FLUI Frame Pipeline

```rust
pub struct FramePipeline {
    build_owner: BuildOwner,
    pipeline_owner: PipelineOwner,
}

impl FramePipeline {
    pub fn begin_frame(&mut self, tree: &mut ElementTree, render_tree: &mut RenderTree) {
        // 1. Animation phase (signal updates, timers)
        self.tick_animations();
        
        // 2. Build phase
        self.build_owner.build_scope(tree);
        
        // 3. Layout phase
        self.pipeline_owner.flush_layout(render_tree);
        
        // 4. Paint phase  
        self.pipeline_owner.flush_paint(render_tree);
        
        // 5. Compositing (in rendering engine)
        
        // 6. Finalize phase
        self.build_owner.finalize_tree(tree);
    }
}
```

## Summary: Flutter → FLUI BuildOwner

| Flutter | FLUI | Description |
|---------|------|-------------|
| `BuildOwner` | `BuildOwner` | Build manager |
| `_dirtyElements` | `dirty_elements` | Pending rebuilds |
| `_inactiveElements` | `inactive_elements` | Pending disposal |
| `_globalKeyRegistry` | `global_keys` | GlobalKey lookup |
| `scheduleBuildFor()` | `schedule_build_for()` | Queue rebuild |
| `buildScope()` | `build_scope()` | Execute builds |
| `finalizeTree()` | `finalize_tree()` | Cleanup |
| `lockState()` | (Rust's borrow checker) | State protection |
| `onBuildScheduled` | `on_build_scheduled` | Frame request |
| `_registerGlobalKey()` | `register_global_key()` | Add key |
| `_unregisterGlobalKey()` | `unregister_global_key()` | Remove key |
