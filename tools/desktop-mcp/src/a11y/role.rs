//! The vocabulary the tools speak, whatever the OS underneath: roles from
//! AccessKit (the one set already mapped to UI Automation, AppKit and
//! AT-SPI in one codebase, and FLUI's own semantics vocabulary), and actions
//! named after the tools that perform them.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What kind of control an element is, in AccessKit's names. The native
/// name (`Node::native_role`) is reported alongside, so a role this list
/// does not cover is still visible as what the OS calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    Button,
    CheckBox,
    RadioButton,
    Switch,
    ComboBox,
    TextInput,
    PasswordInput,
    Link,
    Image,
    Label,
    List,
    ListItem,
    Menu,
    MenuBar,
    MenuItem,
    ProgressIndicator,
    ScrollBar,
    Slider,
    SpinButton,
    Status,
    TabList,
    Tab,
    Toolbar,
    Tooltip,
    Tree,
    TreeItem,
    Group,
    Grid,
    Row,
    Cell,
    ColumnHeader,
    Table,
    Document,
    Window,
    Dialog,
    Pane,
    TitleBar,
    Splitter,
    /// A control this vocabulary has no name for; `native_role` says what
    /// the OS calls it.
    Unknown,
}

impl Role {
    /// The name in the wire form (`check_box`), for messages and outlines.
    pub fn name(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::CheckBox => "check_box",
            Self::RadioButton => "radio_button",
            Self::Switch => "switch",
            Self::ComboBox => "combo_box",
            Self::TextInput => "text_input",
            Self::PasswordInput => "password_input",
            Self::Link => "link",
            Self::Image => "image",
            Self::Label => "label",
            Self::List => "list",
            Self::ListItem => "list_item",
            Self::Menu => "menu",
            Self::MenuBar => "menu_bar",
            Self::MenuItem => "menu_item",
            Self::ProgressIndicator => "progress_indicator",
            Self::ScrollBar => "scroll_bar",
            Self::Slider => "slider",
            Self::SpinButton => "spin_button",
            Self::Status => "status",
            Self::TabList => "tab_list",
            Self::Tab => "tab",
            Self::Toolbar => "toolbar",
            Self::Tooltip => "tooltip",
            Self::Tree => "tree",
            Self::TreeItem => "tree_item",
            Self::Group => "group",
            Self::Grid => "grid",
            Self::Row => "row",
            Self::Cell => "cell",
            Self::ColumnHeader => "column_header",
            Self::Table => "table",
            Self::Document => "document",
            Self::Window => "window",
            Self::Dialog => "dialog",
            Self::Pane => "pane",
            Self::TitleBar => "title_bar",
            Self::Splitter => "splitter",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An action an element offers, named after the tool that performs it:
/// what `Node::actions` lists is what the agent can call on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionName {
    Invoke,
    Toggle,
    SetValue,
    Select,
    Focus,
    Expand,
    Collapse,
    ScrollIntoView,
}

impl ActionName {
    /// The tool's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Invoke => "invoke",
            Self::Toggle => "toggle",
            Self::SetValue => "set_value",
            Self::Select => "select",
            Self::Focus => "focus",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
            Self::ScrollIntoView => "scroll_into_view",
        }
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A checkable element's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    True,
    False,
    /// Partly checked (a tri-state box, a parent with some children on).
    Mixed,
}

impl Serialize for Checked {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::True => s.serialize_bool(true),
            Self::False => s.serialize_bool(false),
            Self::Mixed => s.serialize_str("mixed"),
        }
    }
}

impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Flag(true) => Ok(Self::True),
            Raw::Flag(false) => Ok(Self::False),
            Raw::Text(t) if t == "mixed" => Ok(Self::Mixed),
            Raw::Text(t) => Err(serde::de::Error::custom(format!(
                "`{t}` is not a checked state; use true, false or \"mixed\""
            ))),
        }
    }
}

impl JsonSchema for Checked {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Checked".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "true, false, or \"mixed\" for a partly checked element",
            "anyOf": [{ "type": "boolean" }, { "const": "mixed" }]
        })
    }
}

impl fmt::Display for Checked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::True => "checked",
            Self::False => "unchecked",
            Self::Mixed => "mixed",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip_through_their_wire_names() {
        for role in [
            Role::CheckBox,
            Role::TextInput,
            Role::Unknown,
            Role::TabList,
        ] {
            let wire = serde_json::Value::String(role.name().into());
            assert_eq!(serde_json::to_value(role).ok(), Some(wire.clone()));
            assert_eq!(serde_json::from_value::<Role>(wire).ok(), Some(role));
        }
        assert!(
            serde_json::from_value::<Role>(serde_json::json!("Edit")).is_err(),
            "a native name is not a role"
        );
    }

    #[test]
    fn checked_serializes_as_a_flag_or_mixed() {
        assert_eq!(
            serde_json::to_value(Checked::True).ok(),
            Some(serde_json::json!(true))
        );
        assert_eq!(
            serde_json::to_value(Checked::Mixed).ok(),
            Some(serde_json::json!("mixed"))
        );
        assert_eq!(
            serde_json::from_value::<Checked>(serde_json::json!("mixed")).ok(),
            Some(Checked::Mixed)
        );
        assert!(serde_json::from_value::<Checked>(serde_json::json!("on")).is_err());
    }
}
