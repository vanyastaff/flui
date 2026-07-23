//! `Chip`/`FilterChip` widget-level mount/interaction coverage.
//!
//! Complements `chip.rs`'s own unit tests (M3 default token-table probes,
//! `chip_states`'s pure-query regression, painter geometry pins) with
//! end-to-end mount/dispatch proof this crate's own conventions require for
//! every interactive component (see `tests/checkbox.rs`/`tests/ink_well.rs`/
//! `tests/elevated_button.rs`): a real down+up reaches
//! [`Chip::on_pressed`]/[`FilterChip::on_selected`], a themed override
//! actually reaches the mounted render tree (not just a cascade function
//! evaluated in isolation), and — the one genuinely load-bearing claim
//! `chip.rs`'s own module docs make without previously proving it — that
//! the delete icon's small nested `InkWell` and the chip's own outer
//! `InkWell` route a tap to exactly one of them, never both, depending on
//! where the pointer lands. That claim rests on the same "most specific
//! first" nested-detector resolution
//! `crates/flui-widgets/tests/gesture_detector_advanced.rs`'s
//! `overlapping_detectors_quick_tap_resolves_to_the_inner_tap` establishes
//! for a tap-vs-long-press pair; this file is the first place it is proven
//! for two plain, same-gesture-type taps at genuinely disjoint (not fully
//! overlapping) sub-regions.
//!
//! **Not covered here** (see `chip.rs`'s own unit tests instead, since
//! neither needs a render tree): the M3 default token-table branch
//! order/combined-state pins, `chip_states`'s pure-query shape, and
//! `ChipBorderPainter`/`ChipCheckmarkPainter`'s own paint-invocation/
//! geometry proofs (real `Canvas`/`DisplayList` recordings).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, loose};
use flui_material::chip::CHIP_ICON_SIZE;
use flui_material::{Chip, ChipThemeData, FilterChip, Theme, ThemeData, ThemeDataOverrides};
use flui_types::Color;
use flui_widgets::Text;

/// `_ChipDefaultsM3`/`_FilterChipDefaultsM3.padding` (`chip.dart`/
/// `filter_chip.dart`, oracle tag `3.44.0`, `EdgeInsets.all(8.0)`) — a
/// local re-citation of the oracle constant `chip.rs`'s own `PADDING` is
/// (correctly) private, needed here only to locate the delete icon's
/// rendered position from the outside.
const CONTAINER_PADDING: f32 = 8.0;

/// Every `Chip`/`FilterChip` needs a [`Theme`] ancestor (`Theme::of` panics
/// without one) — mirrors `tests/checkbox.rs`'s own `themed` helper.
fn themed(child: impl flui_view::IntoView) -> Theme {
    Theme::new(ThemeData::light(), child)
}

/// `_ChipDefaultsM3`/`_FilterChipDefaultsM3`'s formatted `Debug` string for
/// a given resolved [`Color`] — what `RenderPhysicalShape`'s
/// `Diagnosticable::debug_fill_properties` writes into its `"color"`
/// property, matching `tests/elevated_button.rs`'s own `color_property`
/// helper.
fn color_property(color: Color) -> String {
    format!("{color:?}")
}

/// The mounted chip's overall container size — the outermost
/// `RenderSemanticsAnnotations` node, which sizes to its `CustomPaint`/
/// `Material`/content chain in full (mirrors `tests/checkbox.rs`'s own
/// semantics-node-as-container-size assertion).
fn container_size(laid: &common::LaidOut) -> flui_types::Size {
    let semantics = laid
        .find_by_render_type("RenderSemanticsAnnotations")
        .expect("Chip/FilterChip must mount a Semantics wrapper");
    laid.size(semantics)
}

/// The approximate on-screen center of the delete icon in a mounted chip
/// with no avatar: the icon sits flush against the container's right edge,
/// inset only by the container's own outer padding.
fn delete_icon_center(laid: &common::LaidOut) -> (f32, f32) {
    let size = container_size(laid);
    (
        size.width.get() - CONTAINER_PADDING - CHIP_ICON_SIZE / 2.0,
        size.height.get() / 2.0,
    )
}

/// A point safely inside the chip's own tap target but well clear of the
/// delete icon's small box at the right edge — near the left edge, inside
/// the outlined border.
fn chip_only_point() -> (f32, f32) {
    (CONTAINER_PADDING + 2.0, 16.0)
}

// ------------------------------------------------------------------
// (a) Tap dispatch reaches the widget's own callback.
// ------------------------------------------------------------------

#[test]
fn tap_fires_on_pressed_for_a_pressable_chip() {
    let taps = Rc::new(RefCell::new(0_u32));
    let counter = Rc::clone(&taps);
    let laid = lay_out(
        themed(Chip::new(Text::new("Tag")).on_pressed(move || {
            *counter.borrow_mut() += 1;
        })),
        loose(300.0),
    );

    let (x, y) = chip_only_point();
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    assert_eq!(
        *taps.borrow(),
        1,
        "a tap on a pressable chip must fire on_pressed"
    );
}

#[test]
fn tap_fires_on_selected_with_the_flipped_value_for_a_filter_chip() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(
            FilterChip::new(Text::new("Vegetarian"))
                .selected(false)
                .on_selected(move |next| {
                    *recorder.borrow_mut() = Some(next);
                }),
        ),
        loose(300.0),
    );

    let (x, y) = chip_only_point();
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    assert_eq!(
        *observed.borrow(),
        Some(true),
        "a tap on an unselected, enabled FilterChip must fire on_selected(true)",
    );
}

// ------------------------------------------------------------------
// (b) Delete-icon vs. chip-body tap separation — nested InkWells.
// ------------------------------------------------------------------

/// Mutation-run: dropping `chip.rs`'s `wrap_local_gesture_arena` call (so
/// `Chip::build` returns its `Semantics` tree directly, with no local
/// `GestureArenaScope`) was confirmed to make this test fail —
/// `presses.borrow()` reads `1` instead of the expected `0`, meaning BOTH
/// the delete button's `InkWell` and the chip body's own `InkWell` fired
/// independently for the same contact. That is not a hypothetical: it is
/// the exact `GestureDetector` "standalone" fallback (no ambient
/// `GestureArenaScope` above it, so each detector closes its own private
/// arena — see `GestureDetector`'s own module docs, "Arena acquisition")
/// that this crate's plain `common::lay_out` harness exercises with no
/// scope wrapper, and — confirmed by grepping the workspace — that no
/// `flui-app`/binding-level ancestor installs anywhere either, so this was
/// a real, previously unverified double-fire defect for any nested tap
/// target, not a test-harness artifact.
#[test]
fn tapping_the_delete_icon_fires_on_deleted_only_not_the_chip_tap() {
    let presses = Rc::new(RefCell::new(0_u32));
    let deletions = Rc::new(RefCell::new(0_u32));
    let press_counter = Rc::clone(&presses);
    let delete_counter = Rc::clone(&deletions);

    let laid = lay_out(
        themed(
            Chip::new(Text::new("Tag"))
                .on_pressed(move || {
                    *press_counter.borrow_mut() += 1;
                })
                .on_deleted(move || {
                    *delete_counter.borrow_mut() += 1;
                }),
        ),
        loose(300.0),
    );

    let (x, y) = delete_icon_center(&laid);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    assert_eq!(
        *deletions.borrow(),
        1,
        "a tap on the delete icon must fire on_deleted",
    );
    assert_eq!(
        *presses.borrow(),
        0,
        "a tap on the delete icon must NOT also fire the chip's own on_pressed — the nested \
         delete InkWell must win the shared arena, not let both recognizers fire",
    );
}

#[test]
fn tapping_elsewhere_on_the_chip_fires_the_chip_tap_only_not_on_deleted() {
    let presses = Rc::new(RefCell::new(0_u32));
    let deletions = Rc::new(RefCell::new(0_u32));
    let press_counter = Rc::clone(&presses);
    let delete_counter = Rc::clone(&deletions);

    let laid = lay_out(
        themed(
            Chip::new(Text::new("Tag"))
                .on_pressed(move || {
                    *press_counter.borrow_mut() += 1;
                })
                .on_deleted(move || {
                    *delete_counter.borrow_mut() += 1;
                }),
        ),
        loose(300.0),
    );

    let (x, y) = chip_only_point();
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    assert_eq!(
        *presses.borrow(),
        1,
        "a tap away from the delete icon must fire the chip's own on_pressed",
    );
    assert_eq!(
        *deletions.borrow(),
        0,
        "a tap away from the delete icon must NOT fire on_deleted",
    );
}

// ------------------------------------------------------------------
// (c) Selected FilterChip fill reaches the mounted Material.
// ------------------------------------------------------------------

/// Mutation-run: hardcoding `FilterChip::build`'s `background_color` to
/// `Color::TRANSPARENT` regardless of `filter_chip_default_background_color`'s
/// result was confirmed to make this test fail (`got Color { r: 0, g: 0, b:
/// 0, a: 0 }, expected Color { r: 232, g: 222, b: 248, a: 255 }`) — the
/// exact "wiring mutation survives" gap the pre-fix test suite had, since
/// only the pure default-table function itself was ever unit-tested, never
/// its actual use inside `build()`.
#[test]
fn selected_filter_chip_fill_reaches_the_mounted_material() {
    let laid = lay_out(
        themed(
            FilterChip::new(Text::new("Vegetarian"))
                .selected(true)
                .on_selected(|_| {}),
        ),
        loose(300.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("FilterChip must compose a Material container surface");

    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(
            ThemeData::light().color_scheme.secondary_container
        )),
        "a selected, enabled FilterChip's container must fill with secondaryContainer through \
         the real mount — not the transparent fill an unselected/base chip uses",
    );
}

// ------------------------------------------------------------------
// (d) Disabled chip + delete icon are inert through real dispatch.
// ------------------------------------------------------------------

#[test]
fn disabled_chip_and_its_delete_icon_are_both_inert_through_dispatch() {
    let presses = Rc::new(RefCell::new(0_u32));
    let deletions = Rc::new(RefCell::new(0_u32));
    let press_counter = Rc::clone(&presses);
    let delete_counter = Rc::clone(&deletions);

    let laid = lay_out(
        themed(
            Chip::new(Text::new("Tag"))
                .enabled(false)
                .on_pressed(move || {
                    *press_counter.borrow_mut() += 1;
                })
                .on_deleted(move || {
                    *delete_counter.borrow_mut() += 1;
                }),
        ),
        loose(300.0),
    );

    let (delete_x, delete_y) = delete_icon_center(&laid);
    laid.dispatch_pointer_down(delete_x, delete_y);
    laid.dispatch_pointer_up(delete_x, delete_y);

    let (chip_x, chip_y) = chip_only_point();
    laid.dispatch_pointer_down(chip_x, chip_y);
    laid.dispatch_pointer_up(chip_x, chip_y);

    assert_eq!(
        *presses.borrow(),
        0,
        "a disabled chip must swallow taps on its own body"
    );
    assert_eq!(
        *deletions.borrow(),
        0,
        "a disabled chip must swallow taps on its delete icon too — Flutter parity: \
         `onTap: widget.isEnabled ? widget.onDeleted : null`",
    );
}

#[test]
fn disabled_filter_chip_and_its_delete_icon_are_both_inert_through_dispatch() {
    // `FilterChip` has no separate `enabled` builder — Flutter parity:
    // `isEnabled => onSelected != null` (see `chip.rs`'s `FilterChip::is_enabled`).
    let deletions = Rc::new(RefCell::new(0_u32));
    let delete_counter = Rc::clone(&deletions);

    let laid = lay_out(
        themed(
            FilterChip::new(Text::new("Vegetarian")).on_deleted(move || {
                *delete_counter.borrow_mut() += 1;
            }),
        ),
        loose(300.0),
    );

    let (delete_x, delete_y) = delete_icon_center(&laid);
    laid.dispatch_pointer_down(delete_x, delete_y);
    laid.dispatch_pointer_up(delete_x, delete_y);

    assert_eq!(
        *deletions.borrow(),
        0,
        "a FilterChip with no on_selected (disabled) must swallow taps on its delete icon too",
    );
}

// ------------------------------------------------------------------
// Theme tier beats default — proven through the real mount, not
// `Option::or_else` re-implemented inline in the test (see chip.rs's
// module doc: `ChipThemeData`'s fields are plain overrides, so this is the
// one place their wiring into `build()` can be proven end to end).
// ------------------------------------------------------------------

/// Mutation-run: replacing `Chip::build`'s `label_color` binding with a bare
/// `chip_content_color_default(states, &colors)` call (dropping the
/// `chip_theme.label_color` read entirely) was confirmed to make this test
/// fail — the resolved `style` carries the M3 default `onSurfaceVariant`
/// (`Color { r: 73, g: 69, b: 79, a: 255 }`) instead of the configured
/// override.
#[test]
fn theme_label_color_reaches_the_mounted_paragraph_beating_the_default() {
    let themed_label_color = Color::rgb(11, 22, 33);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        chip_theme: Some(ChipThemeData {
            label_color: Some(themed_label_color),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(Theme::new(theme, Chip::new(Text::new("Tag"))), loose(300.0));

    let paragraph = laid
        .find_by_render_type("RenderParagraph")
        .expect("Chip must mount a RenderParagraph for its label");
    let style = laid
        .render_property(paragraph, "style")
        .expect("RenderParagraph must expose its resolved style");

    assert!(
        style.contains(&color_property(themed_label_color)),
        "a configured chip_theme.label_color must reach the mounted label's resolved text \
         color — got style {style:?}",
    );
}

#[test]
fn no_theme_override_paints_the_m3_default_label_color() {
    let laid = lay_out(themed(Chip::new(Text::new("Tag"))), loose(300.0));

    let paragraph = laid
        .find_by_render_type("RenderParagraph")
        .expect("Chip must mount a RenderParagraph for its label");
    let style = laid
        .render_property(paragraph, "style")
        .expect("RenderParagraph must expose its resolved style");

    // `_ChipDefaultsM3.labelStyle`: `isEnabled ? onSurfaceVariant : onSurface`
    // — an enabled, undecorated `Chip` resolves `onSurfaceVariant`.
    let default_color = ThemeData::light().color_scheme.on_surface_variant;
    assert!(
        style.contains(&color_property(default_color)),
        "with no chip_theme override, the label must paint the M3 default onSurfaceVariant — \
         got style {style:?}",
    );
}

/// Mutation-run: replacing `Chip::build`'s `side` binding with a bare
/// `chip_default_side(false, self.enabled, &colors)` call (dropping the
/// `chip_theme.side` read entirely) was confirmed to make this test fail —
/// the mounted border painter carries the M3 default `outlineVariant`
/// (`Color { r: 202, g: 196, b: 208, a: 255 }`) instead of the configured
/// override.
#[test]
fn theme_side_reaches_the_mounted_border_painter_beating_the_default() {
    let themed_side_color = Color::rgb(44, 55, 66);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        chip_theme: Some(ChipThemeData {
            side: Some(flui_types::styling::BorderSide::new(
                themed_side_color,
                flui_types::geometry::px(3.0),
                flui_types::styling::BorderStyle::Solid,
            )),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(Theme::new(theme, Chip::new(Text::new("Tag"))), loose(300.0));

    let custom_paint = laid
        .find_by_render_type("RenderCustomPaint")
        .expect("Chip must mount a RenderCustomPaint for its border");
    let painter = laid
        .render_property(custom_paint, "foreground_painter")
        .expect("RenderCustomPaint must expose its foreground painter");

    assert!(
        painter.contains(&color_property(themed_side_color)),
        "a configured chip_theme.side must reach the mounted border painter's resolved color — \
         got painter {painter:?}",
    );
}

#[test]
fn no_theme_override_paints_the_m3_default_side_color() {
    let laid = lay_out(themed(Chip::new(Text::new("Tag"))), loose(300.0));

    let custom_paint = laid
        .find_by_render_type("RenderCustomPaint")
        .expect("Chip must mount a RenderCustomPaint for its border");
    let painter = laid
        .render_property(custom_paint, "foreground_painter")
        .expect("RenderCustomPaint must expose its foreground painter");

    // `_ChipDefaultsM3.side`: `isEnabled ? outlineVariant : onSurface@12%`.
    let default_color = ThemeData::light().color_scheme.outline_variant;
    assert!(
        painter.contains(&color_property(default_color)),
        "with no chip_theme override, the border must paint the M3 default outlineVariant — \
         got painter {painter:?}",
    );
}
