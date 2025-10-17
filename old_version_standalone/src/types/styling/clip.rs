//! Clipping behavior types
//!
//! This module contains enums for controlling how widgets should be clipped,
//! similar to Flutter's Clip enum.

/// Different ways to clip a widget's content.
///
/// Similar to Flutter's `Clip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clip {
    /// No clipping.
    ///
    /// The widget may paint outside its bounds. This is the most efficient option
    /// when you know the widget will not paint outside its bounds or when the
    /// painting is inconsequential.
    None,

    /// Clip to the canvas save layer bounds.
    ///
    /// This is the most efficient option after `None` but may have
    /// jagged edges since it doesn't apply anti-aliasing.
    #[default]
    HardEdge,

    /// Clip with anti-aliasing.
    ///
    /// This clips with smoothed edges but is more expensive than `HardEdge`.
    /// This is typically what you want for clipping rounded rectangles.
    AntiAlias,

    /// Clip with anti-aliasing and an additional save layer.
    ///
    /// This is the most expensive option and is only necessary in rare cases
    /// for complex compositing scenarios.
    AntiAliasWithSaveLayer,
}

impl Clip {
    /// Check if this clip mode requires clipping.
    pub fn should_clip(self) -> bool {
        !matches!(self, Clip::None)
    }

    /// Check if this clip mode uses anti-aliasing.
    pub fn uses_anti_alias(self) -> bool {
        matches!(self, Clip::AntiAlias | Clip::AntiAliasWithSaveLayer)
    }

    /// Check if this clip mode uses a save layer.
    pub fn uses_save_layer(self) -> bool {
        matches!(self, Clip::AntiAliasWithSaveLayer)
    }

    /// Get the performance cost of this clip mode (0 = cheapest, 3 = most expensive).
    pub fn performance_cost(self) -> u8 {
        match self {
            Clip::None => 0,
            Clip::HardEdge => 1,
            Clip::AntiAlias => 2,
            Clip::AntiAliasWithSaveLayer => 3,
        }
    }

    /// Choose the most appropriate clip mode for the given scenario.
    pub fn choose(needs_anti_alias: bool, needs_save_layer: bool) -> Self {
        if needs_save_layer {
            Clip::AntiAliasWithSaveLayer
        } else if needs_anti_alias {
            Clip::AntiAlias
        } else {
            Clip::HardEdge
        }
    }
}

/// Path operations for combining clipping paths.
///
/// Similar to Flutter's `PathOperation` but more commonly used with clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOperation {
    /// Subtract the second path from the first path.
    Difference,

    /// Intersect the two paths.
    Intersect,

    /// Union (inclusive-or) the two paths.
    Union,

    /// Exclusive-or the two paths.
    Xor,

    /// Subtract the first path from the second path.
    ReverseDifference,
}

/// The fill rule for paths when clipping.
///
/// Similar to Flutter's `PathFillType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathFillType {
    /// The non-zero winding rule.
    ///
    /// A point is inside the path if a line from the point to infinity crosses
    /// more clockwise than counterclockwise path segments.
    #[default]
    NonZero,

    /// The even-odd winding rule.
    ///
    /// A point is inside the path if a line from the point to infinity crosses
    /// an odd number of path segments, regardless of direction.
    EvenOdd,
}

/// Custom clipping behavior configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipBehavior {
    /// The clip mode to use.
    pub clip: Clip,

    /// Whether to clip children that overflow the bounds.
    pub clip_overflow: bool,

    /// Whether to use a custom clipper.
    pub use_custom_clipper: bool,
}

impl ClipBehavior {
    /// Create a new clip behavior.
    pub const fn new(clip: Clip, clip_overflow: bool, use_custom_clipper: bool) -> Self {
        Self {
            clip,
            clip_overflow,
            use_custom_clipper,
        }
    }

    /// No clipping at all.
    pub const NONE: Self = Self::new(Clip::None, false, false);

    /// Default clipping with hard edges.
    pub const DEFAULT: Self = Self::new(Clip::HardEdge, true, false);

    /// Anti-aliased clipping.
    pub const ANTI_ALIAS: Self = Self::new(Clip::AntiAlias, true, false);

    /// Check if any clipping should be applied.
    pub fn should_clip(&self) -> bool {
        self.clip.should_clip() && (self.clip_overflow || self.use_custom_clipper)
    }

    /// Get the performance cost (0 = cheapest, 3 = most expensive).
    pub fn performance_cost(&self) -> u8 {
        if !self.should_clip() {
            0
        } else {
            self.clip.performance_cost()
        }
    }
}

impl Default for ClipBehavior {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<Clip> for ClipBehavior {
    fn from(clip: Clip) -> Self {
        Self::new(clip, true, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_default() {
        assert_eq!(Clip::default(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_properties() {
        assert!(!Clip::None.should_clip());
        assert!(Clip::HardEdge.should_clip());
        assert!(Clip::AntiAlias.should_clip());
        assert!(Clip::AntiAliasWithSaveLayer.should_clip());

        assert!(!Clip::None.uses_anti_alias());
        assert!(!Clip::HardEdge.uses_anti_alias());
        assert!(Clip::AntiAlias.uses_anti_alias());
        assert!(Clip::AntiAliasWithSaveLayer.uses_anti_alias());

        assert!(!Clip::None.uses_save_layer());
        assert!(!Clip::HardEdge.uses_save_layer());
        assert!(!Clip::AntiAlias.uses_save_layer());
        assert!(Clip::AntiAliasWithSaveLayer.uses_save_layer());
    }

    #[test]
    fn test_clip_performance_cost() {
        assert_eq!(Clip::None.performance_cost(), 0);
        assert_eq!(Clip::HardEdge.performance_cost(), 1);
        assert_eq!(Clip::AntiAlias.performance_cost(), 2);
        assert_eq!(Clip::AntiAliasWithSaveLayer.performance_cost(), 3);

        // Verify ordering
        assert!(Clip::None.performance_cost() < Clip::HardEdge.performance_cost());
        assert!(Clip::HardEdge.performance_cost() < Clip::AntiAlias.performance_cost());
        assert!(Clip::AntiAlias.performance_cost() < Clip::AntiAliasWithSaveLayer.performance_cost());
    }

    #[test]
    fn test_clip_choose() {
        assert_eq!(Clip::choose(false, false), Clip::HardEdge);
        assert_eq!(Clip::choose(true, false), Clip::AntiAlias);
        assert_eq!(Clip::choose(false, true), Clip::AntiAliasWithSaveLayer);
        assert_eq!(Clip::choose(true, true), Clip::AntiAliasWithSaveLayer);
    }

    #[test]
    fn test_path_fill_type_default() {
        assert_eq!(PathFillType::default(), PathFillType::NonZero);
    }

    #[test]
    fn test_clip_behavior_constants() {
        assert_eq!(ClipBehavior::NONE.clip, Clip::None);
        assert!(!ClipBehavior::NONE.clip_overflow);
        assert!(!ClipBehavior::NONE.use_custom_clipper);
        assert!(!ClipBehavior::NONE.should_clip());

        assert_eq!(ClipBehavior::DEFAULT.clip, Clip::HardEdge);
        assert!(ClipBehavior::DEFAULT.clip_overflow);
        assert!(!ClipBehavior::DEFAULT.use_custom_clipper);
        assert!(ClipBehavior::DEFAULT.should_clip());

        assert_eq!(ClipBehavior::ANTI_ALIAS.clip, Clip::AntiAlias);
        assert!(ClipBehavior::ANTI_ALIAS.clip_overflow);
        assert!(ClipBehavior::ANTI_ALIAS.should_clip());
    }

    #[test]
    fn test_clip_behavior_should_clip() {
        let none = ClipBehavior::NONE;
        assert!(!none.should_clip());

        let no_overflow = ClipBehavior::new(Clip::HardEdge, false, false);
        assert!(!no_overflow.should_clip());

        let with_overflow = ClipBehavior::new(Clip::HardEdge, true, false);
        assert!(with_overflow.should_clip());

        let with_custom = ClipBehavior::new(Clip::HardEdge, false, true);
        assert!(with_custom.should_clip());

        let both = ClipBehavior::new(Clip::HardEdge, true, true);
        assert!(both.should_clip());
    }

    #[test]
    fn test_clip_behavior_performance_cost() {
        assert_eq!(ClipBehavior::NONE.performance_cost(), 0);
        assert_eq!(ClipBehavior::DEFAULT.performance_cost(), 1);
        assert_eq!(ClipBehavior::ANTI_ALIAS.performance_cost(), 2);

        let no_clip = ClipBehavior::new(Clip::AntiAlias, false, false);
        assert_eq!(no_clip.performance_cost(), 0); // No clipping, so cost is 0
    }

    #[test]
    fn test_clip_behavior_default() {
        assert_eq!(ClipBehavior::default(), ClipBehavior::DEFAULT);
    }

    #[test]
    fn test_clip_behavior_from_clip() {
        let from_clip: ClipBehavior = Clip::AntiAlias.into();
        assert_eq!(from_clip.clip, Clip::AntiAlias);
        assert!(from_clip.clip_overflow);
        assert!(!from_clip.use_custom_clipper);
    }

    #[test]
    fn test_path_operation_variants() {
        // Just verify all variants exist
        let _ops = [
            PathOperation::Difference,
            PathOperation::Intersect,
            PathOperation::Union,
            PathOperation::Xor,
            PathOperation::ReverseDifference,
        ];
    }
}
