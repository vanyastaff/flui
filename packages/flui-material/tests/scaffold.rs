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

use crate::common;

use common::{lay_out, offset, size, tight};
use flui_material::{AppBar, Scaffold, Theme, ThemeData};
use flui_sdk::geometry::EdgeInsets;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox, Text};

/// The render-tree node for the scaffold's `CustomMultiChildLayout`.
///
/// The shared harness adds the production focus anchor outside the mounted
/// view, so this is resolved by render type instead of assuming a fixed depth
/// below the presentation root.
fn layout_root(laid: &common::LaidOut) -> flui_sdk::foundation::RenderId {
    laid.try_find_by_render_type("RenderCustomMultiChildLayoutBox")
        .expect("Scaffold must mount exactly one CustomMultiChildLayout")
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
        56.0,
        "with no MediaQuery padding, the app bar's measured height is exactly toolbar_height",
    );
    assert_eq!(
        laid.offset(body),
        offset(0.0, 56.0),
        "the body must start exactly at the app bar's measured height, with no extra padding",
    );
}

#[test]
fn floating_action_button_floats_above_the_keyboard() {
    let media_query = MediaQueryData {
        view_insets: EdgeInsets::new(0.0, 0.0, 300.0, 0.0),
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
