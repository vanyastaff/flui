//! Which states and properties WAI-ARIA 1.2 lets each role carry.
//!
//! A user agent must ignore a non-global state or property on a role that
//! does not support it ([ARIA 1.2, "Handling Author Errors"]). FLUI does so at
//! publication: translation reads
//! [`retain_applicable`]'s view, which leaves out what the node's declared role
//! does not support, and the owner reports what was left out. A node with no
//! declared role is not checked: it names no ARIA role whose table could reject
//! anything.
//!
//! [`FIELDS`] is the one place an attribute is tied to the fields that hold it:
//! the view and the report both read it, and a [`RoleAttribute`] without a row
//! fails `every_role_attribute_has_one_field_row`.
//!
//! [ARIA 1.2, "Handling Author Errors"]: https://www.w3.org/TR/wai-aria-1.2/#document-handling_author-errors_states-properties

use std::borrow::Cow;

use crate::attributes::SemanticsAttributes;
use crate::role::SemanticsRole;

/// A state or property whose support depends on the role.
///
/// The global ones (`hidden`, `disabled`, `invalid`, `live`, the naming and
/// relation properties) are not listed: every role carries them. A set's place
/// and size are not listed either: the framework derives them from the
/// collection a node sits in, whatever the role the node itself declares. The
/// rest of a node's place (its level, a table's shape, a cell's position, a
/// header's sort, a container's selection rule) is an author's to declare, so
/// the role it is declared on is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RoleAttribute {
    /// Checked or pressed (`aria-checked`, `aria-pressed`).
    Toggled,
    /// `aria-selected`.
    Selected,
    /// `aria-expanded`.
    Expanded,
    /// `aria-readonly`.
    ReadOnly,
    /// `aria-required`.
    Required,
    /// `aria-modal`.
    Modal,
    /// `aria-valuenow`, `aria-valuemin`, `aria-valuemax`.
    NumericRange,
    /// `aria-placeholder`.
    Placeholder,
    /// `aria-level` of a heading.
    HeadingLevel,
    /// `aria-level` of a tree item, a list item or a row.
    Level,
    /// `aria-rowcount` and `aria-colcount`.
    TableShape,
    /// `aria-rowindex`, `aria-colindex`, `aria-rowspan` and `aria-colspan`.
    CellPosition,
    /// `aria-sort`.
    Sort,
    /// `aria-multiselectable`.
    Multiselectable,
}

impl RoleAttribute {
    /// Every role-dependent attribute.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 14] = [
        Self::Toggled,
        Self::Selected,
        Self::Expanded,
        Self::ReadOnly,
        Self::Required,
        Self::Modal,
        Self::NumericRange,
        Self::Placeholder,
        Self::HeadingLevel,
        Self::Level,
        Self::TableShape,
        Self::CellPosition,
        Self::Sort,
        Self::Multiselectable,
    ];

    /// The ARIA attributes it stands for.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Toggled => "aria-checked/aria-pressed",
            Self::Selected => "aria-selected",
            Self::Expanded => "aria-expanded",
            Self::ReadOnly => "aria-readonly",
            Self::Required => "aria-required",
            Self::Modal => "aria-modal",
            Self::NumericRange => "aria-valuenow/aria-valuemin/aria-valuemax",
            Self::Placeholder => "aria-placeholder",
            Self::HeadingLevel | Self::Level => "aria-level",
            Self::TableShape => "aria-rowcount/aria-colcount",
            Self::CellPosition => "aria-rowindex/aria-colindex/aria-rowspan/aria-colspan",
            Self::Sort => "aria-sort",
            Self::Multiselectable => "aria-multiselectable",
        }
    }
}

/// One role-dependent attribute: whether a node declares it, and how to clear
/// it from a node's view.
struct Field {
    attribute: RoleAttribute,
    declared: fn(&SemanticsAttributes) -> bool,
    clear: fn(&mut SemanticsAttributes),
}

/// The fields behind each [`RoleAttribute`], one row each.
const FIELDS: [Field; 14] = [
    Field {
        attribute: RoleAttribute::Toggled,
        declared: |a| a.states.toggled.is_some(),
        clear: |a| a.states.toggled = None,
    },
    Field {
        attribute: RoleAttribute::Selected,
        declared: |a| a.states.selected.is_some(),
        clear: |a| a.states.selected = None,
    },
    Field {
        attribute: RoleAttribute::Expanded,
        declared: |a| a.states.expanded.is_some(),
        clear: |a| a.states.expanded = None,
    },
    Field {
        attribute: RoleAttribute::ReadOnly,
        declared: |a| a.states.read_only.is_some(),
        clear: |a| a.states.read_only = None,
    },
    Field {
        attribute: RoleAttribute::Required,
        declared: |a| a.states.required.is_some(),
        clear: |a| a.states.required = None,
    },
    Field {
        attribute: RoleAttribute::Modal,
        declared: |a| a.states.modal.is_some(),
        clear: |a| a.states.modal = None,
    },
    Field {
        attribute: RoleAttribute::NumericRange,
        declared: |a| a.numeric_range.is_some(),
        clear: |a| a.numeric_range = None,
    },
    Field {
        attribute: RoleAttribute::Placeholder,
        declared: |a| a.placeholder.is_some(),
        clear: |a| a.placeholder = None,
    },
    Field {
        attribute: RoleAttribute::HeadingLevel,
        declared: |a| a.heading_level.is_some(),
        clear: |a| a.heading_level = None,
    },
    Field {
        attribute: RoleAttribute::Level,
        declared: |a| a.collection.level.is_some(),
        clear: |a| a.collection.level = None,
    },
    Field {
        attribute: RoleAttribute::TableShape,
        declared: |a| a.collection.table.is_some(),
        clear: |a| a.collection.table = None,
    },
    Field {
        attribute: RoleAttribute::CellPosition,
        declared: |a| a.collection.cell.is_some(),
        clear: |a| a.collection.cell = None,
    },
    Field {
        attribute: RoleAttribute::Sort,
        declared: |a| a.collection.sort.is_some(),
        clear: |a| a.collection.sort = None,
    },
    Field {
        attribute: RoleAttribute::Multiselectable,
        declared: |a| a.collection.multiselectable.is_some(),
        clear: |a| a.collection.multiselectable = None,
    },
];

/// Whether `role` supports `attribute` in WAI-ARIA 1.2, with the roles FLUI
/// adds for text input kinds treated as `textbox`. A node with no declared
/// role supports everything: there is no table to check it against.
#[must_use]
pub(crate) fn applicable(role: SemanticsRole, attribute: RoleAttribute) -> bool {
    use RoleAttribute as A;
    use SemanticsRole as R;

    if role == R::None {
        return true;
    }
    let text_input = role.is_text_input();
    match attribute {
        A::Toggled => matches!(
            role,
            R::CheckBox
                | R::RadioButton
                | R::Switch
                | R::MenuItemCheckbox
                | R::MenuItemRadio
                | R::Button
                | R::ListBoxOption
                | R::TreeItem
        ),
        A::Selected => matches!(
            role,
            R::Tab
                | R::ListBoxOption
                | R::Row
                | R::GridCell
                | R::ColumnHeader
                | R::RowHeader
                | R::TreeItem
        ),
        // A row's, a header's expanded, read-only and required states, and a
        // grid cell's expanded state, depend on the table they sit in (a tree
        // grid, a grid), which this role-only table cannot see: they are left
        // out rather than published where ARIA forbids them.
        A::Expanded => matches!(
            role,
            R::Application
                | R::Button
                | R::CheckBox
                | R::Switch
                | R::ComboBox
                | R::Link
                | R::ListBox
                | R::MenuItem
                | R::MenuItemCheckbox
                | R::MenuItemRadio
                | R::Tab
                | R::TreeItem
        ),
        A::ReadOnly => {
            text_input
                || matches!(
                    role,
                    R::CheckBox
                        | R::Switch
                        | R::ComboBox
                        | R::Grid
                        | R::GridCell
                        | R::ListBox
                        | R::RadioGroup
                        | R::Slider
                        | R::SpinButton
                        | R::TreeGrid
                )
        }
        A::Required => {
            text_input
                || matches!(
                    role,
                    R::CheckBox
                        | R::Switch
                        | R::ComboBox
                        | R::GridCell
                        | R::ListBox
                        | R::RadioGroup
                        | R::SpinButton
                        | R::Tree
                        | R::TreeGrid
                )
        }
        A::Modal => matches!(role, R::Window | R::Dialog | R::AlertDialog),
        A::NumericRange => matches!(
            role,
            R::Slider | R::SpinButton | R::ScrollBar | R::ProgressBar | R::Meter | R::Separator
        ),
        A::Placeholder => text_input,
        A::HeadingLevel => role == R::Heading,
        A::Level => matches!(role, R::ListItem | R::Row | R::TreeItem),
        A::TableShape => matches!(role, R::Table | R::Grid | R::TreeGrid),
        A::CellPosition => matches!(
            role,
            R::Cell | R::ColumnHeader | R::GridCell | R::Row | R::RowHeader
        ),
        A::Sort => matches!(role, R::ColumnHeader | R::RowHeader),
        A::Multiselectable => matches!(
            role,
            R::Grid | R::ListBox | R::TabList | R::Tree | R::TreeGrid
        ),
    }
}

/// Whether WAI-ARIA 1.2 forbids `role` a name ("Name From: prohibited"):
/// the text-level roles whose content speaks for itself.
#[must_use]
pub(crate) fn prohibits_name(role: SemanticsRole) -> bool {
    use SemanticsRole as R;
    matches!(
        role,
        R::Caption | R::Code | R::Deletion | R::Emphasis | R::Insertion | R::Paragraph | R::Strong
    )
}

/// The fields `attrs` declares that its role does not support.
fn rejected(attrs: &SemanticsAttributes) -> impl Iterator<Item = &'static Field> + '_ {
    FIELDS
        .iter()
        .filter(|field| (field.declared)(attrs) && !supported(attrs, field.attribute))
}

/// Whether `attrs`'s role supports `attribute` with the value it declares:
/// [`applicable`], and two cases that depend on the node as well as its role.
/// A toggle's mixed state is not allowed on a radio button, a switch or a
/// radio menu item. A separator carries a range only when focusable: ARIA
/// defines the value properties for an interactive separator (a splitter)
/// and none for a static one.
fn supported(attrs: &SemanticsAttributes, attribute: RoleAttribute) -> bool {
    use SemanticsRole as R;
    applicable(attrs.role, attribute)
        && match attribute {
            RoleAttribute::Toggled => {
                attrs.states.toggled != Some(crate::Toggled::Mixed)
                    || !matches!(attrs.role, R::RadioButton | R::Switch | R::MenuItemRadio)
            }
            RoleAttribute::NumericRange => {
                attrs.role != R::Separator || attrs.states.focusable == Some(true)
            }
            _ => true,
        }
}

/// The role-dependent attributes `attrs` declares that its role does not
/// support, in [`FIELDS`] order.
pub(crate) fn inapplicable(
    attrs: &SemanticsAttributes,
) -> impl Iterator<Item = RoleAttribute> + '_ {
    rejected(attrs).map(|field| field.attribute)
}

/// `attrs` as publication reads it: borrowed when the role supports
/// everything it declares, else a copy without what the role does not.
pub(crate) fn retain_applicable(attrs: &SemanticsAttributes) -> Cow<'_, SemanticsAttributes> {
    let mut rejected = rejected(attrs).peekable();
    if rejected.peek().is_none() {
        return Cow::Borrowed(attrs);
    }
    let mut kept = attrs.clone();
    for field in rejected {
        (field.clear)(&mut kept);
    }
    Cow::Owned(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows from WAI-ARIA 1.2's "Supported States and Properties" per role,
    /// with FLUI's text-input kinds read as `textbox` and a node with no
    /// declared role left unchecked.
    #[test]
    fn applicability_follows_aria() {
        use RoleAttribute as A;
        use SemanticsRole as R;
        let rows: &[(R, A, bool)] = &[
            (R::RadioButton, A::Toggled, true),
            (R::Button, A::Toggled, true),
            (R::Link, A::Toggled, false),
            (R::Tab, A::Selected, true),
            (R::ListBoxOption, A::Selected, true),
            (R::RadioButton, A::Selected, false),
            (R::Button, A::Selected, false),
            (R::Button, A::Expanded, true),
            (R::Switch, A::Expanded, true),
            (R::Paragraph, A::Expanded, false),
            (R::TextInput, A::ReadOnly, true),
            (R::PasswordInput, A::ReadOnly, true),
            (R::Button, A::ReadOnly, false),
            (R::CheckBox, A::Required, true),
            (R::Slider, A::Required, false),
            (R::Dialog, A::Modal, true),
            (R::Button, A::Modal, false),
            (R::Slider, A::NumericRange, true),
            (R::ProgressBar, A::NumericRange, true),
            (R::Button, A::NumericRange, false),
            (R::SearchInput, A::Placeholder, true),
            (R::ComboBox, A::Placeholder, false),
            (R::Button, A::Placeholder, false),
            (R::Window, A::Modal, true),
            (R::MenuItemCheckbox, A::ReadOnly, false),
            (R::MenuItemRadio, A::ReadOnly, false),
            (R::Heading, A::HeadingLevel, true),
            (R::Table, A::HeadingLevel, false),
            (R::None, A::Modal, true),
            (R::None, A::Selected, true),
            (R::Row, A::Expanded, false),
            (R::GridCell, A::Expanded, false),
            (R::GridCell, A::ReadOnly, true),
            (R::ColumnHeader, A::ReadOnly, false),
            (R::TreeItem, A::Level, true),
            (R::ListItem, A::Level, true),
            (R::Row, A::Level, true),
            (R::Heading, A::Level, false),
            (R::Button, A::Level, false),
            (R::Table, A::TableShape, true),
            (R::Grid, A::TableShape, true),
            (R::TreeGrid, A::TableShape, true),
            (R::List, A::TableShape, false),
            (R::GridCell, A::CellPosition, true),
            (R::Row, A::CellPosition, true),
            (R::RowHeader, A::CellPosition, true),
            (R::ListItem, A::CellPosition, false),
            (R::ColumnHeader, A::Sort, true),
            (R::RowHeader, A::Sort, true),
            (R::Cell, A::Sort, false),
            (R::ListBox, A::Multiselectable, true),
            (R::Tree, A::Multiselectable, true),
            (R::TabList, A::Multiselectable, true),
            (R::Button, A::Multiselectable, false),
            (R::List, A::Multiselectable, false),
        ];
        for &(role, attribute, expected) in rows {
            assert_eq!(
                applicable(role, attribute),
                expected,
                "{role} with {}",
                attribute.aria_name()
            );
        }
    }

    /// Every attribute is tied to its fields exactly once, so the view and the
    /// report cannot disagree about what an attribute covers.
    #[test]
    fn every_role_attribute_has_one_field_row() {
        for attribute in RoleAttribute::ALL {
            let rows = FIELDS
                .iter()
                .filter(|field| field.attribute == attribute)
                .count();
            assert_eq!(rows, 1, "{} has {rows} field rows", attribute.aria_name());
        }
        assert_eq!(FIELDS.len(), RoleAttribute::ALL.len());
    }

    /// A toggle's mixed state is rejected where ARIA forbids it, and kept
    /// where it is allowed.
    #[test]
    fn a_mixed_toggle_is_supported_only_where_aria_allows_it() {
        use SemanticsRole as R;
        for (role, kept) in [
            (R::CheckBox, true),
            (R::MenuItemCheckbox, true),
            (R::Button, true),
            (R::RadioButton, false),
            (R::Switch, false),
            (R::MenuItemRadio, false),
        ] {
            let mut attrs = SemanticsAttributes {
                role,
                ..SemanticsAttributes::default()
            };
            attrs.states.toggled = Some(crate::Toggled::Mixed);
            assert_eq!(inapplicable(&attrs).next().is_none(), kept, "{role}");
            attrs.states.toggled = Some(crate::Toggled::On);
            assert!(inapplicable(&attrs).next().is_none(), "{role} takes on/off");
        }
    }

    /// A separator carries a range only when it is focusable, a splitter; a
    /// static one does not.
    #[test]
    fn a_range_is_supported_on_a_separator_only_when_focusable() {
        for (focusable, kept) in [(Some(true), true), (Some(false), false), (None, false)] {
            let mut attrs = SemanticsAttributes {
                role: SemanticsRole::Separator,
                numeric_range: Some(
                    crate::NumericRange::new(4.0, 0.0, 10.0, 2.0).expect("finite fixture"),
                ),
                ..SemanticsAttributes::default()
            };
            attrs.states.focusable = focusable;
            assert_eq!(
                inapplicable(&attrs).next().is_none(),
                kept,
                "focusable: {focusable:?}"
            );
        }
    }

    /// The view drops exactly what the report names, and borrows when there
    /// is nothing to drop.
    #[test]
    fn the_view_drops_what_the_report_names() {
        let mut attrs = SemanticsAttributes {
            role: SemanticsRole::Button,
            placeholder: Some("Search".into()),
            ..SemanticsAttributes::default()
        };
        attrs.states.selected = Some(true);
        attrs.states.expanded = Some(true);
        assert_eq!(
            inapplicable(&attrs).collect::<Vec<_>>(),
            [RoleAttribute::Selected, RoleAttribute::Placeholder]
        );
        let view = retain_applicable(&attrs);
        assert!(matches!(view, Cow::Owned(_)));
        assert_eq!(view.states.selected, None);
        assert_eq!(view.placeholder, None);
        assert_eq!(view.states.expanded, Some(true), "a supported state stays");

        attrs.states.selected = None;
        attrs.placeholder = None;
        assert!(matches!(retain_applicable(&attrs), Cow::Borrowed(_)));
    }
}
