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

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, offset, size, tight};
use flui_material::{AppBar, NavigationBar, NavigationDestination, Scaffold, Theme, ThemeData};
use flui_types::EdgeInsets;
use flui_types::geometry::px;
use flui_view::prelude::*;
use flui_widgets::icon::IconData;
use flui_widgets::{Icon, MediaQuery, MediaQueryData, SizedBox, Text};

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

/// A body that records the ambient [`MediaQueryData`] it observes at build
/// time, so a test can assert on `Scaffold`'s body-`MediaQuery` re-wrap (the
/// zeroed `padding.top` / `view_insets.bottom`) without a render-geometry
/// proxy for it.
#[derive(Clone, StatelessView)]
struct MediaQueryProbe {
    captured: Rc<RefCell<Option<MediaQueryData>>>,
}

impl StatelessView for MediaQueryProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.borrow_mut() = Some(MediaQuery::of(ctx));
        SizedBox::new(10.0, 10.0)
    }
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

/// An end-to-end geometry proof that a `MediaQuery` change reaches the
/// mounted render tree through the full `Scaffold` rebuild → new
/// `ScaffoldLayoutDelegate` → `CustomMultiChildLayout::update_render_object`
/// → `RenderCustomMultiChildLayoutBox::set_delegate` → relayout pipeline.
///
/// This does **not**, by itself, prove that
/// `ScaffoldLayoutDelegate::should_relayout`'s return value is what gates the
/// relayout: the headless harness's `pump_widget` reruns layout on every
/// pumped frame regardless of that hint (root constraints are unchanged
/// between the two mounts here, so nothing forces a `should_relayout`
/// consultation to be the deciding factor), so a `should_relayout` hardcoded
/// to always return `false` would not make this specific test fail.
/// `should_relayout`'s own comparison logic is pinned directly, independent
/// of this mounted harness, by `scaffold.rs`'s
/// `should_relayout_is_true_when_bottom_min_inset_changes` and
/// `should_relayout_is_true_when_min_view_padding_bottom_changes` unit tests.
#[test]
fn media_query_change_repositions_the_floating_action_button_end_to_end() {
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
    // `view_insets.bottom`.
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
        "the FAB must have actually moved to its new content_bottom, not stayed at its \
         pre-keyboard y",
    );
}

#[test]
fn floating_action_button_clears_the_bottom_safe_area_with_no_keyboard() {
    // A 34px bottom safe-area inset (the iOS home-indicator area) with NO
    // keyboard: `ScaffoldLayoutDelegate`'s `min_view_padding_bottom` (=
    // `padding.bottom`, unaffected since there is no keyboard to zero it)
    // must widen the FAB's safe margin past the flat 16px margin — the
    // formula-level proof lives in `scaffold.rs`'s
    // `fab_y_grows_the_safe_margin_for_a_nonzero_min_view_padding_bottom`;
    // this is the same case mounted end to end.
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(0.0), px(0.0), px(34.0), px(0.0)),
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

    // content_bottom = 800 (no keyboard, no bottom widgets).
    // bottom_content_height = 800 - 800 = 0.
    // safe_margin = max(16, 34 - 0 + 16) = 50.
    // fab_y = 800 - 56 - 50 = 694.
    assert_eq!(
        laid.offset(fab).dy,
        px(694.0),
        "with a 34px bottom safe-area inset and no keyboard, the FAB must be lifted clear of \
         it (y = 694); the flat-margin formula this replaces would have parked it at \
         800 - 56 - 16 = 728, inside the unsafe 34px band",
    );
}

#[test]
fn floating_action_button_x_accounts_for_the_right_safe_area_padding() {
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(0.0), px(20.0), px(0.0), px(0.0)),
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

    assert_eq!(
        laid.offset(fab).dx,
        px(400.0 - 16.0 - 20.0 - 56.0),
        "the FAB's x position must subtract min_insets.right (the right safe-area padding, \
         e.g. a landscape-orientation notch), not just the flat margin",
    );
}

#[test]
fn greedy_body_fills_exactly_the_area_between_the_app_bar_and_the_keyboard() {
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
                    .app_bar(AppBar::new().title(Text::new("Title")))
                    .body(SizedBox::expand()),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let body = laid.child(layout, 0);
    let app_bar = laid.child(layout, 1);

    let content_top = laid.size(app_bar).height;
    assert_eq!(
        content_top,
        px(56.0),
        "sanity: the app bar's measured height"
    );

    // content_bottom = scaffold_height(800) - min_insets.bottom(300) = 500.
    // A body that greedily fills its loose constraints (SizedBox::expand())
    // must land on exactly content_bottom - content_top, not the raw
    // scaffold height, and not a keyboard-agnostic fixed amount.
    assert_eq!(
        laid.size(body),
        size(400.0, 500.0 - 56.0),
        "a greedy body must fill exactly content_bottom - content_top under a keyboard, \
         proving body_max_height actually threads through the keyboard-shrunk content_bottom \
         (previous tests here only used a fixed 10x10 body, which can't distinguish a correct \
         body_max_height from an oversized or undersized one)",
    );
}

#[test]
fn body_media_query_has_zero_top_padding_under_an_app_bar() {
    let captured = Rc::new(RefCell::new(None));
    let probe = MediaQueryProbe {
        captured: Rc::clone(&captured),
    };
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(24.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };

    let _laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .app_bar(AppBar::new().title(Text::new("Title")))
                    .body(probe),
            ),
        ),
        tight(400.0, 800.0),
    );

    let observed = captured
        .borrow()
        .clone()
        .expect("the body must have built at least once and read an ambient MediaQuery");
    assert_eq!(
        observed.padding.top,
        px(0.0),
        "the body's ambient MediaQuery.padding.top must be zeroed when an app bar is present \
         — the app bar already consumed that inset internally; a SafeArea nested in the body \
         reading the un-reduced 24px would double-pad",
    );
}

#[test]
fn body_media_query_keeps_top_padding_with_no_app_bar() {
    let captured = Rc::new(RefCell::new(None));
    let probe = MediaQueryProbe {
        captured: Rc::clone(&captured),
    };
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(24.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };

    let _laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(media_query, Scaffold::new().body(probe)),
        ),
        tight(400.0, 800.0),
    );

    let observed = captured
        .borrow()
        .clone()
        .expect("the body must have built at least once and read an ambient MediaQuery");
    assert_eq!(
        observed.padding.top,
        px(24.0),
        "with no app bar to consume it, the body's ambient MediaQuery.padding.top must pass \
         through unreduced — nothing else has claimed that inset",
    );
}

#[test]
fn body_media_query_has_zero_bottom_view_inset_when_resizing() {
    let captured = Rc::new(RefCell::new(None));
    let probe = MediaQueryProbe {
        captured: Rc::clone(&captured),
    };
    let media_query = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };

    let _laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .resize_to_avoid_bottom_inset(true)
                    .body(probe),
            ),
        ),
        tight(400.0, 800.0),
    );

    let observed = captured
        .borrow()
        .clone()
        .expect("the body must have built at least once and read an ambient MediaQuery");
    assert_eq!(
        observed.view_insets.bottom,
        px(0.0),
        "with resize_to_avoid_bottom_inset(true), the body's ambient MediaQuery.view_insets.bottom \
         must be zeroed — the delegate already shrank the body's own constraints for the \
         keyboard; a body reading the raw 300px would double-avoid it",
    );
}

#[test]
fn body_media_query_keeps_bottom_view_inset_when_not_resizing() {
    let captured = Rc::new(RefCell::new(None));
    let probe = MediaQueryProbe {
        captured: Rc::clone(&captured),
    };
    let media_query = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };

    let _laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .resize_to_avoid_bottom_inset(false)
                    .body(probe),
            ),
        ),
        tight(400.0, 800.0),
    );

    let observed = captured
        .borrow()
        .clone()
        .expect("the body must have built at least once and read an ambient MediaQuery");
    assert_eq!(
        observed.view_insets.bottom,
        px(300.0),
        "with resize_to_avoid_bottom_inset(false), the body's ambient \
         MediaQuery.view_insets.bottom must pass through unreduced — the delegate did not \
         shrink the body's constraints, so the body is responsible for avoiding the keyboard \
         itself if it cares to",
    );
}

#[test]
fn bottom_navigation_bar_shrinks_the_body_and_lifts_the_floating_action_button() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new()
                    .body(SizedBox::expand())
                    .floating_action_button(SizedBox::new(56.0, 56.0))
                    .bottom_navigation_bar(SizedBox::new(400.0, 80.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    let body = laid.child(layout, 0);
    let fab = laid.child(layout, 1);
    let bottom_nav = laid.child(layout, 2);

    // content_bottom = scaffold_height(800) - max(min_insets.bottom(0),
    // bottom_widgets_height(80)) = 720.
    assert_eq!(
        laid.size(body),
        size(400.0, 720.0),
        "the body must shrink to make room above the bottom navigation bar's measured height, \
         not fill the full scaffold height",
    );
    assert_eq!(
        laid.offset(bottom_nav),
        offset(0.0, 720.0),
        "the bottom navigation bar must be pinned to the bottom of the scaffold, full width",
    );
    assert_eq!(
        laid.size(bottom_nav),
        size(400.0, 80.0),
        "the bottom navigation bar is measured at full width, loose height",
    );
    // bottom_content_height = scaffold_height(800) - content_bottom(720) = 80.
    // safe_margin = max(16, 0 - 80 + 16) = 16.
    // fab_y = content_bottom(720) - fab_height(56) - safe_margin(16) = 648.
    assert_eq!(
        laid.offset(fab).dy,
        px(648.0),
        "the floating action button must lift above the bottom navigation bar — it is \
         positioned from content_bottom, which already folds in the bar's measured height; \
         with no bottom-nav-aware content_bottom the FAB would sit at 800 - 56 - 16 = 728, \
         underneath the bar",
    );
}

#[test]
fn body_media_query_has_zero_bottom_padding_under_a_bottom_navigation_bar() {
    let captured = Rc::new(RefCell::new(None));
    let probe = MediaQueryProbe {
        captured: Rc::clone(&captured),
    };
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(0.0), px(0.0), px(34.0), px(0.0)),
        ..MediaQueryData::default()
    };

    let _laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .body(probe)
                    .bottom_navigation_bar(SizedBox::new(400.0, 80.0)),
            ),
        ),
        tight(400.0, 800.0),
    );

    let observed = captured
        .borrow()
        .clone()
        .expect("the body must have built at least once and read an ambient MediaQuery");
    assert_eq!(
        observed.padding.bottom,
        px(0.0),
        "the body's ambient MediaQuery.padding.bottom must be zeroed when a bottom navigation \
         bar is present — the bar already consumes that inset internally (see its own SafeArea \
         wrapping); a SafeArea nested in the body reading the un-reduced 34px would double-pad",
    );
}

/// Real (non-stand-in) destinations for a mounted `NavigationBar` — needed
/// specifically to prove the top-padding leak (finding below): a bare
/// `SizedBox` stand-in can't exercise `NavigationBar`'s own internal
/// `SafeArea`, which is exactly the mechanism under test.
fn two_destinations() -> Vec<NavigationDestination> {
    vec![
        NavigationDestination::new(Icon::new(IconData::new(0xE88A)), "Home"),
        NavigationDestination::new(Icon::new(IconData::new(0xE7FD)), "Profile"),
    ]
}

#[test]
fn bottom_navigation_bar_does_not_leak_the_ambient_top_padding_into_its_own_safe_area() {
    // A nonzero `padding.top` (e.g. a status-bar inset) that has nothing to
    // do with a BOTTOM bar. Oracle: `_ScaffoldSlot.bottomNavigationBar` is
    // added with `removeTopPadding: true` (`scaffold.dart:3155-3169`) — the
    // bar's own `SafeArea` must never see this inset, or it inflates the
    // bar past its fixed 80dp height.
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
                    .body(SizedBox::expand())
                    .bottom_navigation_bar(NavigationBar::new(two_destinations())),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout = layout_root(&laid);
    // No app_bar, no floating_action_button: body is child 0, the bottom
    // navigation bar is child 1.
    let bottom_nav = laid.child(layout, 1);

    assert_eq!(
        laid.size(bottom_nav).height,
        px(80.0),
        "the bottom navigation bar's own SafeArea must not ALSO consume the ambient 24px \
         padding.top on top of its fixed 80dp height — a leaked top inset would inflate the \
         bar to 104 instead of the oracle's height + padding.bottom (80 here, since \
         padding.bottom is 0)",
    );
}
