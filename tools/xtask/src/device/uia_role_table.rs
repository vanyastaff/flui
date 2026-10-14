//! What UI Automation must report for the probe's role-family page
//! (`examples/a11y_probe/roles.rs`), as plain data: the Windows check
//! (`windows_a11y/roles.rs`) reads it, and its guards run on every host, so a
//! lockfile bump or a new role fails CI rather than only a Windows desktop.
//!
//! The expected values are transcribed from the locked `accesskit_windows`
//! ([`ADAPTER_CHECKED_AGAINST`]), composed with FLUI's own role mapping
//! (ADR-0203): the adapter's `control_type` and `aria_role`.

/// The `accesskit_windows` version [`ROLES`] and the family checks were
/// transcribed from.
pub(super) const ADAPTER_CHECKED_AGAINST: &str = "0.35.1";

/// The probe variable that selects a page, and the role-family page's value.
pub(super) const PAGE_ENV: &str = "FLUI_PROBE_FIXTURE";
pub(super) const ROLES_PAGE: &str = "roles";

/// The roles ARIA forbids a name: their fixture's text is the description.
pub(super) const NAME_PROHIBITED: &[&str] = &[
    "caption",
    "code",
    "deletion",
    "emphasis",
    "insertion",
    "paragraph",
    "strong",
];

/// A UI Automation control type, by the name of its `UIA_*ControlTypeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlType {
    Button,
    CheckBox,
    ComboBox,
    DataGrid,
    DataItem,
    Document,
    Edit,
    Group,
    Hyperlink,
    Image,
    List,
    ListItem,
    Menu,
    MenuBar,
    MenuItem,
    Pane,
    ProgressBar,
    RadioButton,
    ScrollBar,
    Separator,
    Slider,
    Spinner,
    StatusBar,
    Tab,
    TabItem,
    Table,
    Text,
    ToolBar,
    ToolTip,
    Tree,
    TreeItem,
    Window,
}

/// Every role but `none`, by its vocabulary name, in vocabulary order: the
/// control type and ARIA role UI Automation reports for its fixture.
pub(super) const ROLES: &[(&str, ControlType, &str)] = &[
    ("button", ControlType::Button, "button"),
    ("checkBox", ControlType::CheckBox, "checkbox"),
    ("radioButton", ControlType::RadioButton, "radio"),
    ("switch", ControlType::Button, "switch"),
    ("link", ControlType::Hyperlink, "link"),
    ("slider", ControlType::Slider, "slider"),
    ("spinButton", ControlType::Spinner, "spinbutton"),
    ("comboBox", ControlType::ComboBox, "combobox"),
    ("scrollBar", ControlType::ScrollBar, "scrollbar"),
    ("separator", ControlType::Separator, "separator"),
    ("textInput", ControlType::Edit, "textbox"),
    ("multilineTextInput", ControlType::Edit, "textbox"),
    ("passwordInput", ControlType::Edit, "textbox"),
    ("searchInput", ControlType::Edit, "searchbox"),
    ("emailInput", ControlType::Edit, "textbox"),
    ("numberInput", ControlType::Edit, "textbox"),
    ("phoneNumberInput", ControlType::Edit, "textbox"),
    ("urlInput", ControlType::Edit, "textbox"),
    ("dateInput", ControlType::Edit, "textbox"),
    ("timeInput", ControlType::Edit, "textbox"),
    ("listBox", ControlType::List, "listbox"),
    ("listBoxOption", ControlType::ListItem, "option"),
    ("radioGroup", ControlType::Group, "radiogroup"),
    ("menu", ControlType::Menu, "menu"),
    ("menuBar", ControlType::MenuBar, "menubar"),
    ("menuItem", ControlType::MenuItem, "menuitem"),
    (
        "menuItemCheckbox",
        ControlType::CheckBox,
        "menuitemcheckbox",
    ),
    ("menuItemRadio", ControlType::RadioButton, "menuitemradio"),
    ("tab", ControlType::TabItem, "tab"),
    ("tabList", ControlType::Tab, "tablist"),
    ("tabPanel", ControlType::Pane, "tabpanel"),
    ("tree", ControlType::Tree, "tree"),
    ("treeItem", ControlType::TreeItem, "treeitem"),
    ("treeGrid", ControlType::DataGrid, "treegrid"),
    ("grid", ControlType::DataGrid, "grid"),
    ("gridCell", ControlType::DataItem, "gridcell"),
    ("toolbar", ControlType::ToolBar, "toolbar"),
    ("table", ControlType::Table, "table"),
    ("row", ControlType::DataItem, "row"),
    ("rowGroup", ControlType::Group, "rowgroup"),
    ("cell", ControlType::DataItem, "cell"),
    ("columnHeader", ControlType::DataItem, "columnheader"),
    ("rowHeader", ControlType::DataItem, "rowheader"),
    ("list", ControlType::List, "list"),
    ("listItem", ControlType::ListItem, "listitem"),
    ("heading", ControlType::Text, "heading"),
    ("paragraph", ControlType::Group, "paragraph"),
    ("image", ControlType::Image, "img"),
    ("figure", ControlType::Group, "figure"),
    ("caption", ControlType::Text, "caption"),
    ("article", ControlType::Group, "article"),
    ("feed", ControlType::Group, "feed"),
    ("group", ControlType::Group, "group"),
    ("note", ControlType::Group, "note"),
    ("blockquote", ControlType::Group, "blockquote"),
    ("code", ControlType::Text, "code"),
    ("emphasis", ControlType::Text, "emphasis"),
    ("strong", ControlType::Text, "strong"),
    ("insertion", ControlType::Group, "insertion"),
    ("deletion", ControlType::Group, "deletion"),
    ("term", ControlType::ListItem, "term"),
    ("definition", ControlType::Group, "definition"),
    ("time", ControlType::Text, "time"),
    ("math", ControlType::Group, "math"),
    ("meter", ControlType::ProgressBar, "meter"),
    ("progressBar", ControlType::ProgressBar, "progressbar"),
    ("tooltip", ControlType::ToolTip, "tooltip"),
    ("document", ControlType::Document, "document"),
    ("application", ControlType::Pane, "application"),
    ("banner", ControlType::Group, "banner"),
    ("main", ControlType::Group, "main"),
    ("navigation", ControlType::Group, "navigation"),
    ("complementary", ControlType::Group, "complementary"),
    ("contentInfo", ControlType::Group, "contentinfo"),
    ("search", ControlType::Group, "search"),
    ("form", ControlType::Group, "form"),
    ("region", ControlType::Group, "region"),
    ("alert", ControlType::Text, "alert"),
    ("status", ControlType::StatusBar, "status"),
    ("log", ControlType::Group, "log"),
    ("marquee", ControlType::Text, "marquee"),
    ("timer", ControlType::Pane, "timer"),
    ("dialog", ControlType::Window, "dialog"),
    ("alertDialog", ControlType::Window, "alertdialog"),
    ("window", ControlType::Window, "region"),
    ("pane", ControlType::Pane, "region"),
    ("scrollView", ControlType::Pane, "region"),
];

/// The fixtures that carry each family's states and properties, by name;
/// the Windows check holds each to its own expectation.
pub(super) const FAMILY_FIXTURES: &[&str] = &[
    "checked check box",
    "mixed check box",
    "switch on",
    "pressed toggle button",
    "checked radio",
    "unchecked menu radio",
    "selected option",
    "unselected tab",
    "collapsed combo box",
    "expanded tree item",
    "progress at 30",
    "splitter at 40",
    "static separator",
    "draft field",
    "read-only field",
    "required field",
    "placeholder field",
    "secret field",
    "level three heading",
    "multilineTextInput fixture",
    "modal dialog",
    "disabled button",
    "status fixture",
    "log fixture",
    "alert fixture",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The expected values were read from the adapter the workspace locks; a
    /// bump fails here until the table is checked against the new one.
    #[test]
    fn the_table_was_checked_against_the_locked_adapter() {
        let lock = include_str!("../../../../Cargo.lock");
        let locked = lock
            .split("[[package]]")
            .find(|entry| entry.contains("name = \"accesskit_windows\""))
            .and_then(|entry| {
                entry
                    .lines()
                    .find_map(|line| line.strip_prefix("version = \""))
                    .map(|version| version.trim_end_matches('"'))
            });
        assert_eq!(locked, Some(ADAPTER_CHECKED_AGAINST));
    }

    /// Every role of the vocabulary but `none` has a row, in vocabulary
    /// order, and every fixture the check reads is on the probe's page.
    #[test]
    fn the_table_covers_the_probe_page() {
        let page = include_str!("../../../../examples/a11y_probe/roles.rs");
        for name in FAMILY_FIXTURES {
            if !name.ends_with(" fixture") {
                assert!(
                    page.contains(&format!("{name:?}")),
                    "{name} is not on the page"
                );
            }
        }
        assert!(
            page.contains(&format!("{PAGE_ENV:?}")) && page.contains(&format!("{ROLES_PAGE:?}")),
            "the page is selected by {PAGE_ENV}={ROLES_PAGE}"
        );
        let roles = include_str!("../../../../crates/flui-protocol/src/semantics.rs");
        let vocabulary: Vec<&str> = roles
            .split("pub enum SemanticsRole")
            .nth(1)
            .and_then(|rest| rest.split("\n    }").next())
            .expect("BUG: the role vocabulary is declared in flui-protocol")
            .lines()
            .filter_map(|line| line.split("=> \"").nth(1))
            .map(|name| name.trim_end_matches("\","))
            .filter(|name| *name != "none")
            .collect();
        let table: Vec<&str> = ROLES.iter().map(|(name, ..)| *name).collect();
        assert_eq!(table, vocabulary, "one row per role, in vocabulary order");
    }
}
