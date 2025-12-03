//! View mode - categorizes view behavior
//!
//! This module defines `ViewMode` which is used to categorize views
//! and determine how they should be processed by the framework.

/// View mode - categorizes view behavior
///
/// Used by the framework to determine how to process a view:
/// - Component views (Stateless, Stateful, etc.) produce child elements
/// - Render views (`RenderBox`, `RenderSliver`) perform layout/paint
///
/// # Examples
///
/// ```rust
/// use flui_view::ViewMode;
///
/// let mode = ViewMode::Stateless;
/// assert!(mode.is_component());
/// assert!(!mode.is_render());
///
/// let render_mode = ViewMode::RenderBox;
/// assert!(render_mode.is_render());
/// assert!(!render_mode.is_component());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum ViewMode {
    /// Empty/unknown mode (default for empty elements)
    #[default]
    Empty = 0,

    /// Stateless component - no internal state
    Stateless = 1,

    /// Stateful component - has mutable state
    Stateful = 2,

    /// Animated component - driven by animation
    Animated = 3,

    /// Provider component - provides data to descendants
    Provider = 4,

    /// Proxy component - wraps single child
    Proxy = 5,

    /// Box render object - participates in box layout
    RenderBox = 6,

    /// Sliver render object - participates in sliver layout
    RenderSliver = 7,
}

impl ViewMode {
    /// Check if this is a component view (builds children)
    #[inline]
    pub const fn is_component(self) -> bool {
        matches!(
            self,
            Self::Stateless | Self::Stateful | Self::Animated | Self::Provider | Self::Proxy
        )
    }

    /// Check if this is a render view (layout/paint)
    #[inline]
    pub const fn is_render(self) -> bool {
        matches!(self, Self::RenderBox | Self::RenderSliver)
    }

    /// Check if this is a provider view
    #[inline]
    pub const fn is_provider(self) -> bool {
        matches!(self, Self::Provider)
    }

    /// Check if this is an empty/unknown mode
    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Empty"),
            Self::Stateless => write!(f, "Stateless"),
            Self::Stateful => write!(f, "Stateful"),
            Self::Animated => write!(f, "Animated"),
            Self::Provider => write!(f, "Provider"),
            Self::Proxy => write!(f, "Proxy"),
            Self::RenderBox => write!(f, "RenderBox"),
            Self::RenderSliver => write!(f, "RenderSliver"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_component() {
        assert!(ViewMode::Stateless.is_component());
        assert!(ViewMode::Stateful.is_component());
        assert!(ViewMode::Animated.is_component());
        assert!(ViewMode::Provider.is_component());
        assert!(ViewMode::Proxy.is_component());
        assert!(!ViewMode::RenderBox.is_component());
        assert!(!ViewMode::RenderSliver.is_component());
        assert!(!ViewMode::Empty.is_component());
    }

    #[test]
    fn test_is_render() {
        assert!(ViewMode::RenderBox.is_render());
        assert!(ViewMode::RenderSliver.is_render());
        assert!(!ViewMode::Stateless.is_render());
        assert!(!ViewMode::Provider.is_render());
        assert!(!ViewMode::Empty.is_render());
    }

    #[test]
    fn test_is_provider() {
        assert!(ViewMode::Provider.is_provider());
        assert!(!ViewMode::Stateless.is_provider());
        assert!(!ViewMode::RenderBox.is_provider());
    }

    #[test]
    fn test_default() {
        assert_eq!(ViewMode::default(), ViewMode::Empty);
    }

    #[test]
    fn test_display() {
        assert_eq!(ViewMode::Stateless.to_string(), "Stateless");
        assert_eq!(ViewMode::RenderBox.to_string(), "RenderBox");
    }
}
