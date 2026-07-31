//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/default_text_style_test.dart`
//! (tag `3.44.0`), both cases. FLUI's text stack is cosmic-text, not
//! Flutter's engine (no Ahem test font), so the ported case below is
//! geometry-**relative** (A vs B under the same constraints) rather than an
//! exact pixel assertion, matching `text_test.rs`'s house rule for the same
//! reason.
//!
//! Widget: `packages/flutter/lib/src/widgets/text.dart` `DefaultTextStyle`
//! (`:55-73`); FLUI's port is `crates/flui-widgets/src/text/default_text_style.rs`.
//! That file's own "Not ported, and named" section already records that
//! `softWrap`, `overflow`, `textWidthBasis`, and `textHeightBehavior` have no
//! FLUI counterpart — `Text`/`RichText` carry only `style`, `text_align`, and
//! `max_lines`. `text_test.rs`'s module doc independently reaches the same
//! conclusion for `'DefaultTextStyle.merge correctly merges arguments'`
//! (a different Dart test exercising the same four fields).
//!
//! `text_test.rs` already ports most of this file's first case as a
//! documented side effect of `text_test.dart`'s `'Text can be created from
//! TextSpans and uses defaultTextStyle'`:
//! - `default_text_style_ambient_font_size_widens_and_heightens_descendant_text`
//!   — the ambient `style` reaches the descendant run.
//! - `default_text_style_max_lines_fallback_caps_a_text_that_sets_none` — the
//!   ambient `max_lines` fallback (`text.dart:765`).
//!
//! Neither file previously exercised the third field FLUI *does* carry —
//! `DefaultTextStyle::text_align`, consumed by `Text::build`'s `textAlign ??
//! defaultTextStyle.textAlign ?? TextAlign.start` fallback (`text.dart:757`,
//! `text.rs:108-112`) — `rg '\.text_align\('` over `crates/flui-widgets/`
//! before this file returned zero call sites. That gap is what this file
//! closes.
//!
//! ## Case ledger
//!
//! 1. `'DefaultTextStyle changes propagate to Text'` — split by field:
//!    - `style`: **already-covered** by `text_test.rs`'s
//!      `default_text_style_ambient_font_size_widens_and_heightens_descendant_text`,
//!      verified open (asserts a larger ambient font size measures both
//!      wider and taller than the unstyled baseline).
//!    - `maxLines`: **already-covered** by `text_test.rs`'s
//!      `default_text_style_max_lines_fallback_caps_a_text_that_sets_none`,
//!      verified open (asserts an ambient `max_lines(1)` produces a shorter
//!      paragraph than no cap).
//!    - `textAlign`: **ported**, SUBSTITUTED —
//!      [`default_text_style_text_align_fallback_shifts_a_text_that_sets_none`].
//!      The oracle reads `RichText.textAlign` directly off the built widget
//!      (`tester.firstWidget(find.byType(RichText))`); FLUI's harness has no
//!      accessor for `RichText`'s `align` field (only geometry probes), so
//!      this substitutes the same paint-offset observable
//!      `right_aligned_text_paints_further_right_than_left_aligned_for_the_same_short_line`
//!      already uses for `Text`'s own `align`. It also substitutes the
//!      oracle's `TextAlign.justify` for `TextAlign::Right`: `Justify` only
//!      stretches inter-word spacing on a line that needs to fill its width,
//!      and Flutter itself never justifies a paragraph's last (here: only)
//!      line — a single short two-word line has no observable justify
//!      effect on `paragraph_first_line_left`, so it cannot distinguish
//!      "ambient value reached `Text`" from "fallback is broken". `Right`
//!      can. What this substitution therefore cannot catch: whether
//!      `TextAlign::Justify` specifically round-trips through the ambient
//!      fallback (untested by either probe), and whether the *exact* Flutter
//!      enum variant crossed the boundary rather than some other
//!      right-shifting one (`End` under LTR resolves identically) — it only
//!      proves the ambient value, whichever it was, reached the run instead
//!      of being dropped in favor of the `TextAlign::Start` default.
//!    - `softWrap`, `overflow`: **out of scope, missing capability** — no
//!      FLUI `DefaultTextStyle`/`Text`/`RichText` field exists for either
//!      (see the module-doc citations above); nothing to port them onto.
//! 2. `'AnimatedDefaultTextStyle changes propagate to Text'` — **out of
//!    scope, missing capability**. `rg -l AnimatedDefaultTextStyle` over
//!    `crates/` returns no hits: FLUI has no `AnimatedDefaultTextStyle`
//!    widget at all (implicit-animation wrapper that tweens `TextStyle`
//!    across a `Duration`), so there is no build target for this case's
//!    two-pump-plus-tick sequence to run against. This is a widget-level
//!    gap, not a narrowable field one: no subset of the case is reachable by
//!    asserting less. The case becomes portable when an implicit-animation
//!    wrapper that tweens `TextStyle` across a `Duration` exists — until then
//!    a same-named type that does not animate would make the port green
//!    while testing nothing the oracle tests.

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::typography::{TextAlign, TextStyle};
use flui_widgets::{DefaultTextStyle, Text};

use crate::harness;

/// A box loose on both axes up to `max_width` × `2000`. `TextAlign` only
/// shifts the paint offset relative to the *incoming* max width the
/// paragraph laid out against, so a tight width (min == max) is the wrong
/// shape here — it forces the paragraph's own content width up to fill the
/// box, leaving no "extra space" for alignment to distribute. A loose bound
/// keeps the box sized to its (short) content while still recording the
/// wider incoming max width the alignment shift is computed against.
/// (Mirrors `text_test.rs`'s identically-named private helper — each parity
/// file owns its own small constraint builders rather than sharing across
/// sibling test-binary modules.)
fn loose_width(max_width: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(max_width), px(0.0), px(2000.0))
}

/// An ambient `DefaultTextStyle::text_align` reaches a descendant `Text`
/// that sets no alignment of its own — Flutter's `textAlign ??
/// defaultTextStyle.textAlign ?? TextAlign.start` fallback (`text.dart:757`,
/// ported at `crates/flui-widgets/src/text/text.rs:108-112`).
///
/// Flutter parity: `default_text_style_test.dart`'s `'DefaultTextStyle
/// changes propagate to Text'`, the `textAlign: TextAlign.justify` half of
/// its second `pumpWidget` — substituted per the module doc's case-ledger
/// entry (`TextAlign::Right` in place of `Justify`; a paint-offset probe in
/// place of a direct `RichText.textAlign` read).
#[test]
fn default_text_style_text_align_fallback_shifts_a_text_that_sets_none() {
    let wide = loose_width(400.0);

    let unstyled = harness::pump_widget(Text::new("hi"), wide);
    let ambient_right = harness::pump_widget(
        DefaultTextStyle::new(TextStyle::default(), Text::new("hi")).text_align(TextAlign::Right),
        wide,
    );

    let unstyled_left =
        unstyled.paragraph_first_line_left(unstyled.find_by_render_type("RenderParagraph"));
    let ambient_right_left = ambient_right
        .paragraph_first_line_left(ambient_right.find_by_render_type("RenderParagraph"));

    assert!(
        ambient_right_left > unstyled_left,
        "an ambient DefaultTextStyle::text_align(Right) must shift a Text \
         that sets no own alignment further right than the unstyled \
         (start-aligned) baseline: ambient_right={ambient_right_left}, \
         unstyled={unstyled_left}"
    );
}
