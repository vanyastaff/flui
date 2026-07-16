//! [`ButtonStyle`] — the property bag the M3 button family resolves against.
//!
//! # Flutter parity
//!
//! `material/button_style.dart`'s `ButtonStyle` (oracle tag `3.44.0`): a
//! bag of nullable, per-state property slots. Every field is `null` by
//! default; a button's visible style comes from resolving each slot through
//! `crate::button_style_button`'s widget → theme → default cascade — see
//! that module's docs for how the cascade consumes this shape.
//!
//! # Slot shape: `Option<WidgetStateProperty<Option<V>>>`
//!
//! The double `Option` encodes two independent "unset" signals the oracle
//! collapses into one nullable field:
//!
//! - **Outer `Option`** — this property was never configured at all (the
//!   whole slot falls through to the next tier: widget → theme → default).
//! - **Inner `Option<V>` inside the [`WidgetStateProperty`]** — the property
//!   IS configured, but has nothing to say for the *current* states (that one
//!   resolution falls through, per [`WidgetStateProperty`]'s own
//!   `Option`-fallthrough contract — see `flui_widgets::widget_state`'s
//!   module docs, the substrate this button family was built against).
//!
//! Both signals fall through identically in
//! `crate::button_style_button`'s resolver, matching the oracle's
//! `getProperty(style)?.resolve(states) ?? …` chain (`button_style_button.dart`,
//! tag `3.44.0`) where a null *property* and a property that *resolves* to
//! null behave the same way.
//!
//! # Shape: an all-optional patch, not `#[non_exhaustive]`
//!
//! Every field here is already `Option`-wrapped — `ButtonStyle` plays the
//! role [`crate::ThemeDataOverrides`] plays for [`crate::ThemeData`]: a patch
//! callers build with a struct literal and `..Default::default()`
//! (`ButtonStyle { background_color: Some(…), ..Default::default() }`), not
//! a value with meaningful non-`None` defaults of its own. Per
//! [`crate::ColorSchemeOverrides`]/[`crate::ThemeDataOverrides`]'s own
//! precedent in this crate, a patch struct built exclusively through
//! `..Default::default()` deliberately stays OFF `#[non_exhaustive]`:
//! `#[non_exhaustive]` blocks external-crate struct-literal construction
//! even with a functional update, which would break the only construction
//! path this type has. Future V1+ slots are still additive for any caller
//! already writing `..Default::default()`, without the `#[non_exhaustive]`
//! ceremony — see those types' doc comments for the same reasoning spelled
//! out in full.
//!
//! # V1 slots vs. the oracle's full field list
//!
//! Ported: `text_style`, `background_color`, `foreground_color`,
//! `overlay_color`, `elevation`, `padding`, `minimum_size`, `fixed_size`,
//! `maximum_size`, `side`, `shape` — the eleven slots every
//! `_TokenDefaultsM3` table in `elevated_button.dart`/`filled_button.dart`/
//! `outlined_button.dart`/`text_button.dart` (oracle tag `3.44.0`) populates.
//!
//! Named omissions, not silently dropped:
//!
//! - **`mouse_cursor`** — FLUI has no `MouseCursor` type yet.
//! - **`icon_color` / `icon_size`** — arrive with a future `.icon()`
//!   constructor on each button type; nothing consumes them yet.
//! - **`animation_duration` / `enable_feedback` / `splash_factory`** — no
//!   implicit shape/elevation animation (`material.rs`'s own named
//!   deferral), no acoustic/haptic feedback substrate, and no ripple
//!   substrate (`InkWell`'s own named deferral) to select a splash factory
//!   for.
//! - **`visual_density` / `tap_target_size`** — FLUI has no `VisualDensity`
//!   type; every button below skips the `_InputPadding`/density-adjustment
//!   step the oracle's `_ButtonStyleState.build` performs.
//! - **`alignment`** — the oracle's `Align`-wrapped child slot; the V1
//!   composition in `crate::button_style_button` omits the `Align` layer
//!   entirely (see that module's docs).
//! - **`shadow_color` / `surface_tint_color`** — `Material`'s own
//!   `surfaceTintColor` is itself a named deferral (`material.rs`), and
//!   `Material` has no `shadow_color` field yet for a resolved
//!   `shadow_color` to feed.
//! - **`icon_alignment` / `background_builder` / `foreground_builder`** —
//!   all three presuppose the icon constructor and/or an extension point
//!   (`crate::button_style_button`'s composition is currently fixed, not
//!   builder-customizable).
//! - **[`ButtonStyle::lerp`]** — arrives when a component first needs
//!   `AnimatedTheme`; nothing here consumes an interpolated style yet.
//!
//! [`ButtonStyle::lerp`]: https://api.flutter.dev/flutter/material/ButtonStyle/lerp.html

use flui_types::styling::BorderSide;
use flui_types::typography::TextStyle;
use flui_types::{Color, EdgeInsets, Pixels, Size};
use flui_widgets::WidgetStateProperty;

use crate::shape::MaterialShape;

/// The visual properties most buttons have in common — Flutter's
/// `ButtonStyle`.
///
/// Every field is `None` by default (Flutter parity: "All of the ButtonStyle
/// properties are null by default", `button_style.dart` doc comment). Build
/// one with a struct literal and `..Default::default()`:
///
/// ```rust
/// use flui_material::ButtonStyle;
/// use flui_widgets::WidgetStateProperty;
/// use flui_types::Color;
///
/// let style = ButtonStyle {
///     background_color: Some(WidgetStateProperty::all(Some(Color::rgb(0, 255, 0)))),
///     ..Default::default()
/// };
/// assert!(style.foreground_color.is_none());
/// ```
///
/// See the module docs for the double-`Option` slot shape, why this type is
/// deliberately not `#[non_exhaustive]`, and the V1 slot list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ButtonStyle {
    /// The style for the button's text descendants. Flutter parity:
    /// `ButtonStyle.textStyle`.
    pub text_style: Option<WidgetStateProperty<Option<TextStyle>>>,

    /// The button's background fill color. Flutter parity:
    /// `ButtonStyle.backgroundColor`.
    pub background_color: Option<WidgetStateProperty<Option<Color>>>,

    /// The color for the button's text descendants — takes precedence over
    /// [`text_style`](Self::text_style)'s own color (see the oracle's own
    /// doc comment on `foregroundColor`). Flutter parity:
    /// `ButtonStyle.foregroundColor`.
    pub foreground_color: Option<WidgetStateProperty<Option<Color>>>,

    /// The state-overlay highlight color, resolved and handed to the
    /// button's `InkWell` as a live property (not a single baked value —
    /// see `crate::button_style_button`'s docs). Flutter parity:
    /// `ButtonStyle.overlayColor`.
    pub overlay_color: Option<WidgetStateProperty<Option<Color>>>,

    /// The elevation of the button's `Material`. Flutter parity:
    /// `ButtonStyle.elevation`.
    pub elevation: Option<WidgetStateProperty<Option<f32>>>,

    /// The padding between the button's boundary and its child. Flutter
    /// parity: `ButtonStyle.padding` (narrowed to `EdgeInsets`; the oracle's
    /// `EdgeInsetsGeometry` directional variant has no FLUI consumer yet).
    pub padding: Option<WidgetStateProperty<Option<EdgeInsets>>>,

    /// The minimum size of the button itself. Flutter parity:
    /// `ButtonStyle.minimumSize`.
    pub minimum_size: Option<WidgetStateProperty<Option<Size>>>,

    /// The button's fixed size, overriding [`minimum_size`](Self::minimum_size)/
    /// [`maximum_size`](Self::maximum_size) on whichever axis is finite.
    /// Flutter parity: `ButtonStyle.fixedSize`.
    pub fixed_size: Option<WidgetStateProperty<Option<Size>>>,

    /// The maximum size of the button itself. Flutter parity:
    /// `ButtonStyle.maximumSize`.
    pub maximum_size: Option<WidgetStateProperty<Option<Size>>>,

    /// The color and weight of the button's outline. Flutter parity:
    /// `ButtonStyle.side`.
    ///
    /// **Data-complete, not yet painted**: this slot resolves correctly
    /// (exercised by [`OutlinedButton`](crate::OutlinedButton)'s
    /// resolved-style tests), but [`MaterialShape`] is fill-and-clip-only —
    /// `Material.shape`'s border painting is a pre-existing named deferral
    /// (see `shape.rs`'s "Named deferral: `OutlinedBorder` sides"). An
    /// `OutlinedButton` in V1 resolves an outline color/width but does not
    /// yet draw a stroke.
    pub side: Option<WidgetStateProperty<Option<BorderSide<Pixels>>>>,

    /// The shape of the button's underlying `Material`. Flutter parity:
    /// `ButtonStyle.shape` (narrowed to [`MaterialShape`]; the oracle's open
    /// `OutlinedBorder` hierarchy is `Material`'s own named deferral).
    pub shape: Option<WidgetStateProperty<Option<MaterialShape>>>,
}

#[cfg(test)]
mod tests {
    use flui_widgets::{WidgetState, WidgetStates};

    use super::*;

    #[test]
    fn default_is_every_slot_unset() {
        let style = ButtonStyle::default();
        assert!(style.text_style.is_none());
        assert!(style.background_color.is_none());
        assert!(style.foreground_color.is_none());
        assert!(style.overlay_color.is_none());
        assert!(style.elevation.is_none());
        assert!(style.padding.is_none());
        assert!(style.minimum_size.is_none());
        assert!(style.fixed_size.is_none());
        assert!(style.maximum_size.is_none());
        assert!(style.side.is_none());
        assert!(style.shape.is_none());
    }

    /// The struct-literal + `..Default::default()` construction path this
    /// type is built around — see the module docs on why this shape (not
    /// `#[non_exhaustive]`) was chosen.
    #[test]
    fn struct_literal_with_default_update_sets_only_the_given_fields() {
        let style = ButtonStyle {
            elevation: Some(WidgetStateProperty::all(Some(4.0))),
            ..Default::default()
        };
        assert_eq!(
            style.elevation.unwrap().resolve(&WidgetStates::NONE),
            Some(4.0)
        );
        assert!(style.background_color.is_none());
    }

    #[test]
    fn equality_is_structural_across_two_equivalently_built_styles() {
        let a = ButtonStyle {
            background_color: Some(WidgetStateProperty::all(Some(Color::rgb(1, 2, 3)))),
            ..Default::default()
        };
        let b = ButtonStyle {
            background_color: Some(WidgetStateProperty::all(Some(Color::rgb(1, 2, 3)))),
            ..Default::default()
        };
        assert_eq!(a, b);
    }

    #[test]
    fn a_configured_property_that_resolves_none_for_a_state_is_still_none() {
        // The inner-Option fallthrough half of the double-Option contract
        // (see the module docs): a `Map` with no matching entry resolves to
        // `None`, distinct from the slot being unset outright.
        let style = ButtonStyle {
            overlay_color: Some(WidgetStateProperty::from_map([(
                WidgetState::Pressed.into(),
                Some(Color::rgb(0, 0, 0)),
            )])),
            ..Default::default()
        };
        assert_eq!(
            style.overlay_color.unwrap().resolve(&WidgetStates::NONE),
            None
        );
    }
}
