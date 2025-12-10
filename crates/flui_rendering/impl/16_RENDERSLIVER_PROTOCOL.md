# RenderSliver Protocol

This document describes the RenderSliver protocol - the layout system for scrollable viewport content.

## Overview

RenderSliver is an alternative to RenderBox for content inside scrolling viewports:
- **SliverConstraints** - scroll state, viewport extent, remaining space
- **SliverGeometry** - paint extent, scroll extent, cache extent

```
RenderObject (abstract base)
    │
    ├── RenderBox (2D box protocol)
    │
    └── RenderSliver (scrolling protocol)
            │
            ├── uses SliverConstraints (input)
            └── produces SliverGeometry (output)
```

## Why Slivers?

Slivers solve the "infinite list" problem:
- Can't create 10,000 RenderBox children upfront
- Need lazy creation of visible items only
- Need efficient scroll offset calculations

```
┌─────────────────────────────────────┐
│          Viewport (visible)         │
│  ┌───────────────────────────────┐  │
│  │  SliverList                   │  │
│  │  ┌─────┐ ┌─────┐ ┌─────┐     │  │  ← Only visible items materialized
│  │  │ 5   │ │ 6   │ │ 7   │     │  │
│  │  └─────┘ └─────┘ └─────┘     │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
         │
         │ Items 0-4 and 8+ don't exist yet
         ▼
```

## SliverConstraints

Input from viewport describing scroll state.

### Flutter Definition

```dart
class SliverConstraints extends Constraints {
  /// Main axis direction (down, up, right, left)
  final AxisDirection axisDirection;
  
  /// Growth direction within axis
  final GrowthDirection growthDirection;
  
  /// User scroll direction (idle, forward, reverse)
  final ScrollDirection userScrollDirection;
  
  /// Distance from viewport leading edge to sliver leading edge
  final double scrollOffset;
  
  /// Total scroll extent of previous slivers
  final double precedingScrollExtent;
  
  /// Overlap from previous sliver (pinned headers)
  final double overlap;
  
  /// Available paint space from overlap to trailing edge
  final double remainingPaintExtent;
  
  /// Cross-axis size
  final double crossAxisExtent;
  
  /// Main-axis viewport size
  final double viewportMainAxisExtent;
  
  /// Extra space to cache ahead/behind visible area
  final double remainingCacheExtent;
  final double cacheOrigin;
}
```

### Key Relationships

```
                    scrollOffset
         ◄──────────────────────────────►
         
┌────────┬─────────────────────────────────┬────────┐
│ before │         viewport                │ after  │
│ cache  │  ◄─── remainingPaintExtent ───► │ cache  │
└────────┴─────────────────────────────────┴────────┘
         ▲                                 ▲
         │                                 │
    overlap                        viewportMainAxisExtent
```

### Rust Translation

```rust
#[derive(Debug, Clone, Copy)]
pub struct SliverConstraints {
    pub axis_direction: AxisDirection,
    pub growth_direction: GrowthDirection,
    pub user_scroll_direction: ScrollDirection,
    pub scroll_offset: f64,
    pub preceding_scroll_extent: f64,
    pub overlap: f64,
    pub remaining_paint_extent: f64,
    pub cross_axis_extent: f64,
    pub viewport_main_axis_extent: f64,
    pub remaining_cache_extent: f64,
    pub cache_origin: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowthDirection {
    Forward,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Idle,
    Forward,
    Reverse,
}

impl SliverConstraints {
    /// Main axis (vertical or horizontal)
    pub fn axis(&self) -> Axis {
        match self.axis_direction {
            AxisDirection::Up | AxisDirection::Down => Axis::Vertical,
            AxisDirection::Left | AxisDirection::Right => Axis::Horizontal,
        }
    }
    
    /// Convert to BoxConstraints for child RenderBox
    pub fn as_box_constraints(&self) -> BoxConstraints {
        match self.axis() {
            Axis::Vertical => BoxConstraints {
                min_width: self.cross_axis_extent,
                max_width: self.cross_axis_extent,
                min_height: 0.0,
                max_height: f64::INFINITY,
            },
            Axis::Horizontal => BoxConstraints {
                min_width: 0.0,
                max_width: f64::INFINITY,
                min_height: self.cross_axis_extent,
                max_height: self.cross_axis_extent,
            },
        }
    }
}
```

## SliverGeometry

Output describing how the sliver occupies space.

### Flutter Definition

```dart
class SliverGeometry {
  /// Total scrollable extent
  final double scrollExtent;
  
  /// Extent currently being painted
  final double paintExtent;
  
  /// Where painting starts (usually 0)
  final double paintOrigin;
  
  /// Extent for layout (usually == paintExtent)
  final double layoutExtent;
  
  /// Maximum paint extent when fully visible
  final double maxPaintExtent;
  
  /// Extent for hit testing
  final double hitTestExtent;
  
  /// Whether sliver should be painted
  final bool visible;
  
  /// Whether content overflows bounds
  final bool hasVisualOverflow;
  
  /// Correction to apply for scroll inconsistency
  final double? scrollOffsetCorrection;
  
  /// Extent to keep in cache
  final double cacheExtent;
}
```

### Key Properties Explained

```
scrollExtent: Total size of sliver content (may be >> viewport)
paintExtent:  How much is visible now (≤ remainingPaintExtent)
layoutExtent: Space consumed for next sliver positioning

Example: 1000px list, 300px visible
  scrollExtent = 1000
  paintExtent = 300
  layoutExtent = 300
```

### Rust Translation

```rust
#[derive(Debug, Clone, Copy)]
pub struct SliverGeometry {
    pub scroll_extent: f64,
    pub paint_extent: f64,
    pub paint_origin: f64,
    pub layout_extent: f64,
    pub max_paint_extent: f64,
    pub hit_test_extent: f64,
    pub visible: bool,
    pub has_visual_overflow: bool,
    pub scroll_offset_correction: Option<f64>,
    pub cache_extent: f64,
}

impl SliverGeometry {
    pub const ZERO: Self = Self {
        scroll_extent: 0.0,
        paint_extent: 0.0,
        paint_origin: 0.0,
        layout_extent: 0.0,
        max_paint_extent: 0.0,
        hit_test_extent: 0.0,
        visible: false,
        has_visual_overflow: false,
        scroll_offset_correction: None,
        cache_extent: 0.0,
    };
    
    /// Create geometry for fully visible content
    pub fn make_visible(extent: f64) -> Self {
        Self {
            scroll_extent: extent,
            paint_extent: extent,
            paint_origin: 0.0,
            layout_extent: extent,
            max_paint_extent: extent,
            hit_test_extent: extent,
            visible: true,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent: extent,
        }
    }
}
```

## RenderSliver Trait

The core trait for scrollable content.

```rust
pub trait RenderSliver: RenderObject {
    // === Constraints & Geometry ===
    
    fn constraints(&self) -> &SliverConstraints;
    
    fn geometry(&self) -> Option<&SliverGeometry>;
    fn set_geometry(&mut self, geometry: SliverGeometry);
    
    // === Layout ===
    
    fn perform_layout(&mut self);
    
    // === Painting ===
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    // === Hit Testing ===
    
    /// Hit test at sliver coordinates (main/cross axis positions)
    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f64,
        cross_axis_position: f64,
    ) -> bool;
    
    // === Child Position Helpers ===
    
    /// Main axis position of child relative to sliver's visible leading edge
    fn child_main_axis_position(&self, child: &dyn RenderObject) -> f64 { 0.0 }
    
    /// Cross axis position of child
    fn child_cross_axis_position(&self, child: &dyn RenderObject) -> f64 { 0.0 }
    
    /// Scroll offset where child starts (for lazy lists)
    fn child_scroll_offset(&self, child: &dyn RenderObject) -> Option<f64> { None }
}
```

## Layout Helpers

Common calculations for sliver layout:

```rust
impl SliverConstraints {
    /// Calculate paint offset for content at given scroll range
    pub fn calculate_paint_offset(&self, from: f64, to: f64) -> f64 {
        assert!(from <= to);
        (to.min(self.scroll_offset + self.remaining_paint_extent) - 
         from.max(self.scroll_offset))
            .clamp(0.0, self.remaining_paint_extent)
    }
    
    /// Calculate cache offset for content at given scroll range
    pub fn calculate_cache_offset(&self, from: f64, to: f64) -> f64 {
        assert!(from <= to);
        (to.min(self.scroll_offset + self.remaining_paint_extent + self.remaining_cache_extent) -
         from.max(self.scroll_offset + self.cache_origin))
            .clamp(0.0, self.remaining_cache_extent)
    }
}
```

## Simple Sliver Implementation

A basic sliver that wraps a single RenderBox:

```rust
/// Sliver that displays a single box child
pub struct RenderSliverToBoxAdapter {
    child: Option<Box<dyn RenderBox>>,
    geometry: Option<SliverGeometry>,
    constraints: SliverConstraints,
}

impl RenderSliver for RenderSliverToBoxAdapter {
    fn perform_layout(&mut self) {
        let Some(child) = &mut self.child else {
            self.geometry = Some(SliverGeometry::ZERO);
            return;
        };
        
        // Layout child with box constraints derived from sliver constraints
        let box_constraints = self.constraints.as_box_constraints();
        child.layout(box_constraints);
        
        // Get child extent along main axis
        let child_extent = match self.constraints.axis() {
            Axis::Vertical => child.size().height,
            Axis::Horizontal => child.size().width,
        };
        
        // Calculate visible portion
        let paint_extent = self.constraints.calculate_paint_offset(0.0, child_extent);
        let cache_extent = self.constraints.calculate_cache_offset(0.0, child_extent);
        
        self.geometry = Some(SliverGeometry {
            scroll_extent: child_extent,
            paint_extent,
            layout_extent: paint_extent,
            cache_extent,
            max_paint_extent: child_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: child_extent > self.constraints.remaining_paint_extent ||
                                 self.constraints.scroll_offset > 0.0,
            scroll_offset_correction: None,
        });
    }
}
```

## Scroll Offset Correction

When lazy lists have inconsistent positions, they generate corrections:

```rust
// Dead reckoning detected item 0 not at offset 0
if first_child_scroll_offset != 0.0 && first_child_index == 0 {
    self.geometry = Some(SliverGeometry {
        scroll_offset_correction: Some(-first_child_scroll_offset),
        ..SliverGeometry::ZERO
    });
    return;
}
```

The viewport applies this correction and re-layouts.

## Sliver vs Box Comparison

| Aspect | RenderBox | RenderSliver |
|--------|-----------|--------------|
| **Input** | BoxConstraints | SliverConstraints |
| **Output** | Size | SliverGeometry |
| **Coordinates** | Fixed 2D | Scroll-relative |
| **Children** | All materialized | Lazy on demand |
| **Use Case** | Fixed content | Scrollable lists |
| **Parent** | Any RenderBox | Viewport only |

## Source Reference

Based on analysis of:
- [RenderSliver class](https://api.flutter.dev/flutter/rendering/RenderSliver-class.html)
- [SliverConstraints class](https://api.flutter.dev/flutter/rendering/SliverConstraints-class.html)
- [SliverGeometry class](https://api.flutter.dev/flutter/rendering/SliverGeometry-class.html)
