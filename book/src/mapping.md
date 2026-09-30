# Flutter → FLUI mapping

A vocabulary table for readers coming from Flutter. Every row is checked against the real trait or
struct in `crates/`.

| Flutter | FLUI | Where |
|---|---|---|
| `StatelessWidget` | `StatelessView` (`#[derive(StatelessView)]`) | `crates/flui-view/src/view/stateless.rs` |
| `StatefulWidget` + `State<T>` | `StatefulView` + `ViewState<V>` | `crates/flui-view/src/view/stateful.rs` |
| `BuildContext` | `BuildContext` trait for `build`; the presentation capabilities (`rebuild_handle()`, `post_frame_handle()`, `text_input_handle()`, `focus_manager()`, ...) live on its `LifecycleContext` subtrait, which only `init_state`/`did_change_dependencies` receive (ADR-0078) | `crates/flui-view/src/context/build_context.rs` |
| `setState(() => ...)` | `Element::set_state`/`set_state_scheduled`, usually reached through `StateCell<T>`/`StateHandle<T>` | `crates/flui-view/src/element/unified.rs`, `crates/flui-view/src/state_cell.rs` |
| `InheritedWidget` | `InheritedView` | `crates/flui-view/src/view/inherited.rs` |
| `Navigator` | `Navigator` / `NavigatorHandle` / `NavigatorState` | `crates/flui-widgets/src/navigator/navigator.rs` |
| `Router` | `Router<R: Routable>` + `RouterHandle<R>`, with `#[derive(Routable)]` on a route enum | `crates/flui-widgets/src/router/` |
| `WidgetsApp` | `WidgetsApp` (`WidgetsApp::new(home)` or `WidgetsApp::router(router)`) | `crates/flui-widgets/src/app/widgets_app.rs` |
| `MaterialApp` / `CupertinoApp` | `MaterialApp` / `CupertinoApp` | `packages/flui-material/src/app.rs`, `packages/flui-cupertino/src/app.rs` |

## Notes

- **`Router` is the primary navigation API.** Routes are values: a `#[derive(Routable)]` enum
  with one `#[route("…")]` pattern per variant, and a `Router<R>` that keeps the stack of route
  values, which a page pushes and pops through the `RouterHandle<R>` it takes in `init_state`
  ([ADR-0093](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0093-router-is-the-primary-navigation-api.md),
  still Proposed; `examples/two_screens.rs` shows the shape). `Navigator` remains underneath
  ([ADR-0019](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0019-navigator-routing-seam.md));
  the string-named routes of
  [ADR-0024](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0024-named-routes-seam.md)
  still ship, but that ADR is Deprecated in favour of the typed router.
- **`MaterialApp` is `WidgetsApp` plus theming.** `MaterialApp` wraps a `WidgetsApp`, resolves
  `ThemeMode` against the platform brightness, publishes the resolved `ThemeData` through
  `Theme`, and installs a `ScaffoldMessenger`. `run_app(MaterialApp::new(home))` is the usual
  root; `Theme::new(ThemeData::light(), <root view>)` passed to `run_app` (see
  `examples/form.rs`) still works for an app that wants only the theme.
- **`Layer` is a closed `enum`, not a class hierarchy** — see
  [View, Element, RenderObject](concepts/view-element-render.md).

## State outside the table

Flutter has no built-in equivalent of FLUI's realm-scoped signals: `Signal<T>` (ADR-0074) is a
`Copy` handle created with `ctx.signal(value)` in `init_state`, read in `build`, and written from an
event callback. See [State](concepts/state.md) for it and the other ways state enters the tree.
