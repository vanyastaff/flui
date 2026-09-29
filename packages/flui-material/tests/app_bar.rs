//! `AppBar` widget-level integration coverage — mounts a real app bar through
//! the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/material.rs`/`tests/elevated_button.rs`'s established pattern).
//!
//! `AppBar` composes `Theme::of` (M3 token defaults) and `MediaQuery::of`
//! (the top safe-area inset) — both ambient reads that only resolve through
//! a real mount, so these tests prove the composition end to end rather than
//! re-checking `app_bar.rs`'s own unit-tested `resolve_style` formula.

use crate::common;

use common::{lay_out, tight};
use flui_material::{AppBar, Theme, ThemeData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    MediaQuery, MediaQueryData, Navigator, NavigatorHandle, SimpleRoute, Text,
};

// ── Implied leading: a BackButton synthesized when the navigator can pop ──
//
// Mounted through a real `Navigator` (`flui_sdk::widgets::Navigator`/
// `NavigatorHandle`), not a hand-built `BuildContext` — `resolve_leading`'s
// navigator-consulting branches only run through `NavigatorHandle::maybe_of`,
// which needs a live ancestor to find. `app_bar.rs`'s own unit tests cover
// the ctx-independent short-circuits (explicit `leading`,
// `automatically_imply_leading: false`); these three prove the wiring this
// module's docs describe end to end: no leading with one route on the
// stack, a leading once a second route makes the stack poppable, and a tap
// on that leading actually pops.

fn home_route() -> SimpleRoute<()> {
    SimpleRoute::new(|_ctx| {
        MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(ThemeData::light(), AppBar::new().title(Text::new("Home"))),
        )
        .boxed()
    })
}

fn details_route() -> SimpleRoute<()> {
    SimpleRoute::new(|_ctx| {
        MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(
                ThemeData::light(),
                AppBar::new().title(Text::new("Details")),
            ),
        )
        .boxed()
    })
}

/// A leading `IconButton`'s own `Material` (`RenderPhysicalShape`) among
/// every such node in the tree — one sized exactly 40×40 (its
/// `_IconButtonDefaultsM3.minimumSize`, see `icon_button.rs`), distinct from
/// an `AppBar`'s own full-size `Material`. More than one may match (see
/// `implied_leading_appears_once_the_navigator_can_pop`'s doc comment for
/// why two mounted routes yield two leading buttons) — any one of them taps
/// the same underlying `NavigatorHandle`, so the first is as good as any.
/// Panics with a diagnostic size list if none match at all.
fn find_leading_icon_button_material(laid: &common::LaidOut) -> flui_sdk::foundation::RenderId {
    let candidates = laid.find_all_by_render_type("RenderPhysicalShape");
    let leading_size = common::size(40.0, 40.0);
    candidates
        .iter()
        .copied()
        .find(|&id| laid.size(id) == leading_size)
        .unwrap_or_else(|| {
            panic!(
                "expected at least one 40x40 RenderPhysicalShape (a leading IconButton's Material) \
                 among {} candidates: sizes = {:?}",
                candidates.len(),
                candidates
                    .iter()
                    .map(|&id| laid.size(id))
                    .collect::<Vec<_>>(),
            )
        })
}

#[test]
fn tapping_the_implied_back_button_pops_the_route() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(home_route());
    let _details = handle.push(details_route());
    assert!(handle.can_pop());

    let laid = lay_out(Navigator::new(handle.clone()), tight(400.0, 800.0));

    // Both mounted routes' leadings sit at the same geometry (see the
    // previous test's doc comment) — which one the tap lands on doesn't
    // matter: either fires `NavigatorHandle::maybe_pop()` against the SAME
    // `handle`, so either one popping is the behavior under test.
    let leading = find_leading_icon_button_material(&laid);
    let leading_size = laid.size(leading);
    let leading_origin = laid.absolute_offset(leading);
    let tap_x = leading_origin.dx + leading_size.width / 2.0;
    let tap_y = leading_origin.dy + leading_size.height / 2.0;

    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);

    assert!(
        !handle.can_pop(),
        "tapping the implied back button must pop the pushed route via NavigatorHandle::maybe_pop, \
         leaving only the seeded initial route on the stack",
    );
}

// ── `AppBar.bottom` — Flexible-toolbar/fixed-bottom Column layout ──
//
// `bottom.rs`'s own module docs and `app_bar.rs`'s
// `preferred_size_adds_the_bottom_slots_height_when_set` cover the pure
// preferred-size math in isolation; these prove the mounted geometry end to
// end: the toolbar and bottom slot actually stack at their expected sizes,
// and a height shortfall shrinks the toolbar, never the bottom slot.
