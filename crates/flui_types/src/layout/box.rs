//! Box layout types - fit and shape

/// How a box should inscribe into another box.
///
/// This is similar to CSS `object-fit` property and Flutter's `BoxFit`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::BoxFit;
///
/// let fill = BoxFit::Fill;
/// let contain = BoxFit::Contain;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxFit {
    /// Fill the target box by distorting the source's aspect ratio.
    ///
    /// The entire source will be rendered to fill the destination box,
    /// distorting the aspect ratio if necessary.
    Fill,

    /// As large as possible while still containing the source entirely within the target box.
    ///
    /// Maintains aspect ratio. May leave empty space.
    ///
    /// Similar to CSS `object-fit: contain`.
    #[default]
    Contain,

    /// As small as possible while still covering the entire target box.
    ///
    /// Maintains aspect ratio. May clip the source.
    ///
    /// Similar to CSS `object-fit: cover`.
    Cover,

    /// Fit width, ignoring height. May overflow vertically.
    ///
    /// Maintains aspect ratio by scaling to match the width.
    FitWidth,

    /// Fit height, ignoring width. May overflow horizontally.
    ///
    /// Maintains aspect ratio by scaling to match the height.
    FitHeight,

    /// Center the source within the target box without scaling.
    ///
    /// If the source is larger than the target, it will be clipped.
    /// If smaller, there will be empty space.
    None,

    /// Center and scale down if needed to fit, but never scale up.
    ///
    /// Like `Contain`, but will not scale up if source is smaller.
    ScaleDown,
}

impl BoxFit {
    /// Returns true if this fit mode may clip content.
    pub const fn may_clip(&self) -> bool {
        matches!(self, BoxFit::Cover | BoxFit::FitWidth | BoxFit::FitHeight | BoxFit::None)
    }

    /// Returns true if this fit mode always maintains aspect ratio.
    pub const fn maintains_aspect_ratio(&self) -> bool {
        !matches!(self, BoxFit::Fill)
    }

    /// Returns true if this fit mode may scale content.
    pub const fn may_scale(&self) -> bool {
        !matches!(self, BoxFit::None)
    }
}

/// The shape of a box.
///
/// This is used to determine how a box should be clipped or rendered.
/// Similar to Flutter's `BoxShape`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::BoxShape;
///
/// let rect = BoxShape::Rectangle;
/// let circle = BoxShape::Circle;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxShape {
    /// A rectangle, possibly with rounded corners.
    ///
    /// When rendering with a border radius, the box will have rounded corners.
    /// Without a border radius, it's a simple rectangle.
    #[default]
    Rectangle,

    /// A circle.
    ///
    /// The box will be clipped to a circle that fits within the box's bounds.
    /// If the box is not square, the circle will be inscribed in the shorter dimension.
    Circle,
}

impl BoxShape {
    /// Returns true if this shape is circular.
    pub const fn is_circle(&self) -> bool {
        matches!(self, BoxShape::Circle)
    }

    /// Returns true if this shape is rectangular.
    pub const fn is_rectangle(&self) -> bool {
        matches!(self, BoxShape::Rectangle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_fit_properties() {
        assert!(BoxFit::Cover.may_clip());
        assert!(!BoxFit::Contain.may_clip());
        assert!(!BoxFit::Fill.may_clip());
        assert!(BoxFit::None.may_clip());

        assert!(BoxFit::Contain.maintains_aspect_ratio());
        assert!(BoxFit::Cover.maintains_aspect_ratio());
        assert!(!BoxFit::Fill.maintains_aspect_ratio());

        assert!(BoxFit::Contain.may_scale());
        assert!(!BoxFit::None.may_scale());
    }

    #[test]
    fn test_box_fit_default() {
        let default = BoxFit::default();
        assert_eq!(default, BoxFit::Contain);
    }

    #[test]
    fn test_box_shape_is_circle() {
        assert!(BoxShape::Circle.is_circle());
        assert!(!BoxShape::Rectangle.is_circle());
    }

    #[test]
    fn test_box_shape_is_rectangle() {
        assert!(BoxShape::Rectangle.is_rectangle());
        assert!(!BoxShape::Circle.is_rectangle());
    }

    #[test]
    fn test_box_shape_default() {
        let default = BoxShape::default();
        assert_eq!(default, BoxShape::Rectangle);
    }
}
