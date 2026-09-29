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

use flui_sdk::painting::Color;
use flui_sdk::widgets::{WidgetStateProperty, WidgetStates};

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
