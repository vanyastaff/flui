//! Semantic flags for describing node properties

/// Flags that describe properties of a semantic node
///
/// Similar to Flutter's `SemanticsFlag`. These are bitflags that can be
/// combined to describe various properties of a semantic node.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::SemanticsFlags;
///
/// let mut flags = SemanticsFlags::default();
/// flags.set_is_button(true);
/// flags.set_is_focusable(true);
///
/// assert!(flags.is_button());
/// assert!(flags.is_focusable());
/// assert!(!flags.is_text_field());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticsFlags {
    bits: u64,
}

impl SemanticsFlags {
    // Flag bit positions
    const HAS_CHECKED_STATE: u64 = 1 << 0;
    const IS_CHECKED: u64 = 1 << 1;
    const IS_SELECTED: u64 = 1 << 2;
    const IS_BUTTON: u64 = 1 << 3;
    const IS_TEXT_FIELD: u64 = 1 << 4;
    const IS_FOCUSED: u64 = 1 << 5;
    const HAS_ENABLED_STATE: u64 = 1 << 6;
    const IS_ENABLED: u64 = 1 << 7;
    #[allow(dead_code)]
    const IS_IN_MUTUALLY_EXCLUSIVE_GROUP: u64 = 1 << 8;
    #[allow(dead_code)]
    const IS_HEADER: u64 = 1 << 9;
    #[allow(dead_code)]
    const IS_OBSCURED: u64 = 1 << 10;
    #[allow(dead_code)]
    const SCOPES_ROUTE: u64 = 1 << 11;
    #[allow(dead_code)]
    const NAMES_ROUTE: u64 = 1 << 12;
    const IS_HIDDEN: u64 = 1 << 13;
    const IS_IMAGE: u64 = 1 << 14;
    #[allow(dead_code)]
    const IS_LIVE_REGION: u64 = 1 << 15;
    #[allow(dead_code)]
    const HAS_TOGGLED_STATE: u64 = 1 << 16;
    #[allow(dead_code)]
    const IS_TOGGLED: u64 = 1 << 17;
    #[allow(dead_code)]
    const HAS_IMPLICIT_SCROLLING: u64 = 1 << 18;
    #[allow(dead_code)]
    const IS_MULTILINE: u64 = 1 << 19;
    #[allow(dead_code)]
    const IS_READ_ONLY: u64 = 1 << 20;
    const IS_FOCUSABLE: u64 = 1 << 21;
    const IS_LINK: u64 = 1 << 22;
    const IS_SLIDER: u64 = 1 << 23;
    #[allow(dead_code)]
    const IS_KEYBOARD_KEY: u64 = 1 << 24;

    /// Creates empty flags
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    /// Checks if the node has a checked state
    pub const fn has_checked_state(&self) -> bool {
        self.bits & Self::HAS_CHECKED_STATE != 0
    }

    /// Sets whether the node has a checked state
    pub fn set_has_checked_state(&mut self, value: bool) {
        if value {
            self.bits |= Self::HAS_CHECKED_STATE;
        } else {
            self.bits &= !Self::HAS_CHECKED_STATE;
        }
    }

    /// Checks if the node is checked
    pub const fn is_checked(&self) -> bool {
        self.bits & Self::IS_CHECKED != 0
    }

    /// Sets whether the node is checked
    pub fn set_is_checked(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_CHECKED;
        } else {
            self.bits &= !Self::IS_CHECKED;
        }
    }

    /// Checks if the node is selected
    pub const fn is_selected(&self) -> bool {
        self.bits & Self::IS_SELECTED != 0
    }

    /// Sets whether the node is selected
    pub fn set_is_selected(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_SELECTED;
        } else {
            self.bits &= !Self::IS_SELECTED;
        }
    }

    /// Checks if the node is a button
    pub const fn is_button(&self) -> bool {
        self.bits & Self::IS_BUTTON != 0
    }

    /// Sets whether the node is a button
    pub fn set_is_button(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_BUTTON;
        } else {
            self.bits &= !Self::IS_BUTTON;
        }
    }

    /// Checks if the node is a text field
    pub const fn is_text_field(&self) -> bool {
        self.bits & Self::IS_TEXT_FIELD != 0
    }

    /// Sets whether the node is a text field
    pub fn set_is_text_field(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_TEXT_FIELD;
        } else {
            self.bits &= !Self::IS_TEXT_FIELD;
        }
    }

    /// Checks if the node is focused
    pub const fn is_focused(&self) -> bool {
        self.bits & Self::IS_FOCUSED != 0
    }

    /// Sets whether the node is focused
    pub fn set_is_focused(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_FOCUSED;
        } else {
            self.bits &= !Self::IS_FOCUSED;
        }
    }

    /// Checks if the node has an enabled state
    pub const fn has_enabled_state(&self) -> bool {
        self.bits & Self::HAS_ENABLED_STATE != 0
    }

    /// Sets whether the node has an enabled state
    pub fn set_has_enabled_state(&mut self, value: bool) {
        if value {
            self.bits |= Self::HAS_ENABLED_STATE;
        } else {
            self.bits &= !Self::HAS_ENABLED_STATE;
        }
    }

    /// Checks if the node is enabled
    pub const fn is_enabled(&self) -> bool {
        self.bits & Self::IS_ENABLED != 0
    }

    /// Sets whether the node is enabled
    pub fn set_is_enabled(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_ENABLED;
        } else {
            self.bits &= !Self::IS_ENABLED;
        }
    }

    /// Checks if the node is hidden
    pub const fn is_hidden(&self) -> bool {
        self.bits & Self::IS_HIDDEN != 0
    }

    /// Sets whether the node is hidden
    pub fn set_is_hidden(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_HIDDEN;
        } else {
            self.bits &= !Self::IS_HIDDEN;
        }
    }

    /// Checks if the node is an image
    pub const fn is_image(&self) -> bool {
        self.bits & Self::IS_IMAGE != 0
    }

    /// Sets whether the node is an image
    pub fn set_is_image(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_IMAGE;
        } else {
            self.bits &= !Self::IS_IMAGE;
        }
    }

    /// Checks if the node is focusable
    pub const fn is_focusable(&self) -> bool {
        self.bits & Self::IS_FOCUSABLE != 0
    }

    /// Sets whether the node is focusable
    pub fn set_is_focusable(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_FOCUSABLE;
        } else {
            self.bits &= !Self::IS_FOCUSABLE;
        }
    }

    /// Checks if the node is a link
    pub const fn is_link(&self) -> bool {
        self.bits & Self::IS_LINK != 0
    }

    /// Sets whether the node is a link
    pub fn set_is_link(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_LINK;
        } else {
            self.bits &= !Self::IS_LINK;
        }
    }

    /// Checks if the node is a slider
    pub const fn is_slider(&self) -> bool {
        self.bits & Self::IS_SLIDER != 0
    }

    /// Sets whether the node is a slider
    pub fn set_is_slider(&mut self, value: bool) {
        if value {
            self.bits |= Self::IS_SLIDER;
        } else {
            self.bits &= !Self::IS_SLIDER;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_flags_default() {
        let flags = SemanticsFlags::default();
        assert!(!flags.is_button());
        assert!(!flags.is_text_field());
        assert!(!flags.is_focused());
    }

    #[test]
    fn test_semantics_flags_set_is_button() {
        let mut flags = SemanticsFlags::new();
        assert!(!flags.is_button());

        flags.set_is_button(true);
        assert!(flags.is_button());

        flags.set_is_button(false);
        assert!(!flags.is_button());
    }

    #[test]
    fn test_semantics_flags_multiple() {
        let mut flags = SemanticsFlags::new();
        flags.set_is_button(true);
        flags.set_is_focusable(true);

        assert!(flags.is_button());
        assert!(flags.is_focusable());
        assert!(!flags.is_text_field());
    }

    #[test]
    fn test_semantics_flags_checked_state() {
        let mut flags = SemanticsFlags::new();
        flags.set_has_checked_state(true);
        flags.set_is_checked(true);

        assert!(flags.has_checked_state());
        assert!(flags.is_checked());
    }

    #[test]
    fn test_semantics_flags_enabled_state() {
        let mut flags = SemanticsFlags::new();
        flags.set_has_enabled_state(true);
        flags.set_is_enabled(false);

        assert!(flags.has_enabled_state());
        assert!(!flags.is_enabled());
    }
}
