//! View protocol and mode definitions
//!
//! This module defines how views are categorized and processed.

/// View mode - categorizes view behavior
///
/// Used by the framework to determine how to process a view:
/// - Component views (Stateless, Stateful) produce child elements
/// - Render views (RenderBox, RenderSliver) perform layout/paint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// Stateless component - no internal state
    Stateless,
    
    /// Stateful component - has mutable state
    Stateful,
    
    /// Animated component - driven by animation
    Animated,
    
    /// Provider component - provides data to descendants
    Provider,
    
    /// Proxy component - wraps single child
    Proxy,
    
    /// Box render object - participates in box layout
    RenderBox,
    
    /// Sliver render object - participates in sliver layout
    RenderSliver,
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
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
