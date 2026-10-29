//! FLUI's own semantics vocabulary: the structural roles a node declares and
//! the actions assistive technology can request of it.
//!
//! The values ([`SemanticsRole::value`], [`SemanticsAction::value`]) are part
//! of this crate's stable surface. A node has exactly one role, interactive
//! kinds included (ADR-0206, ADR-0203).
//!
//! These enums are not the agent-protocol wire vocabulary (see
//! [`crate::wire`]) and carry no serde derive: nothing has chosen a
//! serialized spelling for them yet.

vocabulary! {
    /// The role of a semantics node.
    ///
    /// Every non-abstract WAI-ARIA 1.2 role with an AccessKit counterpart, the
    /// editable-text kinds AccessKit distinguishes, and the desktop roles a
    /// window needs (ADR-0203). DPUB-ARIA and Graphics-ARIA roles are out of
    /// scope. The enum is flat: data ARIA ties to a role (a heading's level, a
    /// cell's position) is an attribute.
    ///
    /// # Values
    ///
    /// The numeric [`value`](Self::value) is dense from zero in declaration
    /// order. `None` declares no role: a named node publishes as static text,
    /// an unnamed one as a plain container.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[repr(u32)]
    pub enum SemanticsRole {
        /// No specific role assigned.
        #[default]
        None => "none",

        // ----- Controls -----
        /// A push button; with a toggled state, a toggle button whose state is
        /// its pressed state.
        Button => "button",
        /// A two- or three-state check box.
        CheckBox => "checkBox",
        /// One option of a mutually exclusive set.
        RadioButton => "radioButton",
        /// An on/off switch.
        Switch => "switch",
        /// A hyperlink.
        Link => "link",
        /// A slider over a range of values.
        Slider => "slider",
        /// A numeric stepper.
        SpinButton => "spinButton",
        /// A text field with a popup of suggestions.
        ComboBox => "comboBox",
        /// A scroll bar.
        ScrollBar => "scrollBar",
        /// A separator the user can move to resize what it divides.
        Separator => "separator",

        // ----- Editable text -----
        /// A single-line editable text field.
        TextInput => "textInput",
        /// A multi-line editable text field.
        MultilineTextInput => "multilineTextInput",
        /// A text field whose characters are obscured.
        PasswordInput => "passwordInput",
        /// A text field for a search query.
        SearchInput => "searchInput",
        /// A text field for an email address.
        EmailInput => "emailInput",
        /// A text field for a number.
        NumberInput => "numberInput",
        /// A text field for a telephone number.
        PhoneNumberInput => "phoneNumberInput",
        /// A text field for a URL.
        UrlInput => "urlInput",
        /// A field for a date.
        DateInput => "dateInput",
        /// A field for a time of day.
        TimeInput => "timeInput",

        // ----- Composite widgets -----
        /// A list of options to pick from.
        ListBox => "listBox",
        /// One option of a list box.
        ListBoxOption => "listBoxOption",
        /// A group of mutually exclusive radio buttons.
        RadioGroup => "radioGroup",
        /// A menu container.
        Menu => "menu",
        /// A horizontal menu bar.
        MenuBar => "menuBar",
        /// An item in a menu.
        MenuItem => "menuItem",
        /// A checkable item in a menu.
        MenuItemCheckbox => "menuItemCheckbox",
        /// One of a mutually exclusive set of items in a menu.
        MenuItemRadio => "menuItemRadio",
        /// A tab in a tab list.
        Tab => "tab",
        /// A list of tabs.
        TabList => "tabList",
        /// The content panel for a tab.
        TabPanel => "tabPanel",
        /// A hierarchical list whose items expand and collapse.
        Tree => "tree",
        /// An item in a tree.
        TreeItem => "treeItem",
        /// A grid whose rows expand and collapse like a tree's items.
        TreeGrid => "treeGrid",
        /// An interactive grid of cells.
        Grid => "grid",
        /// A cell in a grid.
        GridCell => "gridCell",
        /// A row of controls.
        Toolbar => "toolbar",

        // ----- Tables and lists -----
        /// A static table.
        Table => "table",
        /// A row in a table or grid.
        Row => "row",
        /// A group of rows: a table's header, body or footer.
        RowGroup => "rowGroup",
        /// A cell in a table.
        Cell => "cell",
        /// A column's header cell.
        ColumnHeader => "columnHeader",
        /// A row's header cell.
        RowHeader => "rowHeader",
        /// A list container.
        List => "list",
        /// An item in a list.
        ListItem => "listItem",

        // ----- Document structure -----
        /// A heading; its outline level is an attribute.
        Heading => "heading",
        /// A paragraph of text.
        Paragraph => "paragraph",
        /// A picture.
        Image => "image",
        /// A figure: content with its caption.
        Figure => "figure",
        /// The caption of a figure or table.
        Caption => "caption",
        /// A self-contained composition, such as a post or a card.
        Article => "article",
        /// A scrolling list of articles that loads more as it is read.
        Feed => "feed",
        /// A set of related elements not otherwise grouped.
        Group => "group",
        /// A note set aside from the main content.
        Note => "note",
        /// A quotation from another source.
        Blockquote => "blockquote",
        /// A fragment of computer code.
        Code => "code",
        /// Emphasized text.
        Emphasis => "emphasis",
        /// Text of strong importance.
        Strong => "strong",
        /// Text added to a document.
        Insertion => "insertion",
        /// Text removed from a document.
        Deletion => "deletion",
        /// A term being defined.
        Term => "term",
        /// The definition of a term.
        Definition => "definition",
        /// A date or time.
        Time => "time",
        /// A mathematical expression.
        Math => "math",
        /// A scalar measurement within a known range.
        Meter => "meter",
        /// The progress of a task; without a value, an indeterminate one.
        ProgressBar => "progressBar",
        /// A popup describing an element.
        Tooltip => "tooltip",
        /// A document to read rather than operate.
        Document => "document",
        /// A region the application operates itself, bypassing the screen
        /// reader's document navigation.
        Application => "application",

        // ----- Landmarks -----
        /// The site- or app-wide header.
        Banner => "banner",
        /// The main content.
        Main => "main",
        /// Navigation links.
        Navigation => "navigation",
        /// Content complementary to the main content.
        Complementary => "complementary",
        /// Information about the page: the footer.
        ContentInfo => "contentInfo",
        /// A search facility.
        Search => "search",
        /// A form.
        Form => "form",
        /// A generic region with a label.
        Region => "region",

        // ----- Live regions -----
        /// An important, time-sensitive message.
        Alert => "alert",
        /// Advisory information that is not urgent.
        Status => "status",
        /// A log whose new entries are announced.
        Log => "log",
        /// Information that changes and is not essential, such as a ticker.
        Marquee => "marquee",
        /// A counter of elapsed or remaining time.
        Timer => "timer",

        // ----- Windows -----
        /// A dialog window.
        Dialog => "dialog",
        /// A dialog that alerts the user.
        AlertDialog => "alertDialog",
        /// A top-level window.
        Window => "window",
        /// A region of a window, such as a split view's side.
        Pane => "pane",
        /// A viewport whose content can be scrolled.
        ScrollView => "scrollView",

        // ----- Structure without meaning -----
        /// A container declared to mean nothing (ARIA `generic`): published
        /// for structure, with no name. Unlike [`Self::None`], a label does not
        /// make it static text.
        Generic => "generic",
        /// A node whose own semantics are removed (ARIA `presentation`, `none`);
        /// its descendants keep theirs.
        Presentation => "presentation",
    }
}

impl SemanticsRole {
    /// Returns the numeric value of this role: its position in [`Self::ALL`].
    #[inline]
    #[must_use]
    pub const fn value(self) -> u32 {
        self as u32
    }

    /// Returns whether this is an ARIA landmark role.
    ///
    /// Landmark roles help users navigate the page structure.
    #[must_use]
    pub const fn is_landmark(self) -> bool {
        matches!(
            self,
            Self::Banner
                | Self::Main
                | Self::Navigation
                | Self::Complementary
                | Self::ContentInfo
                | Self::Search
                | Self::Form
                | Self::Region
        )
    }

    /// Returns whether this role is a live region whose changes are announced.
    #[must_use]
    pub const fn is_live_region(self) -> bool {
        matches!(
            self,
            Self::Alert | Self::Status | Self::Log | Self::Marquee | Self::Timer
        )
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

    /// Returns whether this is a role of a table or grid.
    #[must_use]
    pub const fn is_table_related(self) -> bool {
        matches!(
            self,
            Self::Table
                | Self::Grid
                | Self::TreeGrid
                | Self::Row
                | Self::RowGroup
                | Self::Cell
                | Self::GridCell
                | Self::ColumnHeader
                | Self::RowHeader
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
        matches!(
            self,
            Self::List | Self::ListItem | Self::ListBox | Self::ListBoxOption
        )
    }

    /// Returns whether this is a tab-related role.
    #[must_use]
    pub const fn is_tab_related(self) -> bool {
        matches!(self, Self::Tab | Self::TabList | Self::TabPanel)
    }

    /// Returns whether this is an editable text field of any kind.
    #[must_use]
    pub const fn is_text_input(self) -> bool {
        matches!(
            self,
            Self::TextInput
                | Self::MultilineTextInput
                | Self::PasswordInput
                | Self::SearchInput
                | Self::EmailInput
                | Self::NumberInput
                | Self::PhoneNumberInput
                | Self::UrlInput
                | Self::DateInput
                | Self::TimeInput
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

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0203's vocabulary, in declaration order: a role or action renamed,
    /// dropped, added or reordered fails here, and with it the numeric values,
    /// which are positions in this order.
    #[test]
    fn vocabulary_names_are_pinned() {
        let roles: Vec<&str> = SemanticsRole::ALL.iter().map(|r| r.name()).collect();
        assert_eq!(
            roles,
            [
                "none",
                "button",
                "checkBox",
                "radioButton",
                "switch",
                "link",
                "slider",
                "spinButton",
                "comboBox",
                "scrollBar",
                "separator",
                "textInput",
                "multilineTextInput",
                "passwordInput",
                "searchInput",
                "emailInput",
                "numberInput",
                "phoneNumberInput",
                "urlInput",
                "dateInput",
                "timeInput",
                "listBox",
                "listBoxOption",
                "radioGroup",
                "menu",
                "menuBar",
                "menuItem",
                "menuItemCheckbox",
                "menuItemRadio",
                "tab",
                "tabList",
                "tabPanel",
                "tree",
                "treeItem",
                "treeGrid",
                "grid",
                "gridCell",
                "toolbar",
                "table",
                "row",
                "rowGroup",
                "cell",
                "columnHeader",
                "rowHeader",
                "list",
                "listItem",
                "heading",
                "paragraph",
                "image",
                "figure",
                "caption",
                "article",
                "feed",
                "group",
                "note",
                "blockquote",
                "code",
                "emphasis",
                "strong",
                "insertion",
                "deletion",
                "term",
                "definition",
                "time",
                "math",
                "meter",
                "progressBar",
                "tooltip",
                "document",
                "application",
                "banner",
                "main",
                "navigation",
                "complementary",
                "contentInfo",
                "search",
                "form",
                "region",
                "alert",
                "status",
                "log",
                "marquee",
                "timer",
                "dialog",
                "alertDialog",
                "window",
                "pane",
                "scrollView",
                "generic",
                "presentation",
            ],
        );
        let actions: Vec<&str> = SemanticsAction::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(
            actions,
            [
                "click",
                "focus",
                "blur",
                "expand",
                "collapse",
                "increment",
                "decrement",
                "scrollUp",
                "scrollDown",
                "scrollLeft",
                "scrollRight",
                "scrollIntoView",
                "setScrollOffset",
                "setTextSelection",
                "replaceSelectedText",
                "setValue",
                "setNumericValue",
                "showContextMenu",
                "showTooltip",
                "hideTooltip",
                "customAction",
            ],
        );
        for (position, role) in SemanticsRole::ALL.iter().enumerate() {
            assert_eq!(role.value() as usize, position, "{role}");
        }
        for (position, action) in SemanticsAction::ALL.iter().enumerate() {
            assert_eq!(action.value(), 1 << position, "{action}");
        }
    }

    /// Every non-abstract WAI-ARIA 1.2 role, transcribed from the
    /// specification's role list, with the role FLUI publishes it as, or why
    /// it has none. Independent of `SemanticsRole::ALL`, so dropping a role
    /// from the enum cannot also drop it from the check.
    const ARIA_1_2_ROLES: &[(&str, Result<SemanticsRole, &str>)] = &[
        ("alert", Ok(SemanticsRole::Alert)),
        ("alertdialog", Ok(SemanticsRole::AlertDialog)),
        ("application", Ok(SemanticsRole::Application)),
        ("article", Ok(SemanticsRole::Article)),
        ("banner", Ok(SemanticsRole::Banner)),
        ("blockquote", Ok(SemanticsRole::Blockquote)),
        ("button", Ok(SemanticsRole::Button)),
        ("caption", Ok(SemanticsRole::Caption)),
        ("cell", Ok(SemanticsRole::Cell)),
        ("checkbox", Ok(SemanticsRole::CheckBox)),
        ("code", Ok(SemanticsRole::Code)),
        ("columnheader", Ok(SemanticsRole::ColumnHeader)),
        ("combobox", Ok(SemanticsRole::ComboBox)),
        ("complementary", Ok(SemanticsRole::Complementary)),
        ("contentinfo", Ok(SemanticsRole::ContentInfo)),
        ("definition", Ok(SemanticsRole::Definition)),
        ("deletion", Ok(SemanticsRole::Deletion)),
        ("dialog", Ok(SemanticsRole::Dialog)),
        ("directory", Err("deprecated in ARIA 1.2; a list")),
        ("document", Ok(SemanticsRole::Document)),
        ("emphasis", Ok(SemanticsRole::Emphasis)),
        ("feed", Ok(SemanticsRole::Feed)),
        ("figure", Ok(SemanticsRole::Figure)),
        ("form", Ok(SemanticsRole::Form)),
        ("generic", Ok(SemanticsRole::Generic)),
        ("grid", Ok(SemanticsRole::Grid)),
        ("gridcell", Ok(SemanticsRole::GridCell)),
        ("group", Ok(SemanticsRole::Group)),
        ("heading", Ok(SemanticsRole::Heading)),
        ("img", Ok(SemanticsRole::Image)),
        ("insertion", Ok(SemanticsRole::Insertion)),
        ("link", Ok(SemanticsRole::Link)),
        ("list", Ok(SemanticsRole::List)),
        ("listbox", Ok(SemanticsRole::ListBox)),
        ("listitem", Ok(SemanticsRole::ListItem)),
        ("log", Ok(SemanticsRole::Log)),
        ("main", Ok(SemanticsRole::Main)),
        ("marquee", Ok(SemanticsRole::Marquee)),
        ("math", Ok(SemanticsRole::Math)),
        ("menu", Ok(SemanticsRole::Menu)),
        ("menubar", Ok(SemanticsRole::MenuBar)),
        ("menuitem", Ok(SemanticsRole::MenuItem)),
        ("menuitemcheckbox", Ok(SemanticsRole::MenuItemCheckbox)),
        ("menuitemradio", Ok(SemanticsRole::MenuItemRadio)),
        ("meter", Ok(SemanticsRole::Meter)),
        ("navigation", Ok(SemanticsRole::Navigation)),
        ("none", Ok(SemanticsRole::Presentation)),
        ("note", Ok(SemanticsRole::Note)),
        ("option", Ok(SemanticsRole::ListBoxOption)),
        ("paragraph", Ok(SemanticsRole::Paragraph)),
        ("presentation", Ok(SemanticsRole::Presentation)),
        ("progressbar", Ok(SemanticsRole::ProgressBar)),
        ("radio", Ok(SemanticsRole::RadioButton)),
        ("radiogroup", Ok(SemanticsRole::RadioGroup)),
        ("region", Ok(SemanticsRole::Region)),
        ("row", Ok(SemanticsRole::Row)),
        ("rowgroup", Ok(SemanticsRole::RowGroup)),
        ("rowheader", Ok(SemanticsRole::RowHeader)),
        ("scrollbar", Ok(SemanticsRole::ScrollBar)),
        ("search", Ok(SemanticsRole::Search)),
        ("searchbox", Ok(SemanticsRole::SearchInput)),
        ("separator", Ok(SemanticsRole::Separator)),
        ("slider", Ok(SemanticsRole::Slider)),
        ("spinbutton", Ok(SemanticsRole::SpinButton)),
        ("status", Ok(SemanticsRole::Status)),
        ("strong", Ok(SemanticsRole::Strong)),
        ("subscript", Err("no AccessKit counterpart")),
        ("superscript", Err("no AccessKit counterpart")),
        ("switch", Ok(SemanticsRole::Switch)),
        ("tab", Ok(SemanticsRole::Tab)),
        ("table", Ok(SemanticsRole::Table)),
        ("tablist", Ok(SemanticsRole::TabList)),
        ("tabpanel", Ok(SemanticsRole::TabPanel)),
        ("term", Ok(SemanticsRole::Term)),
        ("textbox", Ok(SemanticsRole::TextInput)),
        ("time", Ok(SemanticsRole::Time)),
        ("timer", Ok(SemanticsRole::Timer)),
        ("toolbar", Ok(SemanticsRole::Toolbar)),
        ("tooltip", Ok(SemanticsRole::Tooltip)),
        ("tree", Ok(SemanticsRole::Tree)),
        ("treegrid", Ok(SemanticsRole::TreeGrid)),
        ("treeitem", Ok(SemanticsRole::TreeItem)),
    ];

    /// The roles FLUI has beyond ARIA's: the editable-text kinds AccessKit
    /// distinguishes (ARIA's `textbox` with `aria-multiline`, HTML input
    /// types), the desktop window roles, and the absence of a declared role.
    const BEYOND_ARIA: &[SemanticsRole] = &[
        // Declares no role: the role is derived from the node.
        SemanticsRole::None,
        SemanticsRole::MultilineTextInput,
        SemanticsRole::PasswordInput,
        SemanticsRole::EmailInput,
        SemanticsRole::NumberInput,
        SemanticsRole::PhoneNumberInput,
        SemanticsRole::UrlInput,
        SemanticsRole::DateInput,
        SemanticsRole::TimeInput,
        SemanticsRole::Window,
        SemanticsRole::Pane,
        SemanticsRole::ScrollView,
    ];

    /// The vocabulary covers ARIA 1.2 (ADR-0203): every ARIA role has a FLUI
    /// role or a recorded reason, and every FLUI role is an ARIA role's or
    /// one of the documented additions.
    #[test]
    fn every_aria_role_is_covered() {
        let covered: Vec<SemanticsRole> = ARIA_1_2_ROLES
            .iter()
            .filter_map(|(_, role)| role.ok())
            .chain(BEYOND_ARIA.iter().copied())
            .collect();
        for role in SemanticsRole::ALL {
            assert!(
                covered.contains(role),
                "{role} is neither an ARIA 1.2 role's nor a documented addition"
            );
        }
        let names: Vec<&str> = ARIA_1_2_ROLES.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            names, sorted,
            "the ARIA list is sorted and has no duplicates"
        );
        assert_eq!(
            names.len(),
            82,
            "a role added to or dropped from the transcription is deliberate"
        );
    }
}
