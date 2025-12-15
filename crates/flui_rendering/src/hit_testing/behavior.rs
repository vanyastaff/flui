//! Hit test behavior enumeration.

/// How to behave during hit testing.
///
/// This controls how a render object participates in hit testing,
/// determining whether it absorbs hits, passes them through, or
/// defers to a translucent strategy.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `HitTestBehavior` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Targets that defer to their children receive events within their bounds
    /// only if one of their children is hit by the hit test.
    ///
    /// This is the default behavior.
    #[default]
    DeferToChild,

    /// Opaque targets can be hit by hit tests, causing them to both receive
    /// events within their bounds and prevent targets visually behind them
    /// from also receiving events.
    Opaque,

    /// Translucent targets both receive events within their bounds and permit
    /// targets visually behind them to also receive events.
    Translucent,
}

impl HitTestBehavior {
    /// Returns whether this behavior allows hit testing to continue to siblings.
    pub fn allows_pass_through(&self) -> bool {
        matches!(self, Self::Translucent)
    }

    /// Returns whether this behavior requires a child hit for self to be hit.
    pub fn requires_child_hit(&self) -> bool {
        matches!(self, Self::DeferToChild)
    }

    /// Returns whether this behavior causes self to be hit unconditionally
    /// when within bounds.
    pub fn is_opaque(&self) -> bool {
        matches!(self, Self::Opaque)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_defer_to_child() {
        assert_eq!(HitTestBehavior::default(), HitTestBehavior::DeferToChild);
    }

    #[test]
    fn test_allows_pass_through() {
        assert!(!HitTestBehavior::DeferToChild.allows_pass_through());
        assert!(!HitTestBehavior::Opaque.allows_pass_through());
        assert!(HitTestBehavior::Translucent.allows_pass_through());
    }

    #[test]
    fn test_requires_child_hit() {
        assert!(HitTestBehavior::DeferToChild.requires_child_hit());
        assert!(!HitTestBehavior::Opaque.requires_child_hit());
        assert!(!HitTestBehavior::Translucent.requires_child_hit());
    }

    #[test]
    fn test_is_opaque() {
        assert!(!HitTestBehavior::DeferToChild.is_opaque());
        assert!(HitTestBehavior::Opaque.is_opaque());
        assert!(!HitTestBehavior::Translucent.is_opaque());
    }
}
