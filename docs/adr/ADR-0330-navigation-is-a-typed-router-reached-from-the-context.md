# ADR-0330: Navigation is a typed Router, reached from the context, with dialogs as pageless popups

- **Status:** Accepted (2026-11-13)
- **Date:** 2026-11-13
- **Supersedes:** [ADR-0093](ADR-0093-router-is-the-primary-navigation-api.md) in full (it stated
  the Router as the one navigation API, with its handle acquired only in a lifecycle hook and
  dialogs moved to a modal surface outside routes); through it,
  [ADR-0024](ADR-0024-named-routes-seam.md) in full and
  [ADR-0019](ADR-0019-navigator-routing-seam.md) §2 and the result clauses of §3
- **Related:** [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md) §6 (the
  router's stack restored from the application's own document),
  [ADR-0254](ADR-0254-window-contract.md) (the URLs the platform opens reach `AppHandler`),
  [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md) (a UI runtime
  hosts several presentations), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §9 (the closed
  cross-thread vocabulary), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the
  runtime names no widget-catalog type), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  (what a lifecycle context carries), [ADR-0076](ADR-0076-public-overlay-mutation-api.md)
  (`Overlay::of`, the inherited lookup this follows), [ADR-0086](ADR-0086-signal-writes-through-event-context.md)
  §7 (a mutation from `build` is refused), [ADR-0020](ADR-0020-transition-modal-route-seam.md),
  [ADR-0021](ADR-0021-hero-flight-seam.md), [ADR-0025](ADR-0025-local-history-seam.md) and
  [ADR-0064](ADR-0064-animation-completion-is-one-controller-resolved-future.md) (what a page
  keeps on the Router's stack), [ADR-0042](ADR-0042-theming-ownership.md) (`WidgetsApp` owns the
  routing), [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md)
  (owner-thread state carries no `Send`)

## Context and problem statement

FLUI follows Flutter's widget and composition model, and a departure needs a Rust reason or a
measured gain. Navigation on `main` had three public forms, with three meanings for deep links
and restoration:

| What | Flutter counterpart | Users |
|:--|:--|:--|
| String-named routes: `push_named`, `on_generate_route`, `RouteKey<T>`, `RouteArguments` | `Navigator.pushNamed`, `onGenerateRoute`, which Flutter no longer recommends | `flui-widgets`' own tests |
| Imperative doors on `NavigatorHandle` (`push`, `pop_with`, `remove_route`, …), looked up with `maybe_of`/`maybe_of_root` | `Navigator.of(context).push(MaterialPageRoute(…))` | `show_dialog`, popup and context menus, `OpenContainer`, Material's back button and app bar |
| The typed `Router<R>` with `#[derive(Routable)]` | `MaterialApp.router` with go_router's typed routes (`@TypedGoRoute`, `GoRouteData`) | the acceptance application, `WidgetsApp::router` |

ADR-0093 chose the Router and removed the named routes. It also departed from Flutter twice
where nothing in Rust asked for it: a handle came only from a lifecycle hook, where a Flutter
author writes `GoRouter.of(context)` and `ModalRoute.of(context)` in `build`; and dialogs were to
leave the route stack for a modal surface of their own, where Flutter's `showDialog` pushes a
pageless `DialogRoute` that already carries the barrier, the focus trap, the back key and the
result. `MaterialApp` and `CupertinoApp` took only `.navigator(…)`, so an application on a design
system could not use the Router at all.

Which navigation API does an application use, how does a widget reach it, and where do dialogs
live?

## Decision drivers

- one way to navigate: a second door is where the next widget goes ("Recurring defects": a
  replacement that leaves the replaced form alive);
- an author writes navigation as Flutter does, and every difference has a Rust reason;
- every page on the stack has a path, so deep links, restoration and web history are one
  mechanism;
- owner-thread state is `Rc`/`RefCell`, and a `Send` bound appears only where a value leaves the
  thread (ADR-0323);
- `build` describes the tree and drives nothing (ADR-0086 §7);
- a UI runtime hosts several presentations (ADR-0043), so a navigation is addressed to one;
- the runtime and the agent protocol name no widget-catalog type (ADR-0083), and the cross-thread
  vocabulary stays closed (ADR-0027 §9).

## Considered options

1. The typed Router is the one navigation API, reached from the build context like Flutter's
   `GoRouter.of` and `ModalRoute.of`; dialogs and menus are pageless popup routes on the
   navigator beneath it, as in Flutter.
2. ADR-0093 as written: the handle only from a lifecycle hook, and a modal surface outside routes
   for dialogs and menus.
3. Flutter's whole surface: the Router, plus a public `Navigator.push` and named routes beside it.
4. Builder routes: `Router::new().route("/note/:id", |params| …)`.

## Decision

Chosen option: 1, because it leaves one navigation API whose every page is a value with a path,
written the way a Flutter author writes it.

### §1. Routes are a derived, typed enum

An application declares its routes as a type and derives the mapping to and from a path:

```rust
#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/")] Home,
    #[route("/note/:id")] Note { id: NoteId },
}
```

The trait and its derive are named `Routable`, not `Route`: `flui_widgets::Route` is the
page-presentation trait (§4), and a derive named `Route` that implements another trait would break
the convention that a derive carries its trait's name. Dioxus uses the same name for the same
shape, and go_router_builder's `GoRouteData` is the Flutter counterpart.

`#[derive(Routable)]` (in `flui-macros`) generates `fn to_path(&self) -> RoutePath` and
`fn from_path(&RoutePath) -> Result<Self, RouteParseError>`; the trait provides
`fn parse(&str)` (`RoutePath::parse`, then `from_path`) and `fn back_stack(&RoutePath)`, the stack
a location opens with. Field types parse through `FromStr` and print through `Display`. The
derive's contract is the round trip: for every value `r`, `from_path(&r.to_path()) == Ok(r)`. A
path that matches no variant is a typed `RouteParseError::NoMatch`, never a panic and never a
silent fallback to the first variant. Builder-style route tables are not a second public way to
declare routes, and there are no named routes: a name is a variant, its arguments are the
variant's fields.

- **Patterns.** Each variant carries exactly one `#[route("…")]`. A pattern starts with `/`
  (`/` alone is the root) and has no empty segment, no trailing `/`, and no `?`, `#` or `%`. A
  `:name` segment fills the field `name` (`:type` fills `r#type`); every other segment is
  literal text, which `RoutePath` encodes, so a non-ASCII literal is written as itself. Every
  parameter names a field exactly once, and every field is a parameter.
- **Compile-time rejections.** A struct, union or generic type; a tuple variant (nesting is a
  `#[nest]` variant, below); a variant with no `#[route]`, with two, or with anything after the
  pattern; a malformed pattern; a parameter without a field or a field without a parameter; a
  field type that is not `FromStr + Display` (reported on the field); and two patterns of the
  same shape (the same length, the same literals and parameters in the same places), because
  the second could never be parsed.
- **Specificity, not declaration order.** `from_path` tries the patterns shortest first, then
  segment by segment with a literal ranking above a parameter, so `/s/new` wins over `/s/:slug`
  wherever either is declared. A location no pattern matches is `NoMatch`; when a pattern's
  literals match but a segment does not parse as its field, the first such failure is
  `RouteParseError::Param`.
- **The round trip, qualified.** The derive keeps `from_path(&r.to_path()) == Ok(r)` for every
  value whose fields print non-empty (`RoutePath::join` drops an empty segment), whose
  `Display` output `FromStr` reads back as the same value, and whose printed path no pattern
  tried before its own also matches. Beside `#[route("/s/new")]`, `Slug { slug: "new" }` prints
  `/s/new`, which parses as the literal variant; patterns that cross do the same. The derive
  cannot see which values a field type prints, so the overlap is documented on `Routable` rather
  than rejected; two patterns of the same shape, which overlap for every value, are rejected.
- **Nesting.** `#[nest("/settings")] Settings(SettingsRoute)` composes a child route enum into
  the parent's path: one stack, one URL.

### §2. The URL is the source of truth, and dialogs are pageless popups

`Router<R>` owns the navigation state as a value: a stack of `R`. The top of the stack, printed
through `to_path`, is the current location; the stack is what restoration saves and what a deep
link rebuilds. There is no other place the current page is recorded.

- **One URL per presentation.** The outermost `Router` in a presentation's tree is that
  presentation's URL owner (§5): the browser location on the web, the target of the URLs the
  platform opens for that window. A nested `Router` owns a stack that belongs to the page
  containing it: it is saved and restored with that page and does not contribute a second URL.
  A flow that needs no URL of its own (a wizard, a settings pane) is such a nested
  `Router<SubRoute>` with its local stack. A presentation with no `Router` has no URL.
- **Every page is addressable.** A page that enters the stack is an `R`, so it has a path.
- **Inbound URLs take the same path as a push.** A URL the platform opens (it reaches
  `AppHandler` on the owner thread, ADR-0254) or the browser's history moves to is parsed with
  `from_path` and applied as a navigation; a parse error is reported to the Router's
  `on_unknown` hook and leaves the stack unchanged.
- **Dialogs, popups, sheets and menus are pageless popup routes.** `show_dialog`, a popup menu,
  a context menu and `OpenContainer` push a `PopupRoute<T>` on the navigator beneath the Router,
  as Flutter's `showDialog` pushes a `DialogRoute`. A popup belongs to the page beneath it: a
  `replace`, a `go` or a removal of that page takes its popups with it, and they are never in the
  URL, in `RouterHandle::stack` or in what restoration saves. The barrier, the focus scope, the
  back and dismiss keys and the typed result are the route's, so a dialog needs no second
  mechanism for any of them.
- **Restoration** of the stack is ADR-0262 §6: `Router::from_stack` and `RouterHandle::stack`
  carry the whole typed stack, and the application stores it as paths in its own document.

### §3. A widget reaches the Router and its page from the build context

```rust
fn build(&self, _view: &NoteList, ctx: &dyn BuildContext) -> impl IntoView {
    let router = Router::<AppRoute>::of(ctx).expect("BUG: the list is under the app's Router");
    let page = PageHandle::of(ctx).expect("BUG: the list is on a page");
    Button::new(Text::new("Open"))
        .on_pressed(move |_ctx| { let _ = router.push(AppRoute::Note { id: NoteId(1) }); })
}
```

- **`Router::<R>::of(ctx: &dyn BuildContext) -> Result<RouterHandle<R>, RouterError>`** is
  Flutter's `GoRouter.of(context)`: it resolves the nearest ancestor `Router<R>` through the
  inherited `RouterScope<R>` the Router places, registers a dependency on it, and returns an
  owned, `!Send` `RouterHandle<R>` that shares the router's state. With no `Router<R>` above it
  answers `Err(RouterError::NoRouter)`.
- **`PageHandle::of(ctx: &dyn BuildContext) -> Option<PageHandle>`** is Flutter's
  `ModalRoute.of(context)` with the `Navigator.pop(context)` family on it, erased of the route
  type: the route the widget is on, a page or a popup. Every modal route places an inherited
  `PageScope` above its barrier and page; it notifies its dependents when the route becomes or
  stops being current or gains or loses the route below it, because the navigator rebuilds a
  route's entry each time it tells the route of a new neighbour (Flutter's
  `ModalRoute.didChangePrevious` → `changedInternalState`). `can_pop` is whether a route sits
  below this one to return to (Flutter's `impliesAppBarDismissal`), and `is_current` whether it
  is on top. The handle names its route, not the top of the stack: `maybe_pop() -> bool` honours
  `PopScope`, and it, `pop()` and `pop_with(value)` (both `-> Result<bool, PageError>`) act only
  while that route is on top, so a callback that fires after a popup covered its page does not
  pop the popup. `push_popup(PopupRoute<T>) -> Result<RouteResult<T>, PageError>` pushes a
  pageless popup that belongs to it. `None` outside every route.
- **The handle is what a callback captures.** A Flutter callback keeps its `BuildContext`, an
  element reference the garbage collector keeps alive. In Rust `&dyn BuildContext` borrows the
  element tree for the length of one `build`, and an event callback is a `'static` closure that
  runs later with an `EventContext` that has no tree position, so it cannot name "the nearest
  ancestor". The callback moves the owned handle `build` resolved; that is the Rust shape of
  Flutter's captured context, and the only difference an author sees.
- **A lifecycle context is a build context**, so a state that keeps a handle resolves it the same
  way in `did_change_dependencies`. There is no second, lifecycle-only door.
- **`build` drives nothing.** A `RouterHandle` or `PageHandle` mutation made while its UI runtime
  builds is refused with `RouterError::DuringBuild` or `PageError::DuringBuild` (`maybe_pop`
  answers `false`) and reported, as ADR-0086 §7 refuses a controller mutation from `build`; the
  stack is unchanged.
- **Edits are synchronous.** `push`, `replace`, `pop` and `go` on the handle edit the stack and go
  through the pure-data history flush of ADR-0019 §1 at once; the pages they add build on the next
  frame.

A page's result is typed and stays on the owner thread:

```rust
impl<R: Routable> RouterHandle<R> {
    pub fn push_for_result<T: 'static>(&self, route: R) -> Result<RouteResult<T>, RouterError>;
    pub fn pop_with<T: 'static>(&self, value: T) -> Result<bool, RouterError>;
}
pub struct RouteResult<T> { /* Rc<RefCell<_>>: !Send */ }
impl<T: 'static> Future for RouteResult<T> {
    type Output = Result<Option<T>, RouteResultError>; // `Ok(None)`: left without a value
}
#[non_exhaustive]
pub enum RouteResultError { TypeMismatch { expected: &'static str } }
```

The value crosses the stack as `Box<dyn Any>` in the result's `Rc`/`RefCell` channel, and a
value of another type is `RouteResultError::TypeMismatch`, not a silent `None`. A page or popup
removed without a value (replaced, swept, its Router unmounted) completes `Ok(None)`, so no
awaiting caller hangs. The Back key reaches the Router through its pages' `BackIntent`, which
honours `PopScope` and bubbles to an enclosing navigator.

### §4. Navigator is the Router's internal page stack

The Router places its pages on a `Navigator` it builds. Transitions, the overlay, hero flights,
local history and completion futures (ADR-0020, ADR-0021, ADR-0025, ADR-0064) keep working,
because they are properties of the routes on that stack.

- **Public:** the route-presentation types a page or popup is built from: `Route`, `PageRoute`,
  `PopupRoute`, `PageTransitions` and the transition builders, `PopScope`, `Hero` and its
  controller types, `RouteResult` and `PageHandle`.
- **Removed:** `Navigator`, `NavigatorHandle` and `NavigatorState` as public items, with every
  imperative door (`push`, `pop_with`, `remove_route`, `maybe_pop`, `push_replacement`,
  `push_and_remove_until`, …), `maybe_of` and `maybe_of_root`; `WidgetsApp::navigator` and
  `NavigatorForm`; and the named routes of ADR-0024 (`*_named*`, `push_keyed`, `route`,
  `on_generate_route`, `on_unknown_route`, `RouteArguments`, `RouteSettings::arguments`,
  `RouteKey<T>`). A page is a route value (§1), a popup goes through `PageHandle::push_popup`,
  and a pop goes through the handle of the route it leaves.
- **App shells.** `WidgetsApp`, `MaterialApp` and `CupertinoApp` take `new(home)`, Flutter's
  `MaterialApp(home:)`: one page with no URL, on which popups open; and `router(Router<R>)`,
  Flutter's `MaterialApp.router`, whose Router owns the app's only navigator. No shell takes a
  navigator handle or observers: a page is a route value, and an app with no Router has one page.

### §5. Navigation is an owner-thread intent, not a cross-thread command

There is no cross-thread navigation command. A navigation that does not come from a widget's own
handle is a value on the owner thread, addressed to a presentation:

```rust
// flui-view, beside `RoutePath`, which moves down from flui-widgets
#[non_exhaustive]
pub enum NavigationOp { Push(RoutePath), Replace(RoutePath), Pop, Go(RoutePath) }
pub struct NavigationIntent { pub op: NavigationOp }
pub trait UrlOwner: 'static { // !Send: owner-thread state
    fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused>;
}
#[non_exhaustive]
pub enum NavigationRefused { NoPresentation, NoUrlOwner, NoMatch(RoutePath), Busy }

// flui-runtime
impl UiRuntime {
    pub fn navigate(&self, presentation: PresentationId, intent: NavigationIntent)
        -> Result<(), NavigationRefused>;
}
```

The presentation's URL owner is its outermost `Router`, which registers itself through its
`RouterScope` into the presentation's one `UrlOwner` slot, declared in `flui-view` so the runtime
names no widget-catalog type (ADR-0083). An intent for a presentation that is gone, or has no URL
owner, is refused, never applied to another window's Router. A path that no route matches goes to
the Router's `on_unknown` and is refused. A `navigate` that re-enters the Router while it commits
an edit is `Busy`, not a second edit inside the first.

The sources of an intent are on the owner thread already: the URLs the platform opens reach
`AppHandler` (ADR-0254), the development agent's window methods arrive through
`UiCommand::Agent`. A thread that needs to navigate writes a signal (`UiCommand::SignalWrite`,
through a `SignalSender`), and code on the owner thread that reads it navigates. This keeps
ADR-0027 §9's vocabulary closed: no second channel without a producer.

### Consequences

- Good, because deep links, restoration and web history get one mechanism: parse a path, set the
  stack.
- Good, because one way to navigate leaves the next widget no older door to copy.
- Good, because an author writes `Router::of(ctx)`, `PageHandle::of(ctx)` and `show_dialog` where
  a Flutter author writes `GoRouter.of(context)`, `ModalRoute.of(context)` and `showDialog`.
- Good, because a dialog keeps the barrier, focus trap, back key and typed result its route
  already has, instead of a second modal mechanism.
- Good, because a result is typed and owner-local: no `Send` bound, no lock, and a wrong type is
  an error the caller sees.
- Bad, because `Navigator.push(MaterialPageRoute(…))` has no counterpart: a page is a route value
  of a `Router<R>`, and code that pushed an arbitrary page becomes a variant. Reversible before
  1.0.
- Bad, because named routes break with no shim: one enum variant per name, a `#[route]` per
  variant, and `push_named` calls becoming `push(AppRoute::…)`.
- Neutral, because a saveable-state contract shared by the back stack, lazy-list eviction and
  hot reload (Compose's `rememberSaveable`) is not decided here; a page's state lives while the
  page is on the stack.

### Confirmation

- §1: `derived_routable_round_trips` (`crates/flui-widgets/tests/routable_derive.rs`), a property
  test over a derived route enum with multi-parameter, string, literal-sibling and non-ASCII
  patterns; `crates/flui-widgets/tests/routable_ui.rs` compiles the accepted shapes
  (`routable_ui/pass/`) and every rejection against its diagnostic (`routable_ui/fail/`); the
  pattern parser, specificity order and conflict detection in
  `crates/flui-macros/src/derive_routable/pattern.rs`.
- §2: in `crates/flui-widgets/tests/router.rs`, rows of the `router_and_widgets_app` table of
  `tests/contracts.rs`: `router_opens_at_a_location_with_its_back_stack`,
  `a_deep_link_adds_its_lower_page_in_the_frame_its_top_settles`,
  `popup_routes_are_admitted_and_leave_the_location_alone`, `stack_leaves_out_popups`,
  `router_never_pops_its_last_page`, and the restoration rows of ADR-0262 §6
  (`from_stack_restores_back_order`, `stack_reads_every_committed_edit`, `empty_stack_is_refused`);
  `show_dialog`'s suite in `packages/flui-material/tests/show_dialog.rs`.
- §3: `router_of_resolves_the_nearest_router_from_build`, `a_mutation_from_build_is_refused`,
  `page_handle_pops_only_its_own_route`, `page_handle_can_pop_follows_the_route_below` and
  `a_popup_pushed_from_a_page_resolves_to_its_value` in `crates/flui-widgets/tests/router.rs`;
  Material's `tapping_the_implied_back_button_pops_the_route`, whose bottom page implies no back
  button; the commit order under re-entrant observers
  (`router_push_commits_before_a_reentrant_observer_pop` and its `replace` and `go` siblings,
  `router_push_preserves_its_commit_after_an_observer_panic`); the typed result:
  `push_for_result_resolves_to_the_value_the_page_is_popped_with` (an `Rc` value, which is not
  `Send`), `a_router_result_of_another_type_is_a_type_mismatch`,
  `a_page_left_without_a_value_resolves_to_none` (a pop, a replace, a `go`),
  `an_unmounted_router_resolves_its_pending_results_to_none` and the `terminal-result-waker` row
  of `router_observer_failure_and_terminal_retirement`, with the `compile_fail` doctest on
  `RouteResult` that it is not `Send`.
- §4: `widgets_app_router_navigates_by_handle_and_the_url_follows` and
  `switching_widgets_app_from_home_to_router_releases_the_navigator`
  (`crates/flui-widgets/tests/widgets_app_router.rs`) and `home_is_seeded_once_as_the_root_route`
  (`crates/flui-widgets/tests/widgets_app.rs`); Material's `page_transitions` suite and `OpenContainer`'s
  `it_opens_under_a_router` run on `MaterialApp::router`; `api/flui.txt` (ADR-0324) lists no
  `Navigator`, `NavigatorHandle` or
  named-route item, and `cargo xtask send-positions` holds the navigator's rows at zero.
- §5: the change that builds `UiRuntime::navigate` adds a test in which, in one UI runtime with
  two presentations, an intent addressed to the second changes that presentation's Router and not
  the first's, and an intent for a presentation without a Router is `NoUrlOwner`.
- `examples/navigation.rs` (`cargo check --example navigation --features material`): a push, a
  page popped with a result, and a dialog.

What of this is not built yet is [Accepted ADR sections not built yet](../gaps.md#accepted-adr-sections-not-built-yet), "ADR-0330".

## Pros and cons of the options

### 1. The typed Router, reached from the context, with pageless popups

- Good, because there is one API, and every page has a path.
- Good, because it is the shape Flutter recommends (`MaterialApp.router`, go_router's typed
  routes, `showDialog`), and the same as Compose's type-safe navigation, SwiftUI's
  `NavigationStack(path:)` and Dioxus's `#[derive(Routable)]`.
- Bad, because every imperative caller (dialogs, menus, `OpenContainer`, back buttons) moves to
  `PageHandle` before the doors can go.

### 2. ADR-0093 as written

- Good, because a handle acquired once in a lifecycle hook costs no lookup per build.
- Bad, because an author cannot write `GoRouter.of(context)` or `ModalRoute.of(context)` in
  `build`; the lookup this saves is one inherited-map read, the cost `Theme::of` already pays.
- Bad, because a modal surface outside routes rebuilds the barrier, focus trap, back key and
  result a `PopupRoute` already has, and is a second modal mechanism beside it.

### 3. Flutter's whole surface

- Good, because nothing moves.
- Bad, because it is Flutter's Navigator 1/2 split: three APIs with different deep-link and
  restoration semantics, and a push that bypasses the URL stays possible. Flutter's own
  documentation steers applications away from two of the three.

### 4. Builder routes

- Bad, because the path and the variant can drift apart, parameters arrive as strings, and it is
  a second way to declare what the derive already expresses.

Also rejected: dialogs and popups as Router routes (a URL that opens a confirmation dialog is
rarely wanted, and it would pull transient overlays into restoration), and a navigation call on
an `EventContext` (a callback has no tree position, so it could only mean "the primary
presentation's router", which is wrong in a multi-window app).

## More information

Flutter's `Router` takes a `RouteInformationParser` and a `RouterDelegate` written by hand; here
both are derived from the route type. The observable contract kept is Flutter's:
nearest-ancestor lookup, pageless routes removed with the page beneath them, deep links through
the same parse path, browser back and forward on the web.

The order of the change: the named routes removed; `Router::of`, `PageHandle` and the popup
callers moved onto it, with `MaterialApp::router` and `CupertinoApp::router`; then `Navigator`,
`NavigatorHandle`, `WidgetsApp::navigator` and `NavigatorForm` leave the public API, and the
navigator's state moves to `Rc`/`RefCell`; then the URL owner and inbound links (`RouterScope`
as the `UrlOwner`, `UiRuntime::navigate`, `on_unknown`). Nesting (`#[nest]`), guards
(`Router::guard(|from: Option<&R>, to: &R| -> Guard<R>)` with `Guard::{Allow, Redirect(R), Deny}`,
synchronous, more than five redirects being `RouterError::RedirectLoop`), and web history
(`pushState`/`popstate`, query and fragment in `RoutePath`) follow as features.
