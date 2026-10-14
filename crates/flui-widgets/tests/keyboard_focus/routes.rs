//! The keyboard follows the navigator: focus enters the current route, keys
//! reach only the route on top, and the back and dismiss keys pop the right
//! route.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_painting::Alignment;
use flui_platform_api::TargetPlatform;
use flui_platform_api::keyboard::{Key, Modifiers, NamedKey};
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContext};
use flui_widgets::interaction::{Actions, ActivateIntent, CallbackAction, Focus};
use flui_widgets::navigator::{Navigator, NavigatorHandle, PageRoute, PopupRoute, RouteAnimation};
use flui_widgets::{Align, Column, SizedBox};

use super::{chord, press, tab};
use crate::common::harness::{Harness, mount};

const FRAME: Duration = Duration::from_millis(50);

/// A focusable 40 × 40 control that counts its activations.
fn control(label: &'static str, activations: &Rc<Cell<u32>>) -> BoxedView {
    let counted = Rc::clone(activations);
    Actions::new(Focus::new(SizedBox::new(40.0, 40.0)).debug_label(label))
        .action(CallbackAction::new(move |_cx, _: &ActivateIntent| {
            counted.set(counted.get() + 1);
        }))
        .into_view()
        .boxed()
}

/// A page route whose page is one control at the bottom right.
fn page(label: &'static str, activations: &Rc<Cell<u32>>) -> PageRoute<i32> {
    let activations = Rc::clone(activations);
    PageRoute::new(
        move |_: &dyn BuildContext, _: &RouteAnimation, _: &RouteAnimation| {
            Align::new(Alignment::BOTTOM_RIGHT)
                .child(control(label, &activations))
                .into_view()
                .boxed()
        },
    )
}

/// A dismissible popup with one control at the bottom right.
fn dialog(label: &'static str, activations: &Rc<Cell<u32>>) -> PopupRoute<i32> {
    let activations = Rc::clone(activations);
    PopupRoute::new(
        move |_: &dyn BuildContext, _: &RouteAnimation, _: &RouteAnimation| {
            Align::new(Alignment::BOTTOM_RIGHT)
                .child(control(label, &activations))
                .into_view()
                .boxed()
        },
    )
    .barrier_dismissible(true)
}

/// A navigator whose home page is one control at the top left.
fn home(activations: &Rc<Cell<u32>>) -> (NavigatorHandle, Harness) {
    let navigator = NavigatorHandle::new();
    let counted = Rc::clone(activations);
    navigator.seed_initial(PageRoute::<i32>::new(move |_, _, _| {
        control("home", &counted)
    }));
    let harness = mount(Navigator::new(navigator.clone()));
    (navigator, harness)
}

fn settle(harness: &mut Harness) {
    for _ in 0..10 {
        harness.pump_for(FRAME);
    }
}

fn focused(harness: &Harness) -> Option<String> {
    harness
        .focus_manager()
        .primary_focus()
        .and_then(|node| node.debug_label().map(str::to_owned))
}

fn enter() -> Key {
    Key::Named(NamedKey::Enter)
}

/// The first Tab of a fresh window lands in the current route, and with a
/// popup open it lands in the popup, not in the page the popup covers.
pub(crate) fn first_tab_enters_the_current_route() {
    let (home_count, popup_count) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (navigator, mut harness) = home(&home_count);
    harness.tick();
    let manager = harness.focus_manager();
    assert_eq!(focused(&harness), None, "mounting takes no focus");
    assert!(tab(&manager));
    assert_eq!(focused(&harness).as_deref(), Some("home"));

    drop(navigator.push(dialog("dialog", &popup_count)));
    settle(&mut harness);
    manager.unfocus();
    assert!(tab(&manager));
    assert_eq!(
        focused(&harness).as_deref(),
        Some("dialog"),
        "the covered page is not a Tab stop"
    );
}

// A push moves the focus out of the page below into the first control of the/n/// new route.
pub(crate) fn push_moves_focus_into_the_new_route() {
    let home_count = Rc::new(Cell::new(0));
    let (navigator, mut harness) = home(&home_count);
    harness.tick();
    assert!(tab(&harness.focus_manager()));
    assert_eq!(focused(&harness).as_deref(), Some("home"));

    drop(navigator.push(PopupRoute::<i32>::new(|_, _, _| {
        Column::new((
            Focus::new(SizedBox::new(40.0, 40.0)).debug_label("first"),
            Focus::new(SizedBox::new(40.0, 40.0)).debug_label("second"),
        ))
        .into_view()
        .boxed()
    })));
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("first"),
        "the new route takes the focus from the page below"
    );
}

/// Closing the top route returns the focus to the control that was focused
/// when the route opened, not to the first control of the page.
pub(crate) fn pop_restores_the_opening_element() {
    let counts = Rc::new(Cell::new(0));
    let navigator = NavigatorHandle::new();
    let shared = Rc::clone(&counts);
    navigator.seed_initial(PageRoute::<i32>::new(move |_, _, _| {
        Column::new((control("first", &shared), control("second", &shared)))
            .into_view()
            .boxed()
    }));
    let mut harness = mount(Navigator::new(navigator.clone()));
    harness.tick();
    let manager = harness.focus_manager();
    assert!(tab(&manager));
    assert!(tab(&manager));
    assert_eq!(focused(&harness).as_deref(), Some("second"));

    drop(navigator.push(dialog("dialog", &counts)));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("dialog"));

    assert!(navigator.pop());
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("second"),
        "the focus returns to the element that opened the route"
    );
}

/// While a route enters or leaves, a key reaches only the route on top: the
/// page it covers or uncovers is neither focused nor activated by it.
pub(crate) fn keys_during_transition_reach_only_the_top_route() {
    let (home_count, popup_count) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (navigator, mut harness) = home(&home_count);
    harness.tick();
    let manager = harness.focus_manager();
    assert!(tab(&manager));

    // The push has just begun: the popup has not built yet, so the page it
    // covers is the only thing that could take the key.
    drop(navigator.push(dialog("dialog", &popup_count)));
    let _ = press(&manager, &enter());
    assert_eq!(home_count.get(), 0, "the covered page ignores the key");

    harness.pump_for(FRAME);
    assert_eq!(focused(&harness).as_deref(), Some("dialog"));
    for _ in 0..3 {
        let _ = tab(&manager);
        assert_eq!(focused(&harness).as_deref(), Some("dialog"));
    }
    let _ = press(&manager, &enter());
    assert_eq!((home_count.get(), popup_count.get()), (0, 1));
    settle(&mut harness);

    // The popup is leaving: the page below is current again.
    assert!(navigator.pop());
    harness.pump_for(FRAME);
    let _ = press(&manager, &enter());
    assert_eq!(
        (home_count.get(), popup_count.get()),
        (1, 1),
        "the leaving popup does not take the key"
    );
}

/// Back pops the current page and nothing at the bottom of the stack;
/// Escape pops a dismissible popup and never a page.
pub(crate) fn back_keys_pop_only_the_current_page() {
    let counts = Rc::new(Cell::new(0));
    let (navigator, mut harness) = home(&counts);
    harness.tick();
    let manager = harness.focus_manager();
    let apple = matches!(
        TargetPlatform::current(),
        TargetPlatform::MacOS | TargetPlatform::iOS
    );
    let (back, modifiers) = if apple {
        (Key::character("["), Modifiers::META)
    } else {
        (Key::Named(NamedKey::ArrowLeft), Modifiers::ALT)
    };
    let escape = Key::Named(NamedKey::Escape);

    assert!(tab(&manager));
    assert!(
        !chord(&manager, &back, modifiers),
        "nothing to go back to: the key reaches the host"
    );
    assert_eq!(navigator.route_ids().len(), 1);

    drop(navigator.push(page("detail", &counts)));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("detail"));
    assert!(!press(&manager, &escape), "Escape never pops a page");
    settle(&mut harness);
    assert_eq!(navigator.route_ids().len(), 2);

    assert!(chord(&manager, &back, modifiers));
    settle(&mut harness);
    assert_eq!(navigator.route_ids().len(), 1, "Back popped the page");
    assert_eq!(focused(&harness).as_deref(), Some("home"));

    drop(navigator.push(dialog("dialog", &counts)));
    settle(&mut harness);
    assert!(press(&manager, &escape), "Escape dismisses the popup");
    settle(&mut harness);
    assert_eq!(navigator.route_ids().len(), 1);

    drop(navigator.push(dialog("dialog", &counts)));
    settle(&mut harness);
    assert!(press(&manager, &Key::Named(NamedKey::BrowserBack)));
    settle(&mut harness);
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "BrowserBack pops a popup too"
    );
}
