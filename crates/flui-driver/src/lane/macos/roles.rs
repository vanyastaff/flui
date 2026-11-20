//! What the macOS accessibility API reports for each semantics role, and how an accessibility
//! element folds back onto the wire's [`Role`], the vocabulary both sides of the projection speak
//! (ADR-0095 §4).
//!
//! [`ROLES`] is transcribed from the locked `accesskit_macos` ([`ADAPTER_CHECKED_AGAINST`])
//! composed with FLUI's role mapping (ADR-0203): for each [`SemanticsRole`], the `AXRole` and
//! `AXSubrole` the adapter gives it. [`wire_role`] reads an element the other way, the subroles
//! that tell an `AXRole`'s kinds apart first. The fold is the one UI Automation's is held to
//! (the Windows lane's role table) wherever the accessibility API keeps the distinction; where
//! it does not (a list box option is static text, a grid is a table), VoiceOver cannot tell the
//! roles apart either, and the fold says so by reading what macOS reports.

use flui_protocol::semantics::SemanticsRole;
use flui_protocol::wire::Role;

/// The `accesskit_macos` version [`ROLES`] was transcribed from.
pub const ADAPTER_CHECKED_AGAINST: &str = "0.27.1";

/// One row of the table: a semantics role, and the role and subrole the accessibility API reports
/// for a node of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RoleRow {
    /// The role a node declares.
    pub role: SemanticsRole,
    /// The `AXRole` the accessibility API reports.
    pub ax_role: &'static str,
    /// The `AXSubrole` it reports, if any.
    pub subrole: Option<&'static str>,
}

impl RoleRow {
    /// The row's native role as a read reports it: see [`native_role`].
    #[must_use]
    pub fn native(&self) -> String {
        native_role(self.ax_role, self.subrole)
    }
}

const fn row(role: SemanticsRole, ax_role: &'static str, subrole: Option<&'static str>) -> RoleRow {
    RoleRow {
        role,
        ax_role,
        subrole,
    }
}

/// Every role but `none`, `generic` and `presentation` (which publish no node of their own), in
/// vocabulary order, the order of the Windows lane's table.
pub const ROLES: &[RoleRow] = &[
    row(SemanticsRole::Button, "AXButton", None),
    row(SemanticsRole::CheckBox, "AXCheckBox", None),
    row(SemanticsRole::RadioButton, "AXRadioButton", None),
    row(SemanticsRole::Switch, "AXCheckBox", Some("AXSwitch")),
    row(SemanticsRole::Link, "AXLink", None),
    row(SemanticsRole::Slider, "AXSlider", None),
    row(SemanticsRole::SpinButton, "AXIncrementor", None),
    row(SemanticsRole::ComboBox, "AXPopUpButton", None),
    row(SemanticsRole::ScrollBar, "AXScrollBar", None),
    row(SemanticsRole::Separator, "AXSplitter", None),
    row(SemanticsRole::TextInput, "AXTextField", None),
    row(SemanticsRole::MultilineTextInput, "AXTextArea", None),
    row(
        SemanticsRole::PasswordInput,
        "AXTextField",
        Some("AXSecureTextField"),
    ),
    row(
        SemanticsRole::SearchInput,
        "AXTextField",
        Some("AXSearchField"),
    ),
    row(SemanticsRole::EmailInput, "AXTextField", None),
    row(SemanticsRole::NumberInput, "AXTextField", None),
    row(SemanticsRole::PhoneNumberInput, "AXTextField", None),
    row(SemanticsRole::UrlInput, "AXTextField", None),
    row(SemanticsRole::DateInput, "AXDateField", None),
    row(SemanticsRole::TimeInput, "AXTimeField", None),
    row(SemanticsRole::ListBox, "AXList", None),
    row(SemanticsRole::ListBoxOption, "AXStaticText", None),
    row(SemanticsRole::RadioGroup, "AXRadioGroup", None),
    row(SemanticsRole::Menu, "AXMenu", None),
    row(SemanticsRole::MenuBar, "AXMenuBar", None),
    row(SemanticsRole::MenuItem, "AXMenuItem", None),
    row(SemanticsRole::MenuItemCheckbox, "AXMenuItem", None),
    row(SemanticsRole::MenuItemRadio, "AXMenuItem", None),
    row(SemanticsRole::Tab, "AXRadioButton", Some("AXTabButton")),
    row(SemanticsRole::TabList, "AXTabGroup", None),
    row(SemanticsRole::TabPanel, "AXGroup", Some("AXTabPanel")),
    row(SemanticsRole::Tree, "AXOutline", None),
    row(SemanticsRole::TreeItem, "AXRow", Some("AXOutlineRow")),
    row(SemanticsRole::TreeGrid, "AXTable", None),
    row(SemanticsRole::Grid, "AXTable", None),
    row(SemanticsRole::GridCell, "AXCell", None),
    row(SemanticsRole::Toolbar, "AXToolbar", None),
    row(SemanticsRole::Table, "AXTable", None),
    row(SemanticsRole::Row, "AXRow", None),
    row(SemanticsRole::RowGroup, "AXGroup", None),
    row(SemanticsRole::Cell, "AXCell", None),
    row(SemanticsRole::ColumnHeader, "AXCell", None),
    row(SemanticsRole::RowHeader, "AXCell", None),
    row(SemanticsRole::List, "AXList", None),
    row(SemanticsRole::ListItem, "AXGroup", None),
    // The adapter reports a heading's role as `Heading`, without the `AX` prefix.
    row(SemanticsRole::Heading, "Heading", None),
    row(SemanticsRole::Paragraph, "AXGroup", None),
    row(SemanticsRole::Image, "AXImage", None),
    row(SemanticsRole::Figure, "AXGroup", None),
    row(SemanticsRole::Caption, "AXGroup", None),
    row(SemanticsRole::Article, "AXGroup", Some("AXDocumentArticle")),
    row(SemanticsRole::Feed, "AXUnknown", Some("AXApplicationGroup")),
    row(SemanticsRole::Group, "AXGroup", Some("AXApplicationGroup")),
    row(SemanticsRole::Note, "AXGroup", Some("AXDocumentNote")),
    row(SemanticsRole::Blockquote, "AXGroup", None),
    row(SemanticsRole::Code, "AXGroup", Some("AXCodeStyleGroup")),
    row(
        SemanticsRole::Emphasis,
        "AXGroup",
        Some("AXEmphasisStyleGroup"),
    ),
    row(SemanticsRole::Strong, "AXGroup", Some("AXStrongStyleGroup")),
    row(
        SemanticsRole::Insertion,
        "AXGroup",
        Some("AXInsertStyleGroup"),
    ),
    row(
        SemanticsRole::Deletion,
        "AXGroup",
        Some("AXDeleteStyleGroup"),
    ),
    row(SemanticsRole::Term, "AXGroup", Some("AXTerm")),
    row(SemanticsRole::Definition, "AXGroup", Some("AXDefinition")),
    row(SemanticsRole::Time, "AXGroup", Some("AXTimeGroup")),
    row(SemanticsRole::Math, "AXGroup", Some("AXDocumentMath")),
    row(SemanticsRole::Meter, "AXLevelIndicator", Some("AXMeter")),
    row(SemanticsRole::ProgressBar, "AXProgressIndicator", None),
    row(
        SemanticsRole::Tooltip,
        "AXGroup",
        Some("AXUserInterfaceTooltip"),
    ),
    row(SemanticsRole::Document, "AXGroup", Some("AXDocument")),
    row(
        SemanticsRole::Application,
        "AXGroup",
        Some("AXWebApplication"),
    ),
    row(SemanticsRole::Banner, "AXGroup", Some("AXLandmarkBanner")),
    row(SemanticsRole::Main, "AXGroup", Some("AXLandmarkMain")),
    row(
        SemanticsRole::Navigation,
        "AXGroup",
        Some("AXLandmarkNavigation"),
    ),
    row(
        SemanticsRole::Complementary,
        "AXGroup",
        Some("AXLandmarkComplementary"),
    ),
    row(
        SemanticsRole::ContentInfo,
        "AXGroup",
        Some("AXLandmarkContentInfo"),
    ),
    row(SemanticsRole::Search, "AXGroup", Some("AXLandmarkSearch")),
    row(SemanticsRole::Form, "AXGroup", Some("AXLandmarkForm")),
    row(SemanticsRole::Region, "AXGroup", Some("AXLandmarkRegion")),
    row(SemanticsRole::Alert, "AXGroup", Some("AXApplicationAlert")),
    row(
        SemanticsRole::Status,
        "AXGroup",
        Some("AXApplicationStatus"),
    ),
    row(SemanticsRole::Log, "AXGroup", Some("AXApplicationLog")),
    row(
        SemanticsRole::Marquee,
        "AXGroup",
        Some("AXApplicationMarquee"),
    ),
    row(SemanticsRole::Timer, "AXGroup", Some("AXApplicationTimer")),
    row(SemanticsRole::Dialog, "AXWindow", Some("AXDialog")),
    row(SemanticsRole::AlertDialog, "AXWindow", Some("AXDialog")),
    // The adapter reports the window's own node as a group: the `NSWindow` is the window.
    row(SemanticsRole::Window, "AXGroup", None),
    row(SemanticsRole::Pane, "AXUnknown", None),
    row(SemanticsRole::ScrollView, "AXUnknown", None),
];

/// The row of `role`; `None` for `none`, `generic` and `presentation`.
#[must_use]
pub fn role_row(role: SemanticsRole) -> Option<RoleRow> {
    ROLES.iter().find(|row| row.role == role).copied()
}

/// An element's native role as a read reports it: its `AXRole`, and `/` and its `AXSubrole` when
/// it has one (`AXCheckBox/AXSwitch`).
#[must_use]
pub fn native_role(ax_role: &str, subrole: Option<&str>) -> String {
    match subrole {
        Some(subrole) => format!("{ax_role}/{subrole}"),
        None => ax_role.to_owned(),
    }
}

/// The wire role an accessibility element of `ax_role` and `subrole` folds to: see the
/// [module docs](self).
#[must_use]
pub fn wire_role(ax_role: &str, subrole: Option<&str>) -> Role {
    let by_subrole = match subrole.map(str::trim) {
        Some("AXSwitch") => Some(Role::Switch),
        Some("AXToggle") => Some(Role::Button),
        Some("AXSecureTextField") => Some(Role::PasswordInput),
        Some("AXTabButton") => Some(Role::Tab),
        Some("AXOutlineRow") => Some(Role::TreeItem),
        Some("AXDialog" | "AXSystemDialog") => Some(Role::Dialog),
        Some("AXApplicationStatus") => Some(Role::Status),
        Some("AXUserInterfaceTooltip") => Some(Role::Tooltip),
        Some("AXDocument") => Some(Role::Document),
        Some("AXTabPanel" | "AXApplicationTimer" | "AXWebApplication") => Some(Role::Pane),
        Some("AXApplicationGroup") => Some(Role::Group),
        Some(
            "AXApplicationAlert"
            | "AXApplicationMarquee"
            | "AXTimeGroup"
            | "AXCodeStyleGroup"
            | "AXEmphasisStyleGroup"
            | "AXStrongStyleGroup",
        ) => Some(Role::Label),
        Some("AXMeter") => Some(Role::ProgressIndicator),
        _ => None,
    };
    if let Some(role) = by_subrole {
        return role;
    }
    match ax_role.trim() {
        "AXButton" | "AXMenuButton" => Role::Button,
        "AXCheckBox" => Role::CheckBox,
        "AXRadioButton" => Role::RadioButton,
        "AXPopUpButton" | "AXComboBox" => Role::ComboBox,
        "AXTextField" | "AXTextArea" | "AXDateField" | "AXTimeField" => Role::TextInput,
        "AXLink" => Role::Link,
        "AXImage" => Role::Image,
        "AXStaticText" => Role::Label,
        "Heading" | "AXHeading" => Role::Heading,
        "AXList" => Role::List,
        "AXRow" => Role::Row,
        "AXCell" => Role::Cell,
        "AXTable" => Role::Table,
        "AXOutline" => Role::Tree,
        "AXMenu" => Role::Menu,
        "AXMenuBar" => Role::MenuBar,
        "AXMenuItem" | "AXMenuBarItem" => Role::MenuItem,
        "AXProgressIndicator" | "AXLevelIndicator" | "AXBusyIndicator" => Role::ProgressIndicator,
        "AXScrollBar" => Role::ScrollBar,
        "AXSlider" => Role::Slider,
        "AXIncrementor" => Role::SpinButton,
        "AXTabGroup" => Role::TabList,
        "AXToolbar" => Role::Toolbar,
        "AXSplitter" => Role::Splitter,
        "AXGroup" | "AXRadioGroup" => Role::Group,
        "AXWindow" => Role::Window,
        "AXSheet" => Role::Dialog,
        "AXScrollArea" => Role::Pane,
        _ => Role::Unknown,
    }
}

/// The wire role a node of `row`'s role folds to, read the way the accessibility API reports it.
#[must_use]
pub fn expected_wire_role(row: &RoleRow) -> Role {
    wire_role(row.ax_role, row.subrole)
}
