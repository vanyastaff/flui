//! How much main-axis space a flex takes.

/// How much space a flex container should occupy in its main axis.
///
/// Mirrors Flutter's `MainAxisSize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisSize {
    /// Minimize the amount of space occupied by the children.
    ///
    /// The widget will be as small as possible while still containing all
    /// children.
    Min,

    /// Maximize the amount of space along the main axis (the default).
    ///
    /// The widget expands to fill the incoming main-axis constraint.
    #[default]
    Max,
}

impl MainAxisSize {
    /// Check if this is Min.
    #[inline]
    pub const fn is_min(self) -> bool {
        matches!(self, MainAxisSize::Min)
    }

    /// Check if this is Max.
    #[inline]
    pub const fn is_max(self) -> bool {
        matches!(self, MainAxisSize::Max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_axis_size_predicates() {
        assert!(MainAxisSize::Min.is_min() && !MainAxisSize::Min.is_max());
        assert!(MainAxisSize::Max.is_max() && !MainAxisSize::Max.is_min());
    }
}
