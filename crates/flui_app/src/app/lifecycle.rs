//! Application lifecycle management.

/// Application lifecycle states.
///
/// These correspond to platform-specific lifecycle events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLifecycle {
    /// App is starting up.
    Starting,

    /// App is resumed and visible.
    Resumed,

    /// App is inactive (e.g., during a phone call).
    Inactive,

    /// App is paused (backgrounded).
    Paused,

    /// App is being destroyed.
    Detached,
}

impl AppLifecycle {
    /// Check if the app is in an active state.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Resumed)
    }

    /// Check if the app is visible.
    pub fn is_visible(&self) -> bool {
        matches!(self, Self::Resumed | Self::Inactive)
    }

    /// Check if the app should be rendering.
    pub fn should_render(&self) -> bool {
        matches!(self, Self::Resumed)
    }
}

impl Default for AppLifecycle {
    fn default() -> Self {
        Self::Starting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_states() {
        assert!(AppLifecycle::Resumed.is_active());
        assert!(!AppLifecycle::Paused.is_active());

        assert!(AppLifecycle::Resumed.is_visible());
        assert!(AppLifecycle::Inactive.is_visible());
        assert!(!AppLifecycle::Paused.is_visible());

        assert!(AppLifecycle::Resumed.should_render());
        assert!(!AppLifecycle::Inactive.should_render());
    }
}
