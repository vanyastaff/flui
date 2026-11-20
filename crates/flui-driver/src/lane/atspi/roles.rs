//! What AT-SPI reports for each semantics role, and how an AT-SPI role folds back onto the wire's
//! [`Role`], the vocabulary both sides of the projection speak (ADR-0095 §4).
//!
//! [`ROLES`] is transcribed from the locked `accesskit_atspi_common` ([`ADAPTER_CHECKED_AGAINST`],
//! which `accesskit_unix` serves the app's tree through) composed with FLUI's role mapping
//! (ADR-0203): for each [`SemanticsRole`], the AT-SPI role the adapter gives it. [`wire_role`]
//! reads a node the other way.
//!
//! AT-SPI keeps fewer kinds apart than the wire, whose roles are the fold to what UI Automation
//! can express: one AT-SPI role stands for several wire roles in three places, and the fold reads
//! the one most rows mean. A `panel` is a group (a radio group, a figure, a feed, a row group, a
//! group) and also a pane or a scroll view; a `table` is a table and also a grid; a `table cell`
//! is a cell and also a grid cell. [`expected_wire_role`] is what a node of a row's role reads as
//! through AT-SPI, which for a grid, a grid cell, a pane and a scroll view is not what the app
//! publishes; [`keeps_apart`] names the roles AT-SPI keeps.

use flui_protocol::semantics::SemanticsRole;
use flui_protocol::wire::Role;

/// The `accesskit_atspi_common` version [`ROLES`] was transcribed from.
pub const ADAPTER_CHECKED_AGAINST: &str = "0.21.0";

/// An AT-SPI role, by its number in `AtspiRole` (`atspi-constants.h`), as `GetRole` answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtspiRole(pub u32);

/// The names AT-SPI gives its roles (`GetRoleName`), by number; a number past the end is a role
/// this table does not know.
const NAMES: &[&str] = &[
    "invalid",
    "accelerator label",
    "alert",
    "animation",
    "arrow",
    "calendar",
    "canvas",
    "check box",
    "check menu item",
    "color chooser",
    "column header",
    "combo box",
    "date editor",
    "desktop icon",
    "desktop frame",
    "dial",
    "dialog",
    "directory pane",
    "drawing area",
    "file chooser",
    "filler",
    "focus traversable",
    "font chooser",
    "frame",
    "glass pane",
    "html container",
    "icon",
    "image",
    "internal frame",
    "label",
    "layered pane",
    "list",
    "list item",
    "menu",
    "menu bar",
    "menu item",
    "option pane",
    "page tab",
    "page tab list",
    "panel",
    "password text",
    "popup menu",
    "progress bar",
    "button",
    "radio button",
    "radio menu item",
    "root pane",
    "row header",
    "scroll bar",
    "scroll pane",
    "separator",
    "slider",
    "spin button",
    "split pane",
    "status bar",
    "table",
    "table cell",
    "table column header",
    "table row header",
    "tearoff menu item",
    "terminal",
    "text",
    "toggle button",
    "tool bar",
    "tool tip",
    "tree",
    "tree table",
    "unknown",
    "viewport",
    "window",
    "extended",
    "header",
    "footer",
    "paragraph",
    "ruler",
    "application",
    "autocomplete",
    "editbar",
    "embedded",
    "entry",
    "chart",
    "caption",
    "document frame",
    "heading",
    "page",
    "section",
    "redundant object",
    "form",
    "link",
    "input method window",
    "table row",
    "tree item",
    "document spreadsheet",
    "document presentation",
    "document text",
    "document web",
    "document email",
    "comment",
    "list box",
    "grouping",
    "image map",
    "notification",
    "info bar",
    "level bar",
    "title bar",
    "block quote",
    "audio",
    "video",
    "definition",
    "article",
    "landmark",
    "log",
    "marquee",
    "math",
    "rating",
    "timer",
    "static",
    "math fraction",
    "math root",
    "subscript",
    "superscript",
    "description list",
    "description term",
    "description value",
    "footnote",
    "content deletion",
    "content insertion",
    "mark",
    "suggestion",
    "push button menu",
];

impl AtspiRole {
    /// `ATSPI_ROLE_ALERT`.
    pub const ALERT: Self = Self(2);
    /// `ATSPI_ROLE_CANVAS`.
    pub const CANVAS: Self = Self(6);
    /// `ATSPI_ROLE_CHECK_BOX`.
    pub const CHECK_BOX: Self = Self(7);
    /// `ATSPI_ROLE_CHECK_MENU_ITEM`.
    pub const CHECK_MENU_ITEM: Self = Self(8);
    /// `ATSPI_ROLE_COLUMN_HEADER`.
    pub const COLUMN_HEADER: Self = Self(10);
    /// `ATSPI_ROLE_COMBO_BOX`.
    pub const COMBO_BOX: Self = Self(11);
    /// `ATSPI_ROLE_DATE_EDITOR`.
    pub const DATE_EDITOR: Self = Self(12);
    /// `ATSPI_ROLE_DIALOG`.
    pub const DIALOG: Self = Self(16);
    /// `ATSPI_ROLE_FRAME`: a window with a title, which is how the adapter reports a window.
    pub const FRAME: Self = Self(23);
    /// `ATSPI_ROLE_IMAGE`.
    pub const IMAGE: Self = Self(27);
    /// `ATSPI_ROLE_LABEL`.
    pub const LABEL: Self = Self(29);
    /// `ATSPI_ROLE_LIST`.
    pub const LIST: Self = Self(31);
    /// `ATSPI_ROLE_LIST_ITEM`.
    pub const LIST_ITEM: Self = Self(32);
    /// `ATSPI_ROLE_MENU`.
    pub const MENU: Self = Self(33);
    /// `ATSPI_ROLE_MENU_BAR`.
    pub const MENU_BAR: Self = Self(34);
    /// `ATSPI_ROLE_MENU_ITEM`.
    pub const MENU_ITEM: Self = Self(35);
    /// `ATSPI_ROLE_PAGE_TAB`.
    pub const PAGE_TAB: Self = Self(37);
    /// `ATSPI_ROLE_PAGE_TAB_LIST`.
    pub const PAGE_TAB_LIST: Self = Self(38);
    /// `ATSPI_ROLE_PANEL`.
    pub const PANEL: Self = Self(39);
    /// `ATSPI_ROLE_PASSWORD_TEXT`.
    pub const PASSWORD_TEXT: Self = Self(40);
    /// `ATSPI_ROLE_PROGRESS_BAR`.
    pub const PROGRESS_BAR: Self = Self(42);
    /// `ATSPI_ROLE_BUTTON` (`PUSH_BUTTON`).
    pub const BUTTON: Self = Self(43);
    /// `ATSPI_ROLE_RADIO_BUTTON`.
    pub const RADIO_BUTTON: Self = Self(44);
    /// `ATSPI_ROLE_RADIO_MENU_ITEM`.
    pub const RADIO_MENU_ITEM: Self = Self(45);
    /// `ATSPI_ROLE_ROW_HEADER`.
    pub const ROW_HEADER: Self = Self(47);
    /// `ATSPI_ROLE_SCROLL_BAR`.
    pub const SCROLL_BAR: Self = Self(48);
    /// `ATSPI_ROLE_SCROLL_PANE`.
    pub const SCROLL_PANE: Self = Self(49);
    /// `ATSPI_ROLE_SEPARATOR`.
    pub const SEPARATOR: Self = Self(50);
    /// `ATSPI_ROLE_SLIDER`.
    pub const SLIDER: Self = Self(51);
    /// `ATSPI_ROLE_SPIN_BUTTON`.
    pub const SPIN_BUTTON: Self = Self(52);
    /// `ATSPI_ROLE_STATUS_BAR`.
    pub const STATUS_BAR: Self = Self(54);
    /// `ATSPI_ROLE_TABLE`.
    pub const TABLE: Self = Self(55);
    /// `ATSPI_ROLE_TABLE_CELL`.
    pub const TABLE_CELL: Self = Self(56);
    /// `ATSPI_ROLE_TERMINAL`.
    pub const TERMINAL: Self = Self(60);
    /// `ATSPI_ROLE_TOGGLE_BUTTON`: a switch, or a button that toggles.
    pub const TOGGLE_BUTTON: Self = Self(62);
    /// `ATSPI_ROLE_TOOL_BAR`.
    pub const TOOL_BAR: Self = Self(63);
    /// `ATSPI_ROLE_TOOL_TIP`.
    pub const TOOL_TIP: Self = Self(64);
    /// `ATSPI_ROLE_TREE`.
    pub const TREE: Self = Self(65);
    /// `ATSPI_ROLE_TREE_TABLE`.
    pub const TREE_TABLE: Self = Self(66);
    /// `ATSPI_ROLE_UNKNOWN`.
    pub const UNKNOWN: Self = Self(67);
    /// `ATSPI_ROLE_WINDOW`: a window without a title.
    pub const WINDOW: Self = Self(69);
    /// `ATSPI_ROLE_EXTENDED`: a role named by its role description.
    pub const EXTENDED: Self = Self(70);
    /// `ATSPI_ROLE_HEADER`.
    pub const HEADER: Self = Self(71);
    /// `ATSPI_ROLE_FOOTER`.
    pub const FOOTER: Self = Self(72);
    /// `ATSPI_ROLE_PARAGRAPH`.
    pub const PARAGRAPH: Self = Self(73);
    /// `ATSPI_ROLE_EMBEDDED`.
    pub const EMBEDDED: Self = Self(78);
    /// `ATSPI_ROLE_ENTRY`.
    pub const ENTRY: Self = Self(79);
    /// `ATSPI_ROLE_CAPTION`.
    pub const CAPTION: Self = Self(81);
    /// `ATSPI_ROLE_DOCUMENT_FRAME`.
    pub const DOCUMENT_FRAME: Self = Self(82);
    /// `ATSPI_ROLE_HEADING`.
    pub const HEADING: Self = Self(83);
    /// `ATSPI_ROLE_SECTION`.
    pub const SECTION: Self = Self(85);
    /// `ATSPI_ROLE_FORM`.
    pub const FORM: Self = Self(87);
    /// `ATSPI_ROLE_LINK`.
    pub const LINK: Self = Self(88);
    /// `ATSPI_ROLE_TABLE_ROW`.
    pub const TABLE_ROW: Self = Self(90);
    /// `ATSPI_ROLE_TREE_ITEM`.
    pub const TREE_ITEM: Self = Self(91);
    /// `ATSPI_ROLE_DOCUMENT_WEB`.
    pub const DOCUMENT_WEB: Self = Self(95);
    /// `ATSPI_ROLE_COMMENT`.
    pub const COMMENT: Self = Self(97);
    /// `ATSPI_ROLE_LIST_BOX`.
    pub const LIST_BOX: Self = Self(98);
    /// `ATSPI_ROLE_NOTIFICATION`.
    pub const NOTIFICATION: Self = Self(101);
    /// `ATSPI_ROLE_LEVEL_BAR`.
    pub const LEVEL_BAR: Self = Self(103);
    /// `ATSPI_ROLE_TITLE_BAR`.
    pub const TITLE_BAR: Self = Self(104);
    /// `ATSPI_ROLE_BLOCK_QUOTE`.
    pub const BLOCK_QUOTE: Self = Self(105);
    /// `ATSPI_ROLE_AUDIO`.
    pub const AUDIO: Self = Self(106);
    /// `ATSPI_ROLE_VIDEO`.
    pub const VIDEO: Self = Self(107);
    /// `ATSPI_ROLE_ARTICLE`.
    pub const ARTICLE: Self = Self(109);
    /// `ATSPI_ROLE_LANDMARK`.
    pub const LANDMARK: Self = Self(110);
    /// `ATSPI_ROLE_LOG`.
    pub const LOG: Self = Self(111);
    /// `ATSPI_ROLE_MARQUEE`.
    pub const MARQUEE: Self = Self(112);
    /// `ATSPI_ROLE_MATH`.
    pub const MATH: Self = Self(113);
    /// `ATSPI_ROLE_TIMER`.
    pub const TIMER: Self = Self(115);
    /// `ATSPI_ROLE_STATIC`.
    pub const STATIC: Self = Self(116);
    /// `ATSPI_ROLE_DESCRIPTION_LIST`.
    pub const DESCRIPTION_LIST: Self = Self(121);
    /// `ATSPI_ROLE_DESCRIPTION_TERM`.
    pub const DESCRIPTION_TERM: Self = Self(122);
    /// `ATSPI_ROLE_DESCRIPTION_VALUE`.
    pub const DESCRIPTION_VALUE: Self = Self(123);
    /// `ATSPI_ROLE_FOOTNOTE`.
    pub const FOOTNOTE: Self = Self(124);
    /// `ATSPI_ROLE_CONTENT_DELETION`.
    pub const CONTENT_DELETION: Self = Self(125);
    /// `ATSPI_ROLE_CONTENT_INSERTION`.
    pub const CONTENT_INSERTION: Self = Self(126);
    /// `ATSPI_ROLE_MARK`.
    pub const MARK: Self = Self(127);
    /// `ATSPI_ROLE_SUGGESTION`.
    pub const SUGGESTION: Self = Self(128);

    /// The role's name, as `GetRoleName` spells it; `None` for a number AT-SPI does not define.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        NAMES.get(usize::try_from(self.0).ok()?).copied()
    }

    /// How a projection names the role: its AT-SPI name, or its number where it has none.
    #[must_use]
    pub fn native_name(self) -> String {
        self.name()
            .map_or_else(|| format!("role {}", self.0), str::to_owned)
    }
}

/// One row of the table: a semantics role, and the AT-SPI role a node of it reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RoleRow {
    /// The role a node declares.
    pub role: SemanticsRole,
    /// The AT-SPI role the adapter reports.
    pub atspi: AtspiRole,
}

const fn row(role: SemanticsRole, atspi: AtspiRole) -> RoleRow {
    RoleRow { role, atspi }
}

/// Every role but `none`, `generic` and `presentation` (which publish no node of their own), in
/// vocabulary order.
pub const ROLES: &[RoleRow] = &[
    row(SemanticsRole::Button, AtspiRole::BUTTON),
    row(SemanticsRole::CheckBox, AtspiRole::CHECK_BOX),
    row(SemanticsRole::RadioButton, AtspiRole::RADIO_BUTTON),
    row(SemanticsRole::Switch, AtspiRole::TOGGLE_BUTTON),
    row(SemanticsRole::Link, AtspiRole::LINK),
    row(SemanticsRole::Slider, AtspiRole::SLIDER),
    row(SemanticsRole::SpinButton, AtspiRole::SPIN_BUTTON),
    row(SemanticsRole::ComboBox, AtspiRole::COMBO_BOX),
    row(SemanticsRole::ScrollBar, AtspiRole::SCROLL_BAR),
    row(SemanticsRole::Separator, AtspiRole::SEPARATOR),
    row(SemanticsRole::TextInput, AtspiRole::ENTRY),
    row(SemanticsRole::MultilineTextInput, AtspiRole::ENTRY),
    row(SemanticsRole::PasswordInput, AtspiRole::PASSWORD_TEXT),
    row(SemanticsRole::SearchInput, AtspiRole::ENTRY),
    row(SemanticsRole::EmailInput, AtspiRole::ENTRY),
    row(SemanticsRole::NumberInput, AtspiRole::ENTRY),
    row(SemanticsRole::PhoneNumberInput, AtspiRole::ENTRY),
    row(SemanticsRole::UrlInput, AtspiRole::ENTRY),
    row(SemanticsRole::DateInput, AtspiRole::DATE_EDITOR),
    row(SemanticsRole::TimeInput, AtspiRole::DATE_EDITOR),
    row(SemanticsRole::ListBox, AtspiRole::LIST_BOX),
    row(SemanticsRole::ListBoxOption, AtspiRole::LIST_ITEM),
    row(SemanticsRole::RadioGroup, AtspiRole::PANEL),
    row(SemanticsRole::Menu, AtspiRole::MENU),
    row(SemanticsRole::MenuBar, AtspiRole::MENU_BAR),
    row(SemanticsRole::MenuItem, AtspiRole::MENU_ITEM),
    row(SemanticsRole::MenuItemCheckbox, AtspiRole::CHECK_MENU_ITEM),
    row(SemanticsRole::MenuItemRadio, AtspiRole::RADIO_MENU_ITEM),
    row(SemanticsRole::Tab, AtspiRole::PAGE_TAB),
    row(SemanticsRole::TabList, AtspiRole::PAGE_TAB_LIST),
    row(SemanticsRole::TabPanel, AtspiRole::SCROLL_PANE),
    row(SemanticsRole::Tree, AtspiRole::TREE),
    row(SemanticsRole::TreeItem, AtspiRole::TREE_ITEM),
    row(SemanticsRole::TreeGrid, AtspiRole::TREE_TABLE),
    row(SemanticsRole::Grid, AtspiRole::TABLE),
    row(SemanticsRole::GridCell, AtspiRole::TABLE_CELL),
    row(SemanticsRole::Toolbar, AtspiRole::TOOL_BAR),
    row(SemanticsRole::Table, AtspiRole::TABLE),
    row(SemanticsRole::Row, AtspiRole::TABLE_ROW),
    row(SemanticsRole::RowGroup, AtspiRole::PANEL),
    row(SemanticsRole::Cell, AtspiRole::TABLE_CELL),
    row(SemanticsRole::ColumnHeader, AtspiRole::COLUMN_HEADER),
    row(SemanticsRole::RowHeader, AtspiRole::ROW_HEADER),
    row(SemanticsRole::List, AtspiRole::LIST),
    row(SemanticsRole::ListItem, AtspiRole::LIST_ITEM),
    row(SemanticsRole::Heading, AtspiRole::HEADING),
    row(SemanticsRole::Paragraph, AtspiRole::PARAGRAPH),
    row(SemanticsRole::Image, AtspiRole::IMAGE),
    row(SemanticsRole::Figure, AtspiRole::PANEL),
    row(SemanticsRole::Caption, AtspiRole::CAPTION),
    row(SemanticsRole::Article, AtspiRole::ARTICLE),
    row(SemanticsRole::Feed, AtspiRole::PANEL),
    row(SemanticsRole::Group, AtspiRole::PANEL),
    row(SemanticsRole::Note, AtspiRole::COMMENT),
    row(SemanticsRole::Blockquote, AtspiRole::BLOCK_QUOTE),
    row(SemanticsRole::Code, AtspiRole::STATIC),
    row(SemanticsRole::Emphasis, AtspiRole::STATIC),
    row(SemanticsRole::Strong, AtspiRole::STATIC),
    row(SemanticsRole::Insertion, AtspiRole::CONTENT_INSERTION),
    row(SemanticsRole::Deletion, AtspiRole::CONTENT_DELETION),
    row(SemanticsRole::Term, AtspiRole::DESCRIPTION_TERM),
    row(SemanticsRole::Definition, AtspiRole::DESCRIPTION_VALUE),
    row(SemanticsRole::Time, AtspiRole::STATIC),
    row(SemanticsRole::Math, AtspiRole::MATH),
    row(SemanticsRole::Meter, AtspiRole::LEVEL_BAR),
    row(SemanticsRole::ProgressBar, AtspiRole::PROGRESS_BAR),
    row(SemanticsRole::Tooltip, AtspiRole::TOOL_TIP),
    row(SemanticsRole::Document, AtspiRole::DOCUMENT_FRAME),
    row(SemanticsRole::Application, AtspiRole::EMBEDDED),
    row(SemanticsRole::Banner, AtspiRole::LANDMARK),
    row(SemanticsRole::Main, AtspiRole::LANDMARK),
    row(SemanticsRole::Navigation, AtspiRole::LANDMARK),
    row(SemanticsRole::Complementary, AtspiRole::LANDMARK),
    row(SemanticsRole::ContentInfo, AtspiRole::LANDMARK),
    row(SemanticsRole::Search, AtspiRole::LANDMARK),
    row(SemanticsRole::Form, AtspiRole::FORM),
    row(SemanticsRole::Region, AtspiRole::LANDMARK),
    row(SemanticsRole::Alert, AtspiRole::NOTIFICATION),
    row(SemanticsRole::Status, AtspiRole::STATUS_BAR),
    row(SemanticsRole::Log, AtspiRole::LOG),
    row(SemanticsRole::Marquee, AtspiRole::MARQUEE),
    row(SemanticsRole::Timer, AtspiRole::TIMER),
    row(SemanticsRole::Dialog, AtspiRole::DIALOG),
    row(SemanticsRole::AlertDialog, AtspiRole::ALERT),
    row(SemanticsRole::Window, AtspiRole::FRAME),
    row(SemanticsRole::Pane, AtspiRole::PANEL),
    row(SemanticsRole::ScrollView, AtspiRole::PANEL),
];

/// The row of `role`; `None` for `none`, `generic` and `presentation`.
#[must_use]
pub fn role_row(role: SemanticsRole) -> Option<RoleRow> {
    ROLES.iter().find(|row| row.role == role).copied()
}

/// The wire role a node AT-SPI reports as `role` folds to: see the [module docs](self).
#[must_use]
pub fn wire_role(role: AtspiRole) -> Role {
    match role {
        AtspiRole::BUTTON => Role::Button,
        AtspiRole::TOGGLE_BUTTON => Role::Switch,
        AtspiRole::CHECK_BOX | AtspiRole::CHECK_MENU_ITEM => Role::CheckBox,
        AtspiRole::RADIO_BUTTON | AtspiRole::RADIO_MENU_ITEM => Role::RadioButton,
        AtspiRole::LINK => Role::Link,
        AtspiRole::SLIDER => Role::Slider,
        AtspiRole::SPIN_BUTTON => Role::SpinButton,
        AtspiRole::COMBO_BOX => Role::ComboBox,
        AtspiRole::SCROLL_BAR => Role::ScrollBar,
        AtspiRole::SEPARATOR => Role::Splitter,
        AtspiRole::ENTRY | AtspiRole::DATE_EDITOR => Role::TextInput,
        AtspiRole::PASSWORD_TEXT => Role::PasswordInput,
        AtspiRole::LIST | AtspiRole::LIST_BOX | AtspiRole::DESCRIPTION_LIST => Role::List,
        AtspiRole::LIST_ITEM | AtspiRole::DESCRIPTION_TERM | AtspiRole::FOOTNOTE => Role::ListItem,
        AtspiRole::MENU => Role::Menu,
        AtspiRole::MENU_BAR => Role::MenuBar,
        AtspiRole::MENU_ITEM => Role::MenuItem,
        AtspiRole::PAGE_TAB => Role::Tab,
        AtspiRole::PAGE_TAB_LIST => Role::TabList,
        AtspiRole::TREE => Role::Tree,
        AtspiRole::TREE_ITEM => Role::TreeItem,
        AtspiRole::TREE_TABLE => Role::Grid,
        AtspiRole::TABLE => Role::Table,
        AtspiRole::TABLE_CELL => Role::Cell,
        AtspiRole::TABLE_ROW => Role::Row,
        AtspiRole::COLUMN_HEADER => Role::ColumnHeader,
        AtspiRole::ROW_HEADER => Role::RowHeader,
        AtspiRole::TOOL_BAR => Role::Toolbar,
        AtspiRole::TOOL_TIP => Role::Tooltip,
        AtspiRole::HEADING => Role::Heading,
        AtspiRole::IMAGE | AtspiRole::CANVAS => Role::Image,
        AtspiRole::LABEL
        | AtspiRole::STATIC
        | AtspiRole::CAPTION
        | AtspiRole::NOTIFICATION
        | AtspiRole::MARQUEE
        | AtspiRole::MARK => Role::Label,
        AtspiRole::LEVEL_BAR | AtspiRole::PROGRESS_BAR => Role::ProgressIndicator,
        AtspiRole::STATUS_BAR => Role::Status,
        AtspiRole::DIALOG | AtspiRole::ALERT => Role::Dialog,
        AtspiRole::FRAME | AtspiRole::WINDOW => Role::Window,
        AtspiRole::SCROLL_PANE | AtspiRole::EMBEDDED | AtspiRole::TIMER => Role::Pane,
        AtspiRole::DOCUMENT_FRAME | AtspiRole::DOCUMENT_WEB | AtspiRole::TERMINAL => Role::Document,
        AtspiRole::TITLE_BAR => Role::TitleBar,
        AtspiRole::PANEL
        | AtspiRole::SECTION
        | AtspiRole::PARAGRAPH
        | AtspiRole::ARTICLE
        | AtspiRole::COMMENT
        | AtspiRole::BLOCK_QUOTE
        | AtspiRole::CONTENT_INSERTION
        | AtspiRole::CONTENT_DELETION
        | AtspiRole::DESCRIPTION_VALUE
        | AtspiRole::MATH
        | AtspiRole::LANDMARK
        | AtspiRole::FORM
        | AtspiRole::LOG
        | AtspiRole::HEADER
        | AtspiRole::FOOTER
        | AtspiRole::AUDIO
        | AtspiRole::VIDEO
        | AtspiRole::SUGGESTION => Role::Group,
        _ => Role::Unknown,
    }
}

/// The wire role a node of `row`'s role reads as through AT-SPI.
#[must_use]
pub fn expected_wire_role(row: &RoleRow) -> Role {
    wire_role(row.atspi)
}

/// Whether AT-SPI keeps `role` apart from the other roles it reads the same way: false for the
/// roles whose AT-SPI role stands for several wire roles and folds to another (see the
/// [module docs](self)), whose projection differs from what the app publishes by its role alone.
#[must_use]
pub fn keeps_apart(role: SemanticsRole) -> bool {
    !matches!(
        role,
        SemanticsRole::Grid
            | SemanticsRole::GridCell
            | SemanticsRole::Pane
            | SemanticsRole::ScrollView
    )
}
