//! FLUI's own semantics vocabulary: the structural roles a node declares and
//! the actions assistive technology can request of it.
//!
//! The values ([`SemanticsRole::value`], [`SemanticsAction::value`]) are part
//! of this crate's stable surface. A node has exactly one role, interactive
//! kinds included (ADR-0202, ADR-0203).
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
    /// The numeric [`value`](Self::value) is FLUI's own and is stable.
    /// `None` declares no role: a named node publishes as static text, an
    /// unnamed one as a plain container.
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
        /// A viewport whose content can be scrolled.
        ScrollView = 33 => "scrollView",
        /// A push button; with a toggled state, a toggle button whose state is
        /// its pressed state.
        Button = 34 => "button",
        /// A two- or three-state check box.
        CheckBox = 35 => "checkBox",
        /// One option of a mutually exclusive set.
        RadioButton = 36 => "radioButton",
        /// An on/off switch.
        Switch = 37 => "switch",
        /// A hyperlink.
        Link = 38 => "link",
        /// A slider over a range of values.
        Slider = 39 => "slider",
        /// A picture.
        Image = 40 => "image",
        /// A heading; its outline level is an attribute.
        Heading = 41 => "heading",
        /// A single-line editable text field.
        TextInput = 42 => "textInput",
        /// A multi-line editable text field.
        MultilineTextInput = 43 => "multilineTextInput",
        /// A text field whose characters are obscured.
        PasswordInput = 44 => "passwordInput",
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

    /// Returns whether this is an editable text field of any kind.
    #[must_use]
    pub const fn is_text_input(self) -> bool {
        matches!(
            self,
            Self::TextInput | Self::MultilineTextInput | Self::PasswordInput
        )
    }
}

impl std::fmt::Display for SemanticsRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

vocabulary! {
    /// What assistive technology can ask of a semantics node.
    ///
    /// The list follows `accesskit::Action` (ADR-0203), so every request a
    /// platform adapter can deliver has a name here and every name has a
    /// platform path. Each action is one bit, so a node's supported actions
    /// travel as a `u64` mask.
    ///
    /// AccessKit's one `SetValue` carries a text or a number; FLUI keeps the
    /// two as [`SetValue`](Self::SetValue) and
    /// [`SetNumericValue`](Self::SetNumericValue), because which of them a
    /// node accepts decides what it publishes (the value pattern it offers and
    /// whether a range is writable).
    ///
    /// # Bit layout
    ///
    /// Bits are dense from zero in declaration order: [`value`](Self::value)
    /// is `1 << position`. No bit is reserved.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[repr(u8)]
    pub enum SemanticsAction {
        /// Activate the node: press a button, toggle a check box, follow a link.
        Click => "click",
        /// Give the node keyboard focus.
        Focus => "focus",
        /// Take keyboard focus away from the node.
        Blur => "blur",
        /// Set an expandable control to its expanded state.
        Expand => "expand",
        /// Set an expandable control to its collapsed state.
        Collapse => "collapse",
        /// Raise the value one step.
        Increment => "increment",
        /// Lower the value one step.
        Decrement => "decrement",
        /// Scroll the content up.
        ScrollUp => "scrollUp",
        /// Scroll the content down.
        ScrollDown => "scrollDown",
        /// Scroll the content left.
        ScrollLeft => "scrollLeft",
        /// Scroll the content right.
        ScrollRight => "scrollRight",
        /// Bring the node into view, scrolling whatever contains it.
        ScrollIntoView => "scrollIntoView",
        /// Scroll the node's content to an offset in its own coordinates.
        SetScrollOffset => "setScrollOffset",
        /// Select a range of the node's text.
        SetTextSelection => "setTextSelection",
        /// Replace the selected text.
        ReplaceSelectedText => "replaceSelectedText",
        /// Set the node's value to a text.
        SetValue => "setValue",
        /// Set an exact numeric value within the node's range.
        SetNumericValue => "setNumericValue",
        /// Open the node's context menu.
        ShowContextMenu => "showContextMenu",
        /// Show the node's tooltip.
        ShowTooltip => "showTooltip",
        /// Hide the node's tooltip.
        HideTooltip => "hideTooltip",
        /// Run one of the node's custom actions.
        CustomAction => "customAction",
    }
}

impl SemanticsAction {
    /// Returns the bitmask value for this action: `1 << position`, its
    /// position in [`Self::ALL`].
    #[inline]
    #[must_use]
    pub const fn value(self) -> u64 {
        1 << self as u8
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
                | Self::ScrollIntoView
                | Self::SetScrollOffset
        )
    }

    /// Returns whether this action edits or selects text.
    #[must_use]
    pub const fn is_text_action(self) -> bool {
        matches!(
            self,
            Self::SetTextSelection | Self::ReplaceSelectedText | Self::SetValue
        )
    }

    /// Returns whether this action moves keyboard focus.
    #[must_use]
    pub const fn is_focus_action(self) -> bool {
        matches!(self, Self::Focus | Self::Blur)
    }
}

impl std::fmt::Display for SemanticsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
