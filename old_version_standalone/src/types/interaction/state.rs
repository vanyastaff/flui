//! Widget state types
//!
//! This module contains types for representing widget interaction states,
//! similar to Flutter's MaterialState system.

/// Represents the interactive state of a widget.
///
/// Similar to Flutter's MaterialState.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WidgetState {
    /// The widget is in its default, inactive state.
    Default,
    /// The widget is being hovered over (mouse cursor is over it).
    Hovered,
    /// The widget is being pressed or clicked.
    Pressed,
    /// The widget is focused (has keyboard focus).
    Focused,
    /// The widget is disabled and cannot be interacted with.
    Disabled,
    /// The widget is selected (e.g., checkbox is checked).
    Selected,
    /// The widget has an error state.
    Error,
}

impl WidgetState {
    /// Check if the widget is interactive (not disabled).
    pub fn is_interactive(&self) -> bool {
        !matches!(self, WidgetState::Disabled)
    }

    /// Check if the widget is active (hovered, pressed, or focused).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            WidgetState::Hovered | WidgetState::Pressed | WidgetState::Focused
        )
    }
}

impl Default for WidgetState {
    fn default() -> Self {
        WidgetState::Default
    }
}

/// Represents a set of widget states (for combined states).
///
/// Similar to Flutter's Set<MaterialState>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetStates {
    hovered: bool,
    pressed: bool,
    focused: bool,
    disabled: bool,
    selected: bool,
    error: bool,
}

impl WidgetStates {
    /// Create a new empty state set.
    pub const fn new() -> Self {
        Self {
            hovered: false,
            pressed: false,
            focused: false,
            disabled: false,
            selected: false,
            error: false,
        }
    }

    /// Create a state set from a single state.
    pub fn from_state(state: WidgetState) -> Self {
        let mut states = Self::new();
        match state {
            WidgetState::Default => {}
            WidgetState::Hovered => states.hovered = true,
            WidgetState::Pressed => states.pressed = true,
            WidgetState::Focused => states.focused = true,
            WidgetState::Disabled => states.disabled = true,
            WidgetState::Selected => states.selected = true,
            WidgetState::Error => states.error = true,
        }
        states
    }

    /// Add a state to the set.
    pub fn add(&mut self, state: WidgetState) {
        match state {
            WidgetState::Default => {}
            WidgetState::Hovered => self.hovered = true,
            WidgetState::Pressed => self.pressed = true,
            WidgetState::Focused => self.focused = true,
            WidgetState::Disabled => self.disabled = true,
            WidgetState::Selected => self.selected = true,
            WidgetState::Error => self.error = true,
        }
    }

    /// Remove a state from the set.
    pub fn remove(&mut self, state: WidgetState) {
        match state {
            WidgetState::Default => {}
            WidgetState::Hovered => self.hovered = false,
            WidgetState::Pressed => self.pressed = false,
            WidgetState::Focused => self.focused = false,
            WidgetState::Disabled => self.disabled = false,
            WidgetState::Selected => self.selected = false,
            WidgetState::Error => self.error = false,
        }
    }

    /// Check if a state is present in the set.
    pub fn contains(&self, state: WidgetState) -> bool {
        match state {
            WidgetState::Default => !self.is_active(),
            WidgetState::Hovered => self.hovered,
            WidgetState::Pressed => self.pressed,
            WidgetState::Focused => self.focused,
            WidgetState::Disabled => self.disabled,
            WidgetState::Selected => self.selected,
            WidgetState::Error => self.error,
        }
    }

    /// Check if the widget is hovered.
    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Check if the widget is pressed.
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    /// Check if the widget is focused.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Check if the widget is disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Check if the widget is selected.
    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// Check if the widget has an error.
    pub fn has_error(&self) -> bool {
        self.error
    }

    /// Check if the widget is interactive (not disabled).
    pub fn is_interactive(&self) -> bool {
        !self.disabled
    }

    /// Check if any active state is set (hovered, pressed, or focused).
    pub fn is_active(&self) -> bool {
        self.hovered || self.pressed || self.focused
    }

    /// Builder: set hovered state.
    pub fn with_hovered(mut self, value: bool) -> Self {
        self.hovered = value;
        self
    }

    /// Builder: set pressed state.
    pub fn with_pressed(mut self, value: bool) -> Self {
        self.pressed = value;
        self
    }

    /// Builder: set focused state.
    pub fn with_focused(mut self, value: bool) -> Self {
        self.focused = value;
        self
    }

    /// Builder: set disabled state.
    pub fn with_disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self
    }

    /// Builder: set selected state.
    pub fn with_selected(mut self, value: bool) -> Self {
        self.selected = value;
        self
    }

    /// Builder: set error state.
    pub fn with_error(mut self, value: bool) -> Self {
        self.error = value;
        self
    }
}

impl Default for WidgetStates {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_state() {
        assert_eq!(WidgetState::default(), WidgetState::Default);
        assert!(WidgetState::Default.is_interactive());
        assert!(WidgetState::Hovered.is_interactive());
        assert!(!WidgetState::Disabled.is_interactive());

        assert!(WidgetState::Hovered.is_active());
        assert!(WidgetState::Pressed.is_active());
        assert!(WidgetState::Focused.is_active());
        assert!(!WidgetState::Default.is_active());
    }

    #[test]
    fn test_widget_states_creation() {
        let states = WidgetStates::new();
        assert!(!states.is_hovered());
        assert!(!states.is_pressed());
        assert!(!states.is_focused());
        assert!(!states.is_disabled());

        let hovered = WidgetStates::from_state(WidgetState::Hovered);
        assert!(hovered.is_hovered());
        assert!(!hovered.is_pressed());
    }

    #[test]
    fn test_widget_states_add_remove() {
        let mut states = WidgetStates::new();

        states.add(WidgetState::Hovered);
        assert!(states.is_hovered());
        assert!(states.contains(WidgetState::Hovered));

        states.add(WidgetState::Pressed);
        assert!(states.is_pressed());

        states.remove(WidgetState::Hovered);
        assert!(!states.is_hovered());
        assert!(states.is_pressed());
    }

    #[test]
    fn test_widget_states_checks() {
        let states = WidgetStates::new()
            .with_hovered(true)
            .with_selected(true);

        assert!(states.is_hovered());
        assert!(states.is_selected());
        assert!(!states.is_disabled());
        assert!(states.is_interactive());
        assert!(states.is_active());
    }

    #[test]
    fn test_widget_states_builder() {
        let states = WidgetStates::new()
            .with_hovered(true)
            .with_pressed(true)
            .with_focused(true)
            .with_disabled(false)
            .with_selected(true)
            .with_error(false);

        assert!(states.is_hovered());
        assert!(states.is_pressed());
        assert!(states.is_focused());
        assert!(!states.is_disabled());
        assert!(states.is_selected());
        assert!(!states.has_error());
    }

    #[test]
    fn test_widget_states_disabled() {
        let disabled = WidgetStates::new().with_disabled(true);

        assert!(disabled.is_disabled());
        assert!(!disabled.is_interactive());
    }
}
