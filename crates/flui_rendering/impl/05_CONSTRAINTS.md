# Constraints System

The `Constraints` abstract class defines the protocol for passing layout information from parent to child.

## Abstract Interface

```dart
@immutable
abstract class Constraints {
  const Constraints();
  
  /// Whether only one size satisfies these constraints
  bool get isTight;
  
  /// Whether the constraints are in canonical form
  bool get isNormalized;
  
  /// Validate constraints (debug only)
  bool debugAssertIsValid({
    bool isAppliedConstraint = false,
    InformationCollector? informationCollector,
  });
}
```

## Constraint Properties

### isTight

```
┌─────────────────────────────────────────────────────────────────────┐
│                         isTight Property                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Tight constraints = exactly one valid size                         │
│                                                                     │
│  For BoxConstraints:                                                │
│  isTight = (minWidth == maxWidth) && (minHeight == maxHeight)       │
│                                                                     │
│  Examples:                                                          │
│                                                                     │
│  TIGHT:                          NOT TIGHT:                         │
│  ┌─────────────────────┐         ┌─────────────────────┐           │
│  │ min: (100, 100)     │         │ min: (0, 0)         │           │
│  │ max: (100, 100)     │         │ max: (200, 300)     │           │
│  │                     │         │                     │           │
│  │ Only valid size:    │         │ Many valid sizes:   │           │
│  │ 100 x 100           │         │ 0x0, 100x150,       │           │
│  └─────────────────────┘         │ 200x300, etc.       │           │
│                                  └─────────────────────┘           │
│                                                                     │
│  Used for relayout boundary determination:                          │
│  - If constraints.isTight, child is a relayout boundary             │
│  - Parent already determined exact size, child size can't change    │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### isNormalized

```
┌─────────────────────────────────────────────────────────────────────┐
│                      isNormalized Property                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Normalized = constraints in canonical form                         │
│                                                                     │
│  For BoxConstraints:                                                │
│  isNormalized = minWidth <= maxWidth && minHeight <= maxHeight      │
│              && minWidth >= 0 && minHeight >= 0                     │
│                                                                     │
│  NORMALIZED:                     NOT NORMALIZED:                    │
│  ┌─────────────────────┐         ┌─────────────────────┐           │
│  │ min: (50, 50)       │         │ min: (200, 200)     │ min > max │
│  │ max: (100, 100)     │         │ max: (100, 100)     │           │
│  └─────────────────────┘         └─────────────────────┘           │
│                                                                     │
│  Non-normalized constraints are normalized by prioritizing min:     │
│  - If minWidth > maxWidth, use minWidth for both                    │
│  - If minHeight > maxHeight, use minHeight for both                 │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## BoxConstraints (Flutter's Main Constraint Type)

Although not in `object.dart`, BoxConstraints is the primary constraint type:

```
┌───────────────────────────────────────────────────────────────────────┐
│                         BoxConstraints                                 │
├───────────────────────────────────────────────────────────────────────┤
│  - minWidth: double                                                   │
│  - maxWidth: double                                                   │
│  - minHeight: double                                                  │
│  - maxHeight: double                                                  │
├───────────────────────────────────────────────────────────────────────┤
│  Constructors:                                                        │
│  - BoxConstraints(min/max width/height)                               │
│  - BoxConstraints.tight(Size)           // exact size                │
│  - BoxConstraints.loose(Size)           // 0 to size                 │
│  - BoxConstraints.expand(w, h)          // infinite or exact         │
│  - BoxConstraints.tightFor(w, h)        // tight in specified dims   │
├───────────────────────────────────────────────────────────────────────┤
│  Key Methods:                                                         │
│  - constrain(Size) -> Size              // clamp to constraints      │
│  - constrainWidth(double) -> double     // clamp width               │
│  - constrainHeight(double) -> double    // clamp height              │
│  - enforce(BoxConstraints) -> BoxConstraints  // intersect           │
│  - deflate(EdgeInsets) -> BoxConstraints      // reduce by insets    │
│  - loosen() -> BoxConstraints           // minWidth=minHeight=0      │
│  - tighten(w, h) -> BoxConstraints      // set min=max              │
└───────────────────────────────────────────────────────────────────────┘
```

## Constraint Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Constraint Flow in Layout                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│                    Constraints flow DOWN                            │
│                    ────────────────────►                            │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ Parent                                                        │  │
│  │                                                               │  │
│  │  performLayout() {                                            │  │
│  │    // Create constraints for child                            │  │
│  │    final childConstraints = BoxConstraints(                   │  │
│  │      maxWidth: constraints.maxWidth - padding,                │  │
│  │      maxHeight: constraints.maxHeight - padding,              │  │
│  │    );                                                         │  │
│  │                                                               │  │
│  │    // Layout child with constraints                           │  │
│  │    child.layout(childConstraints, parentUsesSize: true);      │  │
│  │                                                               │  │
│  │    // Use child's size                                        │  │
│  │    size = Size(child.size.width + padding,                    │  │
│  │               child.size.height + padding);                   │  │
│  │  }                                                            │  │
│  │                                                               │  │
│  └──────────────────────────────┬───────────────────────────────┘  │
│                                 │                                   │
│                                 ▼                                   │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ Child                                                         │  │
│  │                                                               │  │
│  │  performLayout() {                                            │  │
│  │    // Determine size within constraints                       │  │
│  │    size = constraints.constrain(desiredSize);                 │  │
│  │  }                                                            │  │
│  │                                                               │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│                    Sizes flow UP                                    │
│                    ◄────────────────────                            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## parentUsesSize Parameter

```
┌─────────────────────────────────────────────────────────────────────┐
│                    parentUsesSize Impact                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  child.layout(constraints, parentUsesSize: true)                    │
│  ─────────────────────────────────────────────────                  │
│  - Parent reads child.size after layout                             │
│  - Child is NOT a relayout boundary                                 │
│  - Child size change → Parent relayout                              │
│                                                                     │
│  child.layout(constraints, parentUsesSize: false)                   │
│  ──────────────────────────────────────────────────                 │
│  - Parent CANNOT read child.size                                    │
│  - Child MAY BE a relayout boundary (if other conditions met)       │
│  - Child size change → Only child relayout                          │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │ Example: Stack                                              │     │
│  │                                                            │     │
│  │  positioned children:                                      │     │
│  │    child.layout(constraints, parentUsesSize: false)        │     │
│  │    // Stack positions child explicitly, doesn't need size  │     │
│  │                                                            │     │
│  │  non-positioned children:                                  │     │
│  │    child.layout(constraints, parentUsesSize: true)         │     │
│  │    // Stack uses child size for its own size calculation   │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## sizedByParent Optimization

```
┌─────────────────────────────────────────────────────────────────────┐
│                    sizedByParent = true                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  When sizedByParent is true:                                        │
│  1. Size is determined ONLY by constraints                          │
│  2. performResize() is called BEFORE performLayout()                │
│  3. Children don't affect this render object's size                 │
│  4. This render object is ALWAYS a relayout boundary                │
│                                                                     │
│  Layout sequence:                                                   │
│                                                                     │
│  ┌─────────────┐     ┌─────────────────┐     ┌─────────────┐       │
│  │   layout()  │ --> │ performResize() │ --> │performLayout│       │
│  │             │     │ (sets size)     │     │(lays out    │       │
│  │             │     │                 │     │ children)   │       │
│  └─────────────┘     └─────────────────┘     └─────────────┘       │
│                                                                     │
│  Example use cases:                                                 │
│  - Container that always fills available space                      │
│  - Image with fixed aspect ratio                                    │
│  - Widgets that size based on constraints alone                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Debug Validation

```dart
bool debugAssertIsValid({
  bool isAppliedConstraint = false,
  InformationCollector? informationCollector,
}) {
  // isAppliedConstraint = true means these constraints are 
  // about to be applied during layout() call
  
  // For BoxConstraints, checks:
  // - No NaN values
  // - minWidth <= maxWidth
  // - minHeight <= maxHeight
  // - All values >= 0
  // - When isAppliedConstraint: maxWidth and maxHeight are finite
  
  assert(isNormalized);
  return isNormalized;
}
```

## Other Constraint Types (Not in object.dart)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Other Constraint Systems                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  SliverConstraints (for scrollable content):                        │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ - scrollOffset: double                                       │   │
│  │ - precedingScrollExtent: double                              │   │
│  │ - overlap: double                                            │   │
│  │ - remainingPaintExtent: double                               │   │
│  │ - crossAxisExtent: double                                    │   │
│  │ - axisDirection: AxisDirection                               │   │
│  │ - viewportMainAxisExtent: double                             │   │
│  │ - remainingCacheExtent: double                               │   │
│  │ - cacheOrigin: double                                        │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  Custom constraints:                                                │
│  - Implement Constraints abstract class                             │
│  - Define your own layout protocol                                  │
│  - Used with custom RenderObject subclasses                         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Rust Implementation Notes

```rust
/// Abstract constraint trait
pub trait Constraints: Clone + PartialEq + Send + Sync + 'static {
    /// Whether exactly one size satisfies these constraints
    fn is_tight(&self) -> bool;
    
    /// Whether constraints are in canonical form
    fn is_normalized(&self) -> bool;
    
    /// Debug validation
    #[cfg(debug_assertions)]
    fn debug_assert_is_valid(&self, is_applied: bool) -> bool {
        assert!(self.is_normalized());
        true
    }
}

/// Box layout constraints
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BoxConstraints {
    pub min_width: f64,
    pub max_width: f64,
    pub min_height: f64,
    pub max_height: f64,
}

impl BoxConstraints {
    pub const fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }
    
    pub const fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }
    
    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.clamp(self.min_width, self.max_width),
            height: size.height.clamp(self.min_height, self.max_height),
        }
    }
    
    pub fn enforce(&self, other: &BoxConstraints) -> BoxConstraints {
        BoxConstraints {
            min_width: self.min_width.clamp(other.min_width, other.max_width),
            max_width: self.max_width.clamp(other.min_width, other.max_width),
            min_height: self.min_height.clamp(other.min_height, other.max_height),
            max_height: self.max_height.clamp(other.min_height, other.max_height),
        }
    }
}

impl Constraints for BoxConstraints {
    fn is_tight(&self) -> bool {
        (self.min_width - self.max_width).abs() < f64::EPSILON &&
        (self.min_height - self.max_height).abs() < f64::EPSILON
    }
    
    fn is_normalized(&self) -> bool {
        self.min_width <= self.max_width &&
        self.min_height <= self.max_height &&
        self.min_width >= 0.0 &&
        self.min_height >= 0.0
    }
}
```
