# Flutter BuildContext

This document analyzes the BuildContext interface from Flutter's `framework.dart`.

## BuildContext Definition

**Source:** `framework.dart:2650-3100`

```dart
/// A handle to the location of a widget in the widget tree.
abstract class BuildContext {
  /// The current configuration
  Widget get widget;

  /// The BuildOwner for this context
  BuildOwner? get owner;

  /// Whether this context is still mounted
  bool get mounted;

  /// Whether the widget is currently being built
  bool get debugDoingBuild;

  /// Obtain the nearest RenderObject ancestor
  RenderObject? findRenderObject();

  /// Obtain the Size of the nearest RenderBox ancestor
  Size? findAncestorRenderObjectOfType<T extends RenderObject>()?.size;

  /// Register dependency on InheritedWidget
  T? dependOnInheritedWidgetOfExactType<T extends InheritedWidget>({Object? aspect});

  /// Get InheritedElement without creating dependency
  InheritedElement? getElementForInheritedWidgetOfExactType<T extends InheritedWidget>();

  /// Find ancestor widget of type
  T? findAncestorWidgetOfExactType<T extends Widget>();

  /// Find ancestor state of type
  T? findAncestorStateOfType<T extends State>();

  /// Find root ancestor state of type
  T? findRootAncestorStateOfType<T extends State>();

  /// Visit ancestor elements
  void visitAncestorElements(ConditionalElementVisitor visitor);

  /// Visit child elements
  void visitChildElements(ElementVisitor visitor);

  /// Dispatch notification to ancestors
  void dispatchNotification(Notification notification);

  /// Describe for error messages
  DiagnosticsNode describeElement(String name, {DiagnosticsTreeStyle style});
  DiagnosticsNode describeWidget(String name, {DiagnosticsTreeStyle style});
  List<DiagnosticsNode> describeMissingAncestor({required Type expectedAncestorType});
  DiagnosticsNode describeOwnershipChain(String name);
}
```

## Key Methods

### dependOnInheritedWidgetOfExactType<T>()

**Purpose:** Creates a dependency on an InheritedWidget. When that widget changes, this context's widget will rebuild.

```dart
T? dependOnInheritedWidgetOfExactType<T extends InheritedWidget>({Object? aspect}) {
  final InheritedElement? ancestor = _inheritedElements?[T];
  if (ancestor != null) {
    _dependencies ??= HashSet<InheritedElement>();
    _dependencies!.add(ancestor);
    ancestor._dependents.add(this);
    
    if (aspect != null) {
      ancestor._aspects?.add(aspect);
    }
    
    return ancestor.widget as T;
  }
  return null;
}
```

**Usage:**
```dart
Widget build(BuildContext context) {
  final theme = Theme.of(context); // calls dependOnInheritedWidgetOfExactType
  return Container(color: theme.backgroundColor);
}
```

### getElementForInheritedWidgetOfExactType<T>()

**Purpose:** Gets InheritedElement WITHOUT creating dependency. Use when you need the value but don't want to rebuild when it changes.

```dart
InheritedElement? getElementForInheritedWidgetOfExactType<T extends InheritedWidget>() {
  return _inheritedElements?[T];
}
```

**Usage:**
```dart
// Get value without dependency (won't rebuild when MediaQuery changes)
final size = context.getElementForInheritedWidgetOfExactType<MediaQuery>()
    ?.widget.data.size;
```

### findAncestorWidgetOfExactType<T>()

**Purpose:** Finds nearest ancestor widget of exact type. Does NOT create dependency.

```dart
T? findAncestorWidgetOfExactType<T extends Widget>() {
  Element? ancestor = _parent;
  while (ancestor != null) {
    if (ancestor.widget is T) {
      return ancestor.widget as T;
    }
    ancestor = ancestor._parent;
  }
  return null;
}
```

### findAncestorStateOfType<T>()

**Purpose:** Finds nearest ancestor State of type. Used for imperative access to state.

```dart
T? findAncestorStateOfType<T extends State>() {
  Element? ancestor = _parent;
  while (ancestor != null) {
    if (ancestor is StatefulElement && ancestor.state is T) {
      return ancestor.state as T;
    }
    ancestor = ancestor._parent;
  }
  return null;
}
```

**Usage:**
```dart
// Access Scaffold's state to show snackbar
ScaffoldState? scaffold = context.findAncestorStateOfType<ScaffoldState>();
scaffold?.showSnackBar(SnackBar(content: Text('Hello')));
```

### findRootAncestorStateOfType<T>()

**Purpose:** Finds the ROOT (furthest) ancestor state of type, not nearest.

```dart
T? findRootAncestorStateOfType<T extends State>() {
  T? result;
  Element? ancestor = _parent;
  while (ancestor != null) {
    if (ancestor is StatefulElement && ancestor.state is T) {
      result = ancestor.state as T;
    }
    ancestor = ancestor._parent;
  }
  return result;
}
```

### visitAncestorElements()

**Purpose:** Walk up the tree, calling visitor for each ancestor.

```dart
void visitAncestorElements(ConditionalElementVisitor visitor) {
  Element? ancestor = _parent;
  while (ancestor != null && visitor(ancestor)) {
    ancestor = ancestor._parent;
  }
}
```

**Return false from visitor to stop walking.**

### visitChildElements()

**Purpose:** Visit all direct children of this element.

```dart
void visitChildElements(ElementVisitor visitor);  // Abstract - implemented by Element subclasses
```

### dispatchNotification()

**Purpose:** Send notification up the tree to NotificationListener ancestors.

```dart
void dispatchNotification(Notification notification) {
  Element? ancestor = _parent;
  while (ancestor != null) {
    if (ancestor is _NotificationElement) {
      ancestor._notificationListener(notification);
    }
    ancestor = ancestor._parent;
  }
}
```

## FLUI BuildContext Design

### Trait Definition

```rust
/// Handle to widget location in tree
pub trait BuildContext {
    /// Get current view
    fn view(&self) -> &dyn ViewObject;
    
    /// Get element ID
    fn element_id(&self) -> ElementId;
    
    /// Whether element is still mounted
    fn mounted(&self) -> bool;
    
    /// Get nearest RenderObject
    fn find_render_object(&self) -> Option<RenderId>;
    
    /// Depend on inherited widget (creates rebuild dependency)
    fn depend_on<T: InheritedView + 'static>(&self) -> Option<&T>;
    
    /// Get inherited without dependency
    fn get_inherited<T: InheritedView + 'static>(&self) -> Option<&T>;
    
    /// Find ancestor view of type
    fn find_ancestor_view<T: View + 'static>(&self) -> Option<&T>;
    
    /// Find ancestor state of type
    fn find_ancestor_state<T: ViewState + 'static>(&self) -> Option<&T>;
    
    /// Find root ancestor state
    fn find_root_ancestor_state<T: ViewState + 'static>(&self) -> Option<&T>;
    
    /// Visit ancestors (return false to stop)
    fn visit_ancestors(&self, visitor: impl FnMut(ElementId) -> bool);
    
    /// Visit children
    fn visit_children(&self, visitor: impl FnMut(ElementId));
    
    /// Dispatch notification up tree
    fn dispatch_notification<N: Notification>(&self, notification: N);
}
```

### Concrete Implementation

```rust
pub struct BuildContextImpl<'a> {
    element_id: ElementId,
    tree: &'a ElementTree,
    inherited_cache: &'a InheritedCache,
}

impl<'a> BuildContext for BuildContextImpl<'a> {
    fn element_id(&self) -> ElementId {
        self.element_id
    }
    
    fn mounted(&self) -> bool {
        self.tree.get(self.element_id)
            .map(|e| e.lifecycle == ElementLifecycle::Active)
            .unwrap_or(false)
    }
    
    fn depend_on<T: InheritedView + 'static>(&self) -> Option<&T> {
        let type_id = TypeId::of::<T>();
        
        if let Some(inherited_id) = self.inherited_cache.get(&type_id) {
            // Register dependency
            self.tree.add_dependency(self.element_id, inherited_id);
            
            // Get widget
            self.tree.get(inherited_id)
                .and_then(|e| e.view_object.as_any().downcast_ref::<T>())
        } else {
            None
        }
    }
    
    fn get_inherited<T: InheritedView + 'static>(&self) -> Option<&T> {
        let type_id = TypeId::of::<T>();
        
        // NO dependency registration
        self.inherited_cache.get(&type_id)
            .and_then(|id| self.tree.get(id))
            .and_then(|e| e.view_object.as_any().downcast_ref::<T>())
    }
    
    fn find_ancestor_view<T: View + 'static>(&self) -> Option<&T> {
        let mut current = self.tree.get(self.element_id)?.parent;
        
        while let Some(parent_id) = current {
            if let Some(element) = self.tree.get(parent_id) {
                if let Some(view) = element.view_object.as_any().downcast_ref::<T>() {
                    return Some(view);
                }
                current = element.parent;
            } else {
                break;
            }
        }
        None
    }
    
    fn visit_ancestors(&self, mut visitor: impl FnMut(ElementId) -> bool) {
        let mut current = self.tree.get(self.element_id).and_then(|e| e.parent);
        
        while let Some(ancestor_id) = current {
            if !visitor(ancestor_id) {
                break;
            }
            current = self.tree.get(ancestor_id).and_then(|e| e.parent);
        }
    }
    
    fn visit_children(&self, mut visitor: impl FnMut(ElementId)) {
        if let Some(element) = self.tree.get(self.element_id) {
            for &child_id in &element.children {
                visitor(child_id);
            }
        }
    }
}
```

### Usage Example

```rust
impl StatelessView for ThemeConsumer {
    fn build(&self, ctx: &impl BuildContext) -> impl IntoElement {
        // Creates dependency - rebuilds when Theme changes
        let theme = ctx.depend_on::<ThemeProvider>()
            .expect("ThemeProvider not found in ancestors");
        
        Container::new()
            .color(theme.background_color)
            .child(Text::new(&self.text).color(theme.text_color))
    }
}

impl StatefulView for ScaffoldController {
    type State = ScaffoldState;
    
    fn build(&self, ctx: &impl BuildContext, state: &Self::State) -> impl IntoElement {
        // No dependency - just imperative access
        let parent_scaffold = ctx.find_ancestor_state::<ScaffoldState>();
        
        // Use parent if available
        if let Some(scaffold) = parent_scaffold {
            scaffold.show_snackbar(self.message.clone());
        }
        
        Scaffold::new()
            .body(self.child.clone())
    }
}
```

## Inherited Widget Cache

### Flutter Implementation

```dart
// Each element has inherited cache
Map<Type, InheritedElement>? _inheritedElements;

// Updated when element is mounted/activated
void _updateInheritance() {
  _inheritedElements = _parent?._inheritedElements;
}

// InheritedElement adds itself to cache
@override
void _updateInheritance() {
  final incomingWidgets = _parent?._inheritedElements;
  if (incomingWidgets != null) {
    _inheritedElements = HashMap.of(incomingWidgets);
  } else {
    _inheritedElements = HashMap();
  }
  _inheritedElements![widget.runtimeType] = this;
}
```

### FLUI Implementation

```rust
/// Cache of inherited widgets for fast lookup
pub struct InheritedCache {
    /// TypeId -> ElementId of nearest InheritedWidget of that type
    cache: HashMap<TypeId, ElementId>,
}

impl InheritedCache {
    /// Create cache from parent
    pub fn from_parent(parent: Option<&InheritedCache>) -> Self {
        match parent {
            Some(p) => Self { cache: p.cache.clone() },
            None => Self { cache: HashMap::new() },
        }
    }
    
    /// Register inherited widget (called by InheritedElement)
    pub fn register<T: 'static>(&mut self, element_id: ElementId) {
        self.cache.insert(TypeId::of::<T>(), element_id);
    }
    
    /// Get inherited widget element
    pub fn get(&self, type_id: &TypeId) -> Option<ElementId> {
        self.cache.get(type_id).copied()
    }
}
```

## Summary: Flutter → FLUI BuildContext

| Flutter Method | FLUI Method | Creates Dependency |
|----------------|-------------|-------------------|
| `dependOnInheritedWidgetOfExactType<T>()` | `depend_on::<T>()` | Yes |
| `getElementForInheritedWidgetOfExactType<T>()` | `get_inherited::<T>()` | No |
| `findAncestorWidgetOfExactType<T>()` | `find_ancestor_view::<T>()` | No |
| `findAncestorStateOfType<T>()` | `find_ancestor_state::<T>()` | No |
| `findRootAncestorStateOfType<T>()` | `find_root_ancestor_state::<T>()` | No |
| `visitAncestorElements()` | `visit_ancestors()` | No |
| `visitChildElements()` | `visit_children()` | No |
| `dispatchNotification()` | `dispatch_notification()` | No |
| `findRenderObject()` | `find_render_object()` | No |
| `widget` | `view()` | No |
| `mounted` | `mounted()` | No |
