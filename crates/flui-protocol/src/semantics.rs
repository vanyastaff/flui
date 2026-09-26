//! FLUI's own semantics vocabulary: the structural roles a node declares and
//! the actions assistive technology can request of it.
//!
//! Both are modelled on `dart:ui`'s enums of the same names, and each enum's
//! doc says where its values part from Flutter's. The values
//! ([`SemanticsRole::value`], [`SemanticsAction::value`]) are part of this
//! crate's stable surface. Interactive kinds (button, checkbox, slider, text field) are not
//! roles: flui-semantics carries them as `SemanticsFlag`s, as Flutter does.
//!
//! These enums are not the agent-protocol wire vocabulary (see
//! [`crate::wire`]) and carry no serde derive: nothing has chosen a
//! serialized spelling for them yet.

vocabulary! {
    /// The role of a semantics node.
    ///
    /// Roles provide additional context about the structural type of UI
    /// element to assistive technologies. This helps screen readers and other
    /// accessibility tools present the correct interaction model.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `SemanticsRole` enum from `dart:ui`: every
    /// [`name`](Self::name) is the identifier of a Flutter role. The numeric
    /// [`value`](Self::value) is FLUI's own and is not Flutter's index —
    /// Flutter declares its roles in another order and has a `slider` role,
    /// which FLUI expresses as a flag (checked against
    /// `engine/src/flutter/lib/ui/semantics.dart` on 2026-09-26).
    ///
    /// # Note
    ///
    /// Interactive element types (Button, Checkbox, Slider, etc.) are
    /// represented as flui-semantics' `SemanticsFlag`, not roles. Roles are for
    /// structural elements like tables, menus, landmarks, and regions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[repr(u32)]
    pub enum SemanticsRole {
        /// No specific role assigned.
        #[default]
        None = 0 => "none",
        /// A dialog that alerts the user.
        AlertDialog = 1 => "alertDialog",
        /// A dialog window.
        Dialog = 2 => "dialog",
        /// A tab in a tab bar.
        Tab = 3 => "tab",
        /// A container for tabs.
        TabBar = 4 => "tabBar",
        /// The content panel for a tab.
        TabPanel = 5 => "tabPanel",
        /// A table structure.
        Table = 6 => "table",
        /// A cell in a table.
        Cell = 7 => "cell",
        /// A row in a table.
        Row = 8 => "row",
        /// A column header in a table.
        ColumnHeader = 9 => "columnHeader",
        /// A group of mutually exclusive radio buttons.
        RadioGroup = 10 => "radioGroup",
        /// A menu container.
        Menu = 11 => "menu",
        /// A horizontal menu bar.
        MenuBar = 12 => "menuBar",
        /// An item in a menu.
        MenuItem = 13 => "menuItem",
        /// A checkbox item in a menu.
        MenuItemCheckbox = 14 => "menuItemCheckbox",
        /// A radio item in a menu.
        MenuItemRadio = 15 => "menuItemRadio",
        /// An alert message (live region).
        Alert = 16 => "alert",
        /// A status message (live region).
        Status = 17 => "status",
        /// A list container.
        List = 18 => "list",
        /// An item in a list.
        ListItem = 19 => "listItem",
        /// Complementary content (sidebar, etc.).
        Complementary = 20 => "complementary",
        /// Footer/content info region.
        ContentInfo = 21 => "contentInfo",
        /// Main content region.
        Main = 22 => "main",
        /// Navigation region.
        Navigation = 23 => "navigation",
        /// A generic region with a label.
        Region = 24 => "region",
        /// A form container.
        Form = 25 => "form",
        /// A handle for drag and drop.
        DragHandle = 26 => "dragHandle",
        /// A spin button (numeric stepper).
        SpinButton = 27 => "spinButton",
        /// A combo box (dropdown with text input).
        ComboBox = 28 => "comboBox",
        /// A tooltip popup.
        Tooltip = 29 => "tooltip",
        /// A loading spinner/indicator.
        LoadingSpinner = 30 => "loadingSpinner",
        /// A progress bar.
        ProgressBar = 31 => "progressBar",
        /// A keyboard shortcut indicator.
        HotKey = 32 => "hotKey",
    }
}

impl SemanticsRole {
    /// Returns the numeric value of this role: its position in [`Self::ALL`].
    #[inline]
    #[must_use]
    pub const fn value(self) -> u32 {
        self as u32
    }

    /// Returns whether this is a landmark role.
    ///
    /// Landmark roles help users navigate the page structure.
    #[must_use]
    pub const fn is_landmark(self) -> bool {
        matches!(
            self,
            Self::Complementary | Self::ContentInfo | Self::Main | Self::Navigation | Self::Region
        )
    }

    /// Returns whether this is a live region role.
    ///
    /// Live region roles announce changes automatically.
    #[must_use]
    pub const fn is_live_region(self) -> bool {
        matches!(self, Self::Alert | Self::Status)
    }

    /// Returns whether this is a menu-related role.
    #[must_use]
    pub const fn is_menu_related(self) -> bool {
        matches!(
            self,
            Self::Menu
                | Self::MenuBar
                | Self::MenuItem
                | Self::MenuItemCheckbox
                | Self::MenuItemRadio
        )
    }

    /// Returns whether this is a table-related role.
    #[must_use]
    pub const fn is_table_related(self) -> bool {
        matches!(
            self,
            Self::Table | Self::Cell | Self::Row | Self::ColumnHeader
        )
    }

    /// Returns whether this is a dialog role.
    #[must_use]
    pub const fn is_dialog(self) -> bool {
        matches!(self, Self::Dialog | Self::AlertDialog)
    }

    /// Returns whether this is a list-related role.
    #[must_use]
    pub const fn is_list_related(self) -> bool {
        matches!(self, Self::List | Self::ListItem)
    }

    /// Returns whether this is a tab-related role.
    #[must_use]
    pub const fn is_tab_related(self) -> bool {
        matches!(self, Self::Tab | Self::TabBar | Self::TabPanel)
    }
}

impl std::fmt::Display for SemanticsRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

vocabulary! {
    /// Actions that can be performed on a semantics node.
    ///
    /// These correspond to actions that assistive technologies can request,
    /// such as screen readers activating a button. Each action is one bit, so a
    /// node's supported actions travel as a `u64` mask.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `SemanticsAction` enum. `Tap` through `Focus`
    /// carry Flutter's bits (`1 << 0` to `1 << 22`, checked against
    /// `engine/src/flutter/lib/ui/semantics.dart` on 2026-09-26). The rest
    /// differ: Flutter gives `scrollToOffset` `1 << 23` and has `expand`
    /// (`1 << 24`) and `collapse` (`1 << 25`), while FLUI keeps those three
    /// bits as [`RESERVED_BITS`](Self::RESERVED_BITS), has no expand or
    /// collapse action (an expandable node toggles through its tap handler),
    /// and puts [`ScrollToOffset`](Self::ScrollToOffset) at `1 << 26`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[repr(u64)]
    pub enum SemanticsAction {
        /// Tap action (like clicking a button).
        Tap = 1 << 0 => "tap",
        /// Long press action.
        LongPress = 1 << 1 => "longPress",
        /// Scroll left action.
        ScrollLeft = 1 << 2 => "scrollLeft",
        /// Scroll right action.
        ScrollRight = 1 << 3 => "scrollRight",
        /// Scroll up action.
        ScrollUp = 1 << 4 => "scrollUp",
        /// Scroll down action.
        ScrollDown = 1 << 5 => "scrollDown",
        /// Increase action (for sliders, steppers).
        Increase = 1 << 6 => "increase",
        /// Decrease action (for sliders, steppers).
        Decrease = 1 << 7 => "decrease",
        /// Show on-screen keyboard.
        ShowOnScreen = 1 << 8 => "showOnScreen",
        /// Move cursor forward by character.
        MoveCursorForwardByCharacter = 1 << 9 => "moveCursorForwardByCharacter",
        /// Move cursor backward by character.
        MoveCursorBackwardByCharacter = 1 << 10 => "moveCursorBackwardByCharacter",
        /// Set selection in text field.
        SetSelection = 1 << 11 => "setSelection",
        /// Copy text.
        Copy = 1 << 12 => "copy",
        /// Cut text.
        Cut = 1 << 13 => "cut",
        /// Paste text.
        Paste = 1 << 14 => "paste",
        /// Did gain accessibility focus.
        DidGainAccessibilityFocus = 1 << 15 => "didGainAccessibilityFocus",
        /// Did lose accessibility focus.
        DidLoseAccessibilityFocus = 1 << 16 => "didLoseAccessibilityFocus",
        /// Custom action.
        CustomAction = 1 << 17 => "customAction",
        /// Dismiss action (for dialogs, drawers).
        Dismiss = 1 << 18 => "dismiss",
        /// Move cursor forward by word.
        MoveCursorForwardByWord = 1 << 19 => "moveCursorForwardByWord",
        /// Move cursor backward by word.
        MoveCursorBackwardByWord = 1 << 20 => "moveCursorBackwardByWord",
        /// Set text content.
        SetText = 1 << 21 => "setText",
        /// Focus action.
        Focus = 1 << 22 => "focus",
        /// Scroll to a specific offset.
        ScrollToOffset = 1 << 26 => "scrollToOffset",
    }
}

impl SemanticsAction {
    /// Bits no action may take: `1 << 23`, `1 << 24` and `1 << 25`, the slots
    /// of FLUI's former `Unfocus`, `Expand` and `Collapse` actions.
    ///
    /// Flutter's `dart:ui` uses the same three bits for `scrollToOffset`,
    /// `expand` and `collapse`, so an action placed here takes the bit Flutter
    /// gives that action or none. FLUI keeps the expanded state on the
    /// `HasExpandedState`/`IsExpanded` flags and routes a platform expand or
    /// collapse to the tap handler; un-focus belongs to the platform's focus
    /// management.
    pub const RESERVED_BITS: u64 = (1 << 23) | (1 << 24) | (1 << 25);

    /// Returns the bitmask value for this action (see the enum doc for how it
    /// relates to Flutter's `dart:ui` bit).
    #[inline]
    #[must_use]
    pub const fn value(self) -> u64 {
        self as u64
    }

    /// Returns whether this action is a scroll action.
    #[must_use]
    pub const fn is_scroll_action(self) -> bool {
        matches!(
            self,
            Self::ScrollLeft
                | Self::ScrollRight
                | Self::ScrollUp
                | Self::ScrollDown
                | Self::ScrollToOffset
        )
    }

    /// Returns whether this action is a cursor movement action.
    #[must_use]
    pub const fn is_cursor_action(self) -> bool {
        matches!(
            self,
            Self::MoveCursorForwardByCharacter
                | Self::MoveCursorBackwardByCharacter
                | Self::MoveCursorForwardByWord
                | Self::MoveCursorBackwardByWord
        )
    }

    /// Returns whether this action is a text editing action.
    #[must_use]
    pub const fn is_text_action(self) -> bool {
        matches!(
            self,
            Self::SetSelection | Self::SetText | Self::Copy | Self::Cut | Self::Paste
        )
    }

    /// Returns whether this action is a focus-related action.
    #[must_use]
    pub const fn is_focus_action(self) -> bool {
        matches!(
            self,
            Self::Focus | Self::DidGainAccessibilityFocus | Self::DidLoseAccessibilityFocus
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// A mistyped discriminant would move a role's published value, and a
    /// duplicate would make two roles indistinguishable. This walks every
    /// role rather than pinning a few values by hand.
    #[test]
    fn every_role_is_listed_once_and_valued_by_its_index() {
        let distinct: HashSet<SemanticsRole> = SemanticsRole::ALL.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            SemanticsRole::ALL.len(),
            "a role is listed twice"
        );
        assert_eq!(SemanticsRole::ALL.len(), 33);

        for (index, role) in SemanticsRole::ALL.iter().enumerate() {
            assert_eq!(
                usize::try_from(role.value()).ok(),
                Some(index),
                "{role} is declared at position {index} but valued {}",
                role.value(),
            );
        }
        assert_eq!(SemanticsRole::ALL[0], SemanticsRole::None);
        assert_eq!(SemanticsRole::default(), SemanticsRole::None);
    }

    /// Every action is one bit, no two share one, and none takes a reserved
    /// slot — the rule the enum's doc states, checked instead of trusted.
    #[test]
    fn every_action_is_one_distinct_unreserved_bit() {
        let mut seen = 0_u64;
        for action in SemanticsAction::ALL {
            let bit = action.value();
            assert!(
                bit.is_power_of_two(),
                "{} is not a single bit",
                action.name()
            );
            assert_eq!(seen & bit, 0, "{} shares a bit", action.name());
            assert_eq!(
                bit & SemanticsAction::RESERVED_BITS,
                0,
                "{} takes a reserved bit",
                action.name(),
            );
            seen |= bit;
        }
        assert_eq!(SemanticsAction::ALL.len(), 24);
        assert_eq!(SemanticsAction::RESERVED_BITS.count_ones(), 3);
    }

    #[test]
    fn names_are_flutters_camel_case() {
        assert_eq!(SemanticsRole::None.name(), "none");
        assert_eq!(SemanticsRole::AlertDialog.name(), "alertDialog");
        assert_eq!(SemanticsRole::MenuItem.name(), "menuItem");
        assert_eq!(SemanticsRole::HotKey.to_string(), "hotKey");
        assert_eq!(SemanticsAction::Tap.name(), "tap");
        assert_eq!(SemanticsAction::LongPress.name(), "longPress");
        assert_eq!(SemanticsAction::ScrollToOffset.name(), "scrollToOffset");
    }

    #[test]
    fn action_values_combine_as_a_mask() {
        let combined = SemanticsAction::Tap.value() | SemanticsAction::LongPress.value();
        assert_eq!(combined, 3);
        assert_ne!(combined & SemanticsAction::Tap.value(), 0);
        assert_ne!(combined & SemanticsAction::LongPress.value(), 0);
        assert_eq!(combined & SemanticsAction::ScrollLeft.value(), 0);
    }

    #[test]
    fn test_role_is_landmark() {
        assert!(SemanticsRole::Main.is_landmark());
        assert!(SemanticsRole::Navigation.is_landmark());
        assert!(SemanticsRole::Complementary.is_landmark());
        assert!(!SemanticsRole::None.is_landmark());
        assert!(!SemanticsRole::Menu.is_landmark());
    }

    #[test]
    fn test_role_is_live_region() {
        assert!(SemanticsRole::Alert.is_live_region());
        assert!(SemanticsRole::Status.is_live_region());
        assert!(!SemanticsRole::None.is_live_region());
    }

    #[test]
    fn test_role_is_menu_related() {
        assert!(SemanticsRole::Menu.is_menu_related());
        assert!(SemanticsRole::MenuBar.is_menu_related());
        assert!(SemanticsRole::MenuItem.is_menu_related());
        assert!(!SemanticsRole::List.is_menu_related());
    }

    #[test]
    fn test_role_is_table_related() {
        assert!(SemanticsRole::Table.is_table_related());
        assert!(SemanticsRole::Cell.is_table_related());
        assert!(SemanticsRole::Row.is_table_related());
        assert!(!SemanticsRole::List.is_table_related());
    }

    #[test]
    fn test_role_is_dialog() {
        assert!(SemanticsRole::Dialog.is_dialog());
        assert!(SemanticsRole::AlertDialog.is_dialog());
        assert!(!SemanticsRole::Menu.is_dialog());
    }

    #[test]
    fn test_role_is_list_related() {
        assert!(SemanticsRole::List.is_list_related());
        assert!(SemanticsRole::ListItem.is_list_related());
        assert!(!SemanticsRole::Table.is_list_related());
    }

    #[test]
    fn test_role_is_tab_related() {
        assert!(SemanticsRole::Tab.is_tab_related());
        assert!(SemanticsRole::TabBar.is_tab_related());
        assert!(SemanticsRole::TabPanel.is_tab_related());
        assert!(!SemanticsRole::Menu.is_tab_related());
    }
}
