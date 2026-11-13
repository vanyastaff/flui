# Flutter → FLUI mapping

A vocabulary table for readers coming from Flutter. Every row is checked against the real trait or
struct in `crates/`.

| Flutter | FLUI | Where |
|---|---|---|
| `StatelessWidget` | `StatelessView` (`#[derive(StatelessView)]`) | `crates/flui-view/src/view/stateless.rs` |
| `StatefulWidget` + `State<T>` | `StatefulView` + `ViewState<V>` | `crates/flui-view/src/view/stateful.rs` |
| `BuildContext` | `BuildContext` trait for `build`; the presentation capabilities (`rebuild_handle()`, `post_frame_handle()`, `text_input_handle()`, `focus_manager()`, ...) live on its `LifecycleContext` subtrait, which only `create_state`/`init_state`/`did_change_dependencies` receive (ADR-0078) | `crates/flui-view/src/context/build_context.rs` |
| `setState(() => ...)` | A `Signal<T>` created with `ctx.signal(..)` in `create_state`, written through the `ctx` of an event callback; the signal rebuilds the elements that read it | `crates/flui-view/src/reactive/`, `crates/flui-view/src/context/build_context.rs` |
| `InheritedWidget` | `InheritedView` | `crates/flui-view/src/view/inherited.rs` |
| `Navigator` | The page stack inside a `Router<R>`; a page reaches its router with `Router::of(ctx)` (`GoRouter.of(context)`) and its own route with `PageHandle::of(ctx)` (`ModalRoute.of(context)`), and `Route`, `PageRoute` and `PageTransitions` shape how a page is presented | `crates/flui-widgets/src/navigator/` |
| `Router` | `Router<R: Routable>` + `RouterHandle<R>`, with `#[derive(Routable)]` on a route enum | `crates/flui-widgets/src/router/` |
| `WidgetsApp` | `WidgetsApp` (`WidgetsApp::new(home)` or `WidgetsApp::router(router)`) | `crates/flui-widgets/src/app/widgets_app.rs` |
| `MaterialApp` / `CupertinoApp` | `MaterialApp` / `CupertinoApp` | `packages/flui-material/src/app.rs`, `packages/flui-cupertino/src/app.rs` |
| `WidgetTester` (`testWidgets`, `pumpWidget`, `pump`, `pumpAndSettle`, `tap`, `enterText`) | `Tester` (`Tester::mount`, `pump`, `pump_for`, `pump_and_settle`, `tap`, `enter_text`), synchronous | `crates/flui-testing/src/tester.rs` |
| `find.text`, `find.byKey`, `find.bySemanticsLabel` | `find::text`, `find::id`, `find::button`, `find::role`: each a `Locator` over the semantics tree | `crates/flui-testing/src/find.rs`, `crates/flui-protocol/src/locator.rs` |
| `expect(finder, findsOneWidget)` | `t.expect(locator).to_exist()` (`to_be_gone`, `to_have_count`, `to_have_name`, `to_be_checked`, …) | `crates/flui-testing/src/tester.rs` |

## Notes

- **A test finds what a user perceives.** `flui::testing::find` builds the same `Locator` the
  driver and an agent use, resolved over the window's semantics tree, so a test written against a
  headless `Tester` runs unchanged on a device. `find.byType` and `find.byWidget` have no
  counterpart: the semantics tree carries no widget types, and a type finder ties a test to how a
  screen is composed rather than to what it shows. A widget that publishes no semantics is found
  by the identifier its author gives it (`Semantics::identifier`) and `find::id`. Actions wait for
  their input to land and their frame to run, and an expectation waits while the application has
  a frame or work due now, then fails at once with the window's outline; nothing waits on a clock
  the test did not move with `pump_for`. An ambiguous locator fails with its candidates instead
  of tapping the first.
- **`Router` is the primary navigation API.** Routes are values: a `#[derive(Routable)]` enum
  with one `#[route("…")]` pattern per variant, and a `Router<R>` that keeps the stack of route
  values, which a page pushes and pops through the `RouterHandle<R>` that `Router::of(ctx)`
  resolves in `build`, as `GoRouter.of(context)` does; the callback that navigates moves the
  handle, since it runs with no place in the tree
  ([ADR-0330](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0330-navigation-is-a-typed-router-reached-from-the-context.md)).
  `Navigator` is the Router's internal page stack, not a second way to navigate; Flutter's
  named routes (`pushNamed`, `onGenerateRoute`) have no counterpart, since a route is a typed
  value with a path. Dialogs, popups and menus are pageless popup routes a page pushes through its
  `PageHandle` (`show_dialog(&page, …)` for `showDialog(context: …)`): they never enter the
  URL or the stack a Router restores. A thread that
  needs to navigate writes a signal the page reacts to; there is no cross-thread navigation
  command.
- **`MaterialApp` is `WidgetsApp` plus theming.** `MaterialApp` wraps a `WidgetsApp`, resolves
  `ThemeMode` against the platform brightness, publishes the resolved `ThemeData` through
  `Theme`, and installs a `ScaffoldMessenger`. `run_app(MaterialApp::new(home))` is the usual
  root; `Theme::new(ThemeData::light(), <root view>)` passed to `run_app`
  still works for an app that wants only the theme.
- **`Layer` is a closed `enum`, not a class hierarchy** — see
  [View, Element, RenderObject](concepts/view-element-render.md).

## State outside the table

Flutter has no built-in equivalent of FLUI's presentation-scoped signals: `Signal<T>` (ADR-0074) is a
`Copy` handle created with `ctx.signal(value)` in `create_state`, read in `build`, and written from an
event callback. See [State](concepts/state.md) for it and the other ways state enters the tree.
