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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aria_preserves_roles_that_share_native_control_types() {
        for (aria, role) in [
            ("cell", Role::Cell),
            ("gridcell", Role::GridCell),
            ("row", Role::Row),
            ("rowheader", Role::RowHeader),
            ("columnheader", Role::ColumnHeader),
            ("switch", Role::Switch),
        ] {
            assert_eq!(role_from_aria(aria), Some(role));
            let wire = serde_json::Value::String(role.name().into());
            assert_eq!(serde_json::to_value(role).ok(), Some(wire.clone()));
            assert_eq!(serde_json::from_value::<Role>(wire).ok(), Some(role));
        }
        for aria in ["", "group", "region", "vendor-specific"] {
            assert_eq!(role_from_aria(aria), None);
        }
    }
}
