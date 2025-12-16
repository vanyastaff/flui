//! Semantics flags for boolean properties.
//!
//! This module provides flag types for accessibility properties.

// ============================================================================
// SemanticsFlag
// ============================================================================

/// Boolean properties of a semantics node.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsFlag` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum SemanticsFlag {
    /// Has checked state (for checkboxes, etc.).
    HasCheckedState = 1 << 0,

    /// Is checked.
    IsChecked = 1 << 1,

    /// Is selected.
    IsSelected = 1 << 2,

    /// Is button.
    IsButton = 1 << 3,

    /// Is link.
    IsLink = 1 << 4,

    /// Is text field.
    IsTextField = 1 << 5,

    /// Is slider.
    IsSlider = 1 << 6,

    /// Is keyboard key.
    IsKeyboardKey = 1 << 7,

    /// Is read-only.
    IsReadOnly = 1 << 8,

    /// Is focusable.
    IsFocusable = 1 << 9,

    /// Is focused.
    IsFocused = 1 << 10,

    /// Has enabled state.
    HasEnabledState = 1 << 11,

    /// Is enabled.
    IsEnabled = 1 << 12,

    /// Is in mutually exclusive group.
    IsInMutuallyExclusiveGroup = 1 << 13,

    /// Is header.
    IsHeader = 1 << 14,

    /// Is obscured (for password fields).
    IsObscured = 1 << 15,

    /// Scope route (modal barrier).
    ScopesRoute = 1 << 16,

    /// Names route.
    NamesRoute = 1 << 17,

    /// Is hidden.
    IsHidden = 1 << 18,

    /// Is image.
    IsImage = 1 << 19,

    /// Is live region.
    IsLiveRegion = 1 << 20,

    /// Has toggled state.
    HasToggledState = 1 << 21,

    /// Is toggled.
    IsToggled = 1 << 22,

    /// Has implicit scrolling.
    HasImplicitScrolling = 1 << 23,

    /// Is multiline.
    IsMultiline = 1 << 24,

    /// Is expanded.
    IsExpanded = 1 << 25,

    /// Is checkstate mixed (indeterminate).
    IsCheckStateMixed = 1 << 26,
}

impl SemanticsFlag {
    /// Returns the bitmask value for this flag.
    #[inline]
    pub fn value(self) -> u64 {
        self as u64
    }

    /// Returns the name of this flag.
    pub fn name(self) -> &'static str {
        match self {
            Self::HasCheckedState => "hasCheckedState",
            Self::IsChecked => "isChecked",
            Self::IsSelected => "isSelected",
            Self::IsButton => "isButton",
            Self::IsLink => "isLink",
            Self::IsTextField => "isTextField",
            Self::IsSlider => "isSlider",
            Self::IsKeyboardKey => "isKeyboardKey",
            Self::IsReadOnly => "isReadOnly",
            Self::IsFocusable => "isFocusable",
            Self::IsFocused => "isFocused",
            Self::HasEnabledState => "hasEnabledState",
            Self::IsEnabled => "isEnabled",
            Self::IsInMutuallyExclusiveGroup => "isInMutuallyExclusiveGroup",
            Self::IsHeader => "isHeader",
            Self::IsObscured => "isObscured",
            Self::ScopesRoute => "scopesRoute",
            Self::NamesRoute => "namesRoute",
            Self::IsHidden => "isHidden",
            Self::IsImage => "isImage",
            Self::IsLiveRegion => "isLiveRegion",
            Self::HasToggledState => "hasToggledState",
            Self::IsToggled => "isToggled",
            Self::HasImplicitScrolling => "hasImplicitScrolling",
            Self::IsMultiline => "isMultiline",
            Self::IsExpanded => "isExpanded",
            Self::IsCheckStateMixed => "isCheckStateMixed",
        }
    }
}

// ============================================================================
// SemanticsFlags
// ============================================================================

/// A set of semantics flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticsFlags {
    /// The bitmask of flags.
    flags: u64,
}

impl SemanticsFlags {
    /// Creates an empty flags set.
    #[inline]
    pub fn new() -> Self {
        Self { flags: 0 }
    }

    /// Creates flags from a bitmask.
    #[inline]
    pub fn from_bits(bits: u64) -> Self {
        Self { flags: bits }
    }

    /// Returns the raw bitmask.
    #[inline]
    pub fn bits(&self) -> u64 {
        self.flags
    }

    /// Returns whether the given flag is set.
    #[inline]
    pub fn has(&self, flag: SemanticsFlag) -> bool {
        self.flags & flag.value() != 0
    }

    /// Sets a flag.
    #[inline]
    pub fn set(&mut self, flag: SemanticsFlag) {
        self.flags |= flag.value();
    }

    /// Clears a flag.
    #[inline]
    pub fn clear(&mut self, flag: SemanticsFlag) {
        self.flags &= !flag.value();
    }

    /// Toggles a flag.
    #[inline]
    pub fn toggle(&mut self, flag: SemanticsFlag) {
        self.flags ^= flag.value();
    }

    /// Returns whether any flags are set.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.flags == 0
    }

    /// Merges another flags set into this one.
    #[inline]
    pub fn merge(&mut self, other: &Self) {
        self.flags |= other.flags;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_values() {
        assert_eq!(SemanticsFlag::HasCheckedState.value(), 1);
        assert_eq!(SemanticsFlag::IsChecked.value(), 2);
        assert_eq!(SemanticsFlag::IsSelected.value(), 4);
    }

    #[test]
    fn test_flags_operations() {
        let mut flags = SemanticsFlags::new();
        assert!(flags.is_empty());

        flags.set(SemanticsFlag::IsButton);
        assert!(flags.has(SemanticsFlag::IsButton));
        assert!(!flags.has(SemanticsFlag::IsLink));

        flags.set(SemanticsFlag::IsLink);
        assert!(flags.has(SemanticsFlag::IsLink));

        flags.clear(SemanticsFlag::IsButton);
        assert!(!flags.has(SemanticsFlag::IsButton));
    }

    #[test]
    fn test_flags_merge() {
        let mut flags1 = SemanticsFlags::new();
        flags1.set(SemanticsFlag::IsButton);

        let mut flags2 = SemanticsFlags::new();
        flags2.set(SemanticsFlag::IsEnabled);

        flags1.merge(&flags2);
        assert!(flags1.has(SemanticsFlag::IsButton));
        assert!(flags1.has(SemanticsFlag::IsEnabled));
    }

    #[test]
    fn test_flags_toggle() {
        let mut flags = SemanticsFlags::new();

        flags.toggle(SemanticsFlag::IsButton);
        assert!(flags.has(SemanticsFlag::IsButton));

        flags.toggle(SemanticsFlag::IsButton);
        assert!(!flags.has(SemanticsFlag::IsButton));
    }
}
