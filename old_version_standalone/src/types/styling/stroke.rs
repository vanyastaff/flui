//! Stroke styling for paths and shapes.
//!
//! Similar to Flutter's `StrokeCap` and `StrokeJoin`.

/// How to end the stroke at the beginning and end of a line or path.
///
/// Similar to Flutter's `StrokeCap` and CSS `stroke-linecap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StrokeCap {
    /// Begin and end contours with a flat edge and no extension.
    ///
    /// This is the default stroke cap.
    #[default]
    Butt,

    /// Begin and end contours with a semi-circle extension.
    ///
    /// The extension has a diameter equal to the stroke width.
    Round,

    /// Begin and end contours with a square extension.
    ///
    /// The extension has a length of half the stroke width.
    Square,
}

impl StrokeCap {
    /// Get the CSS `stroke-linecap` value.
    pub fn css_value(&self) -> &'static str {
        match self {
            StrokeCap::Butt => "butt",
            StrokeCap::Round => "round",
            StrokeCap::Square => "square",
        }
    }
}

impl std::fmt::Display for StrokeCap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.css_value())
    }
}

/// How to join path segments at sharp corners.
///
/// Similar to Flutter's `StrokeJoin` and CSS `stroke-linejoin`.
#[derive(Debug, Clone, Copy, PartialEq, Hash, Default)]
pub enum StrokeJoin {
    /// Corners are drawn with sharp edges.
    ///
    /// Can extend beyond the endpoint depending on the miter limit.
    /// This is the default stroke join.
    #[default]
    Miter,

    /// Corners are drawn with rounded edges.
    ///
    /// The radius is half the stroke width.
    Round,

    /// Corners are drawn with beveled edges.
    ///
    /// Connects the endpoints of each stroke segment with a straight line.
    Bevel,
}

impl StrokeJoin {
    /// Get the CSS `stroke-linejoin` value.
    pub fn css_value(&self) -> &'static str {
        match self {
            StrokeJoin::Miter => "miter",
            StrokeJoin::Round => "round",
            StrokeJoin::Bevel => "bevel",
        }
    }
}

impl std::fmt::Display for StrokeJoin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.css_value())
    }
}

// Allow equality comparison for StrokeJoin with f32 miter limit
impl Eq for StrokeJoin {}

/// Complete stroke style for paths and shapes.
///
/// Combines width, cap, join, and miter limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeStyle {
    /// The width of the stroke.
    pub width: f32,

    /// How to cap the ends of the stroke.
    pub cap: StrokeCap,

    /// How to join segments of the stroke.
    pub join: StrokeJoin,

    /// The limit for miter joins, relative to stroke width.
    ///
    /// Only applies when `join` is `StrokeJoin::Miter`.
    /// If the miter would extend beyond this limit, a bevel join is used instead.
    /// Default is 4.0 (matches SVG default).
    pub miter_limit: f32,
}

impl StrokeStyle {
    /// Create a new stroke style with default values.
    pub fn new(width: f32) -> Self {
        Self {
            width,
            cap: StrokeCap::default(),
            join: StrokeJoin::default(),
            miter_limit: 4.0,
        }
    }

    /// Builder: set the stroke cap.
    pub fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }

    /// Builder: set the stroke join.
    pub fn with_join(mut self, join: StrokeJoin) -> Self {
        self.join = join;
        self
    }

    /// Builder: set the miter limit.
    pub fn with_miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit;
        self
    }

    /// Create a hairline stroke (1px width).
    pub fn hairline() -> Self {
        Self::new(1.0)
    }

    /// Create a thin stroke (2px width).
    pub fn thin() -> Self {
        Self::new(2.0)
    }

    /// Create a normal stroke (3px width).
    pub fn normal() -> Self {
        Self::new(3.0)
    }

    /// Create a thick stroke (5px width).
    pub fn thick() -> Self {
        Self::new(5.0)
    }

    /// Create a stroke with round caps and joins.
    pub fn rounded(width: f32) -> Self {
        Self::new(width)
            .with_cap(StrokeCap::Round)
            .with_join(StrokeJoin::Round)
    }
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self::new(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stroke_cap_values() {
        assert_eq!(StrokeCap::Butt.css_value(), "butt");
        assert_eq!(StrokeCap::Round.css_value(), "round");
        assert_eq!(StrokeCap::Square.css_value(), "square");
    }

    #[test]
    fn test_stroke_cap_display() {
        assert_eq!(format!("{}", StrokeCap::Butt), "butt");
        assert_eq!(format!("{}", StrokeCap::Round), "round");
        assert_eq!(format!("{}", StrokeCap::Square), "square");
    }

    #[test]
    fn test_stroke_cap_default() {
        assert_eq!(StrokeCap::default(), StrokeCap::Butt);
    }

    #[test]
    fn test_stroke_join_values() {
        assert_eq!(StrokeJoin::Miter.css_value(), "miter");
        assert_eq!(StrokeJoin::Round.css_value(), "round");
        assert_eq!(StrokeJoin::Bevel.css_value(), "bevel");
    }

    #[test]
    fn test_stroke_join_display() {
        assert_eq!(format!("{}", StrokeJoin::Miter), "miter");
        assert_eq!(format!("{}", StrokeJoin::Round), "round");
        assert_eq!(format!("{}", StrokeJoin::Bevel), "bevel");
    }

    #[test]
    fn test_stroke_join_default() {
        assert_eq!(StrokeJoin::default(), StrokeJoin::Miter);
    }

    #[test]
    fn test_stroke_style_creation() {
        let style = StrokeStyle::new(5.0);
        assert_eq!(style.width, 5.0);
        assert_eq!(style.cap, StrokeCap::Butt);
        assert_eq!(style.join, StrokeJoin::Miter);
        assert_eq!(style.miter_limit, 4.0);
    }

    #[test]
    fn test_stroke_style_builder() {
        let style = StrokeStyle::new(3.0)
            .with_cap(StrokeCap::Round)
            .with_join(StrokeJoin::Bevel)
            .with_miter_limit(10.0);

        assert_eq!(style.width, 3.0);
        assert_eq!(style.cap, StrokeCap::Round);
        assert_eq!(style.join, StrokeJoin::Bevel);
        assert_eq!(style.miter_limit, 10.0);
    }

    #[test]
    fn test_stroke_style_presets() {
        let hairline = StrokeStyle::hairline();
        assert_eq!(hairline.width, 1.0);

        let thin = StrokeStyle::thin();
        assert_eq!(thin.width, 2.0);

        let normal = StrokeStyle::normal();
        assert_eq!(normal.width, 3.0);

        let thick = StrokeStyle::thick();
        assert_eq!(thick.width, 5.0);
    }

    #[test]
    fn test_stroke_style_rounded() {
        let rounded = StrokeStyle::rounded(4.0);
        assert_eq!(rounded.width, 4.0);
        assert_eq!(rounded.cap, StrokeCap::Round);
        assert_eq!(rounded.join, StrokeJoin::Round);
    }

    #[test]
    fn test_stroke_style_default() {
        let style = StrokeStyle::default();
        assert_eq!(style.width, 1.0);
        assert_eq!(style.cap, StrokeCap::Butt);
        assert_eq!(style.join, StrokeJoin::Miter);
    }
}
