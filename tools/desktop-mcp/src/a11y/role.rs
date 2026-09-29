//! What the wire vocabulary cannot know on its own: how a native accessibility
//! API's own names map onto it. The vocabulary itself (`Role`, `ActionName`,
//! `Checked`) is flui-protocol's.

use flui_protocol::Role;

/// Distinctions erased by UIA control types in AccessKit's Windows adapter
/// (accesskit_windows 0.35.0, node.rs::aria_role). Other ARIA values retain
/// the native mapping: for example `group` also describes title bars, and
/// must not erase that more specific native role.
pub(crate) fn role_from_aria(value: &str) -> Option<Role> {
    match value.trim() {
        "cell" => Some(Role::Cell),
        "gridcell" => Some(Role::GridCell),
        "row" => Some(Role::Row),
        "rowheader" => Some(Role::RowHeader),
        "columnheader" => Some(Role::ColumnHeader),
        "switch" => Some(Role::Switch),
        _ => None,
    }
}
