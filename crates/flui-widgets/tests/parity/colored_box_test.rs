//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/basic_test.dart` (tag
//! `3.44.0`), the `'ColoredBox'` group — 8 `testWidgets` cases — plus one
//! bonus case from the same file's top-level test list (see the closing
//! note below).
//!
//! Widget → render-object mapping: `ColoredBox` → [`flui_objects::RenderDecoratedBox`]
//! (`crates/flui-widgets/src/paint/colored_box.rs`). FLUI does not carry a
//! dedicated `RenderColoredBox` on this path (a same-named type exists in
//! `flui_objects` but is a standalone demo/harness leaf object, unrelated to
//! the `ColoredBox` *widget*) — `ColoredBox` realizes as a `RenderDecoratedBox`
//! wrapping a color-only `BoxDecoration`, per `colored_box.rs`'s own module
//! doc.
//!
//! Harness-capability divergence (affects cases 1-4): every one of the 4
//! layout cases in the oracle (`basic_test.dart:1065-1137` at the tag)
//! asserts ONLY paint behavior — `mockCanvas.rects`/`mockCanvas.paints`/
//! `mockContext.children`/`mockContext.offsets`, recorded through
//! `_MockCanvas`/`_MockPaintingContext` fakes. **None of the 4 asserts a
//! size anywhere.** `LaidOut` (`crates/flui-widgets/tests/common/mod.rs`)
//! has no paint-recording capability at all — the same standing gap
//! `row_test.rs`'s module doc documents for paint *order*, here extended to
//! paint *occurrence*. Each of the 4 layout cases below is therefore ported
//! as a SIZE assertion on the `ColoredBox`'s own laid-out geometry under the
//! same scenario (parent constraints × childless/child) — a genuine but
//! **partial** substitution: it pins the layout half of each case (a
//! quantity the oracle itself never checks), not a subset of what the
//! oracle already asserts. The paint half of each case is accounted
//! separately, per row below: cases 3-4's fill-at-size-and-color half is
//! already pinned at render level by existing tests; cases 1-2's
//! zero-size-paint-*skip* half is a genuine, previously-undocumented
//! FLUI/Flutter divergence, now pinned by two new render-level tests (see
//! "Cases 1-2" below).
//!
//! ## Ledger (8 cases)
//! 1. `'ColoredBox - no size, no child'` — **ported in part (layout half)**:
//!    [`colored_box_no_size_no_child_collapses_to_zero`]. Paint half:
//!    divergence, see "Cases 1-2" below.
//! 2. `'ColoredBox - no size, child'` — **ported in part (layout half)**:
//!    [`colored_box_no_size_with_child_collapses_to_zero_but_still_lays_out_the_child`].
//!    Paint half: divergence, see "Cases 1-2" below.
//! 3. `'ColoredBox - size, no child'` — **ported in part (layout half)**:
//!    [`colored_box_with_size_no_child_fills_the_screen`]. Paint half
//!    (fill rect at the box's own size, correct color) already pinned at
//!    render level by `harness_decorated_box_paints_background_before_child`
//!    (`crates/flui-objects/tests/render_object_harness.rs:3324`).
//! 4. `'ColoredBox - size, child'` — **ported in part (layout half)**:
//!    [`colored_box_with_size_and_child_adopts_the_size_of_its_child`]. Paint
//!    half same as case 3, cross-checked against the `Foreground`-position
//!    sibling `harness_decorated_box_foreground_paints_after_child`
//!    (`crates/flui-objects/tests/render_object_harness.rs:3368`).
//! 5. `'ColoredBox - debugFillProperties'` — **ported** (contract, not Dart
//!    formatting — see doc comment on the test itself):
//!    [`colored_box_debug_fill_properties_carries_the_painted_color`].
//! 6. `'ColoredBox - default isAntiAlias'` — **out of scope**, missing knob
//!    (see below).
//! 7. `'ColoredBox - passing isAntiAlias = false'` — **out of scope**,
//!    missing knob (see below).
//! 8. `'ColoredBox golden test - anti-aliasing and rotation variations'` —
//!    **out of scope**, golden. FLUI has no golden-file harness (the same
//!    standing gap `physical_model_test.rs`/`wrap_test.rs`/`container_test.rs`
//!    already cite).
//!
//! **Total: 8 case names = 4 ported in part (layout half only; paint half
//! separately accounted per row) + 1 ported in full (case 5) + 3 out of
//! scope (2 missing-knob + 1 golden) — no narrowing: every oracle assertion
//! is either ported, cross-referenced to an existing/new pin, or named as a
//! divergence.**
//!
//! ### Cases 1-2: the zero-size paint skip
//!
//! Flutter's `ColoredBox` (`_RenderColoredBox.paint`, `widgets/basic.dart`,
//! tag `3.44.0`) guards its `Canvas.drawRect` call with `if (size >
//! Size.zero)` — a zero-size `ColoredBox` never issues a fill command at
//! all, which is exactly what cases 1-2's `mockCanvas.rects, isEmpty` /
//! `mockCanvas.paints, isEmpty` assertions pin. FLUI matches this since
//! 2026-09-04; it did not before, and the divergence was real though
//! invisible (a zero-area fill draws nothing either way, but a caller
//! reading the display list saw a command that should not be there). This
//! cannot be pinned through `LaidOut` (no
//! paint-recording capability — see the divergence note above), so it is
//! pinned instead at render level in
//! `crates/flui-objects/tests/render_object_harness.rs`:
//! `harness_decorated_box_skips_the_fill_rect_at_zero_size_like_flutter`,
//! green since 2026-09-04. The guard went on the FILL rather than on a
//! caller — `paint_box_decoration` records no background when either
//! dimension is zero — so `ColoredBox` matches the oracle exactly while
//! `DecoratedBox`'s border, shadow and image passes still run on a
//! degenerate box.
//!
//! ### Cases 3-4: the anti-alias knob
//!
//! `ColoredBox::anti_alias(bool)` (default `true`) landed 2026-09-04 and both
//! oracle cases are ported below. The flag lives on `RenderDecoratedBox` and
//! travels through `DecorationPaintOptions`, not on `BoxDecoration` — a
//! decoration describes an appearance and is serializable, while
//! anti-aliasing is a rasterization hint. It reaches the BACKGROUND fill
//! only; a `ColoredBox` has no border, shadow or image, so nothing in the
//! oracle says what those should do when it is off.
//!
//! ### Cases 6-7: missing anti-alias knob
//!
//! Neither the `ColoredBox` widget, `BoxDecoration`, nor `RenderDecoratedBox`
//! carries an anti-alias field: `ColoredBox::new` takes only a `color`
//! (no `isAntiAlias` parameter to construct case 7's `isAntiAlias: false`
//! at all), `BoxDecoration` (`crates/flui-types/src/styling/decoration.rs`)
//! has no such field, and `paint_box_decoration`
//! (`crates/flui-painting/src/decoration.rs`) always fills via
//! `Paint::fill(color)`, which hardcodes `anti_alias: true` with no
//! override — so even case 6 (asserting the always-true default) has
//! nothing genuinely configurable to observe: there is no live-paint
//! capture in this harness either (see the divergence note above), so the
//! only way to "assert" it would be to read the hardcoded literal back,
//! which proves nothing. The painting model itself is not the blocker —
//! `flui_types::painting::Paint::anti_alias` exists and rides on every
//! `DrawCommand::DrawRect`/`DrawRRect`/etc. variant
//! (`crates/flui-types/src/painting/paint.rs`) — this is a missing
//! wiring/knob at the `ColoredBox`/`BoxDecoration` layer, not an absent
//! painting capability. Filed as a new `docs/ROADMAP.md` Cross.H entry
//! (search `ColoredBox` there).
//!
//! ### Bonus case (adjacent group, same oracle file): `'Wrap implements
//! debugFillProperties'`
//!
//! **Out of scope**, not trivially portable. The oracle expects 8 diagnostics
//! substrings (`direction`, `alignment`, `spacing`, `runAlignment`,
//! `runSpacing`, `crossAxisAlignment`, `textDirection`, `verticalDirection`).
//! `RenderWrap::debug_fill_properties` (`crates/flui-objects/src/layout/wrap.rs`)
//! emits the first 6 but has no `text_direction`/`vertical_direction`
//! properties at all — `wrap_test.rs`'s own module doc already documents this
//! exact gap (`RenderWrap` "has no `TextDirection`/`VerticalDirection` field
//! at all and always lays out LTR/TTB"), filed under the existing
//! `docs/ROADMAP.md` Cross.H entry for that widget. Porting only the 6
//! available substrings would silently narrow the oracle's 8-property
//! assertion, so this case is accounted OOS here rather than narrowed.

use flui_foundation::{DiagnosticsNode, RenderId};
use flui_rendering::testing::inspect::render_diagnostics;
use flui_types::Color;
use flui_view::ViewExt;
use flui_widgets::{ColoredBox, Row, SizedBox};

use crate::common::{LaidOut, size};
use crate::harness;

/// Flutter's `colorToPaint` constant (`Color(0xFFABCDEF)`, `basic_test.dart`).
const COLOR_TO_PAINT: Color = Color::from_argb(0xFFAB_CDEF);

/// The mounted `ColoredBox`'s own `RenderDecoratedBox` node.
///
/// Every case in this file mounts exactly one `ColoredBox`, so this is
/// always unambiguous — mirrors `container_test.rs`'s
/// `find_by_render_type("RenderDecoratedBox")` usage for the same widget
/// mapping.
fn colored_box_node(laid: &LaidOut) -> RenderId {
    laid.find_by_render_type("RenderDecoratedBox")
}

/// A childless `ColoredBox` squeezed to zero by an ancestor `SizedBox.shrink`
/// reports zero size.
///
/// Flutter parity: `basic_test.dart` `'ColoredBox - no size, no child'`
/// (3.44.0) — oracle tree `Flex(horizontal, ltr) → SizedBox.shrink →
/// ColoredBox`. `Row` is FLUI's `Flex(direction: horizontal)` (parity
/// established by `row_test.rs`); no ambient `Directionality` is required
/// (see `grid_view_test.rs`'s equivalent divergence note). **Layout half
/// only** — the oracle itself asserts no size anywhere, only that
/// `mockCanvas.rects`/`mockCanvas.paints` stay empty (the zero-size box
/// never paints its fill). That paint-skip half is a genuine FLUI/Flutter
/// divergence, not ported here — see this file's module doc, "Cases 1-2:
/// zero-size paint-skip divergence".
#[test]
fn colored_box_no_size_no_child_collapses_to_zero() {
    let laid = harness::pump_widget(
        Row::new(vec![
            SizedBox::shrink()
                .child(ColoredBox::new(COLOR_TO_PAINT))
                .boxed(),
        ]),
        harness::screen(),
    );

    assert_eq!(
        laid.size(colored_box_node(&laid)),
        size(0.0, 0.0),
        "a childless ColoredBox squeezed to zero by SizedBox.shrink must \
         report zero size"
    );
}

/// A `ColoredBox` squeezed to zero still mounts and lays out its child —
/// the child's own layout is independent of whatever the ColoredBox's own
/// fill paint does.
///
/// Flutter parity: `basic_test.dart` `'ColoredBox - no size, child'`
/// (3.44.0) — oracle tree `Flex(horizontal, ltr) → SizedBox.shrink →
/// ColoredBox(child: SizedBox.expand)`. **Layout half only** — the oracle's
/// own assertions are entirely paint-level: `mockCanvas.rects`/`paints` stay
/// empty (the zero-size box itself never paints its fill — the same
/// paint-skip divergence as case 1, see this file's module doc) and
/// `mockContext.children.single`/`offsets.single` pin that the child still
/// receives a `paintChild` call. This test instead pins the child's own
/// laid-out size (also zero, squeezed by the same zero-tight constraints) —
/// a layout-level stand-in proving the child is genuinely mounted and laid
/// out, not a port of the oracle's `paintChild`-call assertion itself (no
/// paint-recording capability exists at this harness level either — see the
/// module doc).
#[test]
fn colored_box_no_size_with_child_collapses_to_zero_but_still_lays_out_the_child() {
    let laid = harness::pump_widget(
        Row::new(vec![
            SizedBox::shrink()
                .child(ColoredBox::new(COLOR_TO_PAINT).child(SizedBox::expand()))
                .boxed(),
        ]),
        harness::screen(),
    );

    let colored_box = colored_box_node(&laid);
    assert_eq!(
        laid.size(colored_box),
        size(0.0, 0.0),
        "a ColoredBox squeezed to zero by SizedBox.shrink must report zero \
         size even when it has a child"
    );
    assert_eq!(
        laid.size(laid.only_child(colored_box)),
        size(0.0, 0.0),
        "the child must still mount and lay out (squeezed to zero by the \
         same tight constraints), independent of the parent's paint-skip"
    );
}

/// A childless `ColoredBox` pumped directly as the tree root fills the
/// entire (tight) test screen.
///
/// Flutter parity: `basic_test.dart` `'ColoredBox - size, no child'`
/// (3.44.0) — oracle asserts `mockCanvas.rects.single ==
/// Rect.fromLTWH(0, 0, 800, 600)` and `mockCanvas.paints.single.color ==
/// colorToPaint`, a paint-level assertion this test does not reproduce.
/// **Layout half only**: this pins the ColoredBox's own laid-out size (a
/// quantity the oracle never asserts). The paint half — a fill rect at the
/// box's own committed size, in the given color — is already pinned at
/// render level by
/// `harness_decorated_box_paints_background_before_child`
/// (`crates/flui-objects/tests/render_object_harness.rs:3324`), which
/// exercises the identical `RenderDecoratedBox` → `paint_box_decoration`
/// path this widget wires into.
#[test]
fn colored_box_with_size_no_child_fills_the_screen() {
    let laid = harness::pump_widget(ColoredBox::new(COLOR_TO_PAINT), harness::screen());

    assert_eq!(
        laid.size(colored_box_node(&laid)),
        size(800.0, 600.0),
        "a childless ColoredBox under the tight 800x600 test screen must \
         fill it"
    );
}

/// A `ColoredBox` with a child pumped directly as the tree root adopts the
/// child's size (which itself fills the tight test screen).
///
/// Flutter parity: `basic_test.dart` `'ColoredBox - size, child'` (3.44.0)
/// — same paint-level `Rect.fromLTWH(0, 0, 800, 600)` +
/// `paints.single.color` assertion as case 3, now with a `SizedBox.expand`
/// child present. **Layout half only**, same caveat as case 3: this pins
/// size, not paint. The paint half is cross-checked against the
/// `DecorationPosition::Foreground` sibling
/// `harness_decorated_box_foreground_paints_after_child`
/// (`crates/flui-objects/tests/render_object_harness.rs:3368`) — the same
/// fill-rect-with-color contract, exercised with a child present, as this
/// case has.
#[test]
fn colored_box_with_size_and_child_adopts_the_size_of_its_child() {
    let laid = harness::pump_widget(
        ColoredBox::new(COLOR_TO_PAINT).child(SizedBox::expand()),
        harness::screen(),
    );

    let colored_box = colored_box_node(&laid);
    assert_eq!(
        laid.size(colored_box),
        size(800.0, 600.0),
        "a ColoredBox with a child must adopt the child's size"
    );
    assert_eq!(
        laid.size(laid.only_child(colored_box)),
        size(800.0, 600.0),
        "the child itself must fill the tight 800x600 test screen"
    );
}

/// `ColoredBox`'s diagnostics carry the painted color through to the
/// mounted render object.
///
/// Flutter parity: `basic_test.dart` `'ColoredBox - debugFillProperties'`
/// (3.44.0) — the oracle instantiates `ColoredBox` directly (no pump) and
/// asserts `properties.properties.first.value == colorToPaint` (a typed
/// `Color` equality on the WIDGET's own `debugFillProperties`). FLUI's
/// `View` has no widget-level diagnostics of its own — only render objects
/// implement `Diagnosticable` — so this pumps the widget live and reads its
/// mounted `RenderDecoratedBox`'s diagnostics instead, checking the same
/// two-part contract the oracle checks (first property, correct color):
/// `RenderDecoratedBox::debug_fill_properties` emits `"decoration"` first
/// (a whole `BoxDecoration` debug dump, since FLUI realizes `ColoredBox` as
/// a color-only `DecoratedBox` rather than a dedicated bare-color render
/// object — see this file's module doc), so the port asserts that first
/// property's STRING value contains the color's own `{:?}` substring rather
/// than pinning Dart's `Color(0xffabcdef)`-style formatting, which FLUI's
/// diagnostics do not produce.
#[test]
fn colored_box_debug_fill_properties_carries_the_painted_color() {
    let laid = harness::pump_widget(ColoredBox::new(COLOR_TO_PAINT), harness::screen());

    let node: DiagnosticsNode = laid.pipeline_owner().with(|owner| {
        render_diagnostics(owner)
            .find_descendant_unique("RenderDecoratedBox")
            .expect("ColoredBox mounts exactly one RenderDecoratedBox")
            .clone()
    });

    let first = node
        .properties()
        .first()
        .expect("RenderDecoratedBox emits at least one diagnostics property");
    assert_eq!(
        first.name(),
        "decoration",
        "the first diagnostics property must be the decoration ColoredBox \
         built (color-only BoxDecoration)"
    );
    assert!(
        first.value().contains(&format!("{COLOR_TO_PAINT:?}")),
        "the decoration property must carry the painted color; got {:?}",
        first.value()
    );
}

/// The widget's `anti_alias` reaches the recorded paint, and defaults to on.
///
/// Flutter parity: `basic_test.dart`'s `'ColoredBox - default isAntiAlias'`
/// and `'ColoredBox - passing isAntiAlias = false'`, which assert
/// `mockCanvas.paints.single.isAntiAlias` after driving `paint` directly.
/// Here the assertion reads the composited layer tree's own `DrawRect`
/// instead of a mock canvas — the same value, one layer further out, and it
/// covers the widget→render-object wiring the render-level pin cannot see.
#[test]
fn colored_box_anti_alias_defaults_on_and_reaches_the_recorded_paint() {
    use flui_painting::DrawCommand;
    use flui_rendering::layer::Layer;

    fn recorded_anti_alias(laid: &LaidOut) -> Vec<bool> {
        let mut flags = Vec::new();
        let Some(tree) = laid.layer_tree() else {
            return flags;
        };
        let Some(root) = tree.root() else {
            return flags;
        };
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(layer) = tree.get_layer(id) else {
                continue;
            };
            let commands = match layer {
                Layer::Picture(picture) => Some(picture.picture()),
                Layer::Canvas(canvas) => Some(canvas.display_list()),
                _ => None,
            };
            if let Some(commands) = commands {
                for command in commands {
                    if let DrawCommand::DrawRect { paint, .. } = command {
                        flags.push(paint.anti_alias);
                    }
                }
            }
            stack.extend(tree.children(id).iter().flat_map(|ids| ids.iter().copied()));
        }
        flags
    }

    let laid = harness::pump_widget(
        SizedBox::new(40.0, 20.0).child(ColoredBox::new(Color::RED)),
        harness::screen(),
    );
    assert_eq!(
        recorded_anti_alias(&laid),
        vec![true],
        "the default matches the oracle's `isAntiAlias: true`",
    );

    let laid = harness::pump_widget(
        SizedBox::new(40.0, 20.0).child(ColoredBox::new(Color::RED).anti_alias(false)),
        harness::screen(),
    );
    assert_eq!(
        recorded_anti_alias(&laid),
        vec![false],
        "and `anti_alias(false)` reaches the paint the layer tree records",
    );
}
