//! Flex parent data - stores flex factor and fit for children

use flui_types::layout::FlexFit;

/// Parent data for children of RenderFlex (Row/Column)
///
/// This data is attached to children of flex containers to control
/// how they are sized and positioned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexParentData {
    /// The flex factor to use for this child.
    ///
    /// If non-zero, the child is flexible and will receive space according to its flex factor.
    /// Higher flex factors get proportionally more space.
    pub flex: i32,

    /// How the child should fit into the available space.
    ///
    /// - FlexFit::Tight: Child must fill allocated space (Expanded behavior)
    /// - FlexFit::Loose: Child can be smaller than allocated space (Flexible behavior)
    pub fit: FlexFit,
}

impl FlexParentData {
    /// Create new flex parent data
    pub fn new(flex: i32, fit: FlexFit) -> Self {
        Self { flex, fit }
    }

    /// Create flex parent data for Expanded widget (tight fit, flex=1)
    pub fn expanded() -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Tight,
        }
    }

    /// Create flex parent data for Flexible widget (loose fit, flex=1)
    pub fn flexible() -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Loose,
        }
    }
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self {
            flex: 0,
            fit: FlexFit::Tight,
        }
    }
}
