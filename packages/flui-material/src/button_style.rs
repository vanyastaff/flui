//! [`ButtonStyle`] — the property bag the M3 button family resolves against.
//!
//! A bag of optional, per-state property slots. Every field is `None` by
//! default; a button's visible style comes from resolving each slot through
//! `crate::button_style_button`'s widget → theme → default cascade — see
//! that module's docs for how the cascade consumes this shape.
//!
//! # Slot shape: `Option<WidgetStateProperty<Option<V>>>`
//!
//! The double `Option` encodes two independent "unset" signals:
//!
//! - **Outer `Option`** — this property was never configured at all (the
//!   whole slot falls through to the next tier: widget → theme → default).
//! - **Inner `Option<V>` inside the [`WidgetStateProperty`]** — the property
//!   IS configured, but has nothing to say for the *current* states (that one
//!   resolution falls through, per [`WidgetStateProperty`]'s own
//!   `Option`-fallthrough contract — see `flui_sdk::widgets::widget_state`'s
//!   module docs, the substrate this button family was built against).
//!
//! Both signals fall through identically in
//! `crate::button_style_button`'s resolver: an unset *property* and a property
//! that *resolves* to `None` behave the same way.
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
//! # V1 slots
//!
//! `text_style`, `background_color`, `foreground_color`,
//! `overlay_color`, `elevation`, `padding`, `minimum_size`, `fixed_size`,
//! `maximum_size`, `side`, `shape` — the eleven slots every M3 default table
//! in the button family populates.
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
//!   type; every button below skips the tap-target padding and
//!   density-adjustment step.
//! - **`alignment`** — an `Align`-wrapped child slot; the V1
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
//! - **`ButtonStyle::lerp`** — arrives when a component first needs
//!   `AnimatedTheme`; nothing here consumes an interpolated style yet.

use flui_sdk::painting::BorderSide;
use flui_sdk::painting::TextStyle;
use flui_sdk::widgets::WidgetStateProperty;
use flui_sdk::{
    geometry::{EdgeInsets, Size},
    painting::Color,
};

use crate::shape::MaterialShape;

/// The visual properties most buttons have in common.
///
/// Every field is `None` by default. Build one with a struct literal and `..Default::default()`:
///
/// ```rust
/// use flui_material::ButtonStyle;
/// use flui_sdk::widgets::WidgetStateProperty;
/// use flui_sdk::painting::Color;
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
    /// The style for the button's text descendants.
    pub text_style: Option<WidgetStateProperty<Option<TextStyle>>>,

    /// The button's background fill color.
    pub background_color: Option<WidgetStateProperty<Option<Color>>>,

    /// The color for the button's text descendants — takes precedence over
    /// [`text_style`](Self::text_style)'s own color.
    pub foreground_color: Option<WidgetStateProperty<Option<Color>>>,

    /// The state-overlay highlight color, resolved and handed to the
    /// button's `InkWell` as a live property (not a single baked value —
    /// see `crate::button_style_button`'s docs).
    pub overlay_color: Option<WidgetStateProperty<Option<Color>>>,

    /// The elevation of the button's `Material`.
    pub elevation: Option<WidgetStateProperty<Option<f64>>>,

    /// The padding between the button's boundary and its child (an
    /// `EdgeInsets`; a directional variant has no FLUI consumer yet).
    pub padding: Option<WidgetStateProperty<Option<EdgeInsets>>>,

    /// The minimum size of the button itself.
    pub minimum_size: Option<WidgetStateProperty<Option<Size>>>,

    /// The button's fixed size, overriding [`minimum_size`](Self::minimum_size)/
    /// [`maximum_size`](Self::maximum_size) on whichever axis is finite.
    pub fixed_size: Option<WidgetStateProperty<Option<Size>>>,

    /// The maximum size of the button itself.
    pub maximum_size: Option<WidgetStateProperty<Option<Size>>>,

    /// The color and weight of the button's outline.
    ///
    /// **Data-complete, not yet painted**: this slot resolves correctly
    /// (exercised by [`OutlinedButton`](crate::OutlinedButton)'s
    /// resolved-style tests), but [`MaterialShape`] is fill-and-clip-only —
    /// `Material.shape`'s border painting is a pre-existing named deferral
    /// (see `shape.rs`'s "Named deferral: `OutlinedBorder` sides"). An
    /// `OutlinedButton` in V1 resolves an outline color/width but does not
    /// yet draw a stroke.
    pub side: Option<WidgetStateProperty<Option<BorderSide<f64>>>>,

    /// The shape of the button's underlying `Material` (a
    /// [`MaterialShape`]; an open border hierarchy is `Material`'s own named
    /// deferral).
    pub shape: Option<WidgetStateProperty<Option<MaterialShape>>>,
}
