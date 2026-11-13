//! `AppBar` widget-level integration coverage — mounts a real app bar through
//! the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/material.rs`/`tests/elevated_button.rs`'s established pattern).
//!
//! `AppBar` composes `Theme::of` (M3 token defaults) and `MediaQuery::of`
//! (the top safe-area inset) — both ambient reads that only resolve through
//! a real mount, so these tests prove the composition end to end rather than
//! re-checking `app_bar.rs`'s own unit-tested `resolve_style` formula.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{lay_out, lay_out_animated, tight};
use flui_material::{AppBar, Theme, ThemeData};
use flui_sdk::animation::Vsync;
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{PageHandle, Routable, Router, RouterHandle, Text};
use flui_testing::a11y::Role;

// ── Implied leading: a BackButton synthesized when the route can pop ──
//
// Mounted through a real `Router`, not a hand-built `BuildContext`:
// `resolve_leading` asks the `PageHandle` of the route the app bar is on,
// which only a mounted route provides. `app_bar.rs`'s own unit tests cover
// the ctx-independent short-circuits (explicit `leading`,
// `automatically_imply_leading: false`); this one proves the wiring end to
// end: the bottom page's app bar implies no leading even while a page above
// it can pop, the top page's does, and a tap on it pops that page.

#[derive(Routable, Clone, Debug, PartialEq)]
enum Screen {
    #[route("/")]
    Home,
    #[route("/details")]
    Details,
}

/// A leading `IconButton`'s own `Material` (`RenderPhysicalShape`): sized
/// exactly 40×40 (its `_IconButtonDefaultsM3.minimumSize`, see
/// `icon_button.rs`), distinct from an `AppBar`'s own full-size `Material`;
/// an offstage covered page has no layout, so it contributes none.
fn leading_icon_button_materials(laid: &common::LaidOut) -> Vec<flui_sdk::foundation::RenderId> {
    let leading_size = common::size(40.0, 40.0);
    laid.find_all_by_render_type("RenderPhysicalShape")
        .into_iter()
        .filter(|&id| laid.try_size(id) == Some(leading_size))
        .collect()
}

pub fn tapping_the_implied_back_button_pops_the_route() {
    let router: Rc<RefCell<Option<RouterHandle<Screen>>>> = Rc::default();
    let home: Rc<RefCell<Option<PageHandle>>> = Rc::default();
    let pages = {
        let (router, home) = (Rc::clone(&router), Rc::clone(&home));
        move |screen: &Screen, ctx: &dyn BuildContext| {
            let title = match screen {
                Screen::Home => {
                    *router.borrow_mut() = Router::<Screen>::of(ctx).ok();
                    *home.borrow_mut() = PageHandle::of(ctx);
                    "Home"
                }
                Screen::Details => "Details",
            };
            MediaQuery::new(
                MediaQueryData::default(),
                Theme::new(ThemeData::light(), AppBar::new().title(Text::new(title))),
            )
            .boxed()
        }
    };
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Router::from_stack(vec![Screen::Home, Screen::Details], pages)
                .expect("a non-empty stack"),
        ),
        tight(400.0, 800.0),
        vsync,
    );
    for _ in 0..10 {
        laid.pump_for(Duration::from_millis(50));
    }
    let router = router.borrow().clone().expect("the home page built");
    let home = home.borrow().clone().expect("the home page is on a route");
    assert!(!home.can_pop(), "nothing is below the bottom page");

    let leadings = leading_icon_button_materials(&laid);
    assert_eq!(
        leadings.len(),
        1,
        "the top page's app bar implies one back button"
    );
    let leading = leadings[0];
    let leading_size = laid.size(leading);
    let leading_origin = laid.absolute_offset(leading);
    let tap_x = leading_origin.dx + leading_size.width / 2.0;
    let tap_y = leading_origin.dy + leading_size.height / 2.0;

    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);
    for _ in 0..10 {
        laid.pump_for(Duration::from_millis(50));
    }

    assert_eq!(
        router.stack(),
        vec![Screen::Home],
        "tapping the implied back button pops the page it is on through PageHandle::maybe_pop",
    );
    assert!(home.is_current());
    assert!(
        leading_icon_button_materials(&laid).is_empty(),
        "the home page alone implies no back button"
    );
}

/// The title is the screen's top-level heading: a screen reader announces it
/// as "heading level 1" and its heading navigation lands on it.
pub fn the_title_announces_as_a_level_one_heading() {
    let mut laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(ThemeData::light(), AppBar::new().title(Text::new("Inbox"))),
        ),
        tight(400.0, 64.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");

    let heading = tree
        .find(Role::Heading)
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    assert_eq!(heading.label(), Some("Inbox"), "{}", tree.describe());
    assert_eq!(heading.heading_level(), Some(1), "{}", tree.describe());
}
