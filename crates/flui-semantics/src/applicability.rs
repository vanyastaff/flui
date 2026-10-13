//! Which states and properties WAI-ARIA 1.2 lets each role carry (R3).
//!
//! A user agent must ignore a non-global state or property on a role that
//! does not support it (ARIA 1.2, "Handling Author Errors"); Chromium filters
//! them at serialization. FLUI does the same at publication: an attribute
//! the node's declared role does not support is not published, and the owner
//! reports it once per changed node. A node with no declared role is not
//! checked: it names no ARIA role whose table could reject anything.

use crate::attributes::SemanticsAttributes;
use crate::role::SemanticsRole;

/// A state or property whose support depends on the role.
///
/// The global ones (`hidden`, `disabled`, `invalid`, `live`, the naming and
/// relation properties) are not listed: every role carries them.
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
    /// `aria-posinset` and `aria-setsize`.
    PlaceInSet,
}

impl RoleAttribute {
    /// The ARIA name the attribute is reported under.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Toggled => "aria-checked/aria-pressed",
            Self::Selected => "aria-selected",
            Self::Expanded => "aria-expanded",
            Self::ReadOnly => "aria-readonly",
            Self::Required => "aria-required",
            Self::Modal => "aria-modal",
            Self::NumericRange => "aria-valuenow",
            Self::Placeholder => "aria-placeholder",
            Self::PlaceInSet => "aria-posinset/aria-setsize",
        }
    }
}

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
                        | R::MenuItemCheckbox
                        | R::MenuItemRadio
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
        A::Modal => matches!(role, R::Dialog | R::AlertDialog),
        A::NumericRange => matches!(
            role,
            R::Slider | R::SpinButton | R::ScrollBar | R::ProgressBar | R::Meter | R::Separator
        ),
        A::Placeholder => text_input || role == R::ComboBox,
        A::PlaceInSet => matches!(
            role,
            R::Article
                | R::ListItem
                | R::ListBoxOption
                | R::MenuItem
                | R::MenuItemCheckbox
                | R::MenuItemRadio
                | R::RadioButton
                | R::Row
                | R::Tab
                | R::TreeItem
        ),
    }
}

/// The role-dependent attributes `attrs` declares that its role does not
/// support, in a fixed order.
pub(crate) fn inapplicable(
    attrs: &SemanticsAttributes,
) -> impl Iterator<Item = RoleAttribute> + '_ {
    let states = &attrs.states;
    [
        (RoleAttribute::Toggled, states.toggled.is_some()),
        (RoleAttribute::Selected, states.selected.is_some()),
        (RoleAttribute::Expanded, states.expanded.is_some()),
        (RoleAttribute::ReadOnly, states.read_only.is_some()),
        (RoleAttribute::Required, states.required.is_some()),
        (RoleAttribute::Modal, states.modal.is_some()),
        (RoleAttribute::NumericRange, attrs.numeric_range.is_some()),
        (RoleAttribute::Placeholder, attrs.placeholder.is_some()),
        (
            RoleAttribute::PlaceInSet,
            attrs.index_in_parent.is_some() || attrs.scroll_child_count.is_some(),
        ),
    ]
    .into_iter()
    .filter(|&(attribute, declared)| declared && !applicable(attrs.role, attribute))
    .map(|(attribute, _)| attribute)
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
            (R::ComboBox, A::Placeholder, true),
            (R::Button, A::Placeholder, false),
            (R::ListItem, A::PlaceInSet, true),
            (R::Tab, A::PlaceInSet, true),
            (R::Button, A::PlaceInSet, false),
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
}
