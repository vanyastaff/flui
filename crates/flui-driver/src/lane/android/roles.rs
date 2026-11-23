//! What Android's accessibility framework reports for each semantics role: the class name
//! `accesskit_android` gives a node (`AccessibilityNodeInfo.setClassName`), the vocabulary
//! TalkBack and UI Automator read roles in, and how a class folds back onto the wire's [`Role`].
//!
//! [`class_for`] is transcribed from the locked `accesskit_android` ([`ADAPTER_CHECKED_AGAINST`],
//! `NodeWrapper::class_name`): it takes the AccessKit role a node publishes, which the app's own
//! read reports as the node's `native_role`. [`ROLES`] composes it with FLUI's role mapping
//! (ADR-0203): for each [`SemanticsRole`], the AccessKit role FLUI publishes and the class that
//! reaches Android.
//!
//! A class keeps few kinds apart: every role the adapter names no class for is an
//! `android.view.View`, a switch and a toggled button are both an `android.widget.ToggleButton`,
//! a list, a list box and a scroll view are all an `android.widget.ListView`. [`wire_role`] reads a
//! class the way a screen reader can, the coarse role it stands for.

use flui_protocol::semantics::SemanticsRole;
use flui_protocol::wire::Role;

/// The `accesskit_android` version [`class_for`] and [`ROLES`] were transcribed from.
pub const ADAPTER_CHECKED_AGAINST: &str = "0.9.1";

/// The class of every role the adapter names no class for.
pub const VIEW: &str = "android.view.View";

/// The class `accesskit_android` gives a node of the AccessKit role `accesskit_role` (its name as
/// AccessKit's `Debug` spells it, which a read reports as `native_role`), `toggled` when the node
/// has a toggled state: the adapter's `NodeWrapper::class_name`, arm for arm.
#[must_use]
pub fn class_for(accesskit_role: &str, toggled: bool) -> &'static str {
    match accesskit_role {
        "TextInput" | "MultilineTextInput" | "SearchInput" | "EmailInput" | "NumberInput"
        | "PasswordInput" | "PhoneNumberInput" | "UrlInput" => "android.widget.EditText",
        "Slider" => "android.widget.SeekBar",
        "ColorWell" | "ComboBox" | "EditableComboBox" | "DateInput" | "DateTimeInput"
        | "WeekInput" | "MonthInput" | "TimeInput" => "android.widget.Spinner",
        "Button" if toggled => "android.widget.ToggleButton",
        "Button" | "PdfActionableHighlight" => "android.widget.Button",
        "CheckBox" => "android.widget.CheckBox",
        "RadioButton" => "android.widget.RadioButton",
        "RadioGroup" => "android.widget.RadioGroup",
        "Switch" => "android.widget.ToggleButton",
        "Canvas" | "Image" | "SvgRoot" => "android.widget.ImageView",
        "Meter" | "ProgressIndicator" => "android.widget.ProgressBar",
        "TabList" => "android.widget.TabWidget",
        "Grid" | "Table" | "TreeGrid" => "android.widget.GridView",
        "DescriptionList" | "List" | "ListBox" | "ScrollView" => "android.widget.ListView",
        "Dialog" => "android.app.Dialog",
        "RootWebArea" => "android.webkit.WebView",
        "MenuItem" | "MenuItemCheckBox" | "MenuItemRadio" => "android.view.MenuItem",
        "Label" => "android.widget.TextView",
        _ => VIEW,
    }
}

/// One row of the table: a semantics role, the AccessKit role FLUI publishes for it, and the class
/// that reaches Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RoleRow {
    /// The role a node declares.
    pub role: SemanticsRole,
    /// The AccessKit role FLUI publishes for it, as a read's `native_role` spells it.
    pub accesskit: &'static str,
    /// The class Android reports for it.
    pub class: &'static str,
    /// Whether a screen reader reads the node's label: not for a role ARIA forbids a name (a
    /// paragraph, a caption, code, emphasis, strong text, an insertion, a deletion), whose label
    /// FLUI publishes as its description, which `accesskit_android` hands Android nowhere.
    pub named: bool,
}

const fn row(role: SemanticsRole, accesskit: &'static str, class: &'static str) -> RoleRow {
    RoleRow {
        role,
        accesskit,
        class,
        named: true,
    }
}

const fn unnamed(role: SemanticsRole, accesskit: &'static str) -> RoleRow {
    RoleRow {
        role,
        accesskit,
        class: VIEW,
        named: false,
    }
}

/// Every role but `none`, `generic` and `presentation` (which publish no node of their own), in
/// vocabulary order, the order of the Windows lane's table.
pub const ROLES: &[RoleRow] = &[
    row(SemanticsRole::Button, "Button", "android.widget.Button"),
    row(
        SemanticsRole::CheckBox,
        "CheckBox",
        "android.widget.CheckBox",
    ),
    row(
        SemanticsRole::RadioButton,
        "RadioButton",
        "android.widget.RadioButton",
    ),
    row(
        SemanticsRole::Switch,
        "Switch",
        "android.widget.ToggleButton",
    ),
    row(SemanticsRole::Link, "Link", VIEW),
    row(SemanticsRole::Slider, "Slider", "android.widget.SeekBar"),
    row(SemanticsRole::SpinButton, "SpinButton", VIEW),
    row(
        SemanticsRole::ComboBox,
        "ComboBox",
        "android.widget.Spinner",
    ),
    row(SemanticsRole::ScrollBar, "ScrollBar", VIEW),
    row(SemanticsRole::Separator, "Splitter", VIEW),
    row(
        SemanticsRole::TextInput,
        "TextInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::MultilineTextInput,
        "MultilineTextInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::PasswordInput,
        "PasswordInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::SearchInput,
        "SearchInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::EmailInput,
        "EmailInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::NumberInput,
        "NumberInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::PhoneNumberInput,
        "PhoneNumberInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::UrlInput,
        "UrlInput",
        "android.widget.EditText",
    ),
    row(
        SemanticsRole::DateInput,
        "DateInput",
        "android.widget.Spinner",
    ),
    row(
        SemanticsRole::TimeInput,
        "TimeInput",
        "android.widget.Spinner",
    ),
    row(SemanticsRole::ListBox, "ListBox", "android.widget.ListView"),
    row(SemanticsRole::ListBoxOption, "ListBoxOption", VIEW),
    row(
        SemanticsRole::RadioGroup,
        "RadioGroup",
        "android.widget.RadioGroup",
    ),
    row(SemanticsRole::Menu, "Menu", VIEW),
    row(SemanticsRole::MenuBar, "MenuBar", VIEW),
    row(SemanticsRole::MenuItem, "MenuItem", "android.view.MenuItem"),
    row(
        SemanticsRole::MenuItemCheckbox,
        "MenuItemCheckBox",
        "android.view.MenuItem",
    ),
    row(
        SemanticsRole::MenuItemRadio,
        "MenuItemRadio",
        "android.view.MenuItem",
    ),
    row(SemanticsRole::Tab, "Tab", VIEW),
    row(
        SemanticsRole::TabList,
        "TabList",
        "android.widget.TabWidget",
    ),
    row(SemanticsRole::TabPanel, "TabPanel", VIEW),
    row(SemanticsRole::Tree, "Tree", VIEW),
    row(SemanticsRole::TreeItem, "TreeItem", VIEW),
    row(
        SemanticsRole::TreeGrid,
        "TreeGrid",
        "android.widget.GridView",
    ),
    row(SemanticsRole::Grid, "Grid", "android.widget.GridView"),
    row(SemanticsRole::GridCell, "GridCell", VIEW),
    row(SemanticsRole::Toolbar, "Toolbar", VIEW),
    row(SemanticsRole::Table, "Table", "android.widget.GridView"),
    row(SemanticsRole::Row, "Row", VIEW),
    row(SemanticsRole::RowGroup, "RowGroup", VIEW),
    row(SemanticsRole::Cell, "Cell", VIEW),
    row(SemanticsRole::ColumnHeader, "ColumnHeader", VIEW),
    row(SemanticsRole::RowHeader, "RowHeader", VIEW),
    row(SemanticsRole::List, "List", "android.widget.ListView"),
    row(SemanticsRole::ListItem, "ListItem", VIEW),
    row(SemanticsRole::Heading, "Heading", VIEW),
    unnamed(SemanticsRole::Paragraph, "Paragraph"),
    row(SemanticsRole::Image, "Image", "android.widget.ImageView"),
    row(SemanticsRole::Figure, "Figure", VIEW),
    unnamed(SemanticsRole::Caption, "Caption"),
    row(SemanticsRole::Article, "Article", VIEW),
    row(SemanticsRole::Feed, "Feed", VIEW),
    row(SemanticsRole::Group, "Group", VIEW),
    row(SemanticsRole::Note, "Note", VIEW),
    row(SemanticsRole::Blockquote, "Blockquote", VIEW),
    unnamed(SemanticsRole::Code, "Code"),
    unnamed(SemanticsRole::Emphasis, "Emphasis"),
    unnamed(SemanticsRole::Strong, "Strong"),
    unnamed(SemanticsRole::Insertion, "ContentInsertion"),
    unnamed(SemanticsRole::Deletion, "ContentDeletion"),
    row(SemanticsRole::Term, "Term", VIEW),
    row(SemanticsRole::Definition, "Definition", VIEW),
    row(SemanticsRole::Time, "Time", VIEW),
    row(SemanticsRole::Math, "Math", VIEW),
    row(SemanticsRole::Meter, "Meter", "android.widget.ProgressBar"),
    row(
        SemanticsRole::ProgressBar,
        "ProgressIndicator",
        "android.widget.ProgressBar",
    ),
    row(SemanticsRole::Tooltip, "Tooltip", VIEW),
    row(SemanticsRole::Document, "Document", VIEW),
    row(SemanticsRole::Application, "Application", VIEW),
    row(SemanticsRole::Banner, "Banner", VIEW),
    row(SemanticsRole::Main, "Main", VIEW),
    row(SemanticsRole::Navigation, "Navigation", VIEW),
    row(SemanticsRole::Complementary, "Complementary", VIEW),
    row(SemanticsRole::ContentInfo, "ContentInfo", VIEW),
    row(SemanticsRole::Search, "Search", VIEW),
    row(SemanticsRole::Form, "Form", VIEW),
    row(SemanticsRole::Region, "Region", VIEW),
    row(SemanticsRole::Alert, "Alert", VIEW),
    row(SemanticsRole::Status, "Status", VIEW),
    row(SemanticsRole::Log, "Log", VIEW),
    row(SemanticsRole::Marquee, "Marquee", VIEW),
    row(SemanticsRole::Timer, "Timer", VIEW),
    row(SemanticsRole::Dialog, "Dialog", "android.app.Dialog"),
    row(SemanticsRole::AlertDialog, "AlertDialog", VIEW),
    row(SemanticsRole::Window, "Window", VIEW),
    row(SemanticsRole::Pane, "Pane", VIEW),
    row(
        SemanticsRole::ScrollView,
        "ScrollView",
        "android.widget.ListView",
    ),
];

/// The row of `role`; `None` for `none`, `generic` and `presentation`.
#[must_use]
pub fn role_row(role: SemanticsRole) -> Option<RoleRow> {
    ROLES.iter().find(|row| row.role == role).copied()
}

/// The wire role a node of the class `class` folds to: the role a screen reader can tell from the
/// class alone, the coarsest where one class stands for several (an `android.view.View` is a
/// group, a `ToggleButton` a button, a `ListView` a list).
#[must_use]
pub fn wire_role(class: &str) -> Role {
    match class.trim() {
        "android.widget.EditText" => Role::TextInput,
        "android.widget.SeekBar" => Role::Slider,
        "android.widget.Spinner" => Role::ComboBox,
        "android.widget.Button" | "android.widget.ToggleButton" => Role::Button,
        "android.widget.CheckBox" => Role::CheckBox,
        "android.widget.RadioButton" => Role::RadioButton,
        "android.widget.ImageView" => Role::Image,
        "android.widget.ProgressBar" => Role::ProgressIndicator,
        "android.widget.TabWidget" => Role::TabList,
        "android.widget.GridView" => Role::Grid,
        "android.widget.ListView" => Role::List,
        "android.app.Dialog" => Role::Dialog,
        "android.webkit.WebView" => Role::Document,
        "android.view.MenuItem" => Role::MenuItem,
        "android.widget.TextView" => Role::Label,
        "android.widget.RadioGroup" | VIEW => Role::Group,
        _ => Role::Unknown,
    }
}
