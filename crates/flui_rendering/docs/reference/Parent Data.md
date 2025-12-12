# Parent Data

**Child metadata types for layout and rendering**

---

## Overview

Parent data is metadata attached to children by their parent render object. It stores information that only the parent needs to know about each child, such as positions, flex factors, or table cell coordinates.

---

## Parent Data Trait

```rust
pub trait ParentData: Debug + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    
    fn detach(&mut self) {
        // Called when child is removed
    }
}
```

---

## Box Protocol Parent Data (9 types)

### 1. BoxParentData

Base parent data for box children - stores only offset.

```rust
#[derive(Debug, Clone)]
pub struct BoxParentData {
    pub offset: Offset,
}

impl ParentData for BoxParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }
}
```

**Used by:** RenderPadding, RenderAlign, most single-child boxes

---

### 2. FlexParentData

Parent data for flex children (Row/Column).

```rust
#[derive(Debug, Clone)]
pub struct FlexParentData {
    pub offset: Offset,
    pub flex: Option<u32>,
    pub fit: FlexFit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexFit {
    Tight,   // Must fill allocated space
    Loose,   // Can be smaller than allocated space
}

impl ParentData for FlexParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            flex: None,
            fit: FlexFit::Tight,
        }
    }
}
```

**Used by:** RenderFlex

---

### 3. StackParentData

Parent data for stack children with positioning.

```rust
#[derive(Debug, Clone)]
pub struct StackParentData {
    pub offset: Offset,
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub left: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl ParentData for StackParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for StackParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            top: None,
            right: None,
            bottom: None,
            left: None,
            width: None,
            height: None,
        }
    }
}
```

**Used by:** RenderStack, RenderIndexedStack

---

### 4. WrapParentData

Parent data for wrap layout children.

```rust
#[derive(Debug, Clone)]
pub struct WrapParentData {
    pub offset: Offset,
    // Wrap doesn't need additional data - just tracks offset
}

impl ParentData for WrapParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for WrapParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }
}
```

**Used by:** RenderWrap

---

### 5. FlowParentData

Parent data for flow layout with transform matrix.

```rust
#[derive(Debug, Clone)]
pub struct FlowParentData {
    pub offset: Offset,
    pub transform: Option<Matrix4>,
}

impl ParentData for FlowParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for FlowParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            transform: None,
        }
    }
}
```

**Used by:** RenderFlow

---

### 6. ListBodyParentData

Parent data for list body children.

```rust
#[derive(Debug, Clone)]
pub struct ListBodyParentData {
    pub offset: Offset,
}

impl ParentData for ListBodyParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for ListBodyParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }
}
```

**Used by:** RenderListBody

---

### 7. TableCellParentData

Parent data for table cells with row/column coordinates.

```rust
#[derive(Debug, Clone)]
pub struct TableCellParentData {
    pub offset: Offset,
    pub x: usize,  // Column index
    pub y: usize,  // Row index
    pub vertical_alignment: TableCellVerticalAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCellVerticalAlignment {
    Top,
    Middle,
    Bottom,
    Baseline,
    Fill,
}

impl ParentData for TableCellParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for TableCellParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            x: 0,
            y: 0,
            vertical_alignment: TableCellVerticalAlignment::Top,
        }
    }
}
```

**Used by:** RenderTable

---

### 8. MultiChildLayoutParentData

Parent data for custom multi-child layout with ID.

```rust
#[derive(Debug, Clone)]
pub struct MultiChildLayoutParentData {
    pub offset: Offset,
    pub id: Option<String>,  // Child identifier for delegate
}

impl ParentData for MultiChildLayoutParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for MultiChildLayoutParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            id: None,
        }
    }
}
```

**Used by:** RenderCustomMultiChildLayoutBox

---

### 9. ListWheelParentData

Parent data for list wheel viewport children.

```rust
#[derive(Debug, Clone)]
pub struct ListWheelParentData {
    pub offset: Offset,
    pub index: usize,
}

impl ParentData for ListWheelParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for ListWheelParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
            index: 0,
        }
    }
}
```

**Used by:** RenderListWheelViewport

---

## Sliver Protocol Parent Data (6 types)

### 10. SliverParentData

Base parent data for sliver children - stores paint offset.

```rust
#[derive(Debug, Clone)]
pub struct SliverParentData {
    pub paint_offset: Offset,
}

impl ParentData for SliverParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for SliverParentData {
    fn default() -> Self {
        Self {
            paint_offset: Offset::ZERO,
        }
    }
}
```

**Used by:** RenderSliverPadding

---

### 11. SliverLogicalParentData

Parent data for nested sliver children with logical offset.

```rust
#[derive(Debug, Clone)]
pub struct SliverLogicalParentData {
    pub layout_offset: f32,  // Logical position along scroll axis
}

impl ParentData for SliverLogicalParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for SliverLogicalParentData {
    fn default() -> Self {
        Self {
            layout_offset: 0.0,
        }
    }
}
```

**Used by:** RenderSliverMainAxisGroup, RenderShrinkWrappingViewport

---

### 12. SliverPhysicalParentData

Parent data for sliver children with physical paint offset.

```rust
#[derive(Debug, Clone)]
pub struct SliverPhysicalParentData {
    pub paint_offset: Offset,
}

impl ParentData for SliverPhysicalParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl Default for SliverPhysicalParentData {
    fn default() -> Self {
        Self {
            paint_offset: Offset::ZERO,
        }
    }
}
```

**Used by:** RenderViewport, RenderSliverCrossAxisGroup

---

### 13. SliverMultiBoxAdaptorParentData

Parent data for box children in sliver lists/grids.

```rust
#[derive(Debug, Clone)]
pub struct SliverMultiBoxAdaptorParentData {
    pub paint_offset: Offset,
    pub index: usize,
    pub keep_alive: bool,      // Should keep child alive when off-screen
    pub kept_alive: bool,      // Is child currently kept alive
}

impl ParentData for SliverMultiBoxAdaptorParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    
    fn detach(&mut self) {
        self.kept_alive = false;
    }
}

impl Default for SliverMultiBoxAdaptorParentData {
    fn default() -> Self {
        Self {
            paint_offset: Offset::ZERO,
            index: 0,
            keep_alive: false,
            kept_alive: false,
        }
    }
}
```

**Used by:** RenderSliverList, RenderSliverFixedExtentList, RenderSliverFillViewport, RenderSliverVariedExtentList

---

### 14. SliverGridParentData

Parent data for grid cells with cross-axis offset.

```rust
#[derive(Debug, Clone)]
pub struct SliverGridParentData {
    pub paint_offset: Offset,
    pub index: usize,
    pub cross_axis_offset: f32,  // Position across cross axis
    pub keep_alive: bool,
    pub kept_alive: bool,
}

impl ParentData for SliverGridParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    
    fn detach(&mut self) {
        self.kept_alive = false;
    }
}

impl Default for SliverGridParentData {
    fn default() -> Self {
        Self {
            paint_offset: Offset::ZERO,
            index: 0,
            cross_axis_offset: 0.0,
            keep_alive: false,
            kept_alive: false,
        }
    }
}
```

**Used by:** RenderSliverGrid

---

### 15. TreeSliverNodeParentData

Parent data for tree structure list items with depth.

```rust
#[derive(Debug, Clone)]
pub struct TreeSliverNodeParentData {
    pub paint_offset: Offset,
    pub index: usize,
    pub depth: usize,  // Tree depth level
    pub keep_alive: bool,
    pub kept_alive: bool,
}

impl ParentData for TreeSliverNodeParentData {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    
    fn detach(&mut self) {
        self.kept_alive = false;
    }
}

impl Default for TreeSliverNodeParentData {
    fn default() -> Self {
        Self {
            paint_offset: Offset::ZERO,
            index: 0,
            depth: 0,
            keep_alive: false,
            kept_alive: false,
        }
    }
}
```

**Used by:** RenderTreeSliver

---

## Parent Data Access

### Setting Parent Data Type

```rust
impl RenderObject {
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        // Override in each render object to set correct parent data type
        if child.parent_data::<MyParentData>().is_err() {
            child.set_parent_data(Box::new(MyParentData::default()));
        }
    }
}
```

### Accessing Parent Data

```rust
impl RenderObject {
    fn parent_data<T: ParentData>(&self) -> &T {
        self.parent_data_raw()
            .as_any()
            .downcast_ref::<T>()
            .expect("Wrong parent data type")
    }
    
    fn parent_data_mut<T: ParentData>(&mut self) -> &mut T {
        self.parent_data_raw_mut()
            .as_any_mut()
            .downcast_mut::<T>()
            .expect("Wrong parent data type")
    }
}
```

### Usage Example

```rust
impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        for child in self.children.iter_mut() {
            // Access parent data
            let parent_data = child.parent_data::<FlexParentData>();
            
            if let Some(flex) = parent_data.flex {
                // Layout flexible child
            }
            
            // Update parent data
            let parent_data = child.parent_data_mut::<FlexParentData>();
            parent_data.offset = Offset::new(x, y);
        }
        
        Size::new(width, height)
    }
}
```

---

## Parent Data Categories

| Category | Types | Common Fields |
|----------|-------|---------------|
| **Basic** | BoxParentData, SliverParentData | offset/paint_offset |
| **Positioning** | StackParentData, TableCellParentData | top, left, x, y |
| **Flex** | FlexParentData, WrapParentData | flex, fit |
| **Indexed** | ListWheelParentData, SliverMultiBoxAdaptorParentData | index |
| **Transform** | FlowParentData | transform matrix |
| **Logical** | SliverLogicalParentData | layout_offset |
| **Keep-Alive** | Sliver*ParentData | keep_alive, kept_alive |

---

## Keep-Alive Mechanism

Sliver parent data types support keep-alive for off-screen children:

```rust
impl RenderSliverList {
    fn collect_garbage(&mut self) {
        for child in self.children.iter_mut() {
            let parent_data = child.parent_data::<SliverMultiBoxAdaptorParentData>();
            
            if parent_data.keep_alive {
                // Keep child alive even when off-screen
                parent_data.kept_alive = true;
            } else if parent_data.kept_alive {
                // Was kept alive but no longer needed
                parent_data.kept_alive = false;
                // Remove child
            }
        }
    }
}
```

---

## File Organization

```
flui-rendering/src/parent_data/
├── mod.rs
├── box_parent_data.rs           # 1. BoxParentData
├── flex_parent_data.rs          # 2. FlexParentData
├── stack_parent_data.rs         # 3. StackParentData
├── wrap_parent_data.rs          # 4. WrapParentData
├── flow_parent_data.rs          # 5. FlowParentData
├── list_body_parent_data.rs    # 6. ListBodyParentData
├── table_cell_parent_data.rs   # 7. TableCellParentData
├── multi_child_layout_parent_data.rs  # 8. MultiChildLayoutParentData
├── list_wheel_parent_data.rs   # 9. ListWheelParentData
├── sliver_parent_data.rs       # 10. SliverParentData
├── sliver_logical_parent_data.rs  # 11. SliverLogicalParentData
├── sliver_physical_parent_data.rs # 12. SliverPhysicalParentData
├── sliver_multi_box_adaptor_parent_data.rs  # 13. SliverMultiBoxAdaptorParentData
├── sliver_grid_parent_data.rs  # 14. SliverGridParentData
└── tree_sliver_node_parent_data.rs  # 15. TreeSliverNodeParentData
```

---

## Summary

| Protocol | Parent Data Types | Total |
|----------|------------------|-------|
| **Box** | 9 | BoxParentData, FlexParentData, StackParentData, WrapParentData, FlowParentData, ListBodyParentData, TableCellParentData, MultiChildLayoutParentData, ListWheelParentData |
| **Sliver** | 6 | SliverParentData, SliverLogicalParentData, SliverPhysicalParentData, SliverMultiBoxAdaptorParentData, SliverGridParentData, TreeSliverNodeParentData |
| **Total** | **15** | |

---

## Next Steps

- [[Object Catalog]] - Which objects use which parent data
- [[Protocol]] - How parent data integrates with protocols
- [[Implementation Guide]] - Setting up parent data in custom objects

---

**See Also:**
- [[Containers]] - How containers interact with parent data
- [[Trait Hierarchy]] - Parent data access methods
