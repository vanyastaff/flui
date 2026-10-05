//! FLUI's own semantics vocabulary: the structural roles a node declares and
//! the actions assistive technology can request of it.
//!
//! The values ([`SemanticsRole::value`], [`SemanticsAction::value`]) are part
//! of this crate's stable surface. Interactive kinds (button, checkbox,
//! slider, text field) are not roles: flui-semantics carries them as
//! `SemanticsFlag`s.
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
    /// # Values
    ///
    /// The numeric [`value`](Self::value) is FLUI's own and is stable. There
    /// is no `slider` role: FLUI expresses it as a flag.
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
    /// # Bit layout
    ///
    /// `Tap` through `Focus` take `1 << 0` to `1 << 22`. Bits `1 << 23` to
    /// `1 << 25` are kept as [`RESERVED_BITS`](Self::RESERVED_BITS): there
    /// [`ScrollToOffset`](Self::ScrollToOffset) uses `1 << 26`, while the
    /// directional and numeric actions use `1 << 27` through `1 << 29`.
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
        /// Set an expandable control to its expanded state.
        Expand = 1 << 27 => "expand",
        /// Set an expandable control to its collapsed state.
        Collapse = 1 << 28 => "collapse",
        /// Set an exact numeric value within the control's admitted range.
        SetNumericValue = 1 << 29 => "setNumericValue",
    }
}

impl SemanticsAction {
    /// Bits no action may take: `1 << 23`, `1 << 24` and `1 << 25`, the slots
    /// of FLUI's former `Unfocus`, `Expand` and `Collapse` actions.
    ///
    /// Platform accessibility bit layouts (`dart:ui`'s among them) assign
    /// these three bits to other actions, so an action placed here would
    /// collide with them. Discrete expand and collapse use distinct FLUI-owned
    /// slots; un-focus belongs to the platform's focus management.
    pub const RESERVED_BITS: u64 = (1 << 23) | (1 << 24) | (1 << 25);

    /// Returns the bitmask value for this action (see the enum doc for the
    /// bit layout).
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
