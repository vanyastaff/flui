//! `Dialog`/`AlertDialog` widget-level integration coverage — mounts each
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/material.rs` use).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{AlertDialog, Dialog, Theme, ThemeData};
use flui_types::Color;
use flui_view::ViewExt;
use flui_widgets::{ColoredBox, GestureDetector, SizedBox, Text};

/// `_DialogDefaultsM3`'s formatted `Debug` string for a resolved
/// [`Color`](flui_types::Color) — the same helper `tests/card.rs`/
/// `tests/elevated_button.rs` use for `RenderPhysicalShape`'s `"color"`
/// diagnostics property.
fn color_property(color: Color) -> String {
    format!("{color:?}")
}

// ============================================================================
// Dialog — _DialogDefaultsM3
// ============================================================================

#[test]
fn dialog_material_matches_dialog_defaults_m3() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(theme, Dialog::new(SizedBox::new(1.0, 1.0))),
        tight(1000.0, 1000.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Dialog must compose a Material (RenderPhysicalShape) surface");

    let color = laid
        .render_property(material, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");
    assert_eq!(
        color,
        color_property(colors.surface_container_high),
        "_DialogDefaultsM3.backgroundColor is ColorScheme.surfaceContainerHigh"
    );

    let elevation = laid
        .render_property(material, "elevation")
        .expect("RenderPhysicalShape reports an \"elevation\" diagnostics property");
    assert_eq!(
        elevation.parse::<f32>(),
        Ok(6.0),
        "_DialogDefaultsM3 constructs with elevation: 6.0"
    );

    let clip_behavior = laid
        .render_property(material, "clip_behavior")
        .expect("RenderPhysicalShape reports a \"clip_behavior\" diagnostics property");
    assert_eq!(
        clip_behavior, "None",
        "_DialogDefaultsM3 constructs with clipBehavior: Clip.none"
    );
}

/// `Dialog.build`'s fallback `constraints` (`BoxConstraints(minWidth:
/// 280.0)`) reaches the Material: a tiny 1x1 child must still be widened to
/// exactly 280 logical pixels under generous incoming constraints.
#[test]
fn default_constraints_enforce_a_280px_minimum_width() {
    let laid = lay_out(
        Theme::new(ThemeData::light(), Dialog::new(SizedBox::new(1.0, 1.0))),
        tight(1000.0, 1000.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Dialog must compose a Material surface");

    assert_eq!(
        laid.size(material).width.get(),
        280.0,
        "BoxConstraints(minWidth: 280.0) must widen a 1px-wide child's Material to 280px"
    );
}

/// `_defaultInsetPadding` (`EdgeInsets.symmetric(horizontal: 40.0, vertical:
/// 24.0)`) reaches the composed `Padding`: the dialog's aligned content sits
/// at `(40, 24)`, not flush against the screen edge.
#[test]
fn default_inset_padding_offsets_the_aligned_content_by_40x24() {
    let laid = lay_out(
        Theme::new(ThemeData::light(), Dialog::new(SizedBox::new(1.0, 1.0))),
        tight(1000.0, 1000.0),
    );

    let aligned = laid
        .find_by_render_type("RenderAlign")
        .expect("Dialog must center its content through an Align");

    assert_eq!(
        laid.offset(aligned),
        common::offset(40.0, 24.0),
        "_defaultInsetPadding (40 horizontal / 24 vertical) must inset the Align \
         that centers the dialog"
    );
}

// ============================================================================
// AlertDialog — title / content / actions composition
// ============================================================================

/// Title, content, and actions each land in their own padded slot: four
/// `RenderPadding` nodes total (the `Dialog`'s own inset, plus one each for
/// title/content/actions) — proof the three slots actually compose into the
/// `Column`, not that any single slot renders in isolation.
#[test]
fn title_content_and_actions_each_compose_their_own_padded_slot() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            AlertDialog::new()
                .title(Text::new("Title"))
                .content(Text::new("Content"))
                .actions(vec![Text::new("Cancel").boxed(), Text::new("OK").boxed()]),
        ),
        tight(1000.0, 1000.0),
    );

    assert_eq!(
        laid.find_all_by_render_type("RenderPadding").len(),
        4,
        "expected the Dialog's own inset padding plus one padded slot each for \
         title/content/actions"
    );
}

/// An `AlertDialog` with no title/content/actions still mounts its bare
/// `Dialog` surface — no padded slot is created for an absent one.
#[test]
fn absent_slots_produce_no_padded_wrapper() {
    let laid = lay_out(
        Theme::new(ThemeData::light(), AlertDialog::new()),
        tight(1000.0, 1000.0),
    );

    assert_eq!(
        laid.find_all_by_render_type("RenderPadding").len(),
        1,
        "only the Dialog's own inset padding should exist when title/content/actions are unset"
    );
}

/// A tap on an action reaches its own `on_tap` handler — the action row is
/// hit-testable, not just laid out.
#[test]
fn a_tap_on_an_action_fires_its_handler() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);

    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            AlertDialog::new().title(Text::new("Delete?")).actions(vec![
                GestureDetector::new()
                    .on_tap(move || {
                        counted.fetch_add(1, Ordering::SeqCst);
                    })
                    .child(
                        ColoredBox::new(Color::rgb(200, 10, 10)).child(SizedBox::new(40.0, 40.0)),
                    )
                    .boxed(),
            ]),
        ),
        tight(1000.0, 1000.0),
    );

    let action = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("the action's ColoredBox must mount");
    let origin = laid.absolute_offset(action);
    let size = laid.size(action);
    let center_x = origin.dx.get() + size.width.get() / 2.0;
    let center_y = origin.dy.get() + size.height.get() / 2.0;

    laid.dispatch_pointer_down(center_x, center_y);
    laid.dispatch_pointer_up(center_x, center_y);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up dispatched at the action's own on-screen position must fire its handler"
    );
}
