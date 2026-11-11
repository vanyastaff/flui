# ADR-0093: Router is the primary navigation API

- **Status:** Accepted (2026-11-04)
- **Date:** 2026-09-25
- **Supersedes:** [ADR-0024](ADR-0024-named-routes-seam.md) in full (string-named routes, their
  registration, `RouteKey<T>` and the named-route errors);
  [ADR-0019](ADR-0019-navigator-routing-seam.md) §2 (the navigator looked up from a build context
  through `Navigator::of`, `maybe_of` and `maybe_of_root`) and §3 only where the pop result
  crosses as `Box<dyn Any + Send>`, a mismatched type completes with `None` and `RouteResult<T>`
  is a `Future<Output = Option<T>>`
- **Related:** [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md) §6 (the
  router's stack restored from the application's own document),
  [ADR-0254](ADR-0254-window-contract.md) (the URLs the platform opens reach `AppHandler`),
  [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md) (a UI runtime
  hosts several presentations), [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §9 (the closed
  cross-thread vocabulary), [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the
  runtime names no widget-catalog type), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  (capabilities are acquired in lifecycle hooks), [ADR-0076](ADR-0076-public-overlay-mutation-api.md)
  (overlay entries), [ADR-0020](ADR-0020-transition-modal-route-seam.md),
  [ADR-0021](ADR-0021-hero-flight-seam.md), [ADR-0025](ADR-0025-local-history-seam.md) and
  [ADR-0064](ADR-0064-animation-completion-is-one-controller-resolved-future.md) (what a page
  keeps on the Router's stack), [ADR-0042](ADR-0042-theming-ownership.md) (`WidgetsApp` owns the
  routing), [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no process-global routing table)
- **Revised:** 2026-11-04 — rewritten before acceptance: `Navigator` becomes the Router's internal
  page stack instead of a frozen public facade (§4); there is no cross-thread navigation command,
  and an owner-thread `NavigationIntent` is addressed to a presentation (§5); a typed result
  without `Send` replaces `RouteKey<T>`; stack restoration is ADR-0262 §6; the URLs the platform
  opens reach `AppHandler` (ADR-0254) instead of a `Platform::on_open_urls` registration.

## Context and problem statement

An application navigates three ways on `main`, each with its own semantics for deep links and
restoration:

| What | Where | Users |
|:--|:--|:--|
| String-named routes (ADR-0024): `route`, `on_generate_route`, `on_unknown_route`, seven `*_named*` doors, `push_keyed`, `RouteKey<T>`, `RouteArguments` | `crates/flui-widgets/src/navigator/named_route.rs`, `navigator.rs` | none outside `flui-widgets`: its tests and `widgets_app.rs`'s docs |
| Imperative pushes on `NavigatorHandle` (ADR-0019): `push`, `pop_with`, `remove_route`, `maybe_pop`, …, found from `build` through `maybe_of`/`maybe_of_root` | `crates/flui-widgets/src/navigator/navigator.rs` (38 `pub fn`) | `show_dialog`, popup menus, `OpenContainer`, Material's back button and app bar |
| The typed `Router<R>` with `#[derive(Routable)]` | `crates/flui-widgets/src/router/`, `crates/flui-macros/src/derive_routable.rs` | the acceptance app (`tests/fixtures/acceptance_app/app.rs`: `Router::from_stack`, `WidgetsApp::router`) |

A pop result crosses the stack as `Box<dyn Any + Send>` and `RouteResult<T>` shares an
`Arc<Mutex<_>>` (`crates/flui-widgets/src/navigator/result.rs`), although only the owner thread
polls it; `tools/xtask/allowlists/send-positions.toml` lists 29 navigator positions.
`MaterialApp` and `CupertinoApp` take only `.navigator(…)`, so an application on Material roots
itself in `WidgetsApp::router` and gives up the design system's app shell. The cross-thread
navigation command (`UiCommand::Navigation`) had no producer and is deleted.

Flutter went through this twice: named routes are no longer recommended because they cannot
customise deep-link handling or support the browser's forward button, and page-backed routes
(Router) coexist with pageless ones (`Navigator.push`, `showDialog`) that are not deep-linkable
and are silently removed with the page below them. SwiftUI made the same correction when it
replaced `NavigationView` with `NavigationStack(path:)`, Compose Navigation moved from string
routes to type-safe ones, and go_router displaced Flutter's named routes.

Which navigation API is the application's, what is left of the Navigator, and how does code that
is not in a page's subtree navigate?

## Decision drivers

- one way to navigate: a second door is where the next widget goes ("Recurring defects": a
  replacement that leaves the replaced form alive);
- every page on the stack has a path, so deep links, restoration and web history are one
  mechanism;
- capabilities are acquired in `create_state`/`init_state`/`did_change_dependencies`, never looked up from
  `build` (ADR-0078);
- owner-thread state is `Rc`/`RefCell`, a `Send` bound appears only where a value leaves the
  thread ("Current forms");
- a UI runtime hosts several presentations (ADR-0043), so a navigation is addressed to one;
- the runtime and the agent protocol name no widget-catalog type (ADR-0083), and the cross-thread
  vocabulary stays closed (ADR-0027 §9).

## Considered options

1. The typed Router is the navigation API; `Navigator` becomes its internal page stack.
2. Router as a layer over the Navigator: named routes, imperative pushes and the Router all stay
   public.
3. Navigator stays primary, with URL synchronisation added on top.
4. Builder routes: `Router::new().route("/note/:id", |params| …)`.

## Decision

Chosen option: 1, because it leaves one navigation API whose every page is a value with a path.

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
shape.

`#[derive(Routable)]` (in `flui-macros`) generates `fn to_path(&self) -> RoutePath` and
`fn from_path(&RoutePath) -> Result<Self, RouteParseError>`; the trait provides
`fn parse(&str)` (`RoutePath::parse`, then `from_path`) and `fn back_stack(&RoutePath)`, the stack
a location opens with. Field types parse through `FromStr` and print through `Display`. The
derive's contract is the round trip: for every value `r`, `from_path(&r.to_path()) == Ok(r)`. A
path that matches no variant is a typed `RouteParseError::NoMatch`, never a panic and never a
silent fallback to the first variant. Builder-style route tables are not a second public way to
declare routes.

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
  `/s/new`, which parses as the literal variant; patterns that cross do the same, so beside
  `#[route("/s/:b")]`, `A { a: "s" }` of `#[route("/:a/new")]` prints `/s/new`, which parses as
  `B { b: "new" }`, because the literal at the first differing position ranks first. The derive
  cannot see which values a field type prints, so the overlap is documented on `Routable` rather
  than rejected; two patterns of the same shape, which overlap for every value, are rejected.
- **Nesting.** `#[nest("/settings")] Settings(SettingsRoute)` composes a child route enum into
  the parent's path: one stack, one URL.

### §2. The URL is the source of truth

`Router<R>` owns the navigation state as a value: a stack of `R`. The top of the stack, printed
through `to_path`, is the current location; the stack is what restoration saves and what a deep
link rebuilds. There is no other place the current page is recorded.

- **One URL per presentation.** The outermost `Router` in a presentation's tree is that
  presentation's URL owner (§5): the browser location on the web, the target of the URLs the
  platform opens for that window. A nested `Router` owns a stack that belongs to the page
  containing it: it is saved and restored with that page and does not contribute a second URL.
  A flow that needs no URL of its own (a wizard, a settings pane) is such a nested
  `Router<SubRoute>` with its local stack. A presentation with no `Router` has no URL.
- **Every push is addressable.** A route that enters the stack is an `R`, so it has a path.
  There is no pageless page.
- **Inbound URLs take the same path as a push.** A URL the platform opens (it reaches
  `AppHandler` on the owner thread, ADR-0254) or the browser's history moves to is parsed with
  `from_path` and applied as a navigation; a parse error is reported to the Router's
  `on_unknown` hook and leaves the stack unchanged.
- **Dialogs, popups, sheets and menus are not routes.** They are overlay entries (ADR-0076),
  owned by the page that opened them, removed when that page leaves the stack, and never
  present in the URL. This is the explicit answer to Flutter's pageless routes: the two kinds
  exist, but only one of them is navigation state.
- **Restoration** of the stack is ADR-0262 §6: `Router::from_stack` and `RouterHandle::stack`
  carry the whole typed stack, and the application stores it as paths in its own document.

The owner adopted this rule on 2026-09-25: every push is URL-addressable, and dialogs and
overlays are excluded.

### §3. The handle is acquired in a lifecycle hook and targets the nearest ancestor

```rust
fn create_state(&self, cx: &dyn LifecycleContext) -> NotesState {
    NotesState { router: Router::<AppRoute>::handle(cx).ok() }
}
// later, in a callback the state built:
router.push(AppRoute::Note { id: NoteId(1) })?;
```

`Router::<R>::handle` takes `&dyn LifecycleContext`, so it is reachable only from `create_state`,
`init_state` and `did_change_dependencies` (ADR-0078). It resolves the **nearest ancestor** `Router<R>` and
returns an owned, `!Send` `RouterHandle<R>` that shares the router's state, so no `GlobalKey` is
involved. With no `Router<R>` above, it returns `Err(RouterError::NoRouter)`. `push`, `replace`,
`pop` and `go` on the handle edit the stack; they take no event context, because the handle
already is the capability. The edit goes through the pure-data history flush of ADR-0019 §1 at
once, and the pages it adds build on the next frame.

`Router::of(w)` from an event callback is rejected: a callback has no position in the tree,
so "nearest ancestor" has no meaning there. A callback uses the handle its state captured.

A page's result is typed and stays on the owner thread:

```rust
impl<R: Routable> RouterHandle<R> {
    pub fn push_for_result<T: 'static>(&self, route: R) -> Result<RouteResult<T>, RouterError>;
    pub fn pop_with<T: 'static>(&self, value: T) -> Result<bool, RouterError>;
    pub fn maybe_pop(&self) -> bool; // honours `PopScope`
}
pub struct RouteResult<T> { /* Rc<RefCell<_>>: !Send */ }
impl<T: 'static> Future for RouteResult<T> {
    type Output = Result<Option<T>, RouteResultError>; // `Ok(None)`: left without a value
}
#[non_exhaustive]
pub enum RouteResultError { TypeMismatch { expected: &'static str } }
```

The value crosses the stack as `Box<dyn Any>` in the result's `Rc`/`RefCell` channel, and a
value of another type is `RouteResultError::TypeMismatch`, not a silent `None`. A page removed
without a value (replaced, swept, its Router unmounted) completes `Ok(None)`, so no awaiting
caller hangs. System back goes to the URL-owning Router's `maybe_pop`.

A widget that pops its own page without knowing `R` (Material's back button, a close button)
acquires a `PageHandle` in a lifecycle hook: `can_pop` and `maybe_pop` of the page it is on,
erased of the route type.

### §4. Navigator becomes the Router's internal page stack

The Router places its pages on a `Navigator` it builds. Transitions, the overlay, hero flights,
local history and completion futures (ADR-0020, ADR-0021, ADR-0025, ADR-0064) keep working,
because they are properties of the pages on that stack.

- **Public:** only the page-presentation extensions, which Material and Cupertino transitions
  need: `Route`, `PageRoute` and `PageTransitions`, with the transition builders a page takes.
- **Removed:** the imperative `NavigatorHandle` doors (`push`, `pop_with`, `remove_route`,
  `maybe_pop`, `push_replacement`, `push_and_remove_until`, …), `NavigatorHandle::maybe_of` and
  `maybe_of_root`, `WidgetsApp::navigator` and `NavigatorForm`. They are removed once dialogs,
  popup menus, context menus and `OpenContainer` move to the overlay (§2), not kept as a thin
  facade: a second way to push is where the next widget would go. Until then the navigator a
  Router builds refuses every page pushed through its facade and admits pageless popups only.
- **Removed with no replacement:** the named routes of ADR-0024: `named_route.rs`, the
  `*_named*` doors, `push_keyed`, `route`, `on_generate_route`, `on_unknown_route`,
  `RouteArguments`, `RouteSettings::arguments` and `RouteKey<T>`. A name has no type and no
  path; a typed result is `push_for_result` (§3), so `RouteKey<T>` has nothing left to bind.
- **App shells:** `MaterialApp::router(Router<R>)` and `CupertinoApp::router(Router<R>)` return a
  router form of the shell, as `WidgetsApp::router` returns `WidgetsApp<RouterForm>`, which has no
  navigator or observer builder, so a router app given a navigator does not compile.

### §5. Navigation is an owner-thread intent, not a cross-thread command

There is no cross-thread navigation command. A navigation that does not come from a page's own
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

The presentation's URL owner is its outermost `Router`, which registers itself through a
`RouterScope` in `init_state` into the presentation's one `UrlOwner` slot, declared in
`flui-view` so the runtime names no widget-catalog type (ADR-0083). An intent for a presentation
that is gone, or has no URL owner, is refused, never applied to another window's Router. A path
that no route matches goes to the Router's `on_unknown` and is refused. A `navigate` that
re-enters the Router while it commits an edit is `Busy`, not a second edit inside the first.

The sources of an intent are on the owner thread already: the URLs the platform opens reach
`AppHandler` (ADR-0254), the development agent's window methods arrive through
`UiCommand::Agent`. A thread that needs to navigate writes a signal (`UiCommand::SignalWrite`,
through a `SignalSender`), and code on the owner thread that reads it navigates. This keeps
ADR-0027 §9's vocabulary closed: no second channel without a producer.

### Consequences

- Good, because deep links, restoration and web history get one mechanism: parse a path, set the
  stack.
- Good, because one way to navigate leaves the next widget no older door to copy.
- Good, because a result is typed and owner-local: no `Send` bound, no lock, and a wrong type is
  an error the caller sees.
- Good, because the runtime's vocabulary names no `flui-widgets` type and has no navigation
  channel without a producer.
- Bad, because the public API narrows further than the text the owner approved on 2026-09-25:
  `Navigator` and `NavigatorHandle` stop being public, and code that pushes an arbitrary
  `NavigatorRoute` has to become a page of a `Router<R>` or an overlay entry. Reversible before
  1.0.
- Bad, because named routes break with no shim. They have no users outside `flui-widgets`;
  migration is one enum variant per name, a `#[route]` per variant, and `push_named` calls
  becoming `push(AppRoute::…)`.
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
  (`from_stack_restores_back_order`, `stack_reads_every_committed_edit`, `empty_stack_is_refused`).
- §3: the `compile_fail` doctest on `Router::handle` beside its compiling twin; the commit order
  under re-entrant observers (`router_push_commits_before_a_reentrant_observer_pop` and its
  `replace` and `go` siblings, `router_push_preserves_its_commit_after_an_observer_panic`); the
  typed result in `crates/flui-widgets/tests/router.rs`:
  `push_for_result_resolves_to_the_value_the_page_is_popped_with` (an `Rc` value, which is not
  `Send`), `a_router_result_of_another_type_is_a_type_mismatch`,
  `a_page_left_without_a_value_resolves_to_none` (a pop, a replace, a `go`),
  `an_unmounted_router_resolves_its_pending_results_to_none` and the `terminal-result-waker` row
  of `router_observer_failure_and_terminal_retirement`, with the `compile_fail` doctest on
  `RouteResult` that it is not `Send`.
- §4: `widgets_app_router_navigates_by_handle_and_the_url_follows` and
  `switching_widgets_app_from_home_to_router_releases_the_navigator`
  (`crates/flui-widgets/tests/widgets_app_router.rs`), and the compile-fail case
  `router_app_takes_no_navigator`. Once the doors are removed, `cargo xtask send-positions`
  holds the navigator's rows at zero, and the change that removes them adds the same compile-fail
  case for `MaterialApp::router` and `CupertinoApp::router`.
- §5: the change that builds `UiRuntime::navigate` adds a test in which, in one UI runtime with
  two presentations, an intent addressed to the second changes that presentation's Router and not
  the first's, and an intent for a presentation without a Router is `NoUrlOwner`.
- The Navigator's own lifecycle order stays pinned by `crates/flui-widgets/tests/navigator.rs`
  and `navigator_public.rs`, which move onto a Router with the doors.

What of this is not built yet is [Accepted ADR sections not built yet](../gaps.md#accepted-adr-sections-not-built-yet), "ADR-0093".

## Pros and cons of the options

### 1. The typed Router, Navigator internal

- Good, because there is one API, and every page has a path.
- Good, because the same shape as Compose's type-safe navigation, SwiftUI's
  `NavigationStack(path:)` and Dioxus's `#[derive(Routable)]`.
- Bad, because every imperative caller (dialogs, menus, `OpenContainer`, back buttons) moves
  before the doors can go.

### 2. Router over a public Navigator

- Good, because nothing moves.
- Bad, because it is Flutter's Navigator 1/2 split: three APIs with different deep-link and
  restoration semantics, and a push that bypasses the URL stays possible.

### 3. Navigator primary, URL synchronised on top

- Bad, because pushes that bypass the URL stay possible, which is the pageless-route problem
  again.

### 4. Builder routes

- Bad, because the path and the variant can drift apart, parameters arrive as strings, and it is
  a second way to declare what the derive already expresses.

Also rejected: dialogs and popups as routes (a URL that opens a confirmation dialog is rarely
wanted, and it would pull transient overlays into restoration), and `Router::of(w)` resolved from
the `Writer` of a callback (a callback has no tree position, so it could only mean "the primary
presentation's router", which is wrong in a multi-window app).

## More information

Flutter's `Router` takes a `RouteInformationParser` and a `RouterDelegate` written by hand; here
both are derived from the route type. The observable contract kept is Flutter's:
nearest-ancestor lookup, deep links through the same parse path, browser back and forward on the
web. Flutter keeps pageless routes as a first-class kind; here overlays are outside the
navigation state (§2).

The order of the change: the router core and the derive with `WidgetsApp::router`; the typed
result (`push_for_result`, `pop_with`, `RouteResult` without `Send`); `MaterialApp::router` and
`CupertinoApp::router`; the named routes removed; the URL owner and inbound links
(`RouterScope`, `UiRuntime::navigate`, `on_unknown`); a modal surface outside routes, under its
own ADR, for `show_dialog`, popup and context menus, the selection popup and `OpenContainer`;
then the `NavigatorHandle` doors and `WidgetsApp::navigator` removed. Nesting (`#[nest]`), guards
(`Router::guard(|from: Option<&R>, to: &R| -> Guard<R>)` with `Guard::{Allow, Redirect(R), Deny}`,
synchronous, more than five redirects being `RouterError::RedirectLoop`), and web history
(`pushState`/`popstate`, query and fragment in `RoutePath`) follow as features.

Sources: decision D14 in the
[decision index](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/design/decisions.md);
the [architecture review](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-09-25-architecture-review/report-architecture.ru.md)
and its survey
[`market/flutter_compose_swiftui.md`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-09-25-architecture-review/market/flutter_compose_swiftui.md).
