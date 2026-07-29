//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/basic_test.dart` (tag
//! `3.44.0`) — the baseline-alignment slice: the 2-case `'Row'` group plus the
//! two top-level `'Row and IgnoreBaseline'` cases that sit near the end of the
//! same file.
//!
//! Widget → render-object mapping: `Row`
//! (`crates/flui-widgets/src/flex/flex.rs`) →
//! [`flui_objects::RenderFlex`]; `IgnoreBaseline`
//! (`crates/flui-widgets/src/layout/ignore_baseline.rs`) →
//! [`flui_objects::RenderIgnoreBaseline`] — a NEW render object and a NEW
//! widget this port introduces (FLUI had neither before).
//!
//! ## Font metrics: the oracle's constants are not portable
//!
//! Every case here is arithmetic over text metrics. The oracle pins the
//! `FlutterTest` font, whose ascent is exactly `0.75 * fontSize`, and then
//! hard-codes the results (`aboveBaseline1 = fontSize1 * 0.75`, `bPos.dy ==
//! 96.0 - 24.0`). FLUI shapes with cosmic-text against whatever font the host
//! machine resolves, so those numbers are machine-dependent — asserting them
//! would pin the test runner's font, not FLUI's layout.
//!
//! These ports therefore read each child's *measured* ascent
//! ([`crate::common::LaidOut::text_baseline`]) and assert the same relations
//! the oracle asserts over its own constants:
//!
//! - row height == tallest ascent + deepest descent (oracle: `aboveBaseline +
//!   belowBaseline`);
//! - each child's `dy` == tallest ascent − its own ascent (oracle:
//!   `aboveBaseline1 - aboveBaseline2`, and `0` for the deepest-ascent child).
//!
//! Substituting measured metrics for the font's constants keeps every oracle
//! assertion — it drops no case and weakens no comparison — so these are
//! recorded as ported, not narrowed. Each test additionally asserts the
//! premise that makes it non-vacuous (that the big-font child really does have
//! the deeper ascent, that the multi-line child really does hang lower), so a
//! font whose metrics collapsed the distinction would fail loudly rather than
//! pass emptily.
//!
//! ## Ledger (4 upstream cases)
//! 1. `'Row' > 'multiple baseline aligned children'` — **ported**:
//!    [`row_baseline_height_spans_the_tallest_ascent_and_deepest_descent`].
//! 2. `'Row' > 'baseline aligned children account for a larger, no-baseline
//!    child size'` — **ported**:
//!    [`row_baseline_grows_to_a_taller_child_that_has_no_baseline`]. The
//!    oracle's no-baseline child is a `FlutterLogo(size: 250)`; FLUI has no
//!    `FlutterLogo`, so a childless `SizedBox` of the same 250×250 stands in —
//!    it is a no-baseline box of the oracle's exact size, which is the only
//!    property the case exercises.
//! 3. `'Row and IgnoreBaseline (control -- with baseline)'` — **ported**:
//!    [`row_baseline_shifts_the_shallower_child_down_to_the_common_baseline`].
//! 4. `'Row and IgnoreBaseline (with ignored baseline)'` — **ported**:
//!    [`ignore_baseline_keeps_its_child_out_of_the_row_s_baseline_alignment`].
//!
//! `textDirection` is dropped from cases 3 and 4's input: `RenderFlex` has no
//! `text_direction` field, so `Row` cannot resolve one — the same standing gap
//! `row_test.rs` documents (`docs/ROADMAP.md` Cross.H, search
//! `RenderFlex`). Both cases are LTR-only in the oracle and their assertions
//! are cross-axis (`dy`), so nothing they check depends on it.
//!
//! **Total: 4 upstream cases = 4 ported (one with a substituted stand-in
//! child), 0 narrowed, 0 out of scope.**

use flui_objects::CrossAxisAlignment;
use flui_types::typography::{TextBaseline, TextStyle};
use flui_view::ViewExt;
use flui_widgets::{IgnoreBaseline, Row, SizedBox, Text};

use crate::common::{LaidOut, loose};
use crate::harness;

/// The oracle's two font sizes. It notes they must stay multiples of 4 for the
/// `FlutterTest` font's own reasons; kept verbatim so the shapes match, though
/// FLUI's measured metrics do not depend on it.
const BIG_FONT: f64 = 52.0;
const SMALL_FONT: f64 = 12.0;

/// The oracle's second `Text`: seven short lines, so it hangs far below the
/// first line's baseline it is aligned by.
const SEVEN_LINES: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven";

fn text(content: &str, font_size: f64) -> Text {
    Text::new(content).style(TextStyle::default().with_font_size(font_size))
}

fn baseline_row(children: Vec<flui_view::BoxedView>) -> Row {
    Row::new(children)
        .cross_axis_alignment(CrossAxisAlignment::Baseline)
        .text_baseline(TextBaseline::Alphabetic)
}

/// The measured ascent and descent of the paragraph rendering `content`.
fn ascent_descent(laid: &LaidOut, content: &str) -> (f32, f32) {
    let id = laid
        .find_text(content)
        .expect("the pumped Row mounts this text");
    let ascent = laid.text_baseline(id);
    (ascent, laid.size(id).height.get() - ascent)
}

/// A baseline-aligned `Row` is as tall as the tallest ascent plus the deepest
/// descent — taller than either child on its own — and shifts each child down
/// by the difference between the tallest ascent and its own.
///
/// Flutter parity: `basic_test.dart` `'Row' > 'multiple baseline aligned
/// children'` (3.44.0). The oracle computes `aboveBaseline`/`belowBaseline`
/// from the `FlutterTest` font's fixed 0.75 ascent ratio; this port measures
/// them instead — see this file's module doc.
#[test]
fn row_baseline_height_spans_the_tallest_ascent_and_deepest_descent() {
    let laid = harness::pump_widget(
        baseline_row(vec![
            text("big text", BIG_FONT).boxed(),
            text(SEVEN_LINES, SMALL_FONT).boxed(),
        ]),
        loose(1000.0),
    );

    let (big_ascent, big_descent) = ascent_descent(&laid, "big text");
    let (lines_ascent, lines_descent) = ascent_descent(&laid, SEVEN_LINES);

    assert!(
        big_ascent > lines_ascent,
        "premise: the 52px text must sit higher above the baseline than the \
         12px text ({big_ascent} vs {lines_ascent}) — otherwise this case \
         asserts nothing",
    );
    assert!(
        lines_descent > big_descent,
        "premise: seven 12px lines aligned by the first line's baseline must \
         hang lower than one 52px line ({lines_descent} vs {big_descent})",
    );

    let row = laid.find_by_render_type("RenderFlex");
    let big = laid.find_text("big text").expect("mounted");
    let lines = laid.find_text(SEVEN_LINES).expect("mounted");

    let row_height = laid.size(row).height.get();
    assert!(
        row_height > laid.size(big).height.get(),
        "the row must be taller than the big text alone",
    );
    assert!(
        row_height > laid.size(lines).height.get(),
        "the row must be taller than the seven-line text alone",
    );
    assert_eq!(
        row_height,
        big_ascent + lines_descent,
        "the row is exactly the tallest ascent plus the deepest descent",
    );

    assert_eq!(
        laid.absolute_offset(big).dy.get(),
        0.0,
        "the deepest-ascent child defines the common baseline and stays at the top",
    );
    assert_eq!(
        laid.absolute_offset(lines).dy.get(),
        big_ascent - lines_ascent,
        "the shallower child shifts down to put its baseline on the same line",
    );
}

/// A child with no baseline still contributes its full height, so one taller
/// than the whole ascent/descent stack sets the row's height by itself — while
/// the baseline-aligned children keep their alignment.
///
/// Flutter parity: `basic_test.dart` `'Row' > 'baseline aligned children
/// account for a larger, no-baseline child size'` (3.44.0), a regression test
/// for flutter/flutter#58898. The oracle's third child is a `FlutterLogo(size:
/// 250)`; a childless 250×250 `SizedBox` stands in — see this file's module
/// doc.
#[test]
fn row_baseline_grows_to_a_taller_child_that_has_no_baseline() {
    let laid = harness::pump_widget(
        baseline_row(vec![
            text("big text", BIG_FONT).boxed(),
            text(SEVEN_LINES, SMALL_FONT).boxed(),
            SizedBox::new(250.0, 250.0).boxed(),
        ]),
        loose(1000.0),
    );

    let (big_ascent, _) = ascent_descent(&laid, "big text");
    let (lines_ascent, lines_descent) = ascent_descent(&laid, SEVEN_LINES);

    assert!(
        big_ascent + lines_descent < 250.0,
        "premise: the baseline stack ({}) must be shorter than the 250px \
         no-baseline child, or the case would not distinguish the two",
        big_ascent + lines_descent,
    );

    let row = laid.find_by_render_type("RenderFlex");
    let big = laid.find_text("big text").expect("mounted");
    let lines = laid.find_text(SEVEN_LINES).expect("mounted");

    assert!(laid.size(row).height.get() > laid.size(big).height.get());
    assert!(laid.size(row).height.get() > laid.size(lines).height.get());
    assert_eq!(
        laid.size(row).height.get(),
        250.0,
        "the no-baseline child outgrows the baseline stack and sets the height",
    );

    assert_eq!(
        laid.absolute_offset(big).dy.get(),
        0.0,
        "the deepest-ascent child still defines the common baseline",
    );
    assert_eq!(
        laid.absolute_offset(lines).dy.get(),
        big_ascent - lines_ascent,
        "the baseline-aligned pair keeps its relative alignment",
    );
}

/// Control for the `IgnoreBaseline` pair: with both children reporting a
/// baseline, the shallower one is pushed down until the two baselines meet.
///
/// Flutter parity: `basic_test.dart` `'Row and IgnoreBaseline (control --
/// with baseline)'` (3.44.0), whose `expect(bPos.dy, 96.0 - 24.0)` is the
/// `FlutterTest` font's ascent difference for 128px and 32px text — measured
/// here rather than hard-coded, see this file's module doc.
#[test]
fn row_baseline_shifts_the_shallower_child_down_to_the_common_baseline() {
    let laid = harness::pump_widget(
        baseline_row(vec![text("a", 128.0).boxed(), text("b", 32.0).boxed()]),
        loose(1000.0),
    );

    let (a_ascent, _) = ascent_descent(&laid, "a");
    let (b_ascent, _) = ascent_descent(&laid, "b");
    assert!(
        a_ascent > b_ascent,
        "premise: 128px text must sit higher above the baseline than 32px text",
    );

    let a = laid.find_text("a").expect("mounted");
    let b = laid.find_text("b").expect("mounted");
    assert_eq!(
        laid.absolute_offset(a).dy.get(),
        0.0,
        "the 128px text defines the common baseline and stays at the top",
    );
    assert_eq!(
        laid.absolute_offset(b).dy.get(),
        a_ascent - b_ascent,
        "the 32px text drops by the ascent difference to meet that baseline",
    );
}

/// Wrapping the deep-ascent child in `IgnoreBaseline` removes it from the
/// row's baseline computation: it is no longer what the other child aligns to,
/// so both children end up flush at the top.
///
/// Flutter parity: `basic_test.dart` `'Row and IgnoreBaseline (with ignored
/// baseline)'` (3.44.0) — `expect(aPos.dy, 0.0); expect(bPos.dy, 0.0);`, both
/// exact and font-independent, ported verbatim.
#[test]
fn ignore_baseline_keeps_its_child_out_of_the_row_s_baseline_alignment() {
    let laid = harness::pump_widget(
        baseline_row(vec![
            IgnoreBaseline::new().child(text("a", 128.0)).boxed(),
            text("b", 32.0).boxed(),
        ]),
        loose(1000.0),
    );

    let a = laid.find_text("a").expect("mounted");
    let b = laid.find_text("b").expect("mounted");
    assert_eq!(
        laid.absolute_offset(a).dy.get(),
        0.0,
        "the ignored child is not baseline-aligned — it sits at the cross start",
    );
    assert_eq!(
        laid.absolute_offset(b).dy.get(),
        0.0,
        "with the only other baseline hidden, the remaining child defines the \
         common baseline and is not shifted either — the control case drops it \
         instead",
    );
}
