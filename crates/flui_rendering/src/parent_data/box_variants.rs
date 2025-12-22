//! Box protocol parent data variants - Specialized types for container layouts.

use flui_types::{Matrix4, Offset};
use std::hash::{Hash, Hasher};

use super::base::ParentData;

use super::container_mixin::ContainerParentDataMixin;

// Re-export RenderId for convenience
use flui_foundation::RenderId;

// ============================================================================
// CONTAINER BOX PARENT DATA (Base for containers)
// ============================================================================

/// Base parent data for box containers with linked list support.
///
/// Combines `BoxParentData` (offset) with `ContainerParentDataMixin`
/// (sibling pointers) for efficient container operations.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerBoxParentData {
    /// Offset from parent's top-left corner.
    pub offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,
}

impl ContainerBoxParentData {
    /// Create with specific offset.
    pub const fn new(offset: Offset) -> Self {
        Self {
            offset,
            container: ContainerParentDataMixin::new(),
        }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Builder: set offset.
    pub const fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Check if at origin.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.offset == Offset::ZERO
    }

    /// Reset to origin.
    pub fn reset(&mut self) {
        self.offset = Offset::ZERO;
    }
}

impl Default for ContainerBoxParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for ContainerBoxParentData {}

impl Hash for ContainerBoxParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.container.hash(state);
    }
}

impl Eq for ContainerBoxParentData {}

// ============================================================================
// FLEX PARENT DATA
// ============================================================================

/// Parent data for flex layouts (Row, Column).
///
/// Stores flex factor and fit mode for flexible children.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexParentData {
    /// Offset from parent's top-left corner.
    pub offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,

    /// Flex factor (how much space to take relative to siblings).
    ///
    /// - `None` = inflexible (use intrinsic size)
    /// - `Some(n)` = flexible with factor n
    pub flex: Option<i32>,

    /// How to fit child in allocated space.
    pub fit: FlexFit,
}

/// How flexible child should fit in allocated space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlexFit {
    /// Child can be smaller than allocated space.
    Loose,

    /// Child must fill allocated space.
    Tight,
}

impl FlexParentData {
    /// Create with offset and flex settings.
    pub const fn new(offset: Offset, flex: Option<i32>, fit: FlexFit) -> Self {
        Self {
            offset,
            container: ContainerParentDataMixin::new(),
            flex,
            fit,
        }
    }

    /// Create inflexible (no flex factor).
    pub const fn inflexible() -> Self {
        Self::new(Offset::ZERO, None, FlexFit::Loose)
    }

    /// Create flexible with factor.
    pub const fn flexible(flex: i32) -> Self {
        Self::new(Offset::ZERO, Some(flex), FlexFit::Tight)
    }

    /// Builder: set flex factor.
    pub const fn with_flex(mut self, flex: Option<i32>) -> Self {
        self.flex = flex;
        self
    }

    /// Builder: set fit mode.
    pub const fn with_fit(mut self, fit: FlexFit) -> Self {
        self.fit = fit;
        self
    }

    /// Check if child is flexible.
    #[inline]
    pub const fn is_flexible(&self) -> bool {
        self.flex.is_some()
    }

    /// Check if child is tight fit.
    #[inline]
    pub const fn is_tight(&self) -> bool {
        matches!(self.fit, FlexFit::Tight)
    }
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self::inflexible()
    }
}

impl ParentData for FlexParentData {}

impl Hash for FlexParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.container.hash(state);
        self.flex.hash(state);
        self.fit.hash(state);
    }
}

impl Eq for FlexParentData {}

// ============================================================================
// STACK PARENT DATA
// ============================================================================

/// Parent data for stack layouts with absolute positioning.
///
/// Children can be positioned using combinations of:
/// - top/bottom for vertical positioning
/// - left/right for horizontal positioning
/// - width/height for explicit sizing
#[derive(Debug, Clone, PartialEq)]
pub struct StackParentData {
    /// Offset from parent (computed during layout).
    pub offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,

    /// Distance from parent's top edge.
    pub top: Option<f32>,

    /// Distance from parent's right edge.
    pub right: Option<f32>,

    /// Distance from parent's bottom edge.
    pub bottom: Option<f32>,

    /// Distance from parent's left edge.
    pub left: Option<f32>,

    /// Explicit width (overrides intrinsic size).
    pub width: Option<f32>,

    /// Explicit height (overrides intrinsic size).
    pub height: Option<f32>,
}

impl StackParentData {
    /// Create with no positioning constraints.
    pub const fn new() -> Self {
        Self {
            offset: Offset::ZERO,
            container: ContainerParentDataMixin::new(),
            top: None,
            right: None,
            bottom: None,
            left: None,
            width: None,
            height: None,
        }
    }

    /// Builder: set top position.
    pub const fn with_top(mut self, top: f32) -> Self {
        self.top = Some(top);
        self
    }

    /// Builder: set right position.
    pub const fn with_right(mut self, right: f32) -> Self {
        self.right = Some(right);
        self
    }

    /// Builder: set bottom position.
    pub const fn with_bottom(mut self, bottom: f32) -> Self {
        self.bottom = Some(bottom);
        self
    }

    /// Builder: set left position.
    pub const fn with_left(mut self, left: f32) -> Self {
        self.left = Some(left);
        self
    }

    /// Builder: set width.
    pub const fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Builder: set height.
    pub const fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Check if child is positioned (any edge specified).
    pub const fn is_positioned(&self) -> bool {
        self.top.is_some() || self.right.is_some() || self.bottom.is_some() || self.left.is_some()
    }
}

impl Default for StackParentData {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentData for StackParentData {}

impl Hash for StackParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.container.hash(state);

        // Hash Option<f32> values
        let hash_opt_f32 = |value: Option<f32>, state: &mut H| match value {
            Some(v) => {
                true.hash(state);
                v.to_bits().hash(state);
            }
            None => false.hash(state),
        };

        hash_opt_f32(self.top, state);
        hash_opt_f32(self.right, state);
        hash_opt_f32(self.bottom, state);
        hash_opt_f32(self.left, state);
        hash_opt_f32(self.width, state);
        hash_opt_f32(self.height, state);
    }
}

impl Eq for StackParentData {}

// ============================================================================
// WRAP PARENT DATA
// ============================================================================

/// Parent data for wrap layouts (horizontal and vertical wrapping).
///
/// Same structure as `ContainerBoxParentData` but used specifically for Wrap.
pub type WrapParentData = ContainerBoxParentData;

// ============================================================================
// FLOW PARENT DATA
// ============================================================================

/// Parent data for custom flow layouts with transforms.
///
/// Stores optional transform matrix applied during painting.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowParentData {
    /// Offset from parent.
    pub offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,

    /// Optional transform applied to child during painting.
    pub transform: Option<Matrix4>,
}

impl FlowParentData {
    /// Create with no transform.
    pub const fn new(offset: Offset) -> Self {
        Self {
            offset,
            container: ContainerParentDataMixin::new(),
            transform: None,
        }
    }

    /// Create at origin with no transform.
    pub const fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Builder: set transform.
    pub fn with_transform(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Check if transform is set.
    #[inline]
    pub const fn has_transform(&self) -> bool {
        self.transform.is_some()
    }
}

impl Default for FlowParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for FlowParentData {}

// No Hash/Eq for FlowParentData due to Matrix4

// ============================================================================
// LIST BODY PARENT DATA
// ============================================================================

/// Parent data for list body layouts.
///
/// Same structure as `ContainerBoxParentData`, used for ListBody widget.
pub type ListBodyParentData = ContainerBoxParentData;

// ============================================================================
// LIST WHEEL PARENT DATA
// ============================================================================

/// Parent data for list wheel scroll view (3D carousel effect).
///
/// Stores child's index in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListWheelParentData {
    /// Offset from parent.
    pub offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,

    /// Index of this child in the list.
    pub index: usize,
}

impl ListWheelParentData {
    /// Create with specific index.
    pub const fn new(index: usize) -> Self {
        Self {
            offset: Offset::ZERO,
            container: ContainerParentDataMixin::new(),
            index,
        }
    }

    /// Builder: set index.
    pub const fn with_index(mut self, index: usize) -> Self {
        self.index = index;
        self
    }
}

impl Default for ListWheelParentData {
    fn default() -> Self {
        Self::new(0)
    }
}

impl ParentData for ListWheelParentData {}

impl Hash for ListWheelParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.container.hash(state);
        self.index.hash(state);
    }
}

// ============================================================================
// MULTI CHILD LAYOUT PARENT DATA
// ============================================================================

/// Parent data for custom multi-child layouts with IDs.
///
/// Allows identifying children by arbitrary IDs for custom layout algorithms.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiChildLayoutParentData {
    /// Offset from parent.
    pub offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,

    /// Optional ID for this child (for custom layout algorithms).
    pub id: Option<String>,
}

impl MultiChildLayoutParentData {
    /// Create with optional ID.
    pub const fn new(offset: Offset, id: Option<String>) -> Self {
        Self {
            offset,
            container: ContainerParentDataMixin::new(),
            id,
        }
    }

    /// Create at origin with no ID.
    pub const fn zero() -> Self {
        Self {
            offset: Offset::ZERO,
            container: ContainerParentDataMixin::new(),
            id: None,
        }
    }

    /// Builder: set ID.
    pub fn with_id(mut self, id: String) -> Self {
        self.id = Some(id);
        self
    }

    /// Check if ID is set.
    #[inline]
    pub fn has_id(&self) -> bool {
        self.id.is_some()
    }
}

impl Default for MultiChildLayoutParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for MultiChildLayoutParentData {}

impl Hash for MultiChildLayoutParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.container.hash(state);
        self.id.hash(state);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_box_parent_data() {
        let data = ContainerBoxParentData::zero().with_offset(Offset::new(10.0, 20.0));

        assert_eq!(data.offset.x, 10.0);
        assert!(!data.is_zero());
    }

    #[test]
    fn test_flex_parent_data() {
        let data = FlexParentData::flexible(2);

        assert!(data.is_flexible());
        assert_eq!(data.flex, Some(2));
        assert!(data.is_tight());
    }

    #[test]
    fn test_stack_parent_data() {
        let data = StackParentData::new().with_top(10.0).with_left(20.0);

        assert!(data.is_positioned());
        assert_eq!(data.top, Some(10.0));
        assert_eq!(data.left, Some(20.0));
    }

    #[test]
    fn test_flow_parent_data() {
        let data = FlowParentData::zero();

        assert!(!data.has_transform());
    }

    #[test]
    fn test_list_wheel_parent_data() {
        let data = ListWheelParentData::new(5);

        assert_eq!(data.index, 5);
    }

    #[test]
    fn test_multi_child_layout_parent_data() {
        let data = MultiChildLayoutParentData::zero().with_id("child1".to_string());

        assert!(data.has_id());
        assert_eq!(data.id.as_ref().unwrap(), "child1");
    }
}
