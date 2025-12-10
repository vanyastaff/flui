# Sliver System Architecture

Slivers are the core abstraction for implementing scrollable content in Flutter's rendering layer. Unlike `RenderBox` which uses Cartesian coordinates, slivers use a scroll-offset-based coordinate system.

## Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Sliver vs Box Protocol                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  RenderBox Protocol:                   Sliver Protocol:             │
│  ─────────────────                     ───────────────              │
│                                                                     │
│  Input:  BoxConstraints                Input:  SliverConstraints    │
│  Output: Size                          Output: SliverGeometry       │
│                                                                     │
│  Coordinate System:                    Coordinate System:           │
│  - Cartesian (x, y)                    - Main axis + cross axis     │
│  - Origin at top-left                  - Origin relative to scroll  │
│  - Fixed position                      - Dynamic based on scroll    │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Types

### AxisDirection & GrowthDirection

```
┌─────────────────────────────────────────────────────────────────────┐
│                    AxisDirection                                     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  AxisDirection.up      AxisDirection.down                           │
│       ▲                     │                                       │
│       │                     ▼                                       │
│  scroll offset         scroll offset                                │
│  increases             increases                                    │
│                                                                     │
│  AxisDirection.left    AxisDirection.right                          │
│  ◄────                      ────►                                   │
│  scroll offset         scroll offset                                │
│  increases             increases                                    │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                    GrowthDirection                                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  GrowthDirection.forward:                                           │
│  - Content ordered same as AxisDirection                            │
│  - Scroll offset 0 = leading edge of content                        │
│                                                                     │
│  GrowthDirection.reverse:                                           │
│  - Content ordered opposite to AxisDirection                        │
│  - Used for center-based viewports (anchor != 0)                    │
│                                                                     │
│  Example (AxisDirection.down):                                      │
│                                                                     │
│  forward:              reverse:                                     │
│  ┌────────────┐        ┌────────────┐                               │
│  │     A      │ ◄─0    │     Z      │ ◄─0                           │
│  │     B      │        │     Y      │                               │
│  │     C      │        │     X      │                               │
│  │    ...     │        │    ...     │                               │
│  │     Z      │        │     A      │                               │
│  └────────────┘        └────────────┘                               │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## SliverConstraints

The input to sliver layout - describes the current scroll state from the sliver's perspective.

```dart
class SliverConstraints extends Constraints {
  final AxisDirection axisDirection;       // Direction of scroll offset increase
  final GrowthDirection growthDirection;   // Content ordering relative to axis
  final ScrollDirection userScrollDirection; // User's current scroll direction
  
  final double scrollOffset;               // How far this sliver is scrolled
  final double precedingScrollExtent;      // Total extent of preceding slivers
  final double overlap;                    // Pixels from previous sliver overlapping
  
  final double remainingPaintExtent;       // Visible space available to paint
  final double crossAxisExtent;            // Width (for vertical scroll)
  final AxisDirection crossAxisDirection;  // LTR or RTL for horizontal cross-axis
  
  final double viewportMainAxisExtent;     // Total viewport size
  final double remainingCacheExtent;       // Cache space available
  final double cacheOrigin;                // Where cache starts (usually negative)
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│                 SliverConstraints Visualization                      │
│                 (AxisDirection.down)                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│                    ◄──── crossAxisExtent ────►                      │
│                                                                     │
│  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─   cacheOrigin        │
│                                                 (negative)          │
│  ┌─────────────────────────────────────────┐                        │
│  │         CACHE AREA (invisible)          │                        │
│  │                                         │                        │
│  ├─────────────────────────────────────────┤─ ─ scrollOffset = 0    │
│  │         VIEWPORT START                  │   (leading visible     │
│  │                                         │    edge)               │
│  │    ┌───────────────────────────┐        │                        │
│  │    │                           │        │    ▲                   │
│  │    │      VISIBLE AREA         │        │    │                   │
│  │    │                           │        │    │ remaining-        │
│  │    │                           │        │    │ PaintExtent       │
│  │    │                           │        │    │                   │
│  │    └───────────────────────────┘        │    ▼                   │
│  │                                         │                        │
│  │         VIEWPORT END                    │─ ─                     │
│  ├─────────────────────────────────────────┤                        │
│  │         CACHE AREA (invisible)          │    remainingCacheExtent│
│  │                                         │                        │
│  └─────────────────────────────────────────┘─ ─                     │
│                                                                     │
│  ◄────────── viewportMainAxisExtent ──────────►                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Key Constraint Fields

| Field | Description |
|-------|-------------|
| `scrollOffset` | How far this sliver's leading edge has scrolled past viewport start |
| `remainingPaintExtent` | Pixels left for painting (viewport bottom - current position) |
| `precedingScrollExtent` | Total scrollExtent of all previous slivers |
| `overlap` | Pixels from previous sliver that overlap into this one |
| `cacheOrigin` | Where to start caching (always ≤ 0) |
| `remainingCacheExtent` | Total cache + visible space available |

## SliverGeometry

The output of sliver layout - describes how much space this sliver occupies.

```dart
class SliverGeometry {
  final double scrollExtent;          // Total scrollable length
  final double paintExtent;           // Visible painted length
  final double paintOrigin;           // Offset from normal paint position
  final double layoutExtent;          // Space reserved for next sliver
  final double maxPaintExtent;        // If infinite paint available
  final double maxScrollObstructionExtent; // Pinned header impact
  final double hitTestExtent;         // Hit testing area
  final double cacheExtent;           // Cached (possibly invisible) length
  final double? crossAxisExtent;      // Cross-axis space (usually null)
  final bool visible;                 // Should paint?
  final bool hasVisualOverflow;       // Content clips?
  final double? scrollOffsetCorrection; // Request scroll adjustment
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│                 SliverGeometry Visualization                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Total content (scrollExtent):                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │    │
│  │ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │    │
│  └─────────────────────────────────────────────────────────────┘    │
│  ◄──────────────────── scrollExtent ─────────────────────────►      │
│                                                                     │
│  After scrollOffset of 100:                                         │
│                                                                     │
│            scrollOffset                                             │
│            ─────►│                                                  │
│                  │                                                  │
│  ┌───────────────┼─────────────────────────────────────────────┐    │
│  │ (scrolled out)│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓(visible)░░░░░░░░░░░░░░░░░░ │    │
│  │               │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓          ░░░░░░░░░░░░░░░░░░ │    │
│  └───────────────┼─────────────────────────────────────────────┘    │
│                  │                                                  │
│                  ◄────────────────►                                 │
│                     paintExtent                                     │
│                                                                     │
│                  ◄─────────────►                                    │
│                    layoutExtent                                     │
│                  (where next sliver starts)                         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Geometry Field Relationships

```
┌─────────────────────────────────────────────────────────────────────┐
│                 Extent Relationships                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Invariants:                                                        │
│  - 0 ≤ paintExtent ≤ remainingPaintExtent                          │
│  - 0 ≤ layoutExtent ≤ paintExtent                                  │
│  - paintExtent ≤ maxPaintExtent                                    │
│  - 0 ≤ hitTestExtent ≤ paintExtent (typically)                     │
│  - cacheExtent ≥ layoutExtent                                      │
│                                                                     │
│  Typical scenarios:                                                 │
│                                                                     │
│  Normal visible:                                                    │
│    paintExtent = layoutExtent = scrollExtent (if fully visible)     │
│                                                                     │
│  Partially scrolled:                                                │
│    paintExtent < scrollExtent                                       │
│    layoutExtent = paintExtent                                       │
│                                                                     │
│  Pinned header:                                                     │
│    paintExtent > 0 (always visible)                                 │
│    layoutExtent = 0 (doesn't push next sliver)                      │
│                                                                     │
│  Off-screen but cached:                                             │
│    paintExtent = 0                                                  │
│    cacheExtent > 0                                                  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## RenderSliver Base Class

```dart
abstract class RenderSliver extends RenderObject {
  // Constraints input
  SliverConstraints get constraints;
  
  // Geometry output
  SliverGeometry? get geometry;
  set geometry(SliverGeometry? value);
  
  // Layout helpers
  double calculatePaintOffset(constraints, {from, to});
  double calculateCacheOffset(constraints, {from, to});
  
  // Child positioning
  double childMainAxisPosition(RenderObject child);
  double childCrossAxisPosition(RenderObject child);
  double? childScrollOffset(RenderObject child);
  
  // Hit testing
  bool hitTest(result, {mainAxisPosition, crossAxisPosition});
  bool hitTestSelf({mainAxisPosition, crossAxisPosition});
  bool hitTestChildren(result, {mainAxisPosition, crossAxisPosition});
  
  // Special behaviors
  double get centerOffsetAdjustment;  // For center slivers
  bool get ensureSemantics;            // Force semantics even if invisible
}
```

## Parent Data Types

### SliverLogicalParentData

For slivers positioned by scroll offset:

```dart
class SliverLogicalParentData extends ParentData {
  double? layoutOffset;  // Position relative to scroll offset 0
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│            Logical Parent Data (scroll-relative)                     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  scroll offset 0                                                    │
│        │                                                            │
│        ▼                                                            │
│  ┌─────────────┐ layoutOffset = 0                                   │
│  │  Sliver A   │                                                    │
│  └─────────────┘                                                    │
│  ┌─────────────┐ layoutOffset = 100                                 │
│  │  Sliver B   │                                                    │
│  └─────────────┘                                                    │
│  ┌─────────────┐ layoutOffset = 250                                 │
│  │  Sliver C   │                                                    │
│  └─────────────┘                                                    │
│                                                                     │
│  Benefits: Fast layout - positions don't change when scrolling      │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### SliverPhysicalParentData

For slivers positioned by absolute coordinates:

```dart
class SliverPhysicalParentData extends ParentData {
  Offset paintOffset = Offset.zero;  // Absolute position
  int? crossAxisFlex;                 // For cross-axis groups
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│            Physical Parent Data (viewport-relative)                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  viewport top-left                                                  │
│        │                                                            │
│        ▼                                                            │
│  ┌─────────────────────────────────────────────┐                    │
│  │                                             │                    │
│  │    ┌─────────┐ paintOffset = (0, 0)         │                    │
│  │    │Sliver A │                              │                    │
│  │    └─────────┘                              │                    │
│  │         ┌─────────┐ paintOffset = (0, 120)  │                    │
│  │         │Sliver B │                         │                    │
│  │         └─────────┘                         │                    │
│  │                                             │                    │
│  └─────────────────────────────────────────────┘                    │
│                                                                     │
│  Used by: RenderViewport (fast painting, not layout)                │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Hit Testing

### SliverHitTestResult & SliverHitTestEntry

```dart
class SliverHitTestResult extends HitTestResult {
  bool addWithAxisOffset({
    Offset? paintOffset,
    double mainAxisOffset,
    double crossAxisOffset,
    double mainAxisPosition,
    double crossAxisPosition,
    SliverHitTest hitTest,
  });
}

class SliverHitTestEntry extends HitTestEntry<RenderSliver> {
  final double mainAxisPosition;   // Distance from leading edge
  final double crossAxisPosition;  // Distance from cross-axis origin
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│              Sliver Hit Test Coordinates                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  For AxisDirection.down:                                            │
│                                                                     │
│  crossAxisPosition                                                  │
│  ─────────────────►                                                 │
│  ┌─────────────────────────────────────────┐ mainAxisPosition       │
│  │                                         │ │                      │
│  │                                         │ │                      │
│  │              ● (hit point)              │ │                      │
│  │              │                          │ ▼                      │
│  │              │                          │                        │
│  │              │                          │                        │
│  └─────────────────────────────────────────┘                        │
│                                                                     │
│  mainAxisPosition: distance from sliver's painted leading edge      │
│  crossAxisPosition: distance from left edge of sliver               │
│                                                                     │
│  For AxisDirection.up:                                              │
│  mainAxisPosition: distance from sliver's painted BOTTOM edge       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## RenderSliverHelpers Mixin

Utility methods for slivers with RenderBox children:

```dart
mixin RenderSliverHelpers implements RenderSliver {
  // Hit test box child with coordinate transformation
  bool hitTestBoxChild(
    BoxHitTestResult result,
    RenderBox child, {
    required double mainAxisPosition,
    required double crossAxisPosition,
  });
  
  // Apply paint transform for box child
  void applyPaintTransformForBoxChild(RenderBox child, Matrix4 transform);
}
```

## RenderSliverSingleBoxAdapter

Base class for slivers containing a single RenderBox:

```dart
abstract class RenderSliverSingleBoxAdapter extends RenderSliver
    with RenderObjectWithChildMixin<RenderBox>, RenderSliverHelpers {
  
  void setupParentData(RenderObject child);
  void setChildParentData(child, constraints, geometry);
  double childMainAxisPosition(RenderBox child);
  void paint(PaintingContext context, Offset offset);
}
```

### RenderSliverToBoxAdapter

Simplest sliver-to-box adapter:

```dart
class RenderSliverToBoxAdapter extends RenderSliverSingleBoxAdapter {
  @override
  void performLayout() {
    if (child == null) {
      geometry = SliverGeometry.zero;
      return;
    }
    
    // Layout child with box constraints derived from sliver constraints
    child!.layout(constraints.asBoxConstraints(), parentUsesSize: true);
    
    // Get child's extent in main axis
    final childExtent = constraints.axis == Axis.horizontal
        ? child!.size.width
        : child!.size.height;
    
    // Calculate how much is visible
    final paintedChildSize = calculatePaintOffset(
      constraints,
      from: 0.0,
      to: childExtent,
    );
    
    geometry = SliverGeometry(
      scrollExtent: childExtent,
      paintExtent: paintedChildSize,
      maxPaintExtent: childExtent,
      hitTestExtent: paintedChildSize,
      hasVisualOverflow: childExtent > constraints.remainingPaintExtent ||
                         constraints.scrollOffset > 0.0,
    );
    
    setChildParentData(child!, constraints, geometry!);
  }
}
```

## Layout Flow Example

```
┌─────────────────────────────────────────────────────────────────────┐
│                 Viewport Layout with Slivers                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  RenderViewport.performLayout():                                    │
│                                                                     │
│  for each sliver:                                                   │
│    1. Create SliverConstraints:                                     │
│       - scrollOffset = viewport.offset - sliver's logical position  │
│       - remainingPaintExtent = viewport end - current position      │
│       - remainingCacheExtent = cache end - current position         │
│                                                                     │
│    2. Layout sliver:                                                │
│       sliver.layout(constraints)                                    │
│                                                                     │
│    3. Read geometry:                                                │
│       - Advance by layoutExtent for next sliver                     │
│       - Record paintExtent for painting                             │
│       - Handle scrollOffsetCorrection if set                        │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │ Sliver 1: scrollOffset=0, remaining=600                    │     │
│  │           → geometry: scroll=100, paint=100, layout=100    │     │
│  ├────────────────────────────────────────────────────────────┤     │
│  │ Sliver 2: scrollOffset=0, remaining=500                    │     │
│  │           → geometry: scroll=200, paint=200, layout=200    │     │
│  ├────────────────────────────────────────────────────────────┤     │
│  │ Sliver 3: scrollOffset=0, remaining=300                    │     │
│  │           → geometry: scroll=150, paint=150, layout=150    │     │
│  ├────────────────────────────────────────────────────────────┤     │
│  │ Sliver 4: scrollOffset=0, remaining=150                    │     │
│  │           → geometry: scroll=400, paint=150, layout=150    │     │
│  │           (only 150 visible, 250 scrolled off bottom)      │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Rust Implementation Notes

```rust
/// Growth direction relative to axis direction
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrowthDirection {
    Forward,
    Reverse,
}

/// Sliver layout constraints
#[derive(Clone, Debug)]
pub struct SliverConstraints {
    pub axis_direction: AxisDirection,
    pub growth_direction: GrowthDirection,
    pub user_scroll_direction: ScrollDirection,
    
    pub scroll_offset: f64,
    pub preceding_scroll_extent: f64,
    pub overlap: f64,
    
    pub remaining_paint_extent: f64,
    pub cross_axis_extent: f64,
    pub cross_axis_direction: AxisDirection,
    
    pub viewport_main_axis_extent: f64,
    pub remaining_cache_extent: f64,
    pub cache_origin: f64,
}

impl Constraints for SliverConstraints {
    fn is_tight(&self) -> bool {
        false  // Sliver constraints are never tight
    }
    
    fn is_normalized(&self) -> bool {
        self.scroll_offset >= 0.0 &&
        self.cross_axis_extent >= 0.0 &&
        self.viewport_main_axis_extent >= 0.0 &&
        self.remaining_paint_extent >= 0.0
    }
}

/// Sliver layout output
#[derive(Clone, Debug, Default)]
pub struct SliverGeometry {
    pub scroll_extent: f64,
    pub paint_extent: f64,
    pub paint_origin: f64,
    pub layout_extent: f64,
    pub max_paint_extent: f64,
    pub max_scroll_obstruction_extent: f64,
    pub hit_test_extent: f64,
    pub cache_extent: f64,
    pub cross_axis_extent: Option<f64>,
    pub visible: bool,
    pub has_visual_overflow: bool,
    pub scroll_offset_correction: Option<f64>,
}

impl SliverGeometry {
    pub const ZERO: Self = Self {
        scroll_extent: 0.0,
        paint_extent: 0.0,
        paint_origin: 0.0,
        layout_extent: 0.0,
        max_paint_extent: 0.0,
        max_scroll_obstruction_extent: 0.0,
        hit_test_extent: 0.0,
        cache_extent: 0.0,
        cross_axis_extent: None,
        visible: false,
        has_visual_overflow: false,
        scroll_offset_correction: None,
    };
}

/// Base trait for sliver render objects
pub trait RenderSliver: RenderObject {
    fn constraints(&self) -> &SliverConstraints;
    fn geometry(&self) -> Option<&SliverGeometry>;
    fn set_geometry(&mut self, geometry: SliverGeometry);
    
    fn calculate_paint_offset(&self, from: f64, to: f64) -> f64;
    fn calculate_cache_offset(&self, from: f64, to: f64) -> f64;
    
    fn child_main_axis_position(&self, child: &dyn RenderObject) -> f64;
    fn child_cross_axis_position(&self, child: &dyn RenderObject) -> f64;
    fn child_scroll_offset(&self, child: &dyn RenderObject) -> Option<f64>;
    
    fn hit_test_sliver(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f64,
        cross_axis_position: f64,
    ) -> bool;
}
```
