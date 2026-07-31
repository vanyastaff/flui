//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/rich_text_test.dart`
//! (tag `3.44.0`, 5 `testWidgets`/`test` cases —
//! `git show 3.44.0:packages/flutter/test/widgets/rich_text_test.dart`).
//!
//! Widget → render-object mapping: `RichText` (`widgets/basic.dart`) →
//! `RenderParagraph` (`rendering/paragraph.dart`), mirrored here by
//! [`flui_widgets::RichText`] (`crates/flui-widgets/src/text/rich_text.rs`) →
//! [`flui_objects::RenderParagraph`]
//! (`crates/flui-objects/src/text/paragraph.rs`). Layout-level span-tree
//! composition (multi-style runs, `max_lines`, child-span styling reaching
//! the shaped run) is already covered by `tests/rich_text.rs` (non-parity)
//! and `tests/parity/text_test.rs` — this file does not repeat that
//! coverage; it exists to account for the 5 upstream cases specifically.
//!
//! ## Case ledger (5 of 5 accounted)
//!
//! 1. `'RichText with recognizers without handlers does not throw'` —
//!    **out of scope**, two independent missing capabilities. First,
//!    FLUI's `TextSpan` has no recognizer abstraction at all — its only
//!    interaction hook is `on_tap: Option<Arc<dyn Fn() + Send + Sync>>`
//!    (`crates/flui-types/src/typography/text_spans.rs:142-161`), so "a
//!    recognizer with no handler attached" (the oracle's actual crash risk:
//!    Flutter's semantics-building code once null-checked a recognizer's
//!    callback) has no analogue to construct — `on_tap: None` is already
//!    the default, ordinary case, not a distinguishable "recognizer w/o
//!    handler" scenario. Second, and independently sufficient: the
//!    assertion under test is a semantics-tree shape
//!    (`matchesSemantics(children: [...])`), and `RenderParagraph` builds
//!    no semantics node at all — it does not override
//!    `describe_semantics_configuration` (default no-op at
//!    `crates/flui-rendering/src/traits/render_object.rs:633`; confirmed by
//!    `grep -n describe_semantics_configuration
//!    crates/flui-objects/src/text/paragraph.rs` → no matches), matching
//!    that render object's own module doc, which lists "text selection,
//!    semantics" as explicitly out of scope
//!    (`crates/flui-objects/src/text/paragraph.rs:10-13`). `TextSpan` does
//!    carry a `semantics_label` field and a `has_semantics()` query
//!    (`text_spans.rs:150`, `:360-365`), but neither is read anywhere
//!    outside `flui-types` itself (`grep -rn has_semantics
//!    crates/flui-objects crates/flui-rendering crates/flui-painting` →
//!    no matches) — the data exists, nothing consumes it. Even setting
//!    aside both gaps, the widget-level test harness this crate's parity
//!    suite runs on (`tests/common/mod.rs`, `tests/parity/harness.rs`) has
//!    no semantics-tree query surface at all — the only semantics-assembly
//!    driver in the workspace is `flui-rendering`'s own
//!    `RenderTester::run_to_semantics`
//!    (`crates/flui-rendering/tests/semantics_assembly.rs`), exercised
//!    there against hand-built test-only render objects, not reachable
//!    from a widget mounted through this crate's harness.
//! 2. `'TextSpan Locale works'` — **out of scope**: `TextSpan` has no
//!    `locale` field (full field list at `text_spans.rs:142-161`: `text`,
//!    `style`, `children`, `semantics_label`, `mouse_cursor`, `on_tap` —
//!    no locale). Blocked independently of the semantics gap in case 1
//!    (which also applies, since the oracle's assertion is a semantics
//!    `attributedLabel`).
//! 3. `'TextSpan spellOut works'` — **out of scope**: `TextSpan` has no
//!    `spell_out` field (same field list as case 2). Blocked independently
//!    of, and in addition to, the semantics gap in case 1.
//! 4. `'WidgetSpan calculate correct intrinsic heights'` — **out of
//!    scope**: FLUI's placeholder-span type (`PlaceholderSpan`,
//!    `text_spans.rs:441-452`) carries a *fixed* `width`/`height`/
//!    `alignment`/`baseline` — data only, no embedded widget reference —
//!    unlike Flutter's `WidgetSpan`, which carries a real `Widget child`
//!    that `RenderParagraph` lays out as an inline child render object.
//!    `RenderParagraph`'s own module doc lists "inline `WidgetSpan`
//!    children" as explicitly out of scope
//!    (`crates/flui-objects/src/text/paragraph.rs:10-13`), and `RichText`
//!    itself never has render-tree children — `has_children()` always
//!    returns `false` (`tests/rich_text.rs`'s
//!    `has_children_is_always_false`). There is no way to embed a real
//!    widget inside a paragraph to build the oracle's mixed
//!    text/`WidgetSpan`/text tree at all, so there is no observable to
//!    assert `IntrinsicHeight` against — inventing a text-only intrinsic
//!    height case here would not be porting this oracle case, it would be
//!    a different, unrelated assertion.
//! 5. `'RichText implements debugFillProperties'` — **ported at contract
//!    level**, narrowed to the 4 of 11 oracle sub-assertions FLUI's
//!    `RichText` actually has fields for. `View` has no widget-level
//!    diagnostics of its own — only render objects implement
//!    `Diagnosticable` (`grep -rn 'impl.*Diagnosticable for'
//!    crates/flui-widgets/src` → no matches; the same standing gap
//!    `colored_box_test.rs` and `unconstrained_box_test.rs` already
//!    document and work around) — so this pumps the widget live and reads
//!    its mounted `RenderParagraph`'s diagnostics instead, the same
//!    substitution those two files use. `RichText`'s own field list
//!    (`crates/flui-widgets/src/text/rich_text.rs:30-35`) is `text`,
//!    `align`, `direction`, `max_lines` — a strict subset of the oracle's
//!    input (`softWrap`, `overflow`, `textScaleFactor`, `locale`,
//!    `strutStyle`, `textWidthBasis`, `textHeightBehavior` have no builder
//!    on FLUI's `RichText` at all, so those 7 sub-assertions cannot be
//!    constructed). The 4 that map — `textAlign: center` → `text_align:
//!    Center`, `textDirection: rtl` → `text_direction: Rtl`, `maxLines: 1`
//!    → `max_lines: 1`, and the span's own text ('rich text') — are ported
//!    as real, falsifiable equality assertions against
//!    `RenderParagraph::debug_fill_properties`'s emitted property values:
//!    [`rich_text_diagnostics_forwards_text_align_direction_max_lines_and_text_to_the_render_object`].
//!
//! **Total: 5 of 5 cases accounted. 1 ported at contract level (narrowed to
//! 4 of 11 sub-assertions), 4 out of scope by missing capability (no
//! recognizer type, no semantics description on `RenderParagraph`, no
//! widget-level semantics query surface in this crate's harness, no
//! `locale`/`spell_out` fields on `TextSpan`, no embeddable-widget inline
//! span). No test asserts FLUI's current behaviour as if it matched the
//! oracle where it does not — every gap is named, not papered over.**

use flui_foundation::DiagnosticsNode;
use flui_rendering::testing::inspect::render_diagnostics;
use flui_types::typography::{TextAlign, TextDirection, TextSpan};
use flui_widgets::RichText;

use crate::harness;

/// The mounted `RichText`'s own `RenderParagraph` diagnostics node.
fn render_node(laid: &crate::common::LaidOut) -> DiagnosticsNode {
    let owner = laid.pipeline_owner();
    let owner = owner.read();
    render_diagnostics(&owner)
        .find_descendant_unique("RenderParagraph")
        .expect("RichText must mount exactly one RenderParagraph")
        .clone()
}

/// `RichText`'s `align`, `direction`, `max_lines`, and text content reach
/// the mounted `RenderParagraph`'s diagnostics.
///
/// Flutter parity: `rich_text_test.dart` `'RichText implements
/// debugFillProperties'` (3.44.0) — ported at contract level to the 4 of 11
/// sub-assertions FLUI's `RichText` has fields for; see this file's module
/// doc for why the other 7 (`softWrap`, `overflow`, `textScaleFactor`,
/// `locale`, `strutStyle`, `textWidthBasis`, `textHeightBehavior`) have no
/// input to construct. Not Dart's exact string formatting (`Alignment`-style
/// `toString`) — the same contract-level substitution
/// `unconstrained_box_test.rs`/`colored_box_test.rs` already establish for
/// widgets that have no diagnostics of their own.
#[test]
fn rich_text_diagnostics_forwards_text_align_direction_max_lines_and_text_to_the_render_object() {
    let laid = harness::pump_widget(
        RichText::new(TextSpan::new("rich text"))
            .align(TextAlign::Center)
            .direction(TextDirection::Rtl)
            .max_lines(1),
        harness::screen(),
    );

    let node = render_node(&laid);

    let text_align = node
        .find_property("text_align")
        .expect("RenderParagraph emits a text_align property");
    assert_eq!(
        text_align.value(),
        format!("{:?}", TextAlign::Center),
        "text_align must carry the configured Center alignment"
    );

    let text_direction = node
        .find_property("text_direction")
        .expect("RenderParagraph emits a text_direction property");
    assert_eq!(
        text_direction.value(),
        format!("{:?}", TextDirection::Rtl),
        "text_direction must carry the configured Rtl direction"
    );

    let max_lines = node
        .find_property("max_lines")
        .expect("RenderParagraph emits a max_lines property");
    assert_eq!(
        max_lines.value(),
        "1",
        "max_lines must carry the configured cap of 1"
    );

    let text = node
        .find_property("text")
        .expect("RenderParagraph emits a text property");
    assert_eq!(
        text.value(),
        "rich text",
        "text must carry the span's plain-text content"
    );
}
