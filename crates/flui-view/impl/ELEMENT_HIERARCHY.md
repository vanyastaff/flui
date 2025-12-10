# Flutter Element Hierarchy

This document analyzes the complete Element class hierarchy in Flutter.

## Source Files
- `packages/flutter/lib/src/widgets/framework.dart`

## Complete Hierarchy

```
Element (abstract)
│
├── ComponentElement (abstract)     ─── Views that COMPOSE other widgets
│   ├── StatelessElement           ─── For StatelessWidget
│   ├── StatefulElement            ─── For StatefulWidget  
│   └── ProxyElement (abstract)    ─── Pass-through widgets
│       ├── ParentDataElement<T>   ─── For ParentDataWidget (Positioned, Flexible)
│       └── InheritedElement       ─── For InheritedWidget (Theme, MediaQuery)
│
├── RenderObjectElement (abstract)  ─── Views that CREATE RenderObjects
│   ├── LeafRenderObjectElement    ─── 0 children (Text, Image, RawImage)
│   ├── SingleChildRenderObjectElement ─── 0-1 child (Padding, Opacity)
│   ├── MultiChildRenderObjectElement  ─── N children (Row, Column, Stack)
│   ├── RootRenderObjectElement    ─── Root of render tree (deprecated)
│   └── RenderTreeRootElement      ─── Independent render tree root
│
├── _NullElement                    ─── Placeholder for uninitialized slots
│
└── RootElement (with RootElementMixin) ─── App root element
```

## Two Fundamental Branches

### 1. ComponentElement - Widget Composition

**Purpose:** Builds OTHER widgets, doesn't create RenderObjects directly.

```dart
abstract class ComponentElement extends Element {
  Element? _child;  // Single child (result of build())
  
  @override
  void performRebuild() {
    Widget built = build();  // Call subclass implementation
    _child = updateChild(_child, built, slot);
  }
  
  /// Subclasses implement this
  Widget build();
}
```

**Key Insight:** ComponentElement has exactly ONE child - the result of `build()`.

| Subclass | Widget Type | build() returns |
|----------|-------------|-----------------|
| StatelessElement | StatelessWidget | `widget.build(this)` |
| StatefulElement | StatefulWidget | `state.build(this)` |
| ProxyElement | ProxyWidget | `widget.child` |

### 2. RenderObjectElement - RenderObject Creation

**Purpose:** Creates and manages RenderObjects for actual rendering.

```dart
abstract class RenderObjectElement extends Element {
  RenderObject? _renderObject;
  
  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    _renderObject = (widget as RenderObjectWidget).createRenderObject(this);
    attachRenderObject(newSlot);
  }
  
  @override
  void update(RenderObjectWidget newWidget) {
    super.update(newWidget);
    (widget as RenderObjectWidget).updateRenderObject(this, renderObject);
  }
  
  // Subclasses implement child management
  void insertRenderObjectChild(RenderObject child, Object? slot);
  void moveRenderObjectChild(RenderObject child, Object? oldSlot, Object? newSlot);
  void removeRenderObjectChild(RenderObject child, Object? slot);
}
```

---

## ComponentElement Subclasses

### StatelessElement

```dart
class StatelessElement extends ComponentElement {
  StatelessElement(StatelessWidget super.widget);

  @override
  Widget build() => (widget as StatelessWidget).build(this);

  @override
  void update(StatelessWidget newWidget) {
    super.update(newWidget);
    rebuild(force: true);  // Always rebuild on update
  }
}
```

### StatefulElement

```dart
class StatefulElement extends ComponentElement {
  StatefulElement(StatefulWidget widget) 
      : _state = widget.createState(), 
        super(widget) {
    state._element = this;
    state._widget = widget;
  }

  State<StatefulWidget>? _state;

  @override
  Widget build() => state.build(this);

  @override
  void _firstBuild() {
    state.initState();
    state.didChangeDependencies();
    super._firstBuild();
  }

  @override
  void update(StatefulWidget newWidget) {
    super.update(newWidget);
    final oldWidget = state._widget!;
    state._widget = widget as StatefulWidget;
    state.didUpdateWidget(oldWidget);
    rebuild(force: true);
  }
}
```

### ProxyElement (abstract)

**Purpose:** Widgets that don't change their child, just "wrap" it with data.

```dart
abstract class ProxyElement extends ComponentElement {
  ProxyElement(ProxyWidget super.widget);

  @override
  Widget build() => (widget as ProxyWidget).child;  // Just return child!

  @override
  void update(ProxyWidget newWidget) {
    super.update(newWidget);
    updated(oldWidget);
    rebuild(force: true);
  }

  /// Called when widget changes - subclasses notify dependents
  void updated(ProxyWidget oldWidget) {
    notifyClients(oldWidget);
  }

  void notifyClients(ProxyWidget oldWidget);
}
```

### ParentDataElement<T>

**Purpose:** Applies ParentData to descendant RenderObjects.

```dart
class ParentDataElement<T extends ParentData> extends ProxyElement {
  ParentDataElement(ParentDataWidget<T> super.widget);

  void _applyParentData(ParentDataWidget<T> widget) {
    void applyParentDataToChild(Element child) {
      if (child is RenderObjectElement) {
        child._updateParentData(widget);
      } else if (child.renderObjectAttachingChild != null) {
        applyParentDataToChild(child.renderObjectAttachingChild!);
      }
    }
    if (renderObjectAttachingChild != null) {
      applyParentDataToChild(renderObjectAttachingChild!);
    }
  }

  @override
  void notifyClients(ParentDataWidget<T> oldWidget) {
    _applyParentData(widget as ParentDataWidget<T>);
  }
}
```

**Example Usage:**
```dart
Stack(
  children: [
    Positioned(  // ParentDataWidget<StackParentData>
      left: 10,
      top: 20,
      child: Text('Hello'),
    ),
  ],
)
```

### InheritedElement

**Purpose:** Provides data to descendants, tracks dependencies.

```dart
class InheritedElement extends ProxyElement {
  InheritedElement(InheritedWidget super.widget);

  // Map of dependent elements to their "aspect" (what they depend on)
  final Map<Element, Object?> _dependents = HashMap<Element, Object?>();

  @override
  void _updateInheritance() {
    // Add self to inheritance chain
    _inheritedElements = _parent?._inheritedElements.put(widget.runtimeType, this);
  }

  void updateDependencies(Element dependent, Object? aspect) {
    setDependencies(dependent, null);  // Record dependency
  }

  void notifyDependent(InheritedWidget oldWidget, Element dependent) {
    dependent.didChangeDependencies();  // Trigger rebuild
  }

  @override
  void updated(InheritedWidget oldWidget) {
    if ((widget as InheritedWidget).updateShouldNotify(oldWidget)) {
      super.updated(oldWidget);  // Only notify if data changed
    }
  }

  @override
  void notifyClients(InheritedWidget oldWidget) {
    for (final dependent in _dependents.keys) {
      notifyDependent(oldWidget, dependent);
    }
  }
}
```

---

## RenderObjectElement Subclasses

### LeafRenderObjectElement - 0 Children

```dart
class LeafRenderObjectElement extends RenderObjectElement {
  LeafRenderObjectElement(LeafRenderObjectWidget super.widget);

  @override
  void insertRenderObjectChild(RenderObject child, Object? slot) {
    assert(false);  // Should never have children
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

**Used by:** `Text`, `Image`, `RawImage`, `RichText`, `CustomPaint` (without child)

### SingleChildRenderObjectElement - 0-1 Child

```dart
class SingleChildRenderObjectElement extends RenderObjectElement {
  SingleChildRenderObjectElement(SingleChildRenderObjectWidget super.widget);

  Element? _child;

  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    _child = updateChild(_child, (widget as SingleChildRenderObjectWidget).child, null);
  }

  @override
  void update(SingleChildRenderObjectWidget newWidget) {
    super.update(newWidget);
    _child = updateChild(_child, newWidget.child, null);
  }

  @override
  void insertRenderObjectChild(RenderObject child, Object? slot) {
    final renderObject = this.renderObject as RenderObjectWithChildMixin<RenderObject>;
    renderObject.child = child;
  }

  @override
  void removeRenderObjectChild(RenderObject child, Object? slot) {
    final renderObject = this.renderObject as RenderObjectWithChildMixin<RenderObject>;
    renderObject.child = null;
  }
}
```

**Used by:** `Padding`, `Opacity`, `ClipRect`, `DecoratedBox`, `Transform`

### MultiChildRenderObjectElement - N Children

```dart
class MultiChildRenderObjectElement extends RenderObjectElement {
  MultiChildRenderObjectElement(MultiChildRenderObjectWidget super.widget);

  late List<Element> _children;
  final Set<Element> _forgottenChildren = HashSet<Element>();

  @override
  void mount(Element? parent, Object? newSlot) {
    super.mount(parent, newSlot);
    final children = List<Element>.filled(widget.children.length, _NullElement.instance);
    Element? previousChild;
    for (var i = 0; i < children.length; i++) {
      final newChild = inflateWidget(
        widget.children[i],
        IndexedSlot<Element?>(i, previousChild),  // Slot = (index, previous)
      );
      children[i] = newChild;
      previousChild = newChild;
    }
    _children = children;
  }

  @override
  void update(MultiChildRenderObjectWidget newWidget) {
    super.update(newWidget);
    _children = updateChildren(_children, newWidget.children, forgottenChildren: _forgottenChildren);
    _forgottenChildren.clear();
  }

  @override
  void insertRenderObjectChild(RenderObject child, IndexedSlot<Element?> slot) {
    final renderObject = this.renderObject as ContainerRenderObjectMixin;
    renderObject.insert(child, after: slot.value?.renderObject);
  }

  @override
  void moveRenderObjectChild(RenderObject child, IndexedSlot<Element?> oldSlot, IndexedSlot<Element?> newSlot) {
    final renderObject = this.renderObject as ContainerRenderObjectMixin;
    renderObject.move(child, after: newSlot.value?.renderObject);
  }

  @override
  void removeRenderObjectChild(RenderObject child, Object? slot) {
    final renderObject = this.renderObject as ContainerRenderObjectMixin;
    renderObject.remove(child);
  }
}
```

**Used by:** `Row`, `Column`, `Stack`, `Flex`, `Wrap`, `Flow`

### RenderTreeRootElement - Independent Root

```dart
abstract class RenderTreeRootElement extends RenderObjectElement {
  RenderTreeRootElement(super.widget);

  @override
  void attachRenderObject(Object? newSlot) {
    _slot = newSlot;
    // Does NOT attach to ancestor - this IS the root
  }

  @override
  void detachRenderObject() {
    _slot = null;
  }
}
```

**Purpose:** For widgets that create their own render tree (e.g., `View` widget for multi-view apps).

---

## Special Elements

### _NullElement

```dart
class _NullElement extends Element {
  _NullElement() : super(const _NullWidget());

  static final _NullElement instance = _NullElement();

  @override
  bool get debugDoingBuild => throw UnimplementedError();
}
```

**Purpose:** Placeholder used in `List<Element>.filled()` before real children are created.

### RootElement (via RootElementMixin)

```dart
mixin RootElementMixin on Element {
  void assignOwner(BuildOwner owner) {
    _owner = owner;
    _parentBuildScope = BuildScope();
  }

  @override
  void mount(Element? parent, Object? newSlot) {
    assert(parent == null);  // Root has no parent!
    assert(newSlot == null);
    super.mount(parent, newSlot);
  }
}

class RootElement extends Element with RootElementMixin {
  RootElement(super.widget);
}
```

**Purpose:** Top-level element in widget tree, owns the BuildOwner.

---

## Element vs Widget Correspondence

| Widget Type | Element Type | Purpose |
|-------------|--------------|---------|
| StatelessWidget | StatelessElement | Compose, no state |
| StatefulWidget | StatefulElement | Compose, with state |
| InheritedWidget | InheritedElement | Provide data to descendants |
| ParentDataWidget | ParentDataElement | Configure ParentData |
| LeafRenderObjectWidget | LeafRenderObjectElement | Render, no children |
| SingleChildRenderObjectWidget | SingleChildRenderObjectElement | Render, one child |
| MultiChildRenderObjectWidget | MultiChildRenderObjectElement | Render, many children |

---

## Key Differences Summary

### ComponentElement vs RenderObjectElement

| Aspect | ComponentElement | RenderObjectElement |
|--------|------------------|---------------------|
| Creates RenderObject? | No | Yes |
| Has `renderObject` field? | No | Yes |
| `build()` returns | Child widget | N/A |
| Child management | Single `_child` | Varies by arity |
| Purpose | Composition | Rendering |

### Proxy vs Other Component Elements

| Aspect | Stateless/Stateful | ProxyElement |
|--------|-------------------|--------------|
| `build()` logic | Custom | Returns `widget.child` |
| On update | Rebuild | Notify clients |
| Purpose | Build new subtree | Pass through with side effects |

---

## Element vs RenderObject: Separation of Concerns

### Why No RenderBoxElement / RenderSliverElement?

Flutter separates concerns along **two orthogonal axes**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ORTHOGONAL HIERARCHIES                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ELEMENT HIERARCHY                      RENDEROBJECT HIERARCHY               │
│  (by child count / arity)               (by layout protocol)                 │
│                                                                              │
│  RenderObjectElement (abstract)         RenderObject (abstract)              │
│  ├── LeafRenderObjectElement              │                                  │
│  │   (0 children)                         ├── RenderBox (abstract)           │
│  │                                        │   • BoxConstraints → Size        │
│  ├── SingleChildRenderObjectElement       │   • hitTest(BoxHitTestResult)    │
│  │   (0-1 child)                          │   ├── RenderPadding              │
│  │                                        │   ├── RenderFlex                 │
│  └── MultiChildRenderObjectElement        │   ├── RenderStack               │
│      (N children)                         │   └── ...                        │
│                                           │                                  │
│                                           └── RenderSliver (abstract)        │
│                                               • SliverConstraints →          │
│                                                 SliverGeometry               │
│                                               • hitTest(SliverHitTestResult) │
│                                               ├── RenderSliverList           │
│                                               ├── RenderSliverGrid           │
│                                               └── ...                        │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key Insight:** One `SingleChildRenderObjectElement` works with BOTH `RenderBox` AND `RenderSliver`:

```dart
// Padding uses SingleChildRenderObjectElement + RenderPadding (RenderBox)
class Padding extends SingleChildRenderObjectWidget {
  @override
  RenderPadding createRenderObject(BuildContext context) => RenderPadding(...);
}

// SliverPadding uses SingleChildRenderObjectElement + RenderSliverPadding (RenderSliver)  
class SliverPadding extends SingleChildRenderObjectWidget {
  @override
  RenderSliverPadding createRenderObject(BuildContext context) => RenderSliverPadding(...);
}
```

### Responsibilities Split

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ELEMENT RESPONSIBILITIES                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Element                                                                     │
│  ───────                                                                     │
│  • Lifecycle management (mount, update, unmount)                             │
│  • Widget tree structure                                                     │
│  • Widget ↔ RenderObject binding                                             │
│  • Dirty state tracking (_dirty flag)                                        │
│  • Child element creation/reconciliation                                     │
│  • BuildOwner coordination                                                   │
│                                                                              │
│  DOES NOT KNOW ABOUT:                                                        │
│  • Layout protocol (Box vs Sliver)                                           │
│  • Constraints type                                                          │
│  • Size/Geometry                                                             │
│  • Paint operations                                                          │
│  • Hit testing                                                               │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                      RENDEROBJECT RESPONSIBILITIES                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  RenderObject                                                                │
│  ────────────                                                                │
│  • Layout protocol (constraints in → geometry out)                           │
│  • Paint operations                                                          │
│  • Hit testing                                                               │
│  • Semantics                                                                 │
│  • Parent data management                                                    │
│  • PipelineOwner coordination                                                │
│                                                                              │
│  DOES NOT KNOW ABOUT:                                                        │
│  • Widget configuration                                                      │
│  • Element lifecycle                                                         │
│  • Build phase                                                               │
│  • State management                                                          │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Protocol Methods Location

```dart
// ═══════════════════════════════════════════════════════════════════════════
// ELEMENT - No protocol methods!
// ═══════════════════════════════════════════════════════════════════════════

abstract class RenderObjectElement extends Element {
  RenderObject? _renderObject;
  
  // Lifecycle only
  void mount(Element? parent, Object? newSlot);
  void update(RenderObjectWidget newWidget);
  void unmount();
  
  // Child management only (delegates to RenderObject)
  void insertRenderObjectChild(RenderObject child, Object? slot);
  void moveRenderObjectChild(RenderObject child, Object? oldSlot, Object? newSlot);
  void removeRenderObjectChild(RenderObject child, Object? slot);
  
  // NO layout(), paint(), hitTest() methods!
}

// ═══════════════════════════════════════════════════════════════════════════
// RENDEROBJECT - All protocol methods!
// ═══════════════════════════════════════════════════════════════════════════

abstract class RenderObject {
  // LAYOUT PROTOCOL
  Constraints? _constraints;
  void layout(Constraints constraints, {bool parentUsesSize = false});
  void performLayout();    // Subclasses override
  void performResize();    // If sizedByParent
  void markNeedsLayout();
  
  // PAINT PROTOCOL
  void paint(PaintingContext context, Offset offset);
  void markNeedsPaint();
  
  // HIT TEST PROTOCOL (defined per-protocol in subclasses)
  void handleEvent(PointerEvent event, HitTestEntry entry);
  
  // SEMANTICS PROTOCOL  
  void describeSemanticsConfiguration(SemanticsConfiguration config);
}

// ═══════════════════════════════════════════════════════════════════════════
// RENDERBOX - Box protocol implementation
// ═══════════════════════════════════════════════════════════════════════════

abstract class RenderBox extends RenderObject {
  Size? _size;
  
  @override
  BoxConstraints get constraints => super.constraints as BoxConstraints;
  
  // Box-specific hit test
  bool hitTest(BoxHitTestResult result, {required Offset position}) {
    if (_size!.contains(position)) {
      if (hitTestChildren(result, position: position) || hitTestSelf(position)) {
        result.add(BoxHitTestEntry(this, position));
        return true;
      }
    }
    return false;
  }
  
  bool hitTestSelf(Offset position) => false;
  bool hitTestChildren(BoxHitTestResult result, {required Offset position}) => false;
}

// ═══════════════════════════════════════════════════════════════════════════
// RENDERSLIVER - Sliver protocol implementation
// ═══════════════════════════════════════════════════════════════════════════

abstract class RenderSliver extends RenderObject {
  SliverGeometry? _geometry;
  
  @override
  SliverConstraints get constraints => super.constraints as SliverConstraints;
  
  // Sliver-specific hit test
  bool hitTest(SliverHitTestResult result, {
    required double mainAxisPosition,
    required double crossAxisPosition,
  });
}
```

### Why This Design?

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          DESIGN RATIONALE                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. ELEMENT TREE ≠ RENDER TREE                                               │
│     ─────────────────────────────                                            │
│     ComponentElement (StatelessElement, StatefulElement) has NO              │
│     RenderObject. Element tree is denser than RenderObject tree.             │
│                                                                              │
│     Widget Tree:     Element Tree:      RenderObject Tree:                   │
│     ───────────      ─────────────      ──────────────────                   │
│     MyApp            MyAppElement       (none)                               │
│     └─Scaffold       └─ScaffoldElement  └─RenderFlex                         │
│       └─Column         └─ColumnElement    └─RenderFlex                       │
│         └─Padding        └─PaddingElem      └─RenderPadding                  │
│           └─Text           └─TextElement      └─RenderParagraph              │
│                                                                              │
│  2. DIFFERENT LIFECYCLES                                                     │
│     ────────────────────                                                     │
│     Element created/destroyed on rebuild.                                    │
│     RenderObject can survive multiple Elements (via GlobalKey).              │
│                                                                              │
│  3. PIPELINE OPERATES ON RENDEROBJECTS                                       │
│     ──────────────────────────────────                                       │
│     PipelineOwner.flushLayout() walks RenderObject tree directly.            │
│     Never touches Element tree during layout/paint.                          │
│                                                                              │
│  4. PROTOCOL IS IMPLEMENTATION DETAIL                                        │
│     ─────────────────────────────────                                        │
│     Element doesn't care if child is Box or Sliver.                          │
│     Protocol enforcement happens at RenderObject level.                      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Three Trees Visualization

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           THREE TREES                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   WIDGET TREE              ELEMENT TREE           RENDEROBJECT TREE          │
│   (Immutable Config)       (Mutable State)        (Layout/Paint)             │
│                                                                              │
│   ┌──────────────┐         ┌──────────────┐                                  │
│   │ MyHomePage   │────────▶│ Stateful     │                                  │
│   │ (Stateful)   │         │ Element      │       (no RenderObject)          │
│   └──────┬───────┘         └──────┬───────┘                                  │
│          │                        │                                          │
│   ┌──────▼───────┐         ┌──────▼───────┐       ┌──────────────┐           │
│   │ Scaffold     │────────▶│ Stateful     │──────▶│ RenderFlex   │           │
│   │ (Stateful)   │         │ Element      │       │ (Box)        │           │
│   └──────┬───────┘         └──────┬───────┘       └──────┬───────┘           │
│          │                        │                      │                   │
│   ┌──────▼───────┐         ┌──────▼───────┐       ┌──────▼───────┐           │
│   │ Column       │────────▶│ MultiChild   │──────▶│ RenderFlex   │           │
│   │ (RenderObj)  │         │ ROElement    │       │ (Box)        │           │
│   └──────┬───────┘         └──────┬───────┘       └──────┬───────┘           │
│          │                        │                      │                   │
│   ┌──────▼───────┐         ┌──────▼───────┐       ┌──────▼───────┐           │
│   │ Padding      │────────▶│ SingleChild  │──────▶│ RenderPadding│           │
│   │ (RenderObj)  │         │ ROElement    │       │ (Box)        │           │
│   └──────┬───────┘         └──────┬───────┘       └──────┬───────┘           │
│          │                        │                      │                   │
│   ┌──────▼───────┐         ┌──────▼───────┐       ┌──────▼───────┐           │
│   │ Text         │────────▶│ LeafRO       │──────▶│RenderParagraph           │
│   │ (RenderObj)  │         │ Element      │       │ (Box)        │           │
│   └──────────────┘         └──────────────┘       └──────────────┘           │
│                                                                              │
│   Legend:                                                                    │
│   ────────                                                                   │
│   ────────▶  Widget creates Element (createElement)                          │
│   ──────▶   Element creates RenderObject (createRenderObject)                │
│   (no RenderObject) = ComponentElement, only composes                        │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Element Child Management vs RenderObject Child Management

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     CHILD MANAGEMENT DELEGATION                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ELEMENT LEVEL                           RENDEROBJECT LEVEL                  │
│  (Element tree ops)                      (RenderObject tree ops)             │
│                                                                              │
│  ┌────────────────────────┐              ┌────────────────────────┐          │
│  │ updateChild(           │              │                        │          │
│  │   oldChild,            │   creates    │ RenderObjectWithChild  │          │
│  │   newWidget,           │ ──────────▶  │ Mixin<RenderBox>       │          │
│  │   slot                 │   or uses    │ ├── child: RenderBox?  │          │
│  │ )                      │              │ └── child = newChild   │          │
│  └────────────────────────┘              └────────────────────────┘          │
│                                                                              │
│  SingleChildRenderObject                 RenderObjectWithChildMixin          │
│  Element:                                <RenderObject>:                     │
│  ┌────────────────────────┐              ┌────────────────────────┐          │
│  │ insertRenderObjectChild│              │ set child(RenderObject?│          │
│  │ (child, slot) {        │  delegates   │   value) {             │          │
│  │   (renderObject as     │ ──────────▶  │   _child?.detach();    │          │
│  │    ROWithChildMixin)   │              │   _child = value;      │          │
│  │   .child = child;      │              │   value?.attach(this); │          │
│  │ }                      │              │ }                      │          │
│  └────────────────────────┘              └────────────────────────┘          │
│                                                                              │
│  MultiChildRenderObject                  ContainerRenderObjectMixin          │
│  Element:                                <RenderObject, ParentData>:         │
│  ┌────────────────────────┐              ┌────────────────────────┐          │
│  │ insertRenderObjectChild│              │ insert(RenderObject    │          │
│  │ (child, slot) {        │  delegates   │   child, {after}) {    │          │
│  │   (renderObject as     │ ──────────▶  │   // Linked list ops   │          │
│  │    ContainerMixin)     │              │   child.parentData     │          │
│  │   .insert(child,       │              │     .nextSibling =     │          │
│  │     after: slot.value  │              │       after?.next;     │          │
│  │       ?.renderObject); │              │   // ...               │          │
│  │ }                      │              │ }                      │          │
│  └────────────────────────┘              └────────────────────────┘          │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## FLUI Design

### Simplified Hierarchy

```
Element (abstract)
│
├── ComponentElement         ─── Views that compose (build() returns child)
│   ├── ViewElement         ─── For stateless View trait
│   ├── HookElement         ─── For views using hooks
│   └── ProxyElement        ─── For InheritedView, ParentDataView
│
└── RenderElement           ─── Views that create RenderObjects
    ├── LeafRenderElement   ─── Leaf arity
    ├── SingleRenderElement ─── Single arity
    └── MultiRenderElement  ─── Variable arity
```

### Key Simplifications

1. **Unified arity system** - Types encode child count at compile-time
2. **Hooks replace StatefulElement** - No separate State class needed
3. **Trait-based dispatch** - `ViewObject` trait instead of many Element types

### Implementation Sketch

```rust
/// Base element for composition views
pub struct ComponentElement {
    child: Option<ElementId>,
    view_object: Box<dyn ViewObject>,
}

/// Base element for render views
pub struct RenderElement<A: Arity> {
    render_object: Box<dyn RenderObject>,
    children: A::Storage,  // Leaf=(), Single=Option<ElementId>, Multi=Vec<ElementId>
}

/// Proxy element for inherited/parent-data
pub struct ProxyElement {
    child: Option<ElementId>,
    proxy_object: Box<dyn ProxyObject>,
}

trait ProxyObject {
    fn child(&self) -> &dyn View;
    fn updated(&self, old: &dyn View, ctx: &mut UpdateContext);
    fn notify_dependents(&self, ctx: &mut NotifyContext);
}
```

### InheritedElement in FLUI

```rust
pub struct InheritedElement<T: 'static> {
    child: Option<ElementId>,
    data: T,
    dependents: HashMap<ElementId, Option<Box<dyn Any>>>,  // aspect
}

impl<T: PartialEq + 'static> InheritedElement<T> {
    fn updated(&mut self, old_data: &T, ctx: &mut UpdateContext) {
        if self.data != *old_data {
            // Notify all dependents
            for &dependent_id in self.dependents.keys() {
                ctx.mark_needs_build(dependent_id);
            }
        }
    }
}
```

---

## Summary

Flutter's Element hierarchy has **two main branches**:

1. **ComponentElement** - For widgets that BUILD other widgets
   - StatelessElement, StatefulElement - different state models
   - ProxyElement - pass-through with side effects (InheritedWidget, ParentDataWidget)

2. **RenderObjectElement** - For widgets that CREATE RenderObjects
   - Leaf/Single/Multi variants for different child counts
   - RenderTreeRootElement for independent render trees

**FLUI can simplify** by:
- Using hooks instead of StatefulElement
- Using compile-time arity instead of runtime child counts
- Using traits (ViewObject, ProxyObject) instead of multiple Element classes
