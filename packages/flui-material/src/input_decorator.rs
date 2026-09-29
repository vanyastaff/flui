//! [`InputDecoration`] + [`InputDecorator`] — the M3 filled text field
//! decoration substrate.
//!
//! **Scope**: M3 filled text field decoration — underline indicator, hint,
//! floating label (snap), helper/error line, hover fill blend, and the M3
//! state table — composed from existing widgets; NOT a `_RenderDecoration`
//! port.
//!
//! Flutter parity: `material/input_decorator.dart` (oracle tag `3.44.0`),
//! narrowed to the filled/underline variant.
//!
//! # Named divergences / deferrals
//!
//! - **Baseline slot layout** — the oracle's `_RenderDecoration` positions
//!   the hint, label, and input text at a shared baseline, overlaid at the
//!   same rect. This substrate composes plain [`flui_sdk::widgets::Column`] rows
//!   instead: at most one of the label/hint rows renders per build (see
//!   `should_show_hint`), and the input content is its own row below them
//!   — not overlaid at the input's rect.
//! - **Exact float metrics + the 167ms/0.75-scale label animation** — the
//!   oracle animates the label's position/size over `_kTransitionDuration`
//!   (167ms) between 1.0 and `_kFinalLabelScale` (0.75,
//!   `input_decorator.dart:41`). V1 snaps: the floating label is scaled by
//!   0.75 instantly, no interpolation, no exact pixel position.
//! - **`isDense`** — no compact content-padding tier.
//! - **`OutlineInputBorder` + label gap** — a real `input_border.dart` port
//!   later; never faked here.
//! - **`prefix`/`suffix`/counter** — no icon or character-count slots.
//! - **Error shake** — the oracle's `_shakingLabelController`; not ported.
//! - **`String`-only label/hint/helper/error slots** — the oracle's
//!   `InputDecoration` additionally accepts a `Widget` for each
//!   (`label`/`hint`/`helper`/`error`); V1 ships `String` only, additively
//!   extensible (a `Widget` variant can be added to each field later without
//!   breaking the `String` construction path).
//!
//! # State-table branch order
//!
//! Every M3 default table below follows the oracle's exact branch order:
//! disabled → error (→ focused → hovered → plain) → focused → hovered →
//! default. The oracle documents *why* focused precedes hovered for this
//! widget specifically (`input_decorator.dart:5945-5953`, tag `3.44.0`):
//! unlike most Material widgets, a focused **and** hovered field (common on
//! desktop, where users often click into a field) must show the focused
//! treatment, not the hovered one.

use std::sync::Arc;

use flui_sdk::foundation::ListenerId;
use flui_sdk::foundation::notifier::Listenable;
use flui_sdk::geometry::EdgeInsets;
use flui_sdk::geometry::Radius;
use flui_sdk::painting::TextStyle;
use flui_sdk::painting::{Border, BorderRadius, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Column, CrossAxisAlignment, DecoratedBox, MouseRegion, Padding, Text, WidgetState,
    WidgetStateProperty, WidgetStates, WidgetStatesController,
};

use crate::color_scheme::ColorScheme;
use crate::theme::Theme;
use crate::theme_data::InputDecorationThemeData;

// ============================================================================
// InputDecoration
// ============================================================================

/// Data describing a text field's decoration — labels, fill, and enabled
/// state. Carries no layout knobs: a future render-object decorator
/// consumes this struct unchanged.
///
/// Flutter parity: `InputDecoration` (`material/input_decorator.dart`,
/// oracle tag `3.44.0`), narrowed to the V1 field list — see the module
/// docs for the full named-divergence list (no `Widget` slots, no
/// `prefix`/`suffix`, no `isDense`, no `border` override).
///
/// Every field is a plain public field; build one with a struct literal and
/// `..Default::default()`, the same convention as [`crate::ButtonStyle`].
#[derive(Clone, Debug, PartialEq)]
pub struct InputDecoration {
    /// The field's label — floats above the content when the field is
    /// focused or non-empty, otherwise sits inline in place of the hint.
    pub label_text: Option<String>,
    /// Placeholder text shown when the field is empty and the label isn't
    /// occupying the inline slot — see the crate's `should_show_hint` helper.
    pub hint_text: Option<String>,
    /// A helper line shown below the content, replaced by
    /// [`error_text`](Self::error_text) when both are set.
    pub helper_text: Option<String>,
    /// An error line shown below the content, replacing
    /// [`helper_text`](Self::helper_text) when both are set.
    pub error_text: Option<String>,
    /// Whether the container is filled (the M3 filled variant this
    /// substrate implements). `false` renders a fully transparent
    /// container with no hover blend — Flutter parity: `_getFillColor`
    /// returns `Colors.transparent` when `filled != true`
    /// (`input_decorator.dart:2131-2140`, tag `3.44.0`).
    pub filled: bool,
    /// Overrides the container's content padding. `None` falls through to
    /// the ambient [`InputDecorationThemeData::content_padding`], then the
    /// M3 default.
    pub content_padding: Option<EdgeInsets>,
    /// Whether the field accepts interaction. `true` by default. Disabled
    /// selects the disabled M3 colors and suppresses the hover blend.
    pub enabled: bool,
}

impl Default for InputDecoration {
    fn default() -> Self {
        Self {
            label_text: None,
            hint_text: None,
            helper_text: None,
            error_text: None,
            filled: false,
            content_padding: None,
            enabled: true,
        }
    }
}

// ============================================================================
// M3 defaults — `_InputDecoratorDefaultsM3` (input_decorator.dart, tag 3.44.0)
// ============================================================================

/// M3 default `fillColor` — `_InputDecoratorDefaultsM3.fillColor`
/// (`input_decorator.dart:5964-5969`). No hovered branch: the container's
/// hover treatment is a separate alpha blend over this color, not a fill
/// state — see [`hover_blended_fill`] and the module doc on why this
/// deliberately does not fold hover into the table.
fn default_fill_color(colors: ColorScheme) -> WidgetStateProperty<Option<Color>> {
    WidgetStateProperty::resolve_with(move |states| {
        Some(if states.contains_state(WidgetState::Disabled) {
            colors.on_surface.with_opacity(0.04)
        } else {
            colors.surface_container_highest
        })
    })
}

/// M3 default `activeIndicatorBorder` (the bottom underline) —
/// `_InputDecoratorDefaultsM3.activeIndicatorBorder`
/// (`input_decorator.dart:5972-5992`).
fn default_active_indicator(colors: ColorScheme) -> WidgetStateProperty<Option<BorderSide<f64>>> {
    WidgetStateProperty::resolve_with(move |states| {
        Some(if states.contains_state(WidgetState::Disabled) {
            BorderSide::new(
                colors.on_surface.with_opacity(0.38),
                1.0,
                BorderStyle::Solid,
            )
        } else if states.contains_state(WidgetState::Error) {
            if states.contains_state(WidgetState::Focused) {
                BorderSide::new(colors.error, 2.0, BorderStyle::Solid)
            } else if states.contains_state(WidgetState::Hovered) {
                BorderSide::new(colors.on_error_container, 1.0, BorderStyle::Solid)
            } else {
                BorderSide::new(colors.error, 1.0, BorderStyle::Solid)
            }
        } else if states.contains_state(WidgetState::Focused) {
            BorderSide::new(colors.primary, 2.0, BorderStyle::Solid)
        } else if states.contains_state(WidgetState::Hovered) {
            BorderSide::new(colors.on_surface, 1.0, BorderStyle::Solid)
        } else {
            BorderSide::new(colors.on_surface_variant, 1.0, BorderStyle::Solid)
        })
    })
}

/// M3 default `hintStyle` — `_InputDecoratorDefaultsM3.hintStyle`
/// (`input_decorator.dart:5956-5961`). Only two branches (disabled/plain):
/// the oracle's hint style carries no base font — a bare
/// `TextStyle(color: ...)`, unlike [`default_label_style`]/
/// [`default_helper_style`] which start from a `TextTheme` role.
fn default_hint_style(colors: ColorScheme) -> WidgetStateProperty<Option<TextStyle>> {
    WidgetStateProperty::resolve_with(move |states| {
        Some(if states.contains_state(WidgetState::Disabled) {
            TextStyle::default().with_color(colors.on_surface.with_opacity(0.38))
        } else {
            TextStyle::default().with_color(colors.on_surface_variant)
        })
    })
}

/// M3 default `labelStyle` — `_InputDecoratorDefaultsM3.labelStyle`
/// (`input_decorator.dart:6043-6064`). `floatingLabelStyle`
/// (`:6067-6088`) is byte-identical to this table in the oracle, so this
/// one slot serves both the floating and inline positions — see the
/// module doc's `String`-only-slots note and [`InputDecorationThemeData::label_style`].
/// `base` is the ambient `TextTheme.bodyLarge` role.
fn default_label_style(
    colors: ColorScheme,
    base: TextStyle,
) -> WidgetStateProperty<Option<TextStyle>> {
    WidgetStateProperty::resolve_with(move |states| {
        let base = base.clone();
        Some(if states.contains_state(WidgetState::Disabled) {
            base.with_color(colors.on_surface.with_opacity(0.38))
        } else if states.contains_state(WidgetState::Error) {
            if states.contains_state(WidgetState::Focused) {
                base.with_color(colors.error)
            } else if states.contains_state(WidgetState::Hovered) {
                base.with_color(colors.on_error_container)
            } else {
                base.with_color(colors.error)
            }
        } else if states.contains_state(WidgetState::Focused) {
            base.with_color(colors.primary)
        } else {
            // Hovered and the oracle's unconditional fallback both resolve
            // to `onSurfaceVariant` (`input_decorator.dart:6060-6063`).
            base.with_color(colors.on_surface_variant)
        })
    })
}

/// M3 default `helperStyle` — `_InputDecoratorDefaultsM3.helperStyle`
/// (`input_decorator.dart:6090-6097`). `base` is the ambient
/// `TextTheme.bodySmall` role.
fn default_helper_style(
    colors: ColorScheme,
    base: TextStyle,
) -> WidgetStateProperty<Option<TextStyle>> {
    WidgetStateProperty::resolve_with(move |states| {
        Some(if states.contains_state(WidgetState::Disabled) {
            base.clone()
                .with_color(colors.on_surface.with_opacity(0.38))
        } else {
            base.clone().with_color(colors.on_surface_variant)
        })
    })
}

/// M3 default `errorStyle` — `_InputDecoratorDefaultsM3.errorStyle`
/// (`input_decorator.dart:6099-6103`). Unconditionally `error`-colored — the
/// oracle applies no other branch (not even `disabled`: an error line only
/// ever appears on an enabled, invalid field). `base` is the ambient
/// `TextTheme.bodySmall` role.
fn default_error_style(
    colors: ColorScheme,
    base: TextStyle,
) -> WidgetStateProperty<Option<TextStyle>> {
    WidgetStateProperty::resolve_with(move |_states| Some(base.clone().with_color(colors.error)))
}

/// M3 default content padding for the filled, non-outline, non-dense case —
/// `EdgeInsets.fromLTRB(12, 8, 12, 8)` (`InputDecoration.contentPadding`'s
/// doc comment, `input_decorator.dart:3333-3334`, tag `3.44.0`).
fn default_content_padding() -> EdgeInsets {
    EdgeInsets::new(8.0, 12.0, 8.0, 12.0)
}

/// `ThemeData.hoverColor`'s default (`theme_data.dart:468`, tag `3.44.0`):
/// `isDark ? Colors.white.withOpacity(0.04) : Colors.black.withOpacity(0.04)`
/// — a fixed brightness-keyed constant, **not** a `ColorScheme` role (the
/// M3 button family's own hover overlay, `onSurface@8%`, is a different,
/// unrelated constant — see `crate::elevated_button`'s
/// `pressed_hovered_focused_overlay`). `_getHoverColor`
/// (`input_decorator.dart:2142-2147`) falls back to this exact field when
/// `InputDecoration.hoverColor` isn't set, which V1 has no override slot
/// for yet.
fn default_hover_color(brightness: Brightness) -> Color {
    match brightness {
        Brightness::Dark => Color::WHITE.with_opacity(0.04),
        Brightness::Light => Color::BLACK.with_opacity(0.04),
    }
}

/// The container fill after the hover blend — `_InputBorderPainter.blendedColor`
/// (`input_decorator.dart:136`, tag `3.44.0`): `Color.alphaBlend(hoverColor,
/// fillColor)` while hovering, `fillColor` unchanged otherwise (the oracle
/// animates between `hoverColor.withAlpha(0)` and `hoverColor`; V1 snaps
/// between the two ends, no interpolation). Guarded exactly like
/// `_getHoverColor` (`input_decorator.dart:2142-2147`): only `filled &&
/// enabled` fields blend at all.
fn hover_blended_fill(
    fill: Color,
    brightness: Brightness,
    filled: bool,
    enabled: bool,
    is_hovering: bool,
) -> Color {
    if filled && enabled && is_hovering {
        default_hover_color(brightness).blend_over(fill)
    } else {
        fill
    }
}

// ============================================================================
// Floating label / hint visibility — pure, directly-tested formulas
// ============================================================================

/// Whether the label should float above the content row rather than sit
/// inline in place of the hint.
///
/// Flutter parity: `_labelShouldWithdraw` (`input_decorator.dart:1969`, tag
/// `3.44.0`): `!isEmpty || (isFocused && decoration.enabled)`. The `enabled`
/// guard is load-bearing: a disabled, empty, "focused" field (focus a field
/// then disable it) must NOT float — see the module's disabled-row test.
#[must_use]
fn label_should_float(is_empty: bool, focused: bool, enabled: bool) -> bool {
    !is_empty || (focused && enabled)
}

/// Whether the hint text should be visible.
///
/// Flutter parity: `showHint` (`input_decorator.dart:2346`) =
/// `isEmpty && !_hasInlineLabel`, where `_hasInlineLabel`
/// (`:2176-2178`) is `!labelShouldWithdraw && hasLabel` — i.e. the label is
/// "inline" (occupying the hint's slot) exactly when it is NOT floating and
/// a label is set.
#[must_use]
fn should_show_hint(is_empty: bool, has_label: bool, float: bool) -> bool {
    is_empty && (!has_label || float)
}

/// The helper-or-error line to render: error replaces helper when both are
/// set (Flutter parity: `_HelperError` shows one or the other, never both —
/// `input_decorator.dart`'s `_HelperErrorState`, tag `3.44.0`). Returns the
/// text and whether it is the error line (vs. the helper line), so the
/// caller can pick [`default_error_style`] vs [`default_helper_style`].
#[must_use]
fn helper_or_error_line(decoration: &InputDecoration) -> Option<(&str, bool)> {
    if let Some(error) = decoration.error_text.as_deref() {
        Some((error, true))
    } else {
        decoration
            .helper_text
            .as_deref()
            .map(|helper| (helper, false))
    }
}

// ============================================================================
// InputDecorator
// ============================================================================

/// Composes [`InputDecoration`] data and a `child` (the field's content —
/// e.g. an `EditableText`, in a future `TextField`) into the M3 filled/
/// underline decoration: fill, underline, floating label, hint, and
/// helper/error line.
///
/// # State inputs
///
/// [`focused`](Self::focused) and [`is_empty`](Self::is_empty) are explicit
/// widget inputs — Flutter parity: `InputDecorator.isFocused`/`isEmpty`
/// (`input_decorator.dart:1868-1958`, tag `3.44.0`). `enabled`/`error` come
/// from [`InputDecoration`] itself. `hovered` is the one state this widget
/// tracks internally, via its own [`MouseRegion`] (the seam is
/// `flui_sdk::widgets::MouseRegion`, not `InkWell`'s press/ripple machinery) —
/// a future `TextField` wires `focused`/`is_empty` from its own `FocusNode`/
/// `TextEditingController`; a standalone consumer passes them explicitly.
#[derive(Clone, Debug, StatefulView)]
pub struct InputDecorator {
    decoration: InputDecoration,
    focused: bool,
    is_empty: bool,
    child: Child,
}

impl InputDecorator {
    /// Create a decorator around `decoration`, initially unfocused and
    /// non-empty (Flutter's own `isFocused`/`isEmpty` defaults — both
    /// `false`, `input_decorator.dart:1877,1879`).
    #[must_use]
    pub fn new(decoration: InputDecoration) -> Self {
        Self {
            decoration,
            focused: false,
            is_empty: false,
            child: Child::empty(),
        }
    }

    /// Set whether the field currently holds keyboard focus.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set whether the field's content is currently empty.
    #[must_use]
    pub fn is_empty(mut self, is_empty: bool) -> Self {
        self.is_empty = is_empty;
        self
    }

    /// Set the decorated content (the field's actual input surface).
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

/// Persistent state behind [`InputDecorator`] — owns the internal hover
/// tracking (see the struct doc's "State inputs" section).
pub struct InputDecoratorState {
    hover: WidgetStatesController,
    hover_listener: Option<ListenerId>,
}

impl std::fmt::Debug for InputDecoratorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputDecoratorState")
            .field("hover", &self.hover.value())
            .finish_non_exhaustive()
    }
}

impl StatefulView for InputDecorator {
    type State = InputDecoratorState;

    fn create_state(&self) -> Self::State {
        InputDecoratorState {
            hover: WidgetStatesController::default(),
            hover_listener: None,
        }
    }
}

impl ViewState<InputDecorator> for InputDecoratorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // ADR-0018: `rebuild_handle()` is acquired here, fired later from the
        // hover-controller listener below — never called from `build`.
        let rebuild = ctx.rebuild_handle();
        self.hover_listener = Some(self.hover.add_listener(Arc::new(move || {
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));
    }

    fn dispose(&mut self) {
        if let Some(id) = self.hover_listener.take() {
            self.hover.remove_listener(id);
        }
    }

    fn build(&self, view: &InputDecorator, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;
        let text_theme = theme.text_theme.clone();
        let decoration_theme: InputDecorationThemeData =
            theme.input_decoration_theme.clone().unwrap_or_default();
        let decoration = &view.decoration;

        // Flutter parity: `_InputDecoratorState.widgetState`
        // (`input_decorator.dart:2250-2254`, tag `3.44.0`) — disabled,
        // focused, hovering (already enabled-gated), error.
        let is_hovering =
            decoration.enabled && self.hover.value().contains_state(WidgetState::Hovered);
        let mut states = WidgetStates::NONE;
        if !decoration.enabled {
            states = states.with_state(WidgetState::Disabled);
        }
        if view.focused {
            states = states.with_state(WidgetState::Focused);
        }
        if is_hovering {
            states = states.with_state(WidgetState::Hovered);
        }
        if decoration.error_text.is_some() {
            states = states.with_state(WidgetState::Error);
        }

        // Fill + hover blend.
        let fill_color = if decoration.filled {
            decoration_theme
                .fill_color
                .as_ref()
                .map_or_else(
                    || default_fill_color(colors).resolve(&states),
                    |p| p.resolve(&states),
                )
                .unwrap_or(Color::TRANSPARENT)
        } else {
            Color::TRANSPARENT
        };
        let blended_fill = hover_blended_fill(
            fill_color,
            colors.brightness,
            decoration.filled,
            decoration.enabled,
            is_hovering,
        );

        // Bottom underline indicator.
        let indicator = decoration_theme
            .active_indicator
            .as_ref()
            .map_or_else(
                || default_active_indicator(colors).resolve(&states),
                |p| p.resolve(&states),
            )
            .unwrap_or_else(BorderSide::none);

        // The 4dp top radius rounds the FILL only (`decoration_rrect` in
        // `flui-painting`'s `paint_box_decoration`) — the non-uniform
        // bottom-only border below paints a straight, unrounded line
        // regardless of `border_radius` (`paint_border`'s non-uniform path
        // ignores the rrect), exactly matching `UnderlineInputBorder`'s own
        // rendering: a rounded fill, a straight underline stroke.
        let box_decoration = BoxDecoration::new()
            .set_color(Some(blended_fill))
            .set_border_radius(Some(BorderRadius::top(Radius::circular(4.0))))
            .set_border(Some(Border::new(None, None, Some(indicator), None)));

        let content_padding = decoration
            .content_padding
            .or(decoration_theme.content_padding)
            .unwrap_or_else(default_content_padding);

        // Label / hint rows.
        let has_label = decoration.label_text.is_some();
        let float = label_should_float(view.is_empty, view.focused, decoration.enabled);
        let show_hint = should_show_hint(view.is_empty, has_label, float);

        let mut rows: Vec<BoxedView> = Vec::new();
        if let Some(label_text) = decoration.label_text.clone() {
            let base_label_style = decoration_theme.label_style.as_ref().map_or_else(
                || {
                    default_label_style(colors, text_theme.body_large.clone().unwrap_or_default())
                        .resolve(&states)
                },
                |p| p.resolve(&states),
            );
            let mut label_style = base_label_style.unwrap_or_default();
            if float {
                // The oracle's `_kFinalLabelScale` (`input_decorator.dart:41`,
                // tag `3.44.0`) — applied as a snap, not the oracle's
                // 167ms-animated interpolation. See the module doc.
                if let Some(font_size) = label_style.font_size {
                    label_style = label_style.with_font_size(font_size * 0.75);
                }
            }
            rows.push(Text::new(label_text).style(label_style).boxed());
        }
        if show_hint && let Some(hint_text) = decoration.hint_text.clone() {
            let hint_style = decoration_theme
                .hint_style
                .as_ref()
                .map_or_else(
                    || default_hint_style(colors).resolve(&states),
                    |p| p.resolve(&states),
                )
                .unwrap_or_default();
            rows.push(Text::new(hint_text).style(hint_style).boxed());
        }
        if let Some(child) = view.child.clone().into_inner() {
            rows.push(child);
        }
        if let Some((line_text, is_error)) = helper_or_error_line(decoration) {
            let base = text_theme.body_small.clone().unwrap_or_default();
            let style = if is_error {
                decoration_theme.error_style.as_ref().map_or_else(
                    || default_error_style(colors, base).resolve(&states),
                    |p| p.resolve(&states),
                )
            } else {
                decoration_theme.helper_style.as_ref().map_or_else(
                    || default_helper_style(colors, base).resolve(&states),
                    |p| p.resolve(&states),
                )
            }
            .unwrap_or_default();
            rows.push(Text::new(line_text.to_string()).style(style).boxed());
        }

        let content = Padding::new(content_padding)
            .child(Column::new(rows).cross_axis_alignment(CrossAxisAlignment::Start));

        let hover_on_enter = self.hover.clone();
        let hover_on_exit = self.hover.clone();

        MouseRegion::new()
            .on_enter(move |_cx, _device, _offset| {
                hover_on_enter.update(WidgetState::Hovered, true);
            })
            .on_exit(move |_cx, _device, _offset| hover_on_exit.update(WidgetState::Hovered, false))
            .child(DecoratedBox::new(box_decoration).child(content))
    }
}

#[cfg(test)]
mod tests {
    use flui_sdk::widgets::WidgetState;

    use super::*;

    fn resolve<T: Clone + Default>(
        property: &WidgetStateProperty<Option<T>>,
        states: WidgetStates,
    ) -> Option<T> {
        property.resolve(&states)
    }

    // ========================================================================
    // InputDecoration
    // ========================================================================

    // ========================================================================
    // State-table pins — `default_fill_color`
    // ========================================================================

    // ========================================================================
    // State-table pins — `default_active_indicator` (combined states pinned)
    // ========================================================================

    /// Mutation-style red-check: swapping the `error`/`focused` branch order
    /// (checking `Focused` before `Error`) would make `error+focused`
    /// resolve `colors.primary` (the top-level focused color) instead of
    /// `colors.error` — this assertion fails under that mutant.
    #[test]
    fn active_indicator_error_branch_is_checked_before_focused_at_the_top_level() {
        let colors = ColorScheme::light();
        let property = default_active_indicator(colors);
        let error_focused = resolve(
            &property,
            WidgetStates::from(WidgetState::Error).with_state(WidgetState::Focused),
        )
        .expect("error+focused branch");
        assert_eq!(
            error_focused.color, colors.error,
            "error must be checked before focused"
        );
        assert_ne!(error_focused.color, colors.primary);
    }

    // ========================================================================
    // State-table pins — `default_hint_style`
    // ========================================================================

    // ========================================================================
    // State-table pins — `default_label_style` (combined states pinned)
    // ========================================================================

    // ========================================================================
    // State-table pins — `default_helper_style` / `default_error_style`
    // ========================================================================

    // ========================================================================
    // Hover blend
    // ========================================================================

    #[test]
    fn hover_blend_matches_blend_over_exactly_when_hovering() {
        let fill = Color::rgb(200, 200, 200);
        let brightness = Brightness::Light;
        let blended = hover_blended_fill(fill, brightness, true, true, true);
        let expected = default_hover_color(brightness).blend_over(fill);
        assert_eq!(blended, expected);
        assert_ne!(blended, fill, "a real blend must change the color");
    }

    // ========================================================================
    // Float truth table — all four (is_empty, focused) corners + disabled
    // ========================================================================

    #[test]
    fn float_truth_table_all_four_corners_plus_disabled() {
        // Corner 1: has text, unfocused, enabled -> floats (non-empty alone floats it).
        assert!(label_should_float(false, false, true));
        // Corner 2: has text, focused, enabled -> floats.
        assert!(label_should_float(false, true, true));
        // Corner 3: empty, unfocused, enabled -> does not float (inline).
        assert!(!label_should_float(true, false, true));
        // Corner 4: empty, focused, enabled -> floats (focus alone floats an
        // empty field).
        assert!(label_should_float(true, true, true));
        // Disabled corner: empty, "focused", but disabled -> does not float.
        // This is the load-bearing `enabled` guard the doc comment names —
        // a field focused then disabled must not keep floating.
        assert!(!label_should_float(true, true, false));
    }

    #[test]
    fn should_show_hint_truth_table() {
        // No label at all: hint tracks emptiness alone.
        assert!(should_show_hint(true, false, false));
        assert!(!should_show_hint(false, false, false));
        // Label present, not floating (inline): hint is suppressed even
        // though the field is empty — the label occupies the slot.
        assert!(!should_show_hint(true, true, false));
        // Label present AND floating: hint still shows when empty (both
        // the floated label and the hint are visible at once).
        assert!(should_show_hint(true, true, true));
        // Non-empty: hint never shows regardless of label/float.
        assert!(!should_show_hint(false, true, true));
    }

    // ========================================================================
    // Helper/error line selection
    // ========================================================================
}
