//! `Scaffold` widget-level integration coverage — mounts a real scaffold
//! through the full render pipeline (`tests/common/mod.rs`), proving the
//! inset contract documented in `scaffold.rs`'s module docs: the app bar's
//! MEASURED height (not a re-added `padding.top`) sets `content_top`, and the
//! floating action button is positioned from `content_bottom` (which already
//! accounts for the keyboard), never from the scaffold's raw height.
//!
//! Slot ordering: `Scaffold::build` pushes `LayoutId`s in `body`, `app_bar`,
//! `floating_action_button` order (whichever are present) — see
//! `scaffold.rs`. Each test below indexes `laid.child(layout_root, n)`
//! against exactly that order for the slots it configures.

mod common;

use common::{lay_out, offset, size, tight};
use flui_material::{AppBar, Scaffold, Theme, ThemeData};
use flui_types::EdgeInsets;
use flui_types::geometry::px;
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox, Text};

/// The render-tree node for `CustomMultiChildLayout` (the scaffold's own
/// `Material` surface is the mounted root; this is that root's only child).
fn layout_root(laid: &common::LaidOut) -> flui_foundation::RenderId {
    let root = laid.root();
    assert_eq!(
        laid.find_by_render_type("RenderCustomMultiChildLayoutBox"),
        Some(laid.only_child(root)),
        "Scaffold must wrap exactly one CustomMultiChildLayout in its own Material surface",
    );
    laid.only_child(root)
}

#[test]
fn body_is_positioned_below_the_app_bar_with_no_padding() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new()
                    .app_bar(AppBar::new().title(Text::new("Title")))
                    .body(SizedBox::new(10.0, 10.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let body = laid.child(layout, 0);
    let app_bar = laid.child(layout, 1);

    assert_eq!(
        laid.size(app_bar).height,
        px(56.0),
        "with no MediaQuery padding, the app bar's measured height is exactly toolbar_height",
    );
    assert_eq!(
        laid.offset(body),
        offset(0.0, 56.0),
        "the body must start exactly at the app bar's measured height, with no extra padding",
    );
}

#[test]
fn app_bar_height_includes_the_top_padding_and_the_body_does_not_double_shift() {
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(24.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .app_bar(AppBar::new().title(Text::new("Title")))
                    .body(SizedBox::new(10.0, 10.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let body = laid.child(layout, 0);
    let app_bar = laid.child(layout, 1);

    assert_eq!(
        laid.size(app_bar).height,
        px(56.0 + 24.0),
        "the app bar's measured height must include the top MediaQuery padding \
         (it consumes that inset itself — see app_bar.rs)",
    );
    assert_eq!(
        laid.offset(body),
        offset(0.0, 80.0),
        "the body must start at the app bar's MEASURED height (80), not at 80 + 24 \
         (the double-shift bug this delegate must not reintroduce)",
    );
}

#[test]
fn no_app_bar_means_the_body_starts_at_the_top() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new().body(SizedBox::new(10.0, 10.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let body = laid.child(layout, 0);

    assert_eq!(
        laid.offset(body),
        offset(0.0, 0.0),
        "with no app bar, content_top must be 0 — the body owns the whole scaffold height",
    );
}

#[test]
fn floating_action_button_floats_above_the_keyboard() {
    let media_query = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(SizedBox::new(56.0, 56.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let fab = laid.child(layout, 1);

    assert_eq!(laid.size(fab), size(56.0, 56.0));
    // content_bottom = scaffold_height(800) - min_insets.bottom(300) = 500.
    // x = width(400) - margin(16) - min_insets.right(0) - fab_width(56) = 328.
    // y = content_bottom(500) - fab_height(56) - margin(16) = 428.
    assert_eq!(
        laid.offset(fab),
        offset(328.0, 428.0),
        "the FAB must be positioned from content_bottom (which already subtracts the \
         keyboard height), never from the scaffold's raw size.height — a raw-height \
         computation would place it at y = 800 - 56 - 16 = 728, under the keyboard",
    );
}

#[test]
fn resize_to_avoid_bottom_inset_false_ignores_the_keyboard() {
    let media_query = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .resize_to_avoid_bottom_inset(false)
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(SizedBox::new(56.0, 56.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let fab = laid.child(layout, 1);

    // With resizing disabled, min_insets.bottom is forced to 0 regardless of
    // the 300px keyboard — content_bottom is the full scaffold height, so
    // the FAB sits at the same y it would with no keyboard at all.
    assert_eq!(
        laid.offset(fab),
        offset(328.0, 800.0 - 56.0 - 16.0),
        "resize_to_avoid_bottom_inset(false) must make the FAB ignore the keyboard inset",
    );
}

#[test]
fn should_relayout_repositions_the_floating_action_button_when_the_keyboard_shows() {
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new()
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(SizedBox::new(56.0, 56.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout_before = layout_root(&laid);
    let fab_before = laid.child(layout_before, 1);
    let y_before_keyboard = laid.offset(fab_before).dy;
    assert_eq!(
        y_before_keyboard,
        px(800.0 - 56.0 - 16.0),
        "sanity: no keyboard yet, the FAB sits at the bottom margin",
    );

    // The keyboard shows: root-swap to the same tree shape with a nonzero
    // `view_insets.bottom` — `ScaffoldLayoutDelegate::should_relayout` must
    // report true (its `min_insets` changed) so the render object actually
    // repositions the FAB, not just accepts a new delegate it never applies.
    let media_query_with_keyboard = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };
    laid.pump_widget(Theme::new(
        ThemeData::light(),
        MediaQuery::new(
            media_query_with_keyboard,
            Scaffold::new()
                .body(SizedBox::new(10.0, 10.0))
                .floating_action_button(SizedBox::new(56.0, 56.0)),
        ),
    ));

    let layout_after = layout_root(&laid);
    let fab_after = laid.child(layout_after, 1);
    let y_after_keyboard = laid.offset(fab_after).dy;

    assert_eq!(
        y_after_keyboard,
        px(500.0 - 56.0 - 16.0),
        "once the keyboard shows, the FAB must move up to float above content_bottom",
    );
    assert!(
        y_after_keyboard < y_before_keyboard,
        "the FAB must have actually moved (relayout fired), not stayed at its pre-keyboard y",
    );
}
