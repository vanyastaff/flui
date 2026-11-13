//! The presentation's URL owner (ADR-0330 §5) through a headless host: a
//! [`NavigationIntent`] addressed to one window reaches the outermost
//! `Router` of that window's tree, as a URL the platform opens for it does.
//!
//! # Scenarios
//!
//! Each window has its own owner; a tree without a Router has none; a path no
//! route matches reaches `on_unknown` and leaves the stack; a `Pop` honours the
//! page's `PopScope`; an intent that arrives inside another edit, inside its
//! own parse or during a build is refused; an unmounted Router lets go of its
//! hook; and when the owning Router leaves, a sibling Router takes the URL over.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_foundation::ChangeNotifier;
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::CloseReason;
use flui_widgets::prelude::*;
use flui_widgets::{
    AnimatedBuilder, NavigationIntent, NavigationOp, NavigationRefused, PopScope, RouteParseError,
    SizedBox, Stack,
};

const FRAME: Duration = Duration::from_millis(50);

/// What the URL-owner slot warns when a second Router claims it.
const CLAIM_WARNING: &str = "claim one presentation's URL";

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: u32 },
}

/// The handle of the Router a page was built under, captured from its page
/// builder.
type Captured<R> = Rc<RefCell<Option<RouterHandle<R>>>>;

fn path(location: &str) -> RoutePath {
    RoutePath::parse(location).expect("a valid location")
}

fn push(location: &str) -> NavigationIntent {
    NavigationIntent::new(NavigationOp::Push(path(location)))
}

fn go(location: &str) -> NavigationIntent {
    NavigationIntent::new(NavigationOp::Go(path(location)))
}

/// A Router whose pages are empty boxes and which hands its handle to
/// `captured` each time it builds one.
fn router<R: Routable>(initial: R, captured: &Captured<R>) -> Router<R> {
    let captured = Rc::clone(captured);
    Router::new(initial, move |_route: &R, ctx: &dyn BuildContext| {
        *captured.borrow_mut() = Router::<R>::of(ctx).ok();
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
}

fn stack<R: Routable>(captured: &Captured<R>) -> Vec<R> {
    captured
        .borrow()
        .as_ref()
        .expect("the Router built a page")
        .stack()
}

fn settle(host: &mut HeadlessHost) {
    for _ in 0..10 {
        let _ = host.pump(FRAME);
    }
}

pub(crate) fn an_intent_reaches_only_the_router_of_its_window() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let first = host.primary_window();
    let second = host.open_window(HeadlessWindow::new(400, 400));
    let first_router = Captured::default();
    let second_router = Captured::default();
    host.attach(&router(AppRoute::Home, &first_router))
        .expect("a fresh window");
    host.attach_to(second, &router(AppRoute::Home, &second_router))
        .expect("a fresh window");
    settle(&mut host);

    assert_eq!(host.navigate(second, push("/note/7")), Ok(()));
    assert_eq!(
        stack(&second_router),
        [AppRoute::Home, AppRoute::Note { id: 7 }]
    );
    assert_eq!(stack(&first_router), [AppRoute::Home], "the other window");

    assert_eq!(host.navigate(first, go("/note/3")), Ok(()));
    settle(&mut host);
    assert_eq!(
        stack(&first_router),
        [AppRoute::Home, AppRoute::Note { id: 3 }]
    );
    assert_eq!(
        stack(&second_router),
        [AppRoute::Home, AppRoute::Note { id: 7 }],
        "the other window"
    );

    assert_eq!(
        host.navigate(first, NavigationIntent::new(NavigationOp::Pop)),
        Ok(())
    );
    assert_eq!(stack(&first_router), [AppRoute::Home]);
    assert_eq!(
        host.navigate(first, NavigationIntent::new(NavigationOp::Pop)),
        Err(NavigationRefused::NothingToPop),
        "the last page stays and the platform handles the Back"
    );
    settle(&mut host);
}

pub(crate) fn a_window_without_a_router_has_no_url_owner() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let second = host.open_window(HeadlessWindow::new(400, 400));
    host.attach(&SizedBox::new(10.0, 10.0))
        .expect("a fresh window");
    host.attach_to(second, &SizedBox::new(10.0, 10.0))
        .expect("a fresh window");
    settle(&mut host);

    assert_eq!(
        host.navigate(host.primary_window(), push("/note/1")),
        Err(NavigationRefused::NoUrlOwner)
    );
    assert!(host.request_close(second, CloseReason::Program));
    assert_eq!(
        host.navigate(second, push("/note/1")),
        Err(NavigationRefused::NoPresentation),
        "a closed window"
    );
}

pub(crate) fn an_unmatched_path_reaches_on_unknown_and_leaves_the_stack() {
    let host = Rc::new(RefCell::new(HeadlessHost::new(HeadlessWindow::new(
        400, 400,
    ))));
    let window = host.borrow().primary_window();
    let captured = Captured::default();
    let unknown = Rc::new(RefCell::new(Vec::new()));
    let redirected = Rc::new(RefCell::new(None));
    // The hook redirects to a page of its own: it runs with no borrow of the
    // Router held, so it may navigate.
    let weak: Weak<RefCell<HeadlessHost>> = Rc::downgrade(&host);
    let hook = {
        let unknown = Rc::clone(&unknown);
        let redirected = Rc::clone(&redirected);
        move |path: &RoutePath| {
            unknown.borrow_mut().push(path.clone());
            let host = weak.upgrade().expect("the host outlives its tree");
            *redirected.borrow_mut() = Some(host.borrow().navigate(window, push("/note/404")));
        }
    };
    host.borrow()
        .attach(&router(AppRoute::Home, &captured).on_unknown(hook))
        .expect("a fresh window");
    settle(&mut host.borrow_mut());

    let missing = path("/settings/display");
    let refused = host.borrow().navigate(window, push(missing.as_str()));
    assert_eq!(refused, Err(NavigationRefused::NoMatch(missing.clone())));
    assert_eq!(*unknown.borrow(), std::slice::from_ref(&missing));
    assert_eq!(*redirected.borrow(), Some(Ok(())), "the hook navigated");
    assert_eq!(
        stack(&captured),
        [AppRoute::Home, AppRoute::Note { id: 404 }]
    );

    // A parameter that does not parse is no match either, and `go` reports it
    // the same way.
    let bad = path("/note/seven");
    let refused = host.borrow().navigate(window, go(bad.as_str()));
    assert_eq!(refused, Err(NavigationRefused::NoMatch(bad.clone())));
    assert_eq!(*unknown.borrow(), [missing, bad]);
    settle(&mut host.borrow_mut());
}

thread_local! {
    // Run once by the next comparison of two `EqRoute`s.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ON_EQ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    // Run once by the next parse of an `EqRoute`.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ON_PARSE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

/// A route whose comparison and parse run test hooks: `go` compares the stack it
/// opens with the current one in the middle of its edit, and an intent
/// parses its path inside its own.
#[derive(Debug, Clone)]
enum EqRoute {
    Home,
    Note(u32),
}

impl PartialEq for EqRoute {
    fn eq(&self, other: &Self) -> bool {
        let hook = ON_EQ.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
        match (self, other) {
            (Self::Home, Self::Home) => true,
            (Self::Note(a), Self::Note(b)) => a == b,
            _ => false,
        }
    }
}

impl Routable for EqRoute {
    fn to_path(&self) -> RoutePath {
        match self {
            Self::Home => RoutePath::root(),
            Self::Note(id) => RoutePath::root().join("note").join(id),
        }
    }

    fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
        let hook = ON_PARSE.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
        let segments: Vec<_> = path.segments().collect();
        match segments.as_slice() {
            [] => Ok(Self::Home),
            [note, id] if note == "note" => id
                .parse()
                .map(Self::Note)
                .map_err(|_| RouteParseError::NoMatch { path: path.clone() }),
            _ => Err(RouteParseError::NoMatch { path: path.clone() }),
        }
    }
}

pub(crate) fn an_intent_inside_another_edit_is_refused_as_busy() {
    let host = Rc::new(RefCell::new(HeadlessHost::new(HeadlessWindow::new(
        400, 400,
    ))));
    let window = host.borrow().primary_window();
    let captured = Captured::default();
    host.borrow()
        .attach(&router(EqRoute::Home, &captured))
        .expect("a fresh window");
    settle(&mut host.borrow_mut());

    let inner = Rc::new(RefCell::new(None));
    {
        let host = Rc::downgrade(&host);
        let inner = Rc::clone(&inner);
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            let host = host.upgrade().expect("the host is alive");
            *inner.borrow_mut() = Some(host.borrow().navigate(window, push("/note/9")));
        });
        ON_EQ.with(|slot| *slot.borrow_mut() = Some(hook));
    }
    let outer = host.borrow().navigate(window, go("/note/3"));
    ON_EQ.with(|slot| slot.borrow_mut().take());

    assert_eq!(*inner.borrow(), Some(Err(NavigationRefused::Busy)));
    assert_eq!(outer, Ok(()), "the edit in progress commits");
    assert_eq!(stack(&captured), [EqRoute::Home, EqRoute::Note(3)]);
    assert_eq!(
        host.borrow().navigate(window, push("/note/9")),
        Ok(()),
        "the owner takes the next intent"
    );
    settle(&mut host.borrow_mut());
}

pub(crate) fn a_sibling_router_takes_the_url_over_when_the_owner_leaves() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let window = host.primary_window();
    let first = Captured::default();
    let second = Captured::default();
    let show_first = Rc::new(Cell::new(true));
    let toggle = Rc::new(ChangeNotifier::new());
    let root = {
        let (first, second, show_first) = (
            Rc::clone(&first),
            Rc::clone(&second),
            Rc::clone(&show_first),
        );
        AnimatedBuilder::new(
            Rc::clone(&toggle) as Rc<dyn flui_foundation::Listenable>,
            move || {
                // The first child changes type when the first Router leaves, so
                // the second keeps its element and its state.
                let lead = if show_first.get() {
                    router(AppRoute::Home, &first).into_view().boxed()
                } else {
                    SizedBox::new(10.0, 10.0).into_view().boxed()
                };
                Stack::new(vec![
                    lead,
                    router(AppRoute::Home, &second).into_view().boxed(),
                ])
            },
        )
    };
    let ((), log) = flui_testing::capture(|| {
        host.attach(&root).expect("a fresh window");
        settle(&mut host);
    });
    assert_eq!(
        log.count_containing(CLAIM_WARNING),
        1,
        "two siblings claim the URL: {log}"
    );

    assert_eq!(host.navigate(window, push("/note/1")), Ok(()));
    assert_eq!(stack(&first), [AppRoute::Home, AppRoute::Note { id: 1 }]);
    assert_eq!(
        stack(&second),
        [AppRoute::Home],
        "the first mounted owns it"
    );

    show_first.set(false);
    toggle.notify_listeners();
    settle(&mut host);
    assert_eq!(host.navigate(window, push("/note/2")), Ok(()));
    assert_eq!(stack(&second), [AppRoute::Home, AppRoute::Note { id: 2 }]);
    settle(&mut host);
}

pub(crate) fn a_router_inside_a_page_does_not_own_the_url() {
    #[derive(Routable, Debug, Clone, PartialEq)]
    enum Tab {
        #[route("/")]
        Inbox,
        #[route("/note/:id")]
        Note { id: u32 },
    }

    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let window = host.primary_window();
    let outer = Captured::<AppRoute>::default();
    let inner = Captured::<Tab>::default();
    let outer_router = {
        let (outer, inner) = (Rc::clone(&outer), Rc::clone(&inner));
        Router::new(
            AppRoute::Home,
            move |route: &AppRoute, ctx: &dyn BuildContext| {
                *outer.borrow_mut() = Router::<AppRoute>::of(ctx).ok();
                match route {
                    AppRoute::Home => router(Tab::Inbox, &inner).into_view().boxed(),
                    AppRoute::Note { .. } => SizedBox::new(10.0, 10.0).into_view().boxed(),
                }
            },
        )
    };
    let ((), log) = flui_testing::capture(|| {
        host.attach(&outer_router).expect("a fresh window");
        settle(&mut host);
    });
    assert_eq!(stack(&inner), [Tab::Inbox], "the inner Router is mounted");
    assert_eq!(
        log.count_containing(CLAIM_WARNING),
        0,
        "the inner Router makes no claim: {log}"
    );

    // Both route types match the path; the outer Router owns it.
    assert_eq!(host.navigate(window, push("/note/5")), Ok(()));
    assert_eq!(stack(&outer), [AppRoute::Home, AppRoute::Note { id: 5 }]);
    assert_eq!(stack(&inner), [Tab::Inbox]);
    settle(&mut host);
}

pub(crate) fn a_parser_that_navigates_again_is_refused_as_busy() {
    let host = Rc::new(RefCell::new(HeadlessHost::new(HeadlessWindow::new(
        400, 400,
    ))));
    let window = host.borrow().primary_window();
    let captured = Captured::default();
    host.borrow()
        .attach(&router(EqRoute::Home, &captured))
        .expect("a fresh window");
    settle(&mut host.borrow_mut());

    let inner = Rc::new(RefCell::new(None));
    {
        let host = Rc::downgrade(&host);
        let inner = Rc::clone(&inner);
        let hook: Box<dyn FnOnce()> = Box::new(move || {
            let host = host.upgrade().expect("the host is alive");
            *inner.borrow_mut() = Some(host.borrow().navigate(window, push("/note/9")));
        });
        ON_PARSE.with(|slot| *slot.borrow_mut() = Some(hook));
    }
    let outer = host.borrow().navigate(window, push("/note/3"));
    ON_PARSE.with(|slot| slot.borrow_mut().take());

    assert_eq!(*inner.borrow(), Some(Err(NavigationRefused::Busy)));
    assert_eq!(outer, Ok(()));
    assert_eq!(stack(&captured), [EqRoute::Home, EqRoute::Note(3)]);
    settle(&mut host.borrow_mut());
}

pub(crate) fn an_intent_during_a_build_is_busy_before_it_is_parsed() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let captured = Captured::default();
    let unknown = Rc::new(Cell::new(0));
    let answers = Rc::new(RefCell::new(Vec::new()));
    // A sibling of the Router: it reaches the slot the Router owns and
    // navigates while it builds.
    let sibling = NavigatesInBuild {
        answers: Rc::clone(&answers),
    };
    let owner = {
        let unknown = Rc::clone(&unknown);
        router(AppRoute::Home, &captured).on_unknown(move |_: &RoutePath| {
            unknown.set(unknown.get() + 1);
        })
    };
    host.attach(&Stack::new(vec![
        owner.into_view().boxed(),
        sibling.into_view().boxed(),
    ]))
    .expect("a fresh window");
    settle(&mut host);

    assert_eq!(
        *answers.borrow(),
        [Err(NavigationRefused::Busy), Err(NavigationRefused::Busy)]
    );
    assert_eq!(unknown.get(), 0, "no hook ran during the build");
    assert_eq!(stack(&captured), [AppRoute::Home]);
}

pub(crate) fn a_pop_intent_honours_the_pages_pop_scope() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let window = host.primary_window();
    let captured = Captured::<AppRoute>::default();
    let invoked = Rc::new(RefCell::new(Vec::new()));
    let guarded = {
        let (captured, invoked) = (Rc::clone(&captured), Rc::clone(&invoked));
        Router::new(
            AppRoute::Home,
            move |route: &AppRoute, ctx: &dyn BuildContext| {
                *captured.borrow_mut() = Router::<AppRoute>::of(ctx).ok();
                let page = SizedBox::new(10.0, 10.0);
                match route {
                    AppRoute::Home => page.into_view().boxed(),
                    AppRoute::Note { .. } => {
                        let invoked = Rc::clone(&invoked);
                        PopScope::new(page)
                            .can_pop(false)
                            .on_pop_invoked(move |_ctx, did_pop| invoked.borrow_mut().push(did_pop))
                            .into_view()
                            .boxed()
                    }
                }
            },
        )
    };
    host.attach(&guarded).expect("a fresh window");
    settle(&mut host);
    assert_eq!(host.navigate(window, push("/note/1")), Ok(()));
    settle(&mut host);

    assert_eq!(
        host.navigate(window, NavigationIntent::new(NavigationOp::Pop)),
        Ok(()),
        "the veto handled the Back"
    );
    settle(&mut host);
    assert_eq!(stack(&captured), [AppRoute::Home, AppRoute::Note { id: 1 }]);
    assert_eq!(
        *invoked.borrow(),
        [false],
        "the scope heard the refused pop"
    );
}

pub(crate) fn an_unmounted_router_lets_go_of_its_unknown_hook() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 400));
    let captured = Captured::<AppRoute>::default();
    let token = Rc::new(());
    let kept = Rc::downgrade(&token);
    let show = Rc::new(Cell::new(true));
    let toggle = Rc::new(ChangeNotifier::new());
    let root = {
        let show = Rc::clone(&show);
        // The hook holds the cell the Router's own handle is captured in: a
        // cycle through the Router's shared state.
        let held = Rc::clone(&captured);
        let owner = router(AppRoute::Home, &captured).on_unknown(move |_: &RoutePath| {
            let _ = (&token, &held);
        });
        AnimatedBuilder::new(
            Rc::clone(&toggle) as Rc<dyn flui_foundation::Listenable>,
            move || {
                if show.get() {
                    owner.clone().into_view().boxed()
                } else {
                    SizedBox::new(10.0, 10.0).into_view().boxed()
                }
            },
        )
    };
    host.attach(&root).expect("a fresh window");
    settle(&mut host);
    assert!(captured.borrow().is_some(), "the Router built its page");

    show.set(false);
    toggle.notify_listeners();
    settle(&mut host);
    drop((host, root, captured));
    assert!(
        kept.upgrade().is_none(),
        "the unmounted Router released its hook and the cycle through it"
    );
}

/// What each navigation answered.
type Answers = Rc<RefCell<Vec<Result<(), NavigationRefused>>>>;

/// Navigates through its presentation's URL-owner slot each time it builds,
/// to a path no route matches and to one a route does, and records the
/// answers.
#[derive(Clone, Debug, StatefulView)]
struct NavigatesInBuild {
    answers: Answers,
}

#[derive(Debug)]
struct NavigatesInBuildState {
    slot: flui_view::__runtime::UrlOwnerSlot,
}

impl StatefulView for NavigatesInBuild {
    type State = NavigatesInBuildState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        NavigatesInBuildState {
            slot: ctx
                .url_owner()
                .expect("the runtime installs the presentation's slot"),
        }
    }
}

impl ViewState<NavigatesInBuild> for NavigatesInBuildState {
    fn build(&self, view: &NavigatesInBuild, _ctx: &dyn BuildContext) -> impl IntoView {
        for location in ["/missing", "/note/1"] {
            let answer = self.slot.navigate(push(location));
            view.answers.borrow_mut().push(answer);
        }
        SizedBox::new(10.0, 10.0)
    }
}
