//! What UI Automation reports for each semantics role, and how a UI Automation element folds back
//! onto the wire's [`Role`], the vocabulary both sides of the projection speak (ADR-0095 §4).
//!
//! [`ROLES`] is transcribed from the locked `accesskit_windows` ([`ADAPTER_CHECKED_AGAINST`])
//! composed with FLUI's role mapping (ADR-0203): for each [`SemanticsRole`], the control type and
//! ARIA role the adapter gives it. [`wire_role`] reads an element the other way, from its control
//! type, the ARIA roles that tell a control type's kinds apart and the password and dialog flags,
//! the fold `flui-semantics` applies in process, so a published node and what Narrator reads of it
//! project to the same role.

use flui_protocol::semantics::SemanticsRole;
use flui_protocol::wire::Role;

/// The `accesskit_windows` version [`ROLES`] was transcribed from.
pub const ADAPTER_CHECKED_AGAINST: &str = "0.35.2";

/// A UI Automation control type, by the name of its `UIA_*ControlTypeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlType {
    /// `UIA_ButtonControlTypeId`.
    Button,
    /// `UIA_CalendarControlTypeId`.
    Calendar,
    /// `UIA_CheckBoxControlTypeId`.
    CheckBox,
    /// `UIA_ComboBoxControlTypeId`.
    ComboBox,
    /// `UIA_EditControlTypeId`.
    Edit,
    /// `UIA_HyperlinkControlTypeId`.
    Hyperlink,
    /// `UIA_ImageControlTypeId`.
    Image,
    /// `UIA_ListItemControlTypeId`.
    ListItem,
    /// `UIA_ListControlTypeId`.
    List,
    /// `UIA_MenuControlTypeId`.
    Menu,
    /// `UIA_MenuBarControlTypeId`.
    MenuBar,
    /// `UIA_MenuItemControlTypeId`.
    MenuItem,
    /// `UIA_ProgressBarControlTypeId`.
    ProgressBar,
    /// `UIA_RadioButtonControlTypeId`.
    RadioButton,
    /// `UIA_ScrollBarControlTypeId`.
    ScrollBar,
    /// `UIA_SliderControlTypeId`.
    Slider,
    /// `UIA_SpinnerControlTypeId`.
    Spinner,
    /// `UIA_StatusBarControlTypeId`.
    StatusBar,
    /// `UIA_TabControlTypeId`.
    Tab,
    /// `UIA_TabItemControlTypeId`.
    TabItem,
    /// `UIA_TextControlTypeId`.
    Text,
    /// `UIA_ToolBarControlTypeId`.
    ToolBar,
    /// `UIA_ToolTipControlTypeId`.
    ToolTip,
    /// `UIA_TreeControlTypeId`.
    Tree,
    /// `UIA_TreeItemControlTypeId`.
    TreeItem,
    /// `UIA_CustomControlTypeId`.
    Custom,
    /// `UIA_GroupControlTypeId`.
    Group,
    /// `UIA_ThumbControlTypeId`.
    Thumb,
    /// `UIA_DataGridControlTypeId`.
    DataGrid,
    /// `UIA_DataItemControlTypeId`.
    DataItem,
    /// `UIA_DocumentControlTypeId`.
    Document,
    /// `UIA_SplitButtonControlTypeId`.
    SplitButton,
    /// `UIA_WindowControlTypeId`.
    Window,
    /// `UIA_PaneControlTypeId`.
    Pane,
    /// `UIA_HeaderControlTypeId`.
    Header,
    /// `UIA_HeaderItemControlTypeId`.
    HeaderItem,
    /// `UIA_TableControlTypeId`.
    Table,
    /// `UIA_TitleBarControlTypeId`.
    TitleBar,
    /// `UIA_SeparatorControlTypeId`.
    Separator,
    /// `UIA_SemanticZoomControlTypeId`.
    SemanticZoom,
    /// `UIA_AppBarControlTypeId`.
    AppBar,
}

/// The control types in `UIA_*ControlTypeId` order, from 50000.
const BY_ID: &[ControlType] = &[
    ControlType::Button,
    ControlType::Calendar,
    ControlType::CheckBox,
    ControlType::ComboBox,
    ControlType::Edit,
    ControlType::Hyperlink,
    ControlType::Image,
    ControlType::ListItem,
    ControlType::List,
    ControlType::Menu,
    ControlType::MenuBar,
    ControlType::MenuItem,
    ControlType::ProgressBar,
    ControlType::RadioButton,
    ControlType::ScrollBar,
    ControlType::Slider,
    ControlType::Spinner,
    ControlType::StatusBar,
    ControlType::Tab,
    ControlType::TabItem,
    ControlType::Text,
    ControlType::ToolBar,
    ControlType::ToolTip,
    ControlType::Tree,
    ControlType::TreeItem,
    ControlType::Custom,
    ControlType::Group,
    ControlType::Thumb,
    ControlType::DataGrid,
    ControlType::DataItem,
    ControlType::Document,
    ControlType::SplitButton,
    ControlType::Window,
    ControlType::Pane,
    ControlType::Header,
    ControlType::HeaderItem,
    ControlType::Table,
    ControlType::TitleBar,
    ControlType::Separator,
    ControlType::SemanticZoom,
    ControlType::AppBar,
];

/// The first `UIA_*ControlTypeId`, `UIA_ButtonControlTypeId`.
const FIRST_ID: i32 = 50_000;

impl ControlType {
    /// The control type whose `UIA_*ControlTypeId` is `id`; `None` for an id UI Automation does
    /// not define.
    #[must_use]
    pub fn from_id(id: i32) -> Option<Self> {
        let index = usize::try_from(id.checked_sub(FIRST_ID)?).ok()?;
        BY_ID.get(index).copied()
    }

    /// The control type's `UIA_*ControlTypeId`.
    #[must_use]
    pub fn id(self) -> i32 {
        let index = BY_ID
            .iter()
            .position(|known| *known == self)
            .expect("BUG: every control type is listed by its id");
        FIRST_ID + i32::try_from(index).expect("BUG: forty-one control types fit an i32")
    }

    /// The control type's name, as UI Automation spells it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Button => "Button",
            Self::Calendar => "Calendar",
            Self::CheckBox => "CheckBox",
            Self::ComboBox => "ComboBox",
            Self::Edit => "Edit",
            Self::Hyperlink => "Hyperlink",
            Self::Image => "Image",
            Self::ListItem => "ListItem",
            Self::List => "List",
            Self::Menu => "Menu",
            Self::MenuBar => "MenuBar",
            Self::MenuItem => "MenuItem",
            Self::ProgressBar => "ProgressBar",
            Self::RadioButton => "RadioButton",
            Self::ScrollBar => "ScrollBar",
            Self::Slider => "Slider",
            Self::Spinner => "Spinner",
            Self::StatusBar => "StatusBar",
            Self::Tab => "Tab",
            Self::TabItem => "TabItem",
            Self::Text => "Text",
            Self::ToolBar => "ToolBar",
            Self::ToolTip => "ToolTip",
            Self::Tree => "Tree",
            Self::TreeItem => "TreeItem",
            Self::Custom => "Custom",
            Self::Group => "Group",
            Self::Thumb => "Thumb",
            Self::DataGrid => "DataGrid",
            Self::DataItem => "DataItem",
            Self::Document => "Document",
            Self::SplitButton => "SplitButton",
            Self::Window => "Window",
            Self::Pane => "Pane",
            Self::Header => "Header",
            Self::HeaderItem => "HeaderItem",
            Self::Table => "Table",
            Self::TitleBar => "TitleBar",
            Self::Separator => "Separator",
            Self::SemanticZoom => "SemanticZoom",
            Self::AppBar => "AppBar",
        }
    }
}

/// One row of the table: a semantics role, and the control type and ARIA role UI Automation
/// reports for a node of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RoleRow {
    /// The role a node declares.
    pub role: SemanticsRole,
    /// The control type UI Automation reports.
    pub control: ControlType,
    /// The ARIA role UI Automation reports (`UIA_AriaRolePropertyId`).
    pub aria: &'static str,
}

const fn row(role: SemanticsRole, control: ControlType, aria: &'static str) -> RoleRow {
    RoleRow {
        role,
        control,
        aria,
    }
}

/// Every role but `none`, `generic` and `presentation` (which publish no node of their own), in
/// vocabulary order.
pub const ROLES: &[RoleRow] = &[
    row(SemanticsRole::Button, ControlType::Button, "button"),
    row(SemanticsRole::CheckBox, ControlType::CheckBox, "checkbox"),
    row(
        SemanticsRole::RadioButton,
        ControlType::RadioButton,
        "radio",
    ),
    row(SemanticsRole::Switch, ControlType::Button, "switch"),
    row(SemanticsRole::Link, ControlType::Hyperlink, "link"),
    row(SemanticsRole::Slider, ControlType::Slider, "slider"),
    row(
        SemanticsRole::SpinButton,
        ControlType::Spinner,
        "spinbutton",
    ),
    row(SemanticsRole::ComboBox, ControlType::ComboBox, "combobox"),
    row(
        SemanticsRole::ScrollBar,
        ControlType::ScrollBar,
        "scrollbar",
    ),
    row(
        SemanticsRole::Separator,
        ControlType::Separator,
        "separator",
    ),
    row(SemanticsRole::TextInput, ControlType::Edit, "textbox"),
    row(
        SemanticsRole::MultilineTextInput,
        ControlType::Edit,
        "textbox",
    ),
    row(SemanticsRole::PasswordInput, ControlType::Edit, "textbox"),
    row(SemanticsRole::SearchInput, ControlType::Edit, "searchbox"),
    row(SemanticsRole::EmailInput, ControlType::Edit, "textbox"),
    row(SemanticsRole::NumberInput, ControlType::Edit, "textbox"),
    row(
        SemanticsRole::PhoneNumberInput,
        ControlType::Edit,
        "textbox",
    ),
    row(SemanticsRole::UrlInput, ControlType::Edit, "textbox"),
    row(SemanticsRole::DateInput, ControlType::Edit, "textbox"),
    row(SemanticsRole::TimeInput, ControlType::Edit, "textbox"),
    row(SemanticsRole::ListBox, ControlType::List, "listbox"),
    row(
        SemanticsRole::ListBoxOption,
        ControlType::ListItem,
        "option",
    ),
    row(SemanticsRole::RadioGroup, ControlType::Group, "radiogroup"),
    row(SemanticsRole::Menu, ControlType::Menu, "menu"),
    row(SemanticsRole::MenuBar, ControlType::MenuBar, "menubar"),
    row(SemanticsRole::MenuItem, ControlType::MenuItem, "menuitem"),
    row(
        SemanticsRole::MenuItemCheckbox,
        ControlType::CheckBox,
        "menuitemcheckbox",
    ),
    row(
        SemanticsRole::MenuItemRadio,
        ControlType::RadioButton,
        "menuitemradio",
    ),
    row(SemanticsRole::Tab, ControlType::TabItem, "tab"),
    row(SemanticsRole::TabList, ControlType::Tab, "tablist"),
    row(SemanticsRole::TabPanel, ControlType::Pane, "tabpanel"),
    row(SemanticsRole::Tree, ControlType::Tree, "tree"),
    row(SemanticsRole::TreeItem, ControlType::TreeItem, "treeitem"),
    row(SemanticsRole::TreeGrid, ControlType::DataGrid, "treegrid"),
    row(SemanticsRole::Grid, ControlType::DataGrid, "grid"),
    row(SemanticsRole::GridCell, ControlType::DataItem, "gridcell"),
    row(SemanticsRole::Toolbar, ControlType::ToolBar, "toolbar"),
    row(SemanticsRole::Table, ControlType::Table, "table"),
    row(SemanticsRole::Row, ControlType::DataItem, "row"),
    row(SemanticsRole::RowGroup, ControlType::Group, "rowgroup"),
    row(SemanticsRole::Cell, ControlType::DataItem, "cell"),
    row(
        SemanticsRole::ColumnHeader,
        ControlType::DataItem,
        "columnheader",
    ),
    row(SemanticsRole::RowHeader, ControlType::DataItem, "rowheader"),
    row(SemanticsRole::List, ControlType::List, "list"),
    row(SemanticsRole::ListItem, ControlType::ListItem, "listitem"),
    row(SemanticsRole::Heading, ControlType::Text, "heading"),
    row(SemanticsRole::Paragraph, ControlType::Group, "paragraph"),
    row(SemanticsRole::Image, ControlType::Image, "img"),
    row(SemanticsRole::Figure, ControlType::Group, "figure"),
    row(SemanticsRole::Caption, ControlType::Text, "caption"),
    row(SemanticsRole::Article, ControlType::Group, "article"),
    row(SemanticsRole::Feed, ControlType::Group, "feed"),
    row(SemanticsRole::Group, ControlType::Group, "group"),
    row(SemanticsRole::Note, ControlType::Group, "note"),
    row(SemanticsRole::Blockquote, ControlType::Group, "blockquote"),
    row(SemanticsRole::Code, ControlType::Text, "code"),
    row(SemanticsRole::Emphasis, ControlType::Text, "emphasis"),
    row(SemanticsRole::Strong, ControlType::Text, "strong"),
    row(SemanticsRole::Insertion, ControlType::Group, "insertion"),
    row(SemanticsRole::Deletion, ControlType::Group, "deletion"),
    row(SemanticsRole::Term, ControlType::ListItem, "term"),
    row(SemanticsRole::Definition, ControlType::Group, "definition"),
    row(SemanticsRole::Time, ControlType::Text, "time"),
    row(SemanticsRole::Math, ControlType::Group, "math"),
    row(SemanticsRole::Meter, ControlType::ProgressBar, "meter"),
    row(
        SemanticsRole::ProgressBar,
        ControlType::ProgressBar,
        "progressbar",
    ),
    row(SemanticsRole::Tooltip, ControlType::ToolTip, "tooltip"),
    row(SemanticsRole::Document, ControlType::Document, "document"),
    row(SemanticsRole::Application, ControlType::Pane, "application"),
    row(SemanticsRole::Banner, ControlType::Group, "banner"),
    row(SemanticsRole::Main, ControlType::Group, "main"),
    row(SemanticsRole::Navigation, ControlType::Group, "navigation"),
    row(
        SemanticsRole::Complementary,
        ControlType::Group,
        "complementary",
    ),
    row(
        SemanticsRole::ContentInfo,
        ControlType::Group,
        "contentinfo",
    ),
    row(SemanticsRole::Search, ControlType::Group, "search"),
    row(SemanticsRole::Form, ControlType::Group, "form"),
    row(SemanticsRole::Region, ControlType::Group, "region"),
    row(SemanticsRole::Alert, ControlType::Text, "alert"),
    row(SemanticsRole::Status, ControlType::StatusBar, "status"),
    row(SemanticsRole::Log, ControlType::Group, "log"),
    row(SemanticsRole::Marquee, ControlType::Text, "marquee"),
    row(SemanticsRole::Timer, ControlType::Pane, "timer"),
    row(SemanticsRole::Dialog, ControlType::Window, "dialog"),
    row(
        SemanticsRole::AlertDialog,
        ControlType::Window,
        "alertdialog",
    ),
    row(SemanticsRole::Window, ControlType::Window, "region"),
    row(SemanticsRole::Pane, ControlType::Pane, "region"),
    row(SemanticsRole::ScrollView, ControlType::Pane, "region"),
];

/// The row of `role`; `None` for `none`, `generic` and `presentation`.
#[must_use]
pub fn role_row(role: SemanticsRole) -> Option<RoleRow> {
    ROLES.iter().find(|row| row.role == role).copied()
}

/// What an element reports beside its control type that changes the wire role it folds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RoleFlags {
    /// `UIA_IsPasswordPropertyId`: an edit is a password field.
    pub password: bool,
    /// `UIA_IsDialogPropertyId`: a window is a dialog.
    pub dialog: bool,
}

impl RoleFlags {
    /// An element that is a password field when `password`, a dialog when `dialog`.
    #[must_use]
    pub const fn new(password: bool, dialog: bool) -> Self {
        Self { password, dialog }
    }
}

/// The wire role a UI Automation element of `control` with the ARIA role `aria` and `flags`
/// folds to: see the [module docs](self).
#[must_use]
pub fn wire_role(control: ControlType, aria: &str, flags: RoleFlags) -> Role {
    // The ARIA roles that tell the kinds of one control type apart, which the adapter keeps.
    match aria.trim() {
        "cell" => return Role::Cell,
        "gridcell" => return Role::GridCell,
        "row" => return Role::Row,
        "rowheader" => return Role::RowHeader,
        "columnheader" => return Role::ColumnHeader,
        "switch" => return Role::Switch,
        "heading" => return Role::Heading,
        _ => {}
    }
    match control {
        ControlType::Button | ControlType::SplitButton => Role::Button,
        ControlType::CheckBox => Role::CheckBox,
        ControlType::RadioButton => Role::RadioButton,
        ControlType::ComboBox => Role::ComboBox,
        ControlType::Edit if flags.password => Role::PasswordInput,
        ControlType::Edit => Role::TextInput,
        ControlType::Hyperlink => Role::Link,
        ControlType::Image => Role::Image,
        ControlType::Text => Role::Label,
        ControlType::List => Role::List,
        ControlType::ListItem => Role::ListItem,
        ControlType::Menu => Role::Menu,
        ControlType::MenuBar => Role::MenuBar,
        ControlType::MenuItem => Role::MenuItem,
        ControlType::ProgressBar => Role::ProgressIndicator,
        ControlType::ScrollBar => Role::ScrollBar,
        ControlType::Slider => Role::Slider,
        ControlType::Spinner => Role::SpinButton,
        ControlType::StatusBar => Role::Status,
        ControlType::Tab => Role::TabList,
        ControlType::TabItem => Role::Tab,
        ControlType::ToolBar | ControlType::AppBar => Role::Toolbar,
        ControlType::ToolTip => Role::Tooltip,
        ControlType::Tree => Role::Tree,
        ControlType::TreeItem => Role::TreeItem,
        ControlType::Group | ControlType::Header => Role::Group,
        ControlType::DataGrid => Role::Grid,
        ControlType::DataItem => Role::Row,
        ControlType::HeaderItem => Role::ColumnHeader,
        ControlType::Table => Role::Table,
        ControlType::Document => Role::Document,
        ControlType::Window if flags.dialog => Role::Dialog,
        ControlType::Window => Role::Window,
        ControlType::Pane => Role::Pane,
        ControlType::TitleBar => Role::TitleBar,
        ControlType::Separator => Role::Splitter,
        ControlType::Calendar
        | ControlType::Custom
        | ControlType::Thumb
        | ControlType::SemanticZoom => Role::Unknown,
    }
}

/// The wire role a node of `row`'s role folds to, read the way UI Automation reports it.
#[must_use]
pub fn expected_wire_role(row: &RoleRow) -> Role {
    let flags = RoleFlags::new(
        row.role == SemanticsRole::PasswordInput,
        matches!(row.role, SemanticsRole::Dialog | SemanticsRole::AlertDialog),
    );
    wire_role(row.control, row.aria, flags)
}
