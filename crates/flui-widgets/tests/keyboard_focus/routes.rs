//! The keyboard follows the navigator: focus enters the current route, keys
//! reach only the route on top, and the back and dismiss keys pop the right
//! route.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_painting::Alignment;
use flui_platform_api::TargetPlatform;
use flui_platform_api::keyboard::{Key, Modifiers, NamedKey};
use flui_platform_api::pointer::PointerButton;
use flui_testing::MouseScript;
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContext};
use flui_widgets::interaction::{Actions, ActivateIntent, CallbackAction, Focus};
use flui_widgets::navigator::{
    Navigator, NavigatorHandle, NavigatorObserver, PageRoute, PopupRoute, RouteAnimation, RouteId,
};
use flui_widgets::{Align, AnimatedBuilder, Column, SizedBox};

use super::{chord, down, press, tab, up};
use crate::common::harness::{Harness, mount};
use crate::common::{lay_out, tight};

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

/// The platform's Back chord: Cmd+[ on Apple, Alt+Left elsewhere.
fn back_chord() -> (Key, Modifiers) {
    if matches!(
        TargetPlatform::current(),
        TargetPlatform::MacOS | TargetPlatform::iOS
    ) {
        (Key::character("["), Modifiers::META)
    } else {
        (Key::Named(NamedKey::ArrowLeft), Modifiers::ALT)
    }
}

/// The first Tab of a fresh window lands in the current route, and with a
/// popup open it lands in the popup, not in the page the popup covers. The
/// first Shift+Tab lands on the current route's last control, again never
/// on the covered page.
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

    // A route of two controls: Tab enters at the first, Shift+Tab at the last.
    drop(navigator.push(PopupRoute::<i32>::new(|_, _, _| {
        Column::new((
            Focus::new(SizedBox::new(40.0, 40.0)).debug_label("first"),
            Focus::new(SizedBox::new(40.0, 40.0)).debug_label("last"),
        ))
        .into_view()
        .boxed()
    })));
    settle(&mut harness);
    for (entry, shift, expected) in [("Tab", false, "first"), ("Shift+Tab", true, "last")] {
        manager.unfocus();
        let modifiers = if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        assert!(chord(&manager, &Key::Named(NamedKey::Tab), modifiers));
        assert_eq!(
            focused(&harness).as_deref(),
            Some(expected),
            "{entry} with nothing focused enters the top route at its {expected} control"
        );
    }
}

/// A push moves the focus out of the page below into the new route: the first
/// control by default, and the route's `autofocus` control when it has one,
/// until the user moves the focus themselves.
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

    drop(navigator.push(PopupRoute::<i32>::new(|_, _, _| {
        Column::new((
            Focus::new(SizedBox::new(40.0, 40.0)).debug_label("first"),
            Focus::new(SizedBox::new(40.0, 40.0))
                .debug_label("title")
                .autofocus(true),
        ))
        .into_view()
        .boxed()
    })));
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("title"),
        "the autofocus control replaces the route's provisional first focus"
    );

    let late = LateAutofocus::new();
    drop(navigator.push(late.route()));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("opening"));
    assert!(tab(&harness.focus_manager()));
    assert_eq!(focused(&harness).as_deref(), Some("next"));
    late.show();
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("next"),
        "once the user moved the focus, an autofocus no longer replaces it"
    );
}

/// A page whose `autofocus` control appears only when [`show`](Self::show) is
/// called, after the route has taken its first focus.
struct LateAutofocus {
    changed: Rc<flui_foundation::ChangeNotifier>,
    shown: Rc<Cell<bool>>,
}

impl LateAutofocus {
    fn new() -> Self {
        Self {
            changed: Rc::new(flui_foundation::ChangeNotifier::new()),
            shown: Rc::new(Cell::new(false)),
        }
    }

    /// A page route with "opening" and "next" controls, then the late one.
    fn route(&self) -> PageRoute<i32> {
        let (changed, shown) = (Rc::clone(&self.changed), Rc::clone(&self.shown));
        PageRoute::new(move |_, _, _| {
            let shown = Rc::clone(&shown);
            Column::new((
                Focus::new(SizedBox::new(40.0, 40.0)).debug_label("opening"),
                Focus::new(SizedBox::new(40.0, 40.0)).debug_label("next"),
                AnimatedBuilder::new(changed.clone(), move || -> BoxedView {
                    if shown.get() {
                        Focus::new(SizedBox::new(40.0, 40.0))
                            .debug_label("late")
                            .autofocus(true)
                            .into_view()
                            .boxed()
                    } else {
                        SizedBox::new(0.0, 0.0).into_view().boxed()
                    }
                }),
            ))
            .into_view()
            .boxed()
        })
    }

    fn show(&self) {
        self.shown.set(true);
        self.changed.notify_listeners();
    }
}

/// Closing a route restores the focus its page had, and an `autofocus` control
/// that appears in the page afterwards does not take it back.
pub(crate) fn late_autofocus_does_not_override_the_restored_focus() {
    let counts = Rc::new(Cell::new(0));
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(page("base", &counts));
    let mut harness = mount(Navigator::new(navigator.clone()));
    harness.tick();

    let late = LateAutofocus::new();
    drop(navigator.push(late.route()));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("opening"));

    drop(navigator.push(dialog("dialog", &counts)));
    settle(&mut harness);
    assert!(navigator.pop());
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("opening"));

    late.show();
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("opening"),
        "the restored focus is final"
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

/// Records the routes a navigator reports removed.
#[derive(Default)]
struct Removals(parking_lot::Mutex<Vec<RouteId>>);

impl NavigatorObserver for Removals {
    fn did_remove(&self, route: RouteId, _previous: Option<RouteId>) {
        self.0.lock().push(route);
    }
}

/// Closing a route whose page's restored control has a focus listener that
/// panics still closes it: the panic reaches the caller, and the observers
/// hear the removal, the route leaves the overlay, and the keyboard reaches
/// only the page below.
pub(crate) fn a_panicking_restored_focus_listener_still_closes_the_route() {
    let counts = Rc::new(Cell::new(0));
    let armed = Rc::new(Cell::new(false));
    let navigator = NavigatorHandle::new();
    let trap = Rc::clone(&armed);
    navigator.seed_initial(PageRoute::<i32>::new(move |_, _, _| {
        let trap = Rc::clone(&trap);
        Focus::new(SizedBox::new(40.0, 40.0))
            .debug_label("home")
            .on_focus_change(move |_, focused| {
                let trapped = focused && trap.replace(false);
                assert!(!trapped, "restored focus listener");
            })
            .into_view()
            .boxed()
    }));
    let removals = Arc::new(Removals::default());
    navigator.add_observer(Arc::clone(&removals) as Arc<dyn NavigatorObserver>);
    let mut harness = mount(Navigator::new(navigator.clone()));
    harness.tick();
    let manager = harness.focus_manager();
    assert!(tab(&manager));
    drop(navigator.push(dialog("dialog", &counts)));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("dialog"));

    let dialog = *navigator.route_ids().last().expect("the dialog is on top");
    armed.set(true);
    let payload = catch_unwind(AssertUnwindSafe(|| navigator.remove_route(dialog)))
        .expect_err("the listener's panic reaches the caller");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"restored focus listener")
    );
    assert_eq!(*removals.0.lock(), [dialog], "observers hear the removal");
    settle(&mut harness);
    assert_eq!(navigator.route_ids().len(), 1);
    manager.unfocus();
    for _ in 0..3 {
        assert!(tab(&manager));
        assert_eq!(
            focused(&harness).as_deref(),
            Some("home"),
            "the closed dialog is no Tab stop"
        );
    }
}

/// An observer that panics at a removal does not keep the removed route's
/// overlay entry and page from being retired: their captures, whose
/// destructors panic too, are retained instead of replacing the observer's
/// failure, which reaches the caller.
pub(crate) fn a_panicking_observer_keeps_the_removal_failure_first() {
    struct Bomb;
    impl Drop for Bomb {
        fn drop(&mut self) {
            assert!(std::thread::panicking(), "route capture destructor");
        }
    }
    struct PanicsOnRemove;
    impl NavigatorObserver for PanicsOnRemove {
        fn did_remove(&self, _route: RouteId, _previous: Option<RouteId>) {
            panic!("remove observer");
        }
    }
    let counts = Rc::new(Cell::new(0));
    let (navigator, mut harness) = home(&counts);
    harness.tick();
    let bomb = Rc::new(Bomb);
    drop(navigator.push(PageRoute::<i32>::new(move |_, _, _| {
        let _held = &bomb;
        SizedBox::new(40.0, 40.0).into_view().boxed()
    })));
    settle(&mut harness);
    let page = *navigator.route_ids().last().expect("the page is on top");
    navigator.add_observer(Arc::new(PanicsOnRemove));
    harness.swap_root(SizedBox::new(1.0, 1.0));
    settle(&mut harness);

    let payload = catch_unwind(AssertUnwindSafe(|| navigator.remove_route(page)))
        .expect_err("the observer's panic reaches the caller");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"remove observer"));
    assert_eq!(navigator.route_ids().len(), 1, "the page is gone");
}

/// A nested navigator whose top changed while a dialog covered its page does
/// not take the focus when the dialog closes: it returns to the control that
/// opened the dialog.
pub(crate) fn a_covered_nested_navigator_does_not_take_the_restored_focus() {
    let counts = Rc::new(Cell::new(0));
    let outer = NavigatorHandle::new();
    let inner = NavigatorHandle::new();
    inner.seed_initial(page("inner-home", &counts));
    let (nested, shared) = (inner.clone(), Rc::clone(&counts));
    outer.seed_initial(PageRoute::<i32>::new(move |_, _, _| {
        Column::new((
            control("opener", &shared),
            SizedBox::new(200.0, 200.0).child(Navigator::new(nested.clone())),
        ))
        .into_view()
        .boxed()
    }));
    let mut harness = mount(Navigator::new(outer.clone()));
    harness.tick();
    let manager = harness.focus_manager();
    assert!(tab(&manager));
    assert_eq!(focused(&harness).as_deref(), Some("opener"));

    drop(outer.push(dialog("dialog", &counts)));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("dialog"));
    drop(inner.push(page("inner-detail", &counts)));
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("dialog"),
        "the covered page's navigator takes no focus from the dialog"
    );

    assert!(outer.pop());
    settle(&mut harness);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("opener"),
        "the focus returns to the control that opened the dialog"
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
    settle(&mut harness);

    // A push cancelled by Back mid-transition hands the focus back to the
    // element that opened it, and the cancelled page takes no key.
    let (back, modifiers) = back_chord();
    drop(navigator.push(page("detail", &popup_count)));
    harness.pump_for(FRAME);
    assert_eq!(focused(&harness).as_deref(), Some("detail"));
    assert!(chord(&manager, &back, modifiers), "Back cancels the push");
    harness.pump_for(FRAME);
    assert_eq!(
        focused(&harness).as_deref(),
        Some("home"),
        "the cancelled push restores the opening element"
    );
    let _ = press(&manager, &enter());
    assert_eq!(
        (home_count.get(), popup_count.get()),
        (2, 1),
        "the cancelled page does not take the key"
    );

    // Space held on the page while a push moves the focus into the popup:
    // its release activates neither the covered page nor the popup.
    let space = Key::character(" ");
    let _ = down(&manager, &space);
    drop(navigator.push(dialog("dialog", &popup_count)));
    harness.pump_for(FRAME);
    assert_eq!(focused(&harness).as_deref(), Some("dialog"));
    up(&manager, &space);
    assert_eq!(
        (home_count.get(), popup_count.get()),
        (2, 1),
        "a release whose press the covered page took activates nothing"
    );
}

/// Back pops the current page and nothing at the bottom of the stack;
/// Escape pops a dismissible popup and never a page.
pub(crate) fn back_keys_pop_only_the_current_page() {
    let counts = Rc::new(Cell::new(0));
    let (navigator, mut harness) = home(&counts);
    harness.tick();
    let manager = harness.focus_manager();
    let (back, modifiers) = back_chord();
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

    // The mouse's back button (XButton1), through the runtime's pointer
    // input, pops the current page and nothing at the bottom of the stack.
    let navigator = NavigatorHandle::new();
    let counted = Rc::clone(&counts);
    navigator.seed_initial(PageRoute::<i32>::new(move |_, _, _| {
        control("home", &counted)
    }));
    let mut laid = lay_out(Navigator::new(navigator.clone()), tight(800.0, 600.0));
    let back_button = || {
        MouseScript::new("back button")
            .press_button(Offset::new(10.0, 10.0), PointerButton::BACK)
            .release()
            .build()
            .expect("the back button is released")
    };
    drop(navigator.push(page("detail", &counts)));
    laid.pump_for(Duration::from_millis(500));
    assert_eq!(navigator.route_ids().len(), 2);
    laid.replay(&back_button());
    laid.pump_for(Duration::from_millis(500));
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "the back button popped the page"
    );
    laid.replay(&back_button());
    laid.pump_for(Duration::from_millis(500));
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "and does nothing at the bottom of the stack"
    );
}

/// Back from inside a nested navigator pops the nested navigator while it has
/// a route to pop, then the enclosing navigator's route; with nothing to pop in
/// either, the key reaches the host.
pub(crate) fn back_keys_pop_the_enclosing_navigator_at_a_nested_root() {
    let counts = Rc::new(Cell::new(0));
    let (outer, mut harness) = home(&counts);
    let inner = NavigatorHandle::new();
    inner.seed_initial(page("inner-home", &counts));
    let nested = inner.clone();
    drop(outer.push(PageRoute::<i32>::new(move |_, _, _| {
        SizedBox::new(200.0, 200.0)
            .child(Navigator::new(nested.clone()))
            .into_view()
            .boxed()
    })));
    settle(&mut harness);
    let manager = harness.focus_manager();
    if focused(&harness).as_deref() != Some("inner-home") {
        assert!(tab(&manager));
    }
    assert_eq!(focused(&harness).as_deref(), Some("inner-home"));
    let (back, modifiers) = back_chord();

    drop(inner.push(page("inner-detail", &counts)));
    settle(&mut harness);
    assert_eq!(focused(&harness).as_deref(), Some("inner-detail"));
    assert!(chord(&manager, &back, modifiers));
    settle(&mut harness);
    assert_eq!(
        (inner.route_ids().len(), outer.route_ids().len()),
        (1, 2),
        "Back pops the nested navigator first"
    );

    assert_eq!(focused(&harness).as_deref(), Some("inner-home"));
    assert!(
        chord(&manager, &back, modifiers),
        "the nested navigator's first route lets Back reach the enclosing one"
    );
    settle(&mut harness);
    assert_eq!(outer.route_ids().len(), 1, "Back popped the enclosing page");
    assert_eq!(focused(&harness).as_deref(), Some("home"));

    assert!(
        !chord(&manager, &back, modifiers),
        "nothing to go back to: the key reaches the host"
    );
}
