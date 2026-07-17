//! Shared per-state `Color` resolution helper for the M3 selection-controls
//! family ([`crate::Checkbox`], [`crate::Switch`], [`crate::Radio`]).
//!
//! Each control's theme slot ([`crate::CheckboxThemeData`],
//! [`crate::SwitchThemeData`], [`crate::RadioThemeData`]) carries the same
//! `Option<WidgetStateProperty<Option<Color>>>` shape for its color fields —
//! "no override" at the theme tier vs. "override present but resolves to no
//! color for this particular state set" both need to collapse to one `None`
//! so a `widget ?? theme ?? default` cascade can `.or_else(...)` through
//! them uniformly. Previously each control carried its own private copy of
//! this three-line function; hoisted here once all three needed the
//! identical shape.

use flui_types::styling::Color;
use flui_widgets::{WidgetStateProperty, WidgetStates};

/// Resolves `property` against `states`, flattening the "no property" and
/// "property present but resolves to `None`" cases into one `None` —
/// exactly the fall-through-to-next-tier shape every color cascade in the
/// selection-controls family wants (`widget?.resolve(states) ??
/// theme?.resolve(states) ?? default`).
pub(crate) fn resolve_state_color(
    property: Option<&WidgetStateProperty<Option<Color>>>,
    states: &WidgetStates,
) -> Option<Color> {
    property.and_then(|p| p.resolve(states))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_widgets::WidgetState;

    #[test]
    fn resolve_state_color_is_none_with_no_property() {
        assert_eq!(resolve_state_color(None, &WidgetStates::NONE), None);
    }

    #[test]
    fn resolve_state_color_is_none_when_the_property_has_no_matching_entry() {
        // Distinguishes "no property at all" from "a property that itself
        // resolves to `None` for this state set" — both must collapse to
        // one `None`, not just the trivially-`None` former case.
        let property: WidgetStateProperty<Option<Color>> = WidgetStateProperty::from_map([(
            flui_widgets::WidgetStateConstraint::Is(WidgetState::Selected),
            Some(Color::rgb(1, 2, 3)),
        )]);
        assert_eq!(
            resolve_state_color(Some(&property), &WidgetStates::NONE),
            None
        );
    }

    #[test]
    fn resolve_state_color_resolves_a_present_property() {
        let property = WidgetStateProperty::all(Some(Color::rgb(1, 2, 3)));
        assert_eq!(
            resolve_state_color(Some(&property), &WidgetStates::NONE),
            Some(Color::rgb(1, 2, 3))
        );
    }
}
