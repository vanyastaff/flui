# ADR-0093: Router is the primary navigation API

- **Status:** Proposed
- **Date:** 2026-09-25
- **Supersedes (on acceptance):** [ADR-0024](ADR-0024-named-routes-seam.md) (string-named
  routes; `RouteKey<T>` of its §3 carries over)
- **Supersedes in part (on acceptance):** [ADR-0019](ADR-0019-navigator-routing-seam.md) (the
  Navigator-first shape; its §1 pure-data history flush and §3 `dyn Any` pop result carry over)
- **Related:** [ADR-0020](ADR-0020-transition-modal-route-seam.md),
  [ADR-0021](ADR-0021-hero-flight-seam.md), [ADR-0025](ADR-0025-local-history-seam.md),
  [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §9 (navigation commands in the closed command
  vocabulary), [ADR-0042](ADR-0042-theming-ownership.md) (`WidgetsApp` owns the navigator),
  [ADR-0064](ADR-0064-animation-completion-is-one-controller-resolved-future.md),
  [ADR-0076](ADR-0076-public-overlay-mutation-api.md),
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)
- **Refs:** decision D14 in the [decision index](../../design/decisions.md); the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md)

This record changed nothing in `crates/` when it was written. Step one of the
[implementation series](#implementation-series) is in `crates/flui-widgets/src/router/`.

## Context

Both navigation ADRs already say they are waiting for this one. ADR-0019's status line says
the typed Router "will supersede the Navigator-first shape recorded here", and ADR-0024 is
Deprecated with "`RouteKey<T>` (§3) is the part to carry over". What exists today is the
Navigator those two records describe:

- **An imperative stack with many front doors.** `NavigatorHandle` carries 19 mutation entry
  points between `push` (`crates/flui-widgets/src/navigator/navigator.rs:1158`) and
  `push_named_and_remove_until` (`navigator.rs:2283`): typed pushes, replacements, pops with and
  without results, `pop_until`, and the named-route doors of ADR-0024: seven `*_named*` methods plus
  `push_keyed`, from `push_named` (`navigator.rs:1953`) to `push_named_and_remove_until`
  (`navigator.rs:2283`). 43 `pub fn` in `navigator.rs` alone.
- **No addressable state.** A pushed route is a `Box<dyn ErasedRoute>` (ADR-0019 §3); nothing in
  `crates/flui-widgets/src/navigator/` turns the stack into a URL or back. The platform already
  delivers URLs the OS opens — `Platform::on_open_urls`
  (`crates/flui-platform/src/traits/platform.rs:550`) — and nothing in `crates/flui-app/src`
  registers for them (`grep -rn on_open_urls crates/flui-app/src` is empty). A deep link has
  nowhere to go.
- **Lookup from `build`.** `NavigatorHandle::maybe_of(ctx: &dyn BuildContext)`
  (`navigator.rs:1592`) and `maybe_of_root` (`navigator.rs:1605`) resolve the navigator from a
  build context. The handle is owned and takes no second lock (ADR-0019 §2), which is right,
  but a build-time lookup is the pattern ADR-0078 moved every other capability away from.
- **A thread-local routing table for commands.** Cross-thread navigation goes through
  `UiCommand::Navigation(NavigatorCommand)` (`crates/flui-app/src/app/ui_realm/commands.rs:95`,
  applied at `commands.rs:437`). `NavigatorCommand::apply_on_owner` resolves its target through
  `thread_local! NAVIGATOR_COMMAND_TARGETS` (`navigator.rs:90-93`, read at `navigator.rs:819`),
  a per-thread map from a process-wide counter (`NEXT_NAVIGATOR_COMMAND_TARGET_ID`,
  `navigator.rs:88`) to `Weak<NavigatorShared>`. The sender is `pub(crate)` and not wired
  (`send_navigation` at `commands.rs:249` carries `expect(dead_code, reason = "typed navigation command sender is
  wired before public runtime vending")`, `commands.rs:242-247`), and the vocabulary names a widget-catalog type.

Flutter went through this twice: named routes are no longer recommended because they cannot
customise deep-link handling or support the browser's forward button, and page-backed routes
(Router) coexist with pageless ones (`Navigator.push`, `showDialog`) that are not deep-linkable
and are silently removed with the page below them. SwiftUI made the same correction when it
replaced `NavigationView` with `NavigationStack`/`NavigationPath`. Both converged on "the
navigation state is a value" (review survey
[`market/flutter_compose_swiftui.md`](../research/2026-09-25-architecture-review/market/flutter_compose_swiftui.md)).

## Decision

### 1. Routes are a derived, typed enum

An application declares its routes as a type and derives the mapping to and from a path:

```rust
#[derive(Routable, Clone, PartialEq)]
enum AppRoute {
    #[route("/")] Home,
    #[route("/note/:id")] Note { id: NoteId },
}
```

The trait and its derive are named `Routable`, not `Route`: `flui_widgets::Route` is already the
Navigator's route-lifecycle trait (`crates/flui-widgets/src/navigator/route.rs`), and a derive
named `Route` that implements another trait would break the convention that a derive carries its
trait's name. Dioxus uses the same name for the same shape.

`#[derive(Routable)]` (in `flui-macros`) generates `fn to_path(&self) -> RoutePath` and
`fn from_path(&RoutePath) -> Result<Self, RouteParseError>`; the trait provides
`fn parse(&str)` (`RoutePath::parse`, then `from_path`) and `fn back_stack(&RoutePath)`, the stack
a location opens with. Field types parse through `FromStr` and print through `Display`. The derive's contract is the round trip: for every value `r`,
`from_path(&r.to_path()) == Ok(r)`. A path that matches no variant is a typed
`RouteParseError::NoMatch`, never a panic and never a silent fallback to the first variant.
Builder-style route tables are not a second public way to declare routes.

### 2. The URL is the source of truth

`Router<R>` owns the navigation state as a value: a stack of `R`. The top of the stack, printed
through `to_path`, is the current location; the stack is what restoration saves and what a deep
link rebuilds. There is no other place the current page is recorded.

- **One URL per presentation.** The outermost `Router` in a presentation's tree owns that
  presentation's URL (the browser location on web, the target of `on_open_urls` delivered to that
  window). A nested `Router` owns a stack that belongs to the page containing it: it is saved and
  restored with that page and does not contribute a second URL. A presentation with no `Router`
  has no URL.

- **Every push is addressable.** A route that enters the stack is an `R`, so it has a path.
  There is no pageless page.
- **Inbound URLs take the same path as a push.** A URL delivered by the platform
  (`on_open_urls`, the browser's history on web) is parsed with `from_path` and applied as a
  navigation; a parse error is reported to the application's `on_unknown` hook and leaves the
  stack unchanged.
- **Dialogs, popups, sheets and menus are not routes.** They are overlay entries (ADR-0076),
  owned by the page that opened them, removed when that page leaves the stack, and never
  present in the URL. This is the explicit answer to Flutter's pageless routes: the two kinds
  exist, but only one of them is navigation state, and the other is documented as not being it.

The owner adopted this rule on 2026-09-25: every push is URL-addressable, and dialogs and
overlays are excluded. The research synthesis had it; the review's final decision had dropped it
without a reason, and this record restores it.

### 3. The handle is acquired in `init_state` and targets the nearest ancestor

```rust
fn init_state(&mut self, cx: &dyn LifecycleContext) {
    self.router = Router::<AppRoute>::handle(cx).ok();
}
// later, in a callback the state built:
router.push(AppRoute::Note { id: NoteId(1) })?;
```

`Router::<R>::handle` takes `&dyn LifecycleContext`, so it is reachable only from `init_state`
and `did_change_dependencies` (ADR-0078). It resolves the **nearest ancestor** `Router<R>` —
the contract of Flutter's `Navigator.of(context)` — and returns an owned, `!Send`
`RouterHandle<R>` that shares the router's state, as `NavigatorHandle` does today (ADR-0019
§2), so no `GlobalKey` is involved. With no `Router<R>` above, it returns
`Err(RouterError::NoRouter)`. `push`, `replace`, `pop` and `go` on the handle edit the stack;
they take no event context, because the handle already is the capability. The edit goes through
the pure-data history flush of ADR-0019 §1 at once, and the pages it adds build on the next
frame.

`Router::of(w)` from an event callback is rejected: a callback has no position in the tree,
so "nearest ancestor" has no meaning there. A callback uses the handle its state captured.

### 4. Navigator is frozen and becomes the Router's page stack

From acceptance, `Navigator` and `NavigatorHandle` get no new public items.

- The Router drives a `Navigator` internally. Its transitions, overlay theater, hero flights,
  local history and completion futures (ADR-0020, ADR-0021, ADR-0025, ADR-0064) keep working
  unchanged, because they are properties of the pages the Router places on that stack.
- The imperative typed doors (`push`, `pop`, `pop_with`, `maybe_pop`, `remove_route`) remain as
  a thin facade. `NavigatorHandle::push<P: NavigatorRoute>` (`navigator.rs:1158`) takes an
  arbitrary route, which has no `to_path`. Under a `Router<R>`, the pops edit the Router's stack,
  and a push is accepted only when the pushed value is an `R` (the Router's page route carries
  it); any other `NavigatorRoute` pushed under a Router is refused, so nothing unaddressable
  enters the stack. The typed facade cannot return a `Result` without a public signature change,
  so a refused push returns an already-completed `RouteResult` (`None`), logs `tracing::error!`
  with the `RouterError::NotAddressable` text, and fails a `debug_assert!`; the named doors
  return `NamedRouteError::NotAddressable`. Pageless popups (`PopupRoute`, which `show_dialog`
  pushes) stay admitted until dialogs move to overlay entries (step 7 below). With no `Router`
  above, `Navigator` keeps its current behaviour for code that has not moved.
- The named-route doors and `on_generate_route` (ADR-0024) are removed when the Router
  ships. `RouteKey<T>` (ADR-0024 §3) survives as the way a typed result is attached to a route:
  `push_for_result::<T>(route)` returns ADR-0019's `RouteResult<T>`, and the result still
  crosses the stack as `Box<dyn Any + Send>` (ADR-0019 §3).

### 5. The command vocabulary names navigation, not Navigator

`UiCommand::Navigation(NavigatorCommand)` becomes a design-neutral navigation intent (a route
path to push or replace, or a pop) addressed to a presentation — a realm can host several
(ADR-0043) — and applied to that presentation's URL-owning Router (§2), not to a
`NavigatorCommandTarget`. An intent for a presentation with no Router is reported, not applied
elsewhere. `NAVIGATOR_COMMAND_TARGETS` and its counter are deleted; the intent is applied by the
realm that owns the presentation, so it can never land on the wrong window's navigator. This keeps ADR-0027 §9's closed vocabulary closed and stops the runtime and the
agent protocol from naming a widget-catalog type.

### Flutter divergences

- Flutter's `Router` takes a `RouteInformationParser` and a `RouterDelegate` written by hand;
  here both are derived from the route type. The observable contract kept is Flutter's:
  nearest-ancestor lookup, deep links through the same parse path, browser back/forward on web.
- Flutter keeps pageless routes as a first-class kind. Here overlays are explicitly outside the
  navigation state (§2).

## Implementation series

1. **Router core (implemented).** `Routable`, `RoutePath`, `RouteParseError`, `Router<R>` and
   `RouterHandle<R>` from `LifecycleContext` (`push`, `replace`, `pop`, `go`, `location`,
   `current`, `can_pop`), in `crates/flui-widgets/src/router/`. The back-stack of a location is
   its prefix chain. Router pages are `PageRoute`s on an addressed Navigator whose facade
   refuses unaddressable pushes (§4), and each page scopes a semantics route.
2. **`#[derive(Routable)]`** in `flui-macros`: `#[route("/…/:param")]` on unit and named-field
   variants, fields through `FromStr`/`Display`, trybuild pass and fail tests, a proptest round
   trip.
3. **Nested routes.** `#[nest("/settings")] Settings(SettingsRoute)` composes a child enum into
   the parent's path: one stack, one URL. A nested `Router` widget is a local stack saved with
   its page (§2); a `RouterScope` marker gives the outermost Router per presentation the URL.
4. **Guards.** `Router::guard(|from: Option<&R>, to: &R| -> Guard<R>)` with
   `Guard::{Allow, Redirect(R), Deny}`, synchronous, applied to push, replace, go and inbound
   URLs; more than five redirects is `RouterError::RedirectLoop`. Asynchronous checks stay in
   app state, because the frame path is synchronous.
5. **`PopScope` with results.** `RouterHandle::push_for_result::<T>(r) -> RouteResult<T>` (the
   `RouteKey<T>` carry-over of ADR-0024 §3) and `pop_with<T>`; `RouterHandle::maybe_pop` honours
   `PopScope`, and system back goes to the URL-owning Router.
6. **Presentation-addressed intents.** `NavigationIntent { presentation, op }` in `flui-runtime`,
   applied by the realm to that presentation's URL-owning Router (§5); `Platform::on_open_urls`
   delivers intents, and the Router gains an `on_unknown` hook. `NAVIGATOR_COMMAND_TARGETS`, the
   `NavigatorCommand*` types and their `globals` entry are deleted.
7. **Freeze and removal.** `WidgetsApp::router(..)` and the Material and Cupertino router
   constructors; the named-route doors, `on_generate_route` and the `WidgetsApp` routes table
   are removed (`flui migrate` rewrites callers); dialogs move to overlay entries and popups stop
   being admitted.
8. **Web history and restoration.** `pushState`/`popstate` and browser back/forward on wasm,
   stack restoration as a list of paths, query and fragment support in `RoutePath`.

## Alternatives considered

- **Builder routes (`Router::new().route("/note/:id", |params| ...)`).** Rejected: the path
  and the variant can drift apart, parameters arrive as strings, and it is a second way to
  declare what the derive already expresses.
- **Keep named routes in the prelude beside the Router.** Rejected: it is Flutter's
  Navigator 1/2 split, two navigation APIs with different deep-link and restoration semantics.
- **`Router::of(w)` resolved from the `Writer` in a callback.** Rejected: a callback has no
  tree position, so it could only mean "the primary presentation's router" — the routing
  `NAVIGATOR_COMMAND_TARGETS` exists to paper over, and wrong in a multi-window app.
- **Make dialogs and popups routes too.** Rejected: a URL that opens a confirmation dialog is
  rarely wanted, and it would pull transient overlays into restoration. The line is drawn once
  and tested.
- **Leave Navigator as the primary API and add URL sync on top.** Rejected: pushes that bypass
  the URL would stay possible, which is the pageless-route problem again.

## Consequences

- Deep links, restoration and web history get one mechanism: parse a path, set the stack.
- Application code that uses named routes breaks when they are removed. Migration is mechanical:
  one enum variant per registered name, a `#[route]` attribute per variant, and `push_named`
  calls become `push(AppRoute::…)`; `flui migrate` can rewrite the calls when the table is
  static. Typed `push`/`pop` callers keep compiling.
- `NavigatorHandle::maybe_of` stays for code under no Router; new code acquires a
  `RouterHandle` in `init_state`, which moves navigation out of `build`.
- `flui-app` loses its only reference to a `flui-widgets` navigation type in the command
  vocabulary (`commands.rs:8`), which the runtime extraction (ADR-0083) needs anyway.
- The deletion of `NAVIGATOR_COMMAND_TARGETS` is one entry leaving the globals allowlist
  (ADR-0097).
- A saveable-state contract shared by the back stack, lazy-list eviction and hot reload (the
  Compose `rememberSaveable` lesson) is not decided here; page state is kept alive while the
  page is on the stack, as today. See [open questions](../../design/open-questions.md).

## Verification

Landed with step one (`crates/flui-widgets/tests/router.rs` and the unit tests in
`crates/flui-widgets/src/router/`):

- **Round trip, hand-written.** `route_path_round_trips_a_hand_written_routable` and
  `parse_reports_no_match_and_bad_params` over a hand-written `Routable`; the derive's property
  test lands with step 2.
- **Nearest ancestor.** `nested_router_handle_targets_the_nearest_router`: a handle acquired
  inside the inner `Router<R>` pushes to the inner stack and leaves the outer one unchanged.
  `router_handle_without_a_router_is_no_router`: with no `Router` above, `NoRouter`.
- **Acquisition is typed.** The `compile_fail` doctest on `Router::handle`, beside a compiling
  twin that takes a `&dyn LifecycleContext`.
- **Every push is addressable.** `pushing_a_page_route_under_a_router_is_not_addressable`: a
  `PageRoute`, a `SimpleRoute` and a named route pushed through the facade are refused and the
  stack and location are unchanged; `facade_pop_updates_the_router_location` pins that the
  facade's pops reach the Router. `popup_routes_are_admitted_and_leave_the_location_alone` pins
  the popup admission that lasts until step 7.
- **Back-stack from a location.** `back_stack_is_the_matching_prefix_chain`,
  `router_opens_at_a_location_with_its_back_stack`, `go_reconciles_only_the_diverging_tail`,
  `go_adds_the_new_back_stack_beneath_the_new_top`.

Still to land, each with its step:

- **Derive round trip** (step 2). A property test over generated values of a test route enum
  asserts `from_path(&r.to_path()) == Ok(r)`; a table test asserts `NoMatch` for unknown paths
  and for malformed parameters.
- **Inbound deep link** (step 6). The headless platform delivers a URL through
  `on_open_urls`; after one frame the stack is the parsed route and the page is mounted and laid out (not merely present).
- **Overlays are not state** (step 7; step one pins only that a popup leaves the path
  unchanged). Opening a dialog leaves the current path unchanged; popping the
  page that opened it removes the dialog's overlay entry.
- **Multi-window** (step 6). In one realm with two presentations, a navigation intent addressed to the
  second presentation changes that presentation's Router and not the primary presentation's.
- **Flutter lifecycle parity** stays pinned by the existing Navigator tests
  (`crates/flui-widgets/tests/navigator.rs`,
  `crates/flui-widgets/tests/navigator_public.rs`), which must keep passing with the Navigator
  running under a Router.
