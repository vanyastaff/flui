# Flutter Parent Data Pattern

This document details Flutter's ParentData mechanism for parent-child communication.

## Overview

ParentData is a **parent-owned storage slot on children** that allows parents to associate layout-specific data with each child without the child knowing the parent's type.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│                     Parent Data Flow                                    │
│                                                                        │
│  ┌──────────────┐                    ┌──────────────┐                  │
│  │   Parent     │   setupParentData  │    Child     │                  │
│  │ (RenderFlex) │ ─────────────────► │ (RenderBox)  │                  │
│  │              │                    │              │                  │
│  │   layout()   │                    │ parentData   │◄─┐               │
│  │     │        │                    │  FlexPData   │  │               │
│  │     ▼        │                    └──────────────┘  │               │
│  │ child.offset │                                       │               │
│  │ child.flex   │ ◄──── reads/writes ───────────────────┘               │
│  └──────────────┘                                                       │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

## Core Classes

### ParentData (Base)

```dart
/// Data stored on a RenderObject by its parent.
class ParentData {
  /// Called when the RenderObject is removed from the tree.
  @protected
  @mustCallSuper
  void detach() {}
}
```

### BoxParentData

```dart
/// ParentData for RenderBox children, storing offset.
class BoxParentData extends ParentData {
  /// The offset at which to paint the child.
  Offset offset = Offset.zero;
}
```

### ContainerBoxParentData

```dart
/// BoxParentData with next/previous sibling links for efficient iteration.
class ContainerBoxParentData<ChildType extends RenderObject> 
    extends BoxParentData {
  
  /// The previous sibling in the parent's child list.
  ChildType? previousSibling;
  
  /// The next sibling in the parent's child list.  
  ChildType? nextSibling;
}
```

### FlexParentData

```dart
/// ParentData for children in a Flex (Row/Column).
class FlexParentData extends ContainerBoxParentData<RenderBox> {
  /// The flex factor for this child.
  int? flex;
  
  /// How a flexible child is inscribed into available space.
  FlexFit fit = FlexFit.tight;
}
```

### StackParentData

```dart
/// ParentData for children in a Stack.
class StackParentData extends ContainerBoxParentData<RenderBox> {
  /// Distance from top edge.
  double? top;
  double? right;
  double? bottom;
  double? left;
  
  /// Width constraint.
  double? width;
  double? height;
  
  bool get isPositioned => top != null || right != null || 
                           bottom != null || left != null ||
                           width != null || height != null;
}
```

## Parent Setup Protocol

### setupParentData

Parents override to ensure correct ParentData type:

```dart
// In parent (e.g., RenderFlex):
@override
void setupParentData(RenderObject child) {
  if (child.parentData is! FlexParentData) {
    child.parentData = FlexParentData();
  }
}
```

### When setupParentData is Called

```dart
// In ContainerRenderObjectMixin:
void adoptChild(RenderObject child) {
  setupParentData(child);  // ◄─── Called here
  super.adoptChild(child);
}
```

## Usage Patterns

### During Layout

```dart
// Parent reads/writes child's parentData:
@override
void performLayout() {
  double offset = 0.0;
  
  RenderBox? child = firstChild;
  while (child != null) {
    final FlexParentData childParentData = child.parentData as FlexParentData;
    
    // Read flex factor
    final int flex = childParentData.flex ?? 0;
    
    // Layout child
    child.layout(constraints, parentUsesSize: true);
    
    // Write position
    childParentData.offset = Offset(offset, 0);
    offset += child.size.width;
    
    // Move to next sibling via linked list
    child = childParentData.nextSibling;
  }
}
```

### During Paint

```dart
@override
void paint(PaintingContext context, Offset offset) {
  RenderBox? child = firstChild;
  while (child != null) {
    final BoxParentData childParentData = child.parentData as BoxParentData;
    
    // Use stored offset
    context.paintChild(child, offset + childParentData.offset);
    
    child = (childParentData as ContainerBoxParentData).nextSibling;
  }
}
```

### During Hit Testing

```dart
@override
bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
  RenderBox? child = lastChild;  // Front to back
  while (child != null) {
    final BoxParentData childParentData = child.parentData as BoxParentData;
    
    final bool isHit = result.addWithPaintOffset(
      offset: childParentData.offset,
      position: position,
      hitTest: (result, transformed) => child!.hitTest(result, position: transformed),
    );
    
    if (isHit) return true;
    
    child = (childParentData as ContainerBoxParentData).previousSibling;
  }
  return false;
}
```

## Widget-Side Integration

### Positioned Widget

```dart
// Widgets can update parent data via ParentDataWidget:
class Positioned extends ParentDataWidget<StackParentData> {
  final double? left, top, right, bottom, width, height;
  
  @override
  void applyParentData(RenderObject renderObject) {
    final StackParentData parentData = renderObject.parentData as StackParentData;
    
    bool needsLayout = false;
    
    if (parentData.left != left) {
      parentData.left = left;
      needsLayout = true;
    }
    // ... similar for other properties
    
    if (needsLayout) {
      final AbstractNode? targetParent = renderObject.parent;
      if (targetParent is RenderObject) {
        targetParent.markNeedsLayout();
      }
    }
  }
}
```

### Flexible Widget

```dart
class Flexible extends ParentDataWidget<FlexParentData> {
  final int flex;
  final FlexFit fit;
  
  @override
  void applyParentData(RenderObject renderObject) {
    final FlexParentData parentData = renderObject.parentData as FlexParentData;
    
    bool needsLayout = false;
    
    if (parentData.flex != flex) {
      parentData.flex = flex;
      needsLayout = true;
    }
    
    if (parentData.fit != fit) {
      parentData.fit = fit;
      needsLayout = true;
    }
    
    if (needsLayout) {
      final AbstractNode? targetParent = renderObject.parent;
      if (targetParent is RenderObject) {
        targetParent.markNeedsLayout();
      }
    }
  }
}
```

## FLUI Implementation

### Base Trait

```rust
/// Parent data stored on children for layout information.
pub trait ParentData: std::any::Any + Send + Sync + std::fmt::Debug {
    /// Called when the child is removed from the tree.
    fn detach(&mut self) {}
    
    /// For downcasting
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
```

### BoxParentData

```rust
/// Basic parent data storing child offset.
#[derive(Debug, Clone, Default)]
pub struct BoxParentData {
    /// Offset at which to paint the child.
    pub offset: Offset,
}

impl ParentData for BoxParentData {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}
```

### ContainerParentData

```rust
/// Parent data with sibling links for efficient iteration.
#[derive(Debug)]
pub struct ContainerBoxParentData<Id> {
    /// The offset at which to paint the child.
    pub offset: Offset,
    
    /// Previous sibling in parent's child list.
    pub previous_sibling: Option<Id>,
    
    /// Next sibling in parent's child list.
    pub next_sibling: Option<Id>,
}
```

### FlexParentData

```rust
#[derive(Debug)]
pub struct FlexParentData<Id> {
    /// Base container data
    pub base: ContainerBoxParentData<Id>,
    
    /// Flex factor for this child
    pub flex: Option<u32>,
    
    /// How flexible child is inscribed
    pub fit: FlexFit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexFit {
    #[default]
    Tight,
    Loose,
}
```

### Type-Safe Access Pattern

```rust
/// Trait for render objects that need specific parent data.
pub trait RequiresParentData {
    type ParentData: ParentData;
}

/// Trait for parents that provide specific parent data.
pub trait ProvidesParentData {
    type ParentData: ParentData + Default;
    
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        // Set up parent data if not already correct type
    }
}
```

### Downcast Helper

```rust
impl dyn ParentData {
    /// Safely downcast to concrete type.
    pub fn downcast_ref<T: ParentData + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
    
    pub fn downcast_mut<T: ParentData + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}
```

### Usage in Layout

```rust
impl RenderFlex {
    fn perform_layout(&mut self, ctx: &mut LayoutContext) {
        let mut offset = 0.0;
        
        for child_id in self.children.iter() {
            // Get child's parent data
            let parent_data = ctx.parent_data_mut::<FlexParentData>(child_id)?;
            
            // Read flex factor
            let flex = parent_data.flex.unwrap_or(0);
            
            // Layout child
            let child_size = ctx.layout_child(child_id, constraints)?;
            
            // Write position
            parent_data.base.offset = Offset::new(offset, 0.0);
            offset += child_size.width;
        }
        
        Ok(Size::new(offset, max_height))
    }
}
```

## Key Design Principles

1. **Parent owns the data**: Parent decides what data to store on children
2. **Child is agnostic**: Child doesn't know/care what parent stores
3. **Type safety via downcasting**: Runtime type check when accessing
4. **Linked list for efficiency**: Sibling links avoid Vec iteration overhead
5. **Widget-side updates**: ParentDataWidget updates data, marks parent dirty

## Sources

- [RenderObject class - Flutter API](https://api.flutter.dev/flutter/rendering/RenderObject-class.html)
- [ParentData class](https://api.flutter.dev/flutter/rendering/ParentData-class.html)
- [BoxParentData class](https://api.flutter.dev/flutter/rendering/BoxParentData-class.html)
- [FlexParentData class](https://api.flutter.dev/flutter/rendering/FlexParentData-class.html)
