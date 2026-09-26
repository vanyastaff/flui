//! The agent-protocol wire vocabulary of ADR-0080: element roles in
//! AccessKit's names, actions named after the tools that perform them, and a
//! checkable element's state.
//!
//! The same names are spoken whatever the OS underneath: AccessKit's role set
//! is the one already mapped to UI Automation, AppKit and AT-SPI in one
//! codebase. With the `serde` feature each type serializes to exactly the
//! spelling ADR-0080 fixed (`check_box`, `set_value`, `true`/`"mixed"`); with
//! `schemars` it describes that spelling as a JSON schema.

use std::fmt;

vocabulary! {
    /// What kind of control an element is, in AccessKit's names written in
    /// snake case.
    ///
    /// A reply reports the OS's own name for the control beside it, so a role
    /// this list does not cover is still visible as what the OS calls it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Role {
        /// A push button.
        Button => "button",
        /// A two- or three-state check box.
        CheckBox => "check_box",
        /// One option of a mutually exclusive set.
        RadioButton => "radio_button",
        /// An on/off switch.
        Switch => "switch",
        /// A drop-down list, possibly with an editable field.
        ComboBox => "combo_box",
        /// A single-line editable text field.
        TextInput => "text_input",
        /// A text field whose characters are obscured.
        PasswordInput => "password_input",
        /// A hyperlink.
        Link => "link",
        /// A picture.
        Image => "image",
        /// Static text.
        Label => "label",
        /// A list of items.
        List => "list",
        /// An item in a list.
        ListItem => "list_item",
        /// A menu.
        Menu => "menu",
        /// A menu bar.
        MenuBar => "menu_bar",
        /// An item in a menu.
        MenuItem => "menu_item",
        /// A progress bar or busy indicator.
        ProgressIndicator => "progress_indicator",
        /// A scroll bar.
        ScrollBar => "scroll_bar",
        /// A slider over a range of values.
        Slider => "slider",
        /// A numeric field with increment and decrement.
        SpinButton => "spin_button",
        /// A status bar or status message.
        Status => "status",
        /// The strip of tabs of a tab control.
        TabList => "tab_list",
        /// One tab.
        Tab => "tab",
        /// A toolbar.
        Toolbar => "toolbar",
        /// A tooltip.
        Tooltip => "tooltip",
        /// A tree view.
        Tree => "tree",
        /// An item in a tree view.
        TreeItem => "tree_item",
        /// A group of related controls.
        Group => "group",
        /// A grid of cells.
        Grid => "grid",
        /// A row of a table or grid.
        Row => "row",
        /// A cell of a table.
        Cell => "cell",
        /// A cell of an interactive grid.
        GridCell => "grid_cell",
        /// A column header.
        ColumnHeader => "column_header",
        /// A row header.
        RowHeader => "row_header",
        /// A table.
        Table => "table",
        /// A document.
        Document => "document",
        /// A top-level window.
        Window => "window",
        /// A dialog.
        Dialog => "dialog",
        /// A pane of a window.
        Pane => "pane",
        /// A window's title bar.
        TitleBar => "title_bar",
        /// A splitter between panes.
        Splitter => "splitter",
        /// A control this vocabulary has no name for; the reply's native role
        /// says what the OS calls it.
        Unknown => "unknown",
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

vocabulary! {
    /// An action an element offers, named after the tool that performs it:
    /// what a node lists as its actions is what the agent can call on it.
    ///
    /// ADR-0080 makes new tools additive, so this enum is `#[non_exhaustive]`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ActionName {
        /// Activate the element, as a click would.
        Invoke => "invoke",
        /// Flip a checkable element's state.
        Toggle => "toggle",
        /// Replace the element's value.
        SetValue => "set_value",
        /// Select the element within its container.
        Select => "select",
        /// Move keyboard focus to the element.
        Focus => "focus",
        /// Expand a collapsed element.
        Expand => "expand",
        /// Collapse an expanded element.
        Collapse => "collapse",
        /// Scroll the element's container until the element is visible.
        ScrollIntoView => "scroll_into_view",
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A checkable element's state.
///
/// On the wire, `true` and `false` are JSON booleans and a partly checked
/// element is the string `"mixed"`. The three states are complete, so this
/// enum is exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Checked {
    /// Checked.
    True,
    /// Not checked.
    False,
    /// Partly checked (a tri-state box, a parent with some children on).
    Mixed,
}

#[cfg(feature = "serde")]
impl serde::Serialize for Checked {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::True => s.serialize_bool(true),
            Self::False => s.serialize_bool(false),
            Self::Mixed => s.serialize_str("mixed"),
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Checked {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
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

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Checked {
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

    /// ADR-0080's "Vocabulary" list, in its order. A tool renamed, dropped or
    /// added without the ADR changing fails here.
    #[test]
    fn the_advertised_action_names_are_adr_0080s() {
        let names: Vec<&str> = ActionName::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(
            names,
            [
                "invoke",
                "toggle",
                "set_value",
                "select",
                "focus",
                "expand",
                "collapse",
                "scroll_into_view",
            ],
        );
    }

    #[test]
    fn display_is_the_wire_name() {
        for role in Role::ALL {
            assert_eq!(role.to_string(), role.name());
        }
        for action in ActionName::ALL {
            assert_eq!(action.to_string(), action.name());
        }
        assert_eq!(Checked::Mixed.to_string(), "mixed");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn every_wire_role_serializes_to_its_name() {
        for &role in Role::ALL {
            let wire = serde_json::Value::String(role.name().into());
            assert_eq!(
                serde_json::to_value(role).ok(),
                Some(wire.clone()),
                "{role:?}"
            );
            assert_eq!(serde_json::from_value::<Role>(wire).ok(), Some(role));
        }
        assert!(
            serde_json::from_value::<Role>(serde_json::json!("Edit")).is_err(),
            "a native name is not a role"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn every_action_name_serializes_to_its_tool_name() {
        for &action in ActionName::ALL {
            let wire = serde_json::Value::String(action.name().into());
            assert_eq!(
                serde_json::to_value(action).ok(),
                Some(wire.clone()),
                "{action:?}"
            );
            assert_eq!(
                serde_json::from_value::<ActionName>(wire).ok(),
                Some(action)
            );
        }
        assert!(serde_json::from_value::<ActionName>(serde_json::json!("SetValue")).is_err());
    }

    #[cfg(feature = "serde")]
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

    /// Every string a schema admits, whether it spells them as an `enum` list
    /// or as one `const` per documented variant.
    #[cfg(feature = "schemars")]
    fn admitted_strings(schema: &serde_json::Value, out: &mut Vec<String>) {
        match schema {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    match (key.as_str(), value) {
                        ("enum", serde_json::Value::Array(items)) => out.extend(
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_owned)),
                        ),
                        ("const", serde_json::Value::String(text)) => out.push(text.clone()),
                        _ => admitted_strings(value, out),
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    admitted_strings(item, out);
                }
            }
            _ => {}
        }
    }

    /// The schema a client validates replies against admits exactly the
    /// vocabulary, so a role the server can send is never one the client's
    /// validator rejects.
    #[cfg(feature = "schemars")]
    #[test]
    fn the_role_schema_lists_every_wire_name() {
        let schema =
            serde_json::to_value(schemars::schema_for!(Role)).expect("a schema serializes to JSON");
        let mut admitted = Vec::new();
        admitted_strings(&schema, &mut admitted);
        let expected: Vec<String> = Role::ALL.iter().map(|r| r.name().to_owned()).collect();
        assert_eq!(admitted, expected, "schema was {schema:#}");

        let schema = serde_json::to_value(schemars::schema_for!(ActionName))
            .expect("a schema serializes to JSON");
        let mut admitted = Vec::new();
        admitted_strings(&schema, &mut admitted);
        let expected: Vec<String> = ActionName::ALL
            .iter()
            .map(|a| a.name().to_owned())
            .collect();
        assert_eq!(admitted, expected, "schema was {schema:#}");
    }
}
