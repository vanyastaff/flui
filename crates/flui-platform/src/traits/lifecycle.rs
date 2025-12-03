//! Platform lifecycle management
//!
//! Provides a unified lifecycle model across platforms with different
//! lifecycle semantics (Desktop vs Mobile vs Web).

/// Lifecycle state machine
///
/// Tracks the application lifecycle state across all platforms.
/// Mobile platforms have richer lifecycle, desktop is simpler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LifecycleState {
    /// App is starting up
    #[default]
    Starting,
    /// App is in foreground and active
    Active,
    /// App is in foreground but inactive (e.g., dialog shown)
    Inactive,
    /// App is in background (mobile) or minimized (desktop)
    Background,
    /// App is being terminated
    Terminating,
}

impl LifecycleState {
    /// Check if app should be rendering
    pub fn should_render(&self) -> bool {
        matches!(self, Self::Active | Self::Inactive)
    }

    /// Check if app is in foreground
    pub fn is_foreground(&self) -> bool {
        matches!(self, Self::Active | Self::Inactive)
    }

    /// Check if app is fully active
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Lifecycle events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// App started
    Started,
    /// App became active (foreground)
    Activated,
    /// App became inactive (but still foreground)
    Deactivated,
    /// App went to background
    Backgrounded,
    /// App returned from background
    Foregrounded,
    /// App is terminating
    Terminating,
    /// Focus gained
    FocusGained,
    /// Focus lost
    FocusLost,
}

/// Platform lifecycle trait
///
/// Provides lifecycle management for embedders.
pub trait PlatformLifecycle: Send + Sync {
    /// Get current lifecycle state
    fn state(&self) -> LifecycleState;

    /// Handle lifecycle event
    fn handle_event(&mut self, event: LifecycleEvent);

    /// Check if rendering should occur
    fn should_render(&self) -> bool {
        self.state().should_render()
    }

    /// Check if app has focus
    fn is_focused(&self) -> bool;

    /// Check if app is visible
    fn is_visible(&self) -> bool;
}

/// Default lifecycle implementation
///
/// Suitable for most platforms with standard behavior.
#[derive(Debug, Default)]
pub struct DefaultLifecycle {
    state: LifecycleState,
    is_focused: bool,
    is_visible: bool,
}

impl DefaultLifecycle {
    /// Create a new lifecycle tracker
    pub fn new() -> Self {
        Self {
            state: LifecycleState::Starting,
            is_focused: true,
            is_visible: true,
        }
    }

    /// Update focus state
    pub fn on_focus_changed(&mut self, focused: bool) {
        self.is_focused = focused;
        tracing::debug!(focused, "Focus changed");
    }

    /// Update visibility state
    pub fn on_visibility_changed(&mut self, visible: bool) {
        self.is_visible = visible;
        tracing::debug!(visible, "Visibility changed");

        // Update lifecycle state based on visibility
        if visible && self.state == LifecycleState::Background {
            self.state = LifecycleState::Active;
        } else if !visible && self.state == LifecycleState::Active {
            self.state = LifecycleState::Background;
        }
    }
}

impl PlatformLifecycle for DefaultLifecycle {
    fn state(&self) -> LifecycleState {
        self.state
    }

    fn handle_event(&mut self, event: LifecycleEvent) {
        tracing::debug!(?event, "Lifecycle event");

        self.state = match event {
            LifecycleEvent::Started => LifecycleState::Active,
            LifecycleEvent::Activated => LifecycleState::Active,
            LifecycleEvent::Deactivated => LifecycleState::Inactive,
            LifecycleEvent::Backgrounded => LifecycleState::Background,
            LifecycleEvent::Foregrounded => LifecycleState::Active,
            LifecycleEvent::Terminating => LifecycleState::Terminating,
            LifecycleEvent::FocusGained => {
                self.is_focused = true;
                self.state // Don't change state
            }
            LifecycleEvent::FocusLost => {
                self.is_focused = false;
                self.state // Don't change state
            }
        };
    }

    fn is_focused(&self) -> bool {
        self.is_focused
    }

    fn is_visible(&self) -> bool {
        self.is_visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_state_transitions() {
        let mut lifecycle = DefaultLifecycle::new();

        lifecycle.handle_event(LifecycleEvent::Started);
        assert_eq!(lifecycle.state(), LifecycleState::Active);
        assert!(lifecycle.should_render());

        lifecycle.handle_event(LifecycleEvent::Backgrounded);
        assert_eq!(lifecycle.state(), LifecycleState::Background);
        assert!(!lifecycle.should_render());

        lifecycle.handle_event(LifecycleEvent::Foregrounded);
        assert_eq!(lifecycle.state(), LifecycleState::Active);
        assert!(lifecycle.should_render());
    }

    #[test]
    fn test_focus_tracking() {
        let mut lifecycle = DefaultLifecycle::new();

        assert!(lifecycle.is_focused());

        lifecycle.handle_event(LifecycleEvent::FocusLost);
        assert!(!lifecycle.is_focused());

        lifecycle.handle_event(LifecycleEvent::FocusGained);
        assert!(lifecycle.is_focused());
    }
}
