# Semantics System

The semantics system provides accessibility information for assistive technologies (screen readers, etc.).

## Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Semantics Architecture                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Render Tree                      Semantics Tree                    │
│  ────────────                     ──────────────                    │
│                                                                     │
│  ┌──────────┐                     ┌──────────────┐                  │
│  │RenderView│──────────────────── │SemanticsNode │ (root)           │
│  └────┬─────┘                     └──────┬───────┘                  │
│       │                                  │                          │
│  ┌────┴────┐                        ┌────┴────┐                     │
│  ▼         ▼                        ▼         ▼                     │
│ ┌──┐     ┌──┐                    ┌────┐    ┌────┐                   │
│ │A │     │B │ (boundary)         │Node│    │Node│                   │
│ └┬─┘     └┬─┘                    └────┘    └──┬─┘                   │
│  │        │                                   │                     │
│  ▼        ▼                                   ▼                     │
│ ┌──┐    ┌──┐                               ┌────┐                   │
│ │A1│    │B1│                               │Node│                   │
│ └──┘    └──┘                               └────┘                   │
│                                                                     │
│  Not all RenderObjects create SemanticsNodes.                       │
│  Semantics can merge up to parent boundaries.                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Key Components

### SemanticsConfiguration

```dart
class SemanticsConfiguration {
  // Boundary behavior
  bool isSemanticBoundary;      // Creates a SemanticsNode
  bool explicitChildNodes;       // Children get own nodes
  bool isBlockingUserActions;    // Block tap, etc.
  
  // Identity
  String? identifier;
  
  // Content
  AttributedString? attributedLabel;
  AttributedString? attributedValue;
  AttributedString? attributedHint;
  String? tooltip;
  
  // State flags
  bool? isEnabled;
  bool? isChecked;
  bool? isToggled;
  bool? isSelected;
  bool? isFocused;
  bool? isHidden;
  bool? isObscured;
  bool? isReadOnly;
  bool? isExpanded;
  
  // Role indicators
  bool isButton;
  bool isLink;
  bool isSlider;
  bool isTextField;
  bool isHeader;
  bool isImage;
  
  // Text properties
  TextDirection? textDirection;
  int? headingLevel;
  int? maxValueLength;
  int? currentValueLength;
  
  // Actions
  VoidCallback? onTap;
  VoidCallback? onLongPress;
  VoidCallback? onScrollLeft;
  VoidCallback? onScrollRight;
  VoidCallback? onScrollUp;
  VoidCallback? onScrollDown;
  VoidCallback? onIncrease;
  VoidCallback? onDecrease;
  VoidCallback? onCopy;
  VoidCallback? onCut;
  VoidCallback? onPaste;
  VoidCallback? onDismiss;
  // ... more actions
  
  // Sorting
  SemanticsSortKey? sortKey;
}
```

### RenderObject Semantics Methods

```dart
abstract class RenderObject {
  /// Describe semantics for this render object
  @protected
  void describeSemanticsConfiguration(SemanticsConfiguration config) {
    // Override to annotate config
  }
  
  /// Mark semantics as needing update
  void markNeedsSemanticsUpdate() {
    if (!attached || owner!._semanticsOwner == null) return;
    _semantics.markNeedsUpdate();
  }
  
  /// Report semantic bounds
  Rect get semanticBounds;
  
  /// Visit children for semantics (may differ from paint order)
  void visitChildrenForSemantics(RenderObjectVisitor visitor) {
    visitChildren(visitor);
  }
  
  /// Assemble the SemanticsNode
  void assembleSemanticsNode(
    SemanticsNode node,
    SemanticsConfiguration config,
    Iterable<SemanticsNode> children,
  ) {
    node.updateWith(
      config: config, 
      childrenInInversePaintOrder: children as List<SemanticsNode>,
    );
  }
  
  /// Send accessibility event
  void sendSemanticsEvent(SemanticsEvent event) {
    // Walks up tree to find nearest semantics node
  }
  
  /// Clear semantics (called on detach)
  void clearSemantics() {
    _semantics.clear();
    visitChildren((child) => child.clearSemantics());
  }
}
```

## _RenderObjectSemantics

Internal class managing semantics state:

```dart
class _RenderObjectSemantics {
  _RenderObjectSemantics(this._renderObject);
  
  final RenderObject _renderObject;
  
  // Configuration
  final _SemanticsConfigurationProvider configProvider;
  
  // State
  bool built = false;
  bool parentDataDirty = true;
  SemanticsNode? cachedSemanticsNode;
  
  // Parent data (from ancestors)
  _SemanticsParentData? _parentData;
  
  void markNeedsUpdate() {
    parentDataDirty = true;
    _renderObject.owner?._nodesNeedingSemantics.add(_renderObject);
    _renderObject.owner?.requestVisualUpdate();
  }
  
  void updateChildren() { /* ... */ }
  void ensureGeometry() { /* ... */ }
  void ensureSemanticsNode() { /* ... */ }
  void clear() { /* ... */ }
}
```

## _SemanticsParentData

Data imposed by parent semantics:

```dart
final class _SemanticsParentData {
  const _SemanticsParentData({
    required this.mergeIntoParent,
    required this.blocksUserActions,
    required this.explicitChildNodes,
    required this.tagsForChildren,
    required this.localeForChildren,
    required this.accessiblityFocusBlockType,
  });
  
  final bool mergeIntoParent;           // From MergeSemantics
  final bool blocksUserActions;         // From IgnorePointer/AbsorbPointer
  final bool explicitChildNodes;        // Children must create nodes
  final Set<SemanticsTag>? tagsForChildren;
  final Locale? localeForChildren;
  final AccessiblityFocusBlockType? accessiblityFocusBlockType;
}
```

## SemanticsAnnotationsMixin

Mixin for render objects with semantic annotations:

```dart
mixin SemanticsAnnotationsMixin on RenderObject {
  late SemanticsProperties _properties;
  late bool _container;
  late bool _explicitChildNodes;
  late bool _excludeSemantics;
  late bool _blockUserActions;
  Locale? _localeForSubtree;
  TextDirection? _textDirection;
  
  // Attributed strings
  AttributedString? _attributedLabel;
  AttributedString? _attributedValue;
  AttributedString? _attributedHint;
  // ...
  
  void initSemanticsAnnotations({...}) { /* ... */ }
  
  @override
  void visitChildrenForSemantics(RenderObjectVisitor visitor) {
    if (excludeSemantics) return;  // Skip children
    super.visitChildrenForSemantics(visitor);
  }
  
  @override
  void describeSemanticsConfiguration(SemanticsConfiguration config) {
    super.describeSemanticsConfiguration(config);
    
    config.isSemanticBoundary = container;
    config.explicitChildNodes = explicitChildNodes;
    config.isBlockingUserActions = blockUserActions;
    
    if (_properties.enabled != null) config.isEnabled = _properties.enabled;
    if (_properties.checked != null) config.isChecked = _properties.checked;
    if (_properties.selected != null) config.isSelected = _properties.selected!;
    // ... many more properties
    
    // Actions with indirection for updates
    if (_properties.onTap != null) config.onTap = _performTap;
    if (_properties.onLongPress != null) config.onLongPress = _performLongPress;
    // ... more actions
  }
  
  void _performTap() => _properties.onTap?.call();
  void _performLongPress() => _properties.onLongPress?.call();
  // ...
}
```

## Semantics Flush Phase

```
┌─────────────────────────────────────────────────────────────────────┐
│                   flushSemantics() Flow                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. Get dirty nodes from _nodesNeedingSemantics                     │
│                                                                     │
│  2. Filter: Skip nodes still needing layout                         │
│                                                                     │
│  3. Sort by depth (shallowest first)                                │
│                                                                     │
│  4. Phase: updateChildren()                                         │
│     - Update children list for each semantics node                  │
│     - Determine which children contribute to semantics              │
│                                                                     │
│  5. Phase: ensureGeometry()                                         │
│     - Compute bounds and transforms                                 │
│     - Apply paint transforms and clips                              │
│                                                                     │
│  6. Phase: ensureSemanticsNode() (reverse order: deepest first)     │
│     - Create/update SemanticsNode for each boundary                 │
│     - Merge child semantics                                         │
│                                                                     │
│  7. semanticsOwner.sendSemanticsUpdate()                            │
│     - Send delta update to platform                                 │
│                                                                     │
│  8. Recurse to child PipelineOwners                                 │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Semantics Merging

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Semantics Merging                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Render Tree:                      Semantics Tree:                  │
│                                                                     │
│  ┌─────────────────────┐          ┌─────────────────────┐          │
│  │ MergeSemantics      │          │ SemanticsNode       │          │
│  │ (boundary=true)     │──────────│ label: "Button: OK" │          │
│  └──────────┬──────────┘          │ isButton: true      │          │
│             │                     │ onTap: ...          │          │
│     ┌───────┴───────┐             └─────────────────────┘          │
│     ▼               ▼                                              │
│  ┌──────────┐   ┌──────────┐      All child semantics              │
│  │ Text     │   │ GestureD │      merged into parent node          │
│  │ "OK"     │   │ (button) │                                       │
│  │ label    │   │ onTap    │                                       │
│  └──────────┘   └──────────┘                                       │
│                                                                     │
│  Without MergeSemantics:           With MergeSemantics:            │
│  - 3 SemanticsNodes               - 1 SemanticsNode                │
│  - Screen reader reads each       - Single logical element          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Semantic Boundaries

```
┌─────────────────────────────────────────────────────────────────────┐
│                   Semantic Boundary Types                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. isSemanticBoundary = true:                                      │
│     - Creates a SemanticsNode at this render object                 │
│     - Children semantics collected into this node                   │
│                                                                     │
│  2. explicitChildNodes = true:                                      │
│     - Each semantic child MUST create its own node                  │
│     - No merging into this node                                     │
│     - Used for lists, grids where items need individual focus       │
│                                                                     │
│  3. isBlockingSemanticsOfPreviouslyPaintedNodes = true:             │
│     - Blocks semantics from earlier siblings                        │
│     - Example: Modal dialog blocks content behind it                │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │ Example: ListView                                           │     │
│  │                                                            │     │
│  │   ListView                                                 │     │
│  │   isSemanticBoundary: true                                 │     │
│  │   explicitChildNodes: true  <-- Each item gets own node    │     │
│  │                                                            │     │
│  │   ├── Item 1 (SemanticsNode)                               │     │
│  │   ├── Item 2 (SemanticsNode)                               │     │
│  │   └── Item 3 (SemanticsNode)                               │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Accessibility Focus

```dart
enum AccessiblityFocusBlockType {
  none,           // Focus not blocked
  blockSubtree,   // Block entire subtree
  blockNode,      // Block this node only
}
```

## Rust Implementation Notes

```rust
/// Semantics configuration
#[derive(Default, Clone)]
pub struct SemanticsConfiguration {
    // Boundary
    pub is_semantic_boundary: bool,
    pub explicit_child_nodes: bool,
    pub is_blocking_user_actions: bool,
    
    // Content
    pub label: Option<AttributedString>,
    pub value: Option<AttributedString>,
    pub hint: Option<AttributedString>,
    
    // State
    pub is_enabled: Option<bool>,
    pub is_checked: Option<bool>,
    pub is_selected: Option<bool>,
    pub is_focused: Option<bool>,
    pub is_hidden: Option<bool>,
    
    // Role
    pub is_button: bool,
    pub is_link: bool,
    pub is_slider: bool,
    pub is_text_field: bool,
    pub is_header: bool,
    pub is_image: bool,
    
    // Actions
    pub on_tap: Option<Box<dyn Fn() + Send + Sync>>,
    pub on_long_press: Option<Box<dyn Fn() + Send + Sync>>,
    pub on_scroll: Option<Box<dyn Fn(ScrollDirection) + Send + Sync>>,
    // ... more actions
}

/// Semantics node
pub struct SemanticsNode {
    pub id: SemanticsId,
    pub rect: Rect,
    pub transform: Matrix4,
    pub config: SemanticsConfiguration,
    pub children: Vec<Arc<SemanticsNode>>,
    pub is_merged_into_parent: bool,
}

/// Trait for render objects with semantics
pub trait RenderObjectSemantics {
    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {}
    
    fn semantic_bounds(&self) -> Rect;
    
    fn visit_children_for_semantics(&self, visitor: impl FnMut(&dyn RenderObject)) {
        self.visit_children(visitor);
    }
    
    fn assemble_semantics_node(
        &self,
        node: &mut SemanticsNode,
        config: &SemanticsConfiguration,
        children: impl Iterator<Item = Arc<SemanticsNode>>,
    );
}

/// Semantics owner (manages semantics tree)
pub struct SemanticsOwner {
    nodes: HashMap<SemanticsId, Arc<RwLock<SemanticsNode>>>,
    root_id: Option<SemanticsId>,
    on_semantics_update: Box<dyn Fn(SemanticsUpdate) + Send + Sync>,
}

impl SemanticsOwner {
    pub fn send_semantics_update(&mut self) {
        let update = self.compute_delta();
        (self.on_semantics_update)(update);
    }
}
```
