//! Which states and properties WAI-ARIA 1.2 lets each role carry.
//!
//! A user agent must ignore a non-global state or property on a role that
//! does not support it (ARIA 1.2, "Handling Author Errors"); Chromium filters
//! them at serialization. FLUI does the same at publication: translation reads
//! [`retain_applicable`]'s view, which leaves out what the node's declared role
//! does not support, and the owner reports what was left out. A node with no
//! declared role is not checked: it names no ARIA role whose table could reject
//! anything.
//!
//! [`FIELDS`] is the one place an attribute is tied to the fields that hold it:
//! the view and the report both read it, and a [`RoleAttribute`] without a row
//! fails `every_role_attribute_has_one_field_row`.

use std::borrow::Cow;

use crate::attributes::SemanticsAttributes;
use crate::role::SemanticsRole;

/// A state or property whose support depends on the role.
///
/// The global ones (`hidden`, `disabled`, `invalid`, `live`, the naming and
/// relation properties) are not listed: every role carries them. A set's place
/// and size are not listed either: the framework derives them from the
/// collection a node sits in, whatever the role the node itself declares.
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
}

impl RoleAttribute {
    /// Every role-dependent attribute.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 9] = [
        Self::Toggled,
        Self::Selected,
        Self::Expanded,
        Self::ReadOnly,
        Self::Required,
        Self::Modal,
        Self::NumericRange,
        Self::Placeholder,
        Self::HeadingLevel,
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
            Self::HeadingLevel => "aria-level",
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
const FIELDS: [Field; 9] = [
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
        A::Expanded => matches!(
            role,
            R::Application
                | R::Button
                | R::CheckBox
                | R::Switch
                | R::ComboBox
                | R::GridCell
                | R::Link
                | R::ListBox
                | R::MenuItem
                | R::MenuItemCheckbox
                | R::MenuItemRadio
                | R::Row
                | R::RowHeader
                | R::ColumnHeader
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
                        | R::ColumnHeader
                        | R::RowHeader
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
                        | R::ColumnHeader
                        | R::RowHeader
                )
        }
        A::Modal => matches!(role, R::Window | R::Dialog | R::AlertDialog),
        A::NumericRange => matches!(
            role,
            R::Slider | R::SpinButton | R::ScrollBar | R::ProgressBar | R::Meter | R::Separator
        ),
        A::Placeholder => text_input,
        A::HeadingLevel => role == R::Heading,
    }
}

/// The fields `attrs` declares that its role does not support.
fn rejected(attrs: &SemanticsAttributes) -> impl Iterator<Item = &'static Field> + '_ {
    FIELDS
        .iter()
        .filter(|field| (field.declared)(attrs) && !applicable(attrs.role, field.attribute))
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
