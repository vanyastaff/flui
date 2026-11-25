//! What the Mac's accessibility API shows of an iOS app on the Simulator's window for each
//! semantics role: the UIKit traits `accesskit_ios` gives a node, the `AXRole` and `AXSubrole` the
//! Simulator's bridge translates those traits to, and how such an element folds back onto the
//! wire's [`Role`].
//!
//! [`role_trait`] is transcribed from the locked `accesskit_ios` ([`ADAPTER_CHECKED_AGAINST`],
//! `NodeWrapper::traits`): it takes the AccessKit role a node publishes, which the app's own read
//! reports as the node's `native_role`; a node with a toggled state adds the toggle-button trait.
//! [`ax_role`] is the bridge's half: Simulator.app (DeviceHub from Xcode 27) vends each iOS
//! element to AppKit through Apple's `AccessibilityPlatformTranslation`, whose role vocabulary
//! `idb` records from the framework (`FBSimulatorBridgeProtocol/BridgeAXTranslatorRole.swift`):
//! a button, a check box that covers toggles with the subrole `AXSwitch`, a text field that covers
//! search fields with the subrole `AXSearchField`, a heading, an image, a link, a slider, static
//! text, and a generic element for an element with none of those traits, beside a group for a
//! container that is no element itself. Which trait picks which of them is read from those names;
//! no Simulator run has pinned it yet, so a run that reads another role corrects this table.
//!
//! The traits keep few roles apart: every role `accesskit_ios` gives no trait (a text field, a
//! list, a progress indicator) is a generic element, so VoiceOver on iOS reads it by its label and
//! value alone. [`ROLES`] composes FLUI's role mapping (ADR-0203), the AccessKit role FLUI
//! publishes for each [`SemanticsRole`], with both halves.

use flui_protocol::semantics::SemanticsRole;
use flui_protocol::wire::Role;

/// The `accesskit_ios` version [`role_trait`] was transcribed from.
pub const ADAPTER_CHECKED_AGAINST: &str = "0.2.2";

/// The subrole the bridge gives a toggle.
pub const SWITCH_SUBROLE: &str = "AXSwitch";

/// The subrole DeviceHub (Xcode 27) gives the group that is the device's screen.
pub const SCREEN_SUBROLE: &str = "iOSContentGroup";

/// The UIKit trait that decides an element's role, as `accesskit_ios` sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RoleTrait {
    /// `UIAccessibilityTraitButton`.
    Button,
    /// `UIAccessibilityTraitLink`.
    Link,
    /// `UIAccessibilityTraitImage`.
    Image,
    /// `UIAccessibilityTraitStaticText`.
    StaticText,
    /// `UIAccessibilityTraitHeader`.
    Header,
    /// `UIAccessibilityTraitAdjustable`.
    Adjustable,
    /// `UIAccessibilityTraitSearchField`.
    SearchField,
    /// `UIAccessibilityTraitNone`: the adapter names no trait for the role.
    None,
}

/// The trait `accesskit_ios` gives a node of the AccessKit role `accesskit_role` (its name as
/// AccessKit's `Debug` spells it, which a read reports as `native_role`): the adapter's
/// `NodeWrapper::traits`, arm for arm.
#[must_use]
pub fn role_trait(accesskit_role: &str) -> RoleTrait {
    match accesskit_role {
        "Button" | "DefaultButton" | "DisclosureTriangle" | "CheckBox" | "RadioButton"
        | "Switch" | "MenuItemCheckBox" | "MenuItemRadio" | "Tab" => RoleTrait::Button,
        "Link" => RoleTrait::Link,
        "Image" => RoleTrait::Image,
        "Label" => RoleTrait::StaticText,
        "Heading" => RoleTrait::Header,
        "Slider" | "SpinButton" => RoleTrait::Adjustable,
        "SearchInput" => RoleTrait::SearchField,
        _ => RoleTrait::None,
    }
}

/// The `AXRole` and `AXSubrole` the bridge gives an element whose role trait is `role_trait`,
/// `toggled` when it also carries the toggle-button trait: see the [module docs](self).
#[must_use]
pub fn ax_role(role_trait: RoleTrait, toggled: bool) -> (&'static str, Option<&'static str>) {
    if toggled {
        return ("AXCheckBox", Some(SWITCH_SUBROLE));
    }
    match role_trait {
        RoleTrait::Button => ("AXButton", None),
        RoleTrait::Link => ("AXLink", None),
        RoleTrait::Image => ("AXImage", None),
        RoleTrait::StaticText => ("AXStaticText", None),
        RoleTrait::Header => ("AXHeading", None),
        RoleTrait::Adjustable => ("AXSlider", None),
        RoleTrait::SearchField => ("AXTextField", Some("AXSearchField")),
        RoleTrait::None => ("AXGenericElement", None),
    }
}

/// The native role, `AXRole` or `AXRole/AXSubrole`, an element of a node of the AccessKit role
/// `accesskit_role` reads as, `toggled` when the node has a toggled state.
#[must_use]
pub fn native_for(accesskit_role: &str, toggled: bool) -> String {
    let (role, subrole) = ax_role(role_trait(accesskit_role), toggled);
    native_role(role, subrole)
}

/// An element's role and subrole as a read reports them: the role alone, or `role/subrole`, the
/// spelling the macOS lane's reads use.
#[must_use]
pub fn native_role(ax_role: &str, subrole: Option<&str>) -> String {
    match subrole {
        Some(subrole) if !subrole.is_empty() => format!("{ax_role}/{subrole}"),
        _ => ax_role.to_owned(),
    }
}

/// The wire role an element of `ax_role` and `subrole` folds to: the role a screen reader can
/// tell from it, the coarsest where one stands for several (a generic element or a group is a
/// group, a text field a text input).
#[must_use]
pub fn wire_role(ax_role: &str, subrole: Option<&str>) -> Role {
    match (ax_role, subrole) {
        ("AXCheckBox", Some(SWITCH_SUBROLE)) => Role::Switch,
        ("AXCheckBox", _) => Role::CheckBox,
        ("AXButton", _) => Role::Button,
        ("AXLink", _) => Role::Link,
        ("AXImage", _) => Role::Image,
        ("AXStaticText", _) => Role::Label,
        ("AXHeading", _) => Role::Heading,
        ("AXSlider", _) => Role::Slider,
        ("AXTextField", _) => Role::TextInput,
        ("AXGenericElement" | "AXGroup", _) => Role::Group,
        _ => Role::Unknown,
    }
}

/// One row of the table: a semantics role, the AccessKit role FLUI publishes for it, and what the
/// Simulator's bridge reports for a node of it with no toggled state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RoleRow {
    /// The role a node declares.
    pub role: SemanticsRole,
    /// The AccessKit role FLUI publishes for it, as a read's `native_role` spells it.
    pub accesskit: &'static str,
}

impl RoleRow {
    /// The native role the bridge reports for the row's node: see [`native_for`].
    #[must_use]
    pub fn native(&self) -> String {
        native_for(self.accesskit, false)
    }
}

const fn row(role: SemanticsRole, accesskit: &'static str) -> RoleRow {
    RoleRow { role, accesskit }
}

/// Every role but `none`, `generic` and `presentation` (which publish no node of their own), in
/// vocabulary order, the order of the Windows lane's table.
pub const ROLES: &[RoleRow] = &[
    row(SemanticsRole::Button, "Button"),
    row(SemanticsRole::CheckBox, "CheckBox"),
    row(SemanticsRole::RadioButton, "RadioButton"),
    row(SemanticsRole::Switch, "Switch"),
    row(SemanticsRole::Link, "Link"),
    row(SemanticsRole::Slider, "Slider"),
    row(SemanticsRole::SpinButton, "SpinButton"),
    row(SemanticsRole::ComboBox, "ComboBox"),
    row(SemanticsRole::ScrollBar, "ScrollBar"),
    row(SemanticsRole::Separator, "Splitter"),
    row(SemanticsRole::TextInput, "TextInput"),
    row(SemanticsRole::MultilineTextInput, "MultilineTextInput"),
    row(SemanticsRole::PasswordInput, "PasswordInput"),
    row(SemanticsRole::SearchInput, "SearchInput"),
    row(SemanticsRole::EmailInput, "EmailInput"),
    row(SemanticsRole::NumberInput, "NumberInput"),
    row(SemanticsRole::PhoneNumberInput, "PhoneNumberInput"),
    row(SemanticsRole::UrlInput, "UrlInput"),
    row(SemanticsRole::DateInput, "DateInput"),
    row(SemanticsRole::TimeInput, "TimeInput"),
    row(SemanticsRole::ListBox, "ListBox"),
    row(SemanticsRole::ListBoxOption, "ListBoxOption"),
    row(SemanticsRole::RadioGroup, "RadioGroup"),
    row(SemanticsRole::Menu, "Menu"),
    row(SemanticsRole::MenuBar, "MenuBar"),
    row(SemanticsRole::MenuItem, "MenuItem"),
    row(SemanticsRole::MenuItemCheckbox, "MenuItemCheckBox"),
    row(SemanticsRole::MenuItemRadio, "MenuItemRadio"),
    row(SemanticsRole::Tab, "Tab"),
    row(SemanticsRole::TabList, "TabList"),
    row(SemanticsRole::TabPanel, "TabPanel"),
    row(SemanticsRole::Tree, "Tree"),
    row(SemanticsRole::TreeItem, "TreeItem"),
    row(SemanticsRole::TreeGrid, "TreeGrid"),
    row(SemanticsRole::Grid, "Grid"),
    row(SemanticsRole::GridCell, "GridCell"),
    row(SemanticsRole::Toolbar, "Toolbar"),
    row(SemanticsRole::Table, "Table"),
    row(SemanticsRole::Row, "Row"),
    row(SemanticsRole::RowGroup, "RowGroup"),
    row(SemanticsRole::Cell, "Cell"),
    row(SemanticsRole::ColumnHeader, "ColumnHeader"),
    row(SemanticsRole::RowHeader, "RowHeader"),
    row(SemanticsRole::List, "List"),
    row(SemanticsRole::ListItem, "ListItem"),
    row(SemanticsRole::Heading, "Heading"),
    row(SemanticsRole::Paragraph, "Paragraph"),
    row(SemanticsRole::Image, "Image"),
    row(SemanticsRole::Figure, "Figure"),
    row(SemanticsRole::Caption, "Caption"),
    row(SemanticsRole::Article, "Article"),
    row(SemanticsRole::Feed, "Feed"),
    row(SemanticsRole::Group, "Group"),
    row(SemanticsRole::Note, "Note"),
    row(SemanticsRole::Blockquote, "Blockquote"),
    row(SemanticsRole::Code, "Code"),
    row(SemanticsRole::Emphasis, "Emphasis"),
    row(SemanticsRole::Strong, "Strong"),
    row(SemanticsRole::Insertion, "ContentInsertion"),
    row(SemanticsRole::Deletion, "ContentDeletion"),
    row(SemanticsRole::Term, "Term"),
    row(SemanticsRole::Definition, "Definition"),
    row(SemanticsRole::Time, "Time"),
    row(SemanticsRole::Math, "Math"),
    row(SemanticsRole::Meter, "Meter"),
    row(SemanticsRole::ProgressBar, "ProgressIndicator"),
    row(SemanticsRole::Tooltip, "Tooltip"),
    row(SemanticsRole::Document, "Document"),
    row(SemanticsRole::Application, "Application"),
    row(SemanticsRole::Banner, "Banner"),
    row(SemanticsRole::Main, "Main"),
    row(SemanticsRole::Navigation, "Navigation"),
    row(SemanticsRole::Complementary, "Complementary"),
    row(SemanticsRole::ContentInfo, "ContentInfo"),
    row(SemanticsRole::Search, "Search"),
    row(SemanticsRole::Form, "Form"),
    row(SemanticsRole::Region, "Region"),
    row(SemanticsRole::Alert, "Alert"),
    row(SemanticsRole::Status, "Status"),
    row(SemanticsRole::Log, "Log"),
    row(SemanticsRole::Marquee, "Marquee"),
    row(SemanticsRole::Timer, "Timer"),
    row(SemanticsRole::Dialog, "Dialog"),
    row(SemanticsRole::AlertDialog, "AlertDialog"),
    row(SemanticsRole::Window, "Window"),
    row(SemanticsRole::Pane, "Pane"),
    row(SemanticsRole::ScrollView, "ScrollView"),
];
