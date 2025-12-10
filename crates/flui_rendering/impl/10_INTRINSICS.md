# Flutter Intrinsic Sizing Protocol

This document details Flutter's intrinsic sizing mechanism in RenderBox.

## Overview

Intrinsic dimensions allow a parent to query the natural size of a child without performing full layout. This is crucial for:

- Text-based layouts (wrap width)
- Table column sizing
- Baseline alignment
- Aspect ratio calculations

## The Four Intrinsic Methods

```dart
// Minimum width to paint without clipping, given height
double getMinIntrinsicWidth(double height)

// Width beyond which more width doesn't reduce height
double getMaxIntrinsicWidth(double height)

// Minimum height to paint without clipping, given width
double getMinIntrinsicHeight(double width)

// Height beyond which more height doesn't reduce width  
double getMaxIntrinsicHeight(double width)
```

## Method Semantics

### getMinIntrinsicWidth(height)

Returns the **smallest width** that allows the box to paint correctly:

```
┌─────────────────────┐
│ This is a long text │  ← Full width: all text on one line
│ that wraps.         │
└─────────────────────┘

┌──────────┐
│ This is  │  ← Min intrinsic width: narrowest that doesn't clip
│ a long   │     any word
│ text     │
│ that     │
│ wraps.   │
└──────────┘
```

For text: typically the width of the longest word.

### getMaxIntrinsicWidth(height)

Returns the **smallest width beyond which increasing width doesn't decrease height**:

```
┌─────────────────────────────────────┐
│ This is a long text that wraps.     │  ← Max intrinsic: single line
└─────────────────────────────────────┘
     ↑
     Making this wider doesn't change height
```

For text: the width needed to display all text on one line.

### getMinIntrinsicHeight(width) / getMaxIntrinsicHeight(width)

Analogous to width methods but for height given a fixed width.

## Implementation Pattern

Flutter separates public API from computation:

```dart
// Public API (with caching)
double getMinIntrinsicWidth(double height) {
  return _computeIntrinsicDimension(
    _IntrinsicDimension.minWidth,
    height,
    computeMinIntrinsicWidth,
  );
}

// Compute method (subclass overrides this)
@protected
double computeMinIntrinsicWidth(double height) {
  return 0.0; // Default: no minimum width
}

// Caching implementation
double _computeIntrinsicDimension(
  _IntrinsicDimension dimension,
  double argument,
  double Function(double) computer,
) {
  // Check cache
  final cached = _cachedIntrinsicDimensions?[dimension][argument];
  if (cached != null) return cached;
  
  // Compute and cache
  final result = computer(argument);
  _cachedIntrinsicDimensions ??= {};
  _cachedIntrinsicDimensions![dimension][argument] = result;
  return result;
}
```

## Common Implementation Patterns

### Leaf Node (e.g., RenderImage)

```dart
@override
double computeMinIntrinsicWidth(double height) {
  if (_width == null && _height == null) return 0.0;
  return _sizeForConstraints(BoxConstraints.tightForFinite(height: height)).width;
}

@override
double computeMaxIntrinsicWidth(double height) {
  return computeMinIntrinsicWidth(height); // Image has fixed aspect ratio
}
```

### Single Child Container (e.g., RenderPadding)

```dart
@override
double computeMinIntrinsicWidth(double height) {
  if (child != null) {
    return child!.getMinIntrinsicWidth(max(0.0, height - _padding.vertical))
           + _padding.horizontal;
  }
  return _padding.horizontal;
}
```

### Multi-Child Container (e.g., RenderFlex - Row/Column)

```dart
// For Row (horizontal flex):
@override
double computeMinIntrinsicWidth(double height) {
  double result = 0.0;
  for (final child in children) {
    result += child.getMinIntrinsicWidth(height);
  }
  return result;
}

@override  
double computeMaxIntrinsicWidth(double height) {
  double result = 0.0;
  for (final child in children) {
    result += child.getMaxIntrinsicWidth(height);
  }
  return result;
}
```

### Text (RenderParagraph)

```dart
@override
double computeMinIntrinsicWidth(double height) {
  _layoutTextWithConstraints(BoxConstraints(maxWidth: double.infinity));
  return _textPainter.minIntrinsicWidth;
}

@override
double computeMaxIntrinsicWidth(double height) {
  _layoutTextWithConstraints(BoxConstraints(maxWidth: double.infinity));
  return _textPainter.maxIntrinsicWidth;
}
```

## Baseline Computation

Related to intrinsics, baseline provides text alignment info:

```dart
double? getDistanceToBaseline(TextBaseline baseline) {
  // Check cache
  if (_cachedBaselines?[baseline] case final double result) {
    return result;
  }
  
  // Compute
  final result = computeDistanceToActualBaseline(baseline);
  
  // Cache
  _cachedBaselines ??= {};
  _cachedBaselines![baseline] = result;
  
  return result;
}

@protected
double? computeDistanceToActualBaseline(TextBaseline baseline) {
  // Default: no baseline
  return null;
}
```

## Cache Invalidation

Intrinsic caches are cleared when layout is invalidated:

```dart
void markNeedsLayout() {
  // ... existing code ...
  
  // Clear intrinsic dimension cache
  _cachedIntrinsicDimensions = null;
  _cachedBaselines = null;
}
```

## Performance Considerations

### Cost of Intrinsic Queries

Intrinsic dimensions can be **expensive** because they may require:
1. Full text layout
2. Recursive child queries
3. Multiple iterations for complex layouts

```
⚠️ Warning: Intrinsic dimensions cause O(N²) layout in worst case!

Example: Table with intrinsic columns
- Each column queries all cells for intrinsics
- Each cell queries its children
- N columns × M rows × child depth = expensive!
```

### When to Use

**Good uses:**
- Table column sizing
- Wrap/shrink-wrap layouts
- Baseline alignment
- Single-line text measurement

**Avoid when:**
- Layout can be computed without intrinsics
- Inside frequently rebuilt widgets
- Deep widget trees with intrinsic queries

### Dry Layout Alternative

Flutter 3.0+ introduced "dry layout" as a more efficient alternative:

```dart
Size getDryLayout(BoxConstraints constraints) {
  // Compute layout size WITHOUT actually laying out
  return computeDryLayout(constraints);
}

@protected
Size computeDryLayout(BoxConstraints constraints) {
  // Override to provide efficient size computation
  // Default falls back to intrinsics
}
```

## FLUI Implementation Considerations

### Trait Design

```rust
/// Intrinsic sizing protocol
pub trait IntrinsicSize {
    /// Minimum width to paint without clipping at given height
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_min_intrinsic_width(height)
    }
    
    /// Maximum useful width at given height
    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_max_intrinsic_width(height)
    }
    
    /// Minimum height to paint without clipping at given width
    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_min_intrinsic_height(width)
    }
    
    /// Maximum useful height at given width
    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }
    
    // Protected compute methods
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
}

/// Baseline computation
pub trait Baseline {
    fn distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_baseline(baseline)
    }
    
    fn compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        None
    }
}
```

### Caching Strategy

```rust
/// Cached intrinsic dimensions
#[derive(Default)]
pub struct IntrinsicCache {
    min_width: HashMap<OrderedFloat<f32>, f32>,
    max_width: HashMap<OrderedFloat<f32>, f32>,
    min_height: HashMap<OrderedFloat<f32>, f32>,
    max_height: HashMap<OrderedFloat<f32>, f32>,
    baselines: HashMap<TextBaseline, Option<f32>>,
}

impl IntrinsicCache {
    pub fn clear(&mut self) {
        self.min_width.clear();
        self.max_width.clear();
        self.min_height.clear();
        self.max_height.clear();
        self.baselines.clear();
    }
    
    pub fn get_min_width(&mut self, height: f32, compute: impl FnOnce() -> f32) -> f32 {
        let key = OrderedFloat(height);
        *self.min_width.entry(key).or_insert_with(compute)
    }
    
    // ... similar for other dimensions
}
```

### Dry Layout Integration

```rust
/// Dry layout for efficient size computation
pub trait DryLayout {
    /// Compute size without performing layout
    fn dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(constraints)
    }
    
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size;
}
```

## Sources

- [RenderBox class - Flutter API](https://api.flutter.dev/flutter/rendering/RenderBox-class.html)
- [getMinIntrinsicWidth method](https://api.flutter.dev/flutter/rendering/RenderBox/getMinIntrinsicWidth.html)
- [getDryLayout method](https://api.flutter.dev/flutter/rendering/RenderBox/getDryLayout.html)
