//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/lib/src/widgets/icon.dart` (there is no
//! `icon_test.dart` geometry suite to port line-for-line; these assertions
//! are derived directly from `Icon.build`, lines 260-357).
//!
//! Widget → render-object mapping:
//! - `Icon` → `SizedBox::square(size)` → `RenderConstrainedBox` (its child is
//!   `Center` → `RenderCenter`, then `RichText` → `RenderParagraph` when an
//!   icon is set).
//!
//! Every `Icon` under test is wrapped in a `Center` so its `SizedBox` sees
//! loose incoming constraints (matching `sized_box_test.rs`/`text_test.rs`
//! convention) — a bare tight 800×600 root would force the `SizedBox` up to
//! the surface size via `BoxConstraints.enforce`, not its own square size.
//!
//! Honesty note (see `Icon`'s module docs and
//! `docs/research/2026-07-02-icon-widget-plan.md`): these tests assert the
//! bounded box size and that the codepoint reached `RenderParagraph` — never
//! a rendered glyph or a non-degenerate paragraph width, since FLUI has no
//! icon font loaded and the codepoint shapes to tofu.

use crate::common::size;
use crate::harness;
use flui_widgets::{Center, Icon, IconData};

/// `Icon::new(data)` with no size override and no ancestor `IconTheme`
/// measures to `IconThemeData::fallback().size` — `24×24`.
///
/// Flutter parity: `icon_theme_data.dart:52` (`IconThemeData.fallback().size
/// == 24.0`) combined with `icon.dart:293` (`tentativeIconSize = size ??
/// iconTheme.size ?? kDefaultFontSize`).
#[test]
fn default_size_is_24_by_24() {
    let laid = harness::pump_widget(
        Center::new().child(Icon::new(IconData::new(0xE87D))),
        harness::screen(),
    );

    let sized_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(sized_box_id),
        size(24.0, 24.0),
        "Icon with no size override and no ancestor IconTheme must measure 24×24"
    );
}

/// `Icon::new(data).size(36.0)` overrides the fallback theme size.
///
/// Flutter parity: `icon.dart:293` — the widget's own `size` wins over the
/// ambient theme.
#[test]
fn explicit_size_overrides_the_theme_default() {
    let laid = harness::pump_widget(
        Center::new().child(Icon::new(IconData::new(0xE87D)).size(36.0)),
        harness::screen(),
    );

    let sized_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(sized_box_id),
        size(36.0, 36.0),
        "Icon::size(36.0) must override the 24.0 fallback theme size"
    );
}

/// An `Icon` with a set `IconData` plumbs its codepoint all the way to a
/// `RenderParagraph` — proving the `TextSpan`/`RichText` composition wires up,
/// without asserting anything about the (currently tofu, no icon font loaded)
/// rendered glyph.
///
/// Flutter parity: `icon.dart:328-332` — `RichText(text: TextSpan(text:
/// String.fromCharCode(icon.codePoint), ...))`.
#[test]
fn codepoint_reaches_render_paragraph() {
    let icon_data = IconData::new(0xE87D);
    let laid = harness::pump_widget(
        Center::new().child(Icon::new(icon_data.clone())),
        harness::screen(),
    );

    let code_point_string = icon_data
        .code_point_string()
        .expect("U+E87D is a valid Unicode scalar value");
    assert!(
        laid.find_text(&code_point_string).is_some(),
        "Icon's codepoint {code_point_string:?} must reach a RenderParagraph"
    );
}

/// `Icon::none()` (Flutter's `Icon(null)`) reserves the `size × size` box but
/// draws no glyph: no `RenderParagraph` is mounted at all.
///
/// Flutter parity: `icon.dart:285-289` — a null `icon` short-circuits to
/// `SizedBox(width: iconSize, height: iconSize)` with no `RichText` child.
#[test]
fn none_icon_reserves_the_box_with_no_render_paragraph() {
    let laid = harness::pump_widget(Center::new().child(Icon::none()), harness::screen());

    let sized_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(sized_box_id),
        size(24.0, 24.0),
        "Icon::none() must still reserve the 24×24 fallback box"
    );
    assert!(
        laid.find_all_by_render_type("RenderParagraph").is_empty(),
        "Icon::none() must not mount a RenderParagraph"
    );
}
