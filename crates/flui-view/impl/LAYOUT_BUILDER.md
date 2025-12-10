# LayoutBuilder Pattern

This document analyzes Flutter's LayoutBuilder - widgets that defer building until layout time when constraints are known.

## Source Files
- `packages/flutter/lib/src/widgets/layout_builder.dart`

## Core Architecture

### The Problem LayoutBuilder Solves

In Flutter, `build()` is called before layout. But sometimes you need to know the available space to decide what to build:

```dart
// Can't do this in build() - constraints not known yet!
Widget build(BuildContext context) {
  if (availableWidth > 600) {
    return WideLayout();
  } else {
    return NarrowLayout();
  }
}
```

LayoutBuilder solves this by deferring the build to layout time.

### AbstractLayoutBuilder

```dart
abstract class AbstractLayoutBuilder<LayoutInfoType> extends RenderObjectWidget {
  const AbstractLayoutBuilder({super.key});

  /// Called at layout time with layout info
  Widget Function(BuildContext context, LayoutInfoType layoutInfo) get builder;

  @override
  RenderObjectElement createElement() => _LayoutBuilderElement<LayoutInfoType>(this);

  /// Whether builder needs to be called again even if constraints are same
  @protected
  bool updateShouldRebuild(covariant AbstractLayoutBuilder<LayoutInfoType> oldWidget) => true;

  @override
  RenderAbstractLayoutBuilderMixin<LayoutInfoType, RenderObject> createRenderObject(BuildContext context);
}
```

**Key Points:**
- `LayoutInfoType` - Generic type for layout information (usually Constraints)
- `builder` - Called during layout, not during build
- `updateShouldRebuild` - Optimization to avoid unnecessary rebuilds

### ConstrainedLayoutBuilder

```dart
abstract class ConstrainedLayoutBuilder<ConstraintType extends Constraints>
    extends AbstractLayoutBuilder<ConstraintType> {
  
  const ConstrainedLayoutBuilder({super.key, required this.builder});

  @override
  final Widget Function(BuildContext context, ConstraintType constraints) builder;
}
```

The builder is called:
1. First time widget is laid out
2. When parent passes different constraints
3. When `updateShouldRebuild` returns true
4. When builder's dependencies change

### LayoutBuilder (Concrete Implementation)

```dart
class LayoutBuilder extends ConstrainedLayoutBuilder<BoxConstraints> {
  const LayoutBuilder({super.key, required super.builder});

  @override
  RenderAbstractLayoutBuilderMixin<BoxConstraints, RenderBox> createRenderObject(
    BuildContext context,
  ) => _RenderLayoutBuilder();
}
```

### The Element

```dart
class _LayoutBuilderElement<LayoutInfoType> extends RenderObjectElement {
  _LayoutBuilderElement(AbstractLayoutBuilder<LayoutInfoType> super.widget);

  Element? _child;
  LayoutInfoType? _previousLayoutInfo;
  bool _needsBuild = true;

  // Custom BuildScope for deferred building
  late final BuildScope _buildScope = BuildScope(scheduleRebuild: _scheduleRebuild);

  @override
  BuildScope get buildScope => _buildScope;

  void _scheduleRebuild() {
    // Defer markNeedsLayout during postFrameCallbacks and idle phases
    final bool deferMarkNeedsLayout = switch (SchedulerBinding.instance.schedulerPhase) {
      SchedulerPhase.idle || SchedulerPhase.postFrameCallbacks => true,
      _ => false,
    };
    
    if (!deferMarkNeedsLayout) {
      renderObject.scheduleLayoutCallback();
    } else {
      _deferredCallbackScheduled = true;
      SchedulerBinding.instance.scheduleFrameCallback(_frameCallback);
    }
  }

  void _rebuildWithConstraints(Constraints _) {
    final LayoutInfoType layoutInfo = renderObject.layoutInfo;
    
    void updateChildCallback() {
      Widget built = widget.builder(this, layoutInfo);
      _child = updateChild(_child, built, null);
      _needsBuild = false;
      _previousLayoutInfo = layoutInfo;
    }

    // Only rebuild if needed or constraints changed
    final callback = _needsBuild || (layoutInfo != _previousLayoutInfo)
        ? updateChildCallback
        : null;
    owner!.buildScope(this, callback);
  }
}
```

**Critical Design:**
- Has its own `BuildScope` - isolated from normal build phase
- Compares `_previousLayoutInfo` to avoid rebuilds when constraints unchanged
- `_needsBuild` flag tracks if widget config changed

### The RenderObject Mixin

```dart
mixin RenderAbstractLayoutBuilderMixin<LayoutInfoType, ChildType extends RenderObject>
    on RenderObjectWithChildMixin<ChildType>, RenderObjectWithLayoutCallbackMixin {
  
  LayoutCallback<Constraints>? _callback;

  void _updateCallback(LayoutCallback<Constraints> value) {
    if (value == _callback) return;
    _callback = value;
    scheduleLayoutCallback();
  }

  /// Called in performLayout to rebuild subtree if needed
  @override
  void layoutCallback() => _callback!(constraints);

  /// The layout info to pass to builder (default: constraints)
  @protected
  LayoutInfoType get layoutInfo => constraints as LayoutInfoType;
}
```

### _RenderLayoutBuilder

```dart
class _RenderLayoutBuilder extends RenderBox
    with
        RenderObjectWithChildMixin<RenderBox>,
        RenderObjectWithLayoutCallbackMixin,
        RenderAbstractLayoutBuilderMixin<BoxConstraints, RenderBox> {

  // Intrinsics throw - can't calculate without running builder
  @override
  double computeMinIntrinsicWidth(double height) {
    assert(_debugThrowIfNotCheckingIntrinsics());
    return 0.0;
  }

  // Dry layout throws - would require speculative builder call
  @override
  Size computeDryLayout(BoxConstraints constraints) {
    assert(debugCannotComputeDryLayout(
      reason: 'Would require running layout callback speculatively...',
    ));
    return Size.zero;
  }

  @override
  void performLayout() {
    final BoxConstraints constraints = this.constraints;
    runLayoutCallback();  // This calls builder
    if (child != null) {
      child!.layout(constraints, parentUsesSize: true);
      size = constraints.constrain(child!.size);
    } else {
      size = constraints.biggest;
    }
  }
}
```

**Limitations:**
- Cannot compute intrinsic dimensions (would require speculative build)
- Cannot compute dry layout
- Builder runs during layout - must not have side effects beyond widget tree

## Usage Patterns

### Responsive Layout

```dart
LayoutBuilder(
  builder: (context, constraints) {
    if (constraints.maxWidth > 600) {
      return WideLayout(children: items);
    } else {
      return NarrowLayout(children: items);
    }
  },
)
```

### Conditional Child Count

```dart
LayoutBuilder(
  builder: (context, constraints) {
    final itemCount = (constraints.maxWidth / 100).floor();
    return Row(
      children: List.generate(itemCount, (i) => ItemWidget(i)),
    );
  },
)
```

### Aspect Ratio Decisions

```dart
LayoutBuilder(
  builder: (context, constraints) {
    final aspectRatio = constraints.maxWidth / constraints.maxHeight;
    return aspectRatio > 1.5 ? LandscapeView() : PortraitView();
  },
)
```

---

## FLUI Design

### Core Trait

```rust
/// View that builds based on layout constraints
pub trait ConstrainedBuilder: View {
    type Constraints: LayoutConstraints;
    
    /// Build the child view given layout constraints
    fn build_with_constraints(
        &self, 
        ctx: &mut BuildContext, 
        constraints: &Self::Constraints
    ) -> impl View;
}
```

### LayoutBuilder View

```rust
/// Builds children based on BoxConstraints
pub struct LayoutBuilder<F>
where
    F: Fn(&mut BuildContext, &BoxConstraints) -> Box<dyn View> + 'static,
{
    builder: F,
}

impl<F> LayoutBuilder<F>
where
    F: Fn(&mut BuildContext, &BoxConstraints) -> Box<dyn View> + 'static,
{
    pub fn new(builder: F) -> Self {
        Self { builder }
    }
}

impl<F> View for LayoutBuilder<F>
where
    F: Fn(&mut BuildContext, &BoxConstraints) -> Box<dyn View> + 'static,
{
    fn build(&self, ctx: &mut BuildContext) -> impl IntoElement {
        // Returns special element that defers to layout
        LayoutBuilderElement::new(self.builder.clone())
    }
}
```

### Element Implementation

```rust
pub struct LayoutBuilderElement<F> {
    builder: F,
    child: Option<ElementId>,
    previous_constraints: Option<BoxConstraints>,
    needs_build: bool,
}

impl<F> Element for LayoutBuilderElement<F>
where
    F: Fn(&mut BuildContext, &BoxConstraints) -> Box<dyn View> + 'static,
{
    fn mount(&mut self, ctx: &mut MountContext) {
        // Don't build child yet - wait for layout
        self.needs_build = true;
    }
    
    fn update(&mut self, new_view: &dyn View, ctx: &mut UpdateContext) {
        self.needs_build = true;
        ctx.mark_needs_layout();
    }
}
```

### RenderObject with Layout Callback

```rust
pub struct LayoutBuilderRender<F> {
    builder: F,
    child: Option<RenderObjectId>,
    layout_callback: Option<Box<dyn FnMut(&BoxConstraints)>>,
}

impl<F> RenderBox for LayoutBuilderRender<F>
where
    F: Fn(&mut BuildContext, &BoxConstraints) -> Box<dyn View> + 'static,
{
    fn perform_layout(&mut self, constraints: &BoxConstraints, ctx: &mut LayoutContext) {
        // Run layout callback to potentially rebuild child
        if let Some(callback) = &mut self.layout_callback {
            callback(constraints);
        }
        
        // Layout child if exists
        if let Some(child_id) = self.child {
            let child_size = ctx.layout_child(child_id, constraints);
            self.size = constraints.constrain(child_size);
        } else {
            self.size = constraints.biggest();
        }
    }
    
    // Cannot compute intrinsics - would need to run builder
    fn compute_intrinsic_width(&self, _height: f32) -> f32 {
        0.0  // Or panic in debug mode
    }
    
    fn compute_dry_layout(&self, _constraints: &BoxConstraints) -> Size {
        panic!("LayoutBuilder cannot compute dry layout");
    }
}
```

### Deferred Build Scope

```rust
/// Isolated build scope for layout-time building
pub struct LayoutBuildScope {
    element_id: ElementId,
    pending_rebuild: bool,
}

impl LayoutBuildScope {
    /// Schedule rebuild during next layout
    pub fn schedule_rebuild(&mut self) {
        self.pending_rebuild = true;
        // Mark render object needs layout
    }
    
    /// Execute deferred build within layout phase
    pub fn run_build<F>(&mut self, constraints: &BoxConstraints, builder: F) -> ElementId
    where
        F: FnOnce(&mut BuildContext, &BoxConstraints) -> Box<dyn View>,
    {
        // Build in isolated scope
        // Update or create child element
        // Return child element id
    }
}
```

### Usage Examples

```rust
// Responsive layout
fn responsive_list(ctx: &mut BuildContext) -> impl View {
    LayoutBuilder::new(|ctx, constraints| {
        if constraints.max_width > 600.0 {
            Box::new(GridView::new(items.clone(), columns: 3))
        } else {
            Box::new(ListView::new(items.clone()))
        }
    })
}

// Dynamic item count
fn dynamic_row(ctx: &mut BuildContext) -> impl View {
    LayoutBuilder::new(|ctx, constraints| {
        let item_count = (constraints.max_width / 100.0) as usize;
        Box::new(Row::new(
            (0..item_count).map(|i| item_widget(i)).collect()
        ))
    })
}
```

### With Closure Capture

```rust
fn settings_panel(items: Vec<SettingItem>) -> impl View {
    // items captured by closure
    LayoutBuilder::new(move |ctx, constraints| {
        let columns = if constraints.max_width > 800.0 { 2 } else { 1 };
        Box::new(SettingsGrid::new(items.clone(), columns))
    })
}
```

### SliverLayoutBuilder Equivalent

```rust
/// LayoutBuilder for sliver constraints
pub struct SliverLayoutBuilder<F>
where
    F: Fn(&mut BuildContext, &SliverConstraints) -> Box<dyn View> + 'static,
{
    builder: F,
}
```

## Key Design Considerations

### 1. Build Phase Isolation

LayoutBuilder builds during layout, not during normal build phase. This requires:
- Separate `BuildScope` 
- Careful scheduling to not dirty tree during wrong phase
- Proper handling of inherited widget dependencies

### 2. Constraint Comparison

Only rebuild when constraints actually change:

```rust
fn should_rebuild(&self, new_constraints: &BoxConstraints) -> bool {
    self.needs_build || self.previous_constraints.as_ref() != Some(new_constraints)
}
```

### 3. Intrinsic Size Limitations

Cannot compute intrinsic dimensions because:
- Would require running builder speculatively
- Builder might have side effects
- Result depends on constraints (circular)

### 4. Performance

- Cache constraint comparison results
- Avoid rebuilding when only position changes
- Use `updateShouldRebuild` for widget config changes

## Summary

LayoutBuilder provides:
1. **Deferred building** - Build at layout time when constraints known
2. **Responsive design** - Different widgets for different sizes
3. **Constraint-aware** - Access to actual layout constraints
4. **Optimization** - Only rebuilds when constraints change

FLUI implementation needs:
- Separate build scope for layout-time building
- Proper integration with pipeline phases
- Constraint caching and comparison
- Clear documentation of intrinsic size limitations
