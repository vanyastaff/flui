# Flutter → FLUI mapping

A vocabulary table for readers coming from Flutter. Every row is checked against the real trait or
struct in `crates/` — see AGENTS.md's Prime Directive on why an unverified mapping isn't
acceptable here: a wrong row would misrepresent either what FLUI follows from Flutter or what it
deliberately changed.

| Flutter | FLUI | Where |
|---|---|---|
| `StatelessWidget` | `StatelessView` (`#[derive(StatelessView)]`) | `crates/flui-view/src/view/stateless.rs` |
| `StatefulWidget` + `State<T>` | `StatefulView` + `ViewState<V>` | `crates/flui-view/src/view/stateful.rs` |
| `BuildContext` | `BuildContext` trait, with lifecycle-acquired capability methods (`rebuild_handle()`, `post_frame_handle()`, `text_input_handle()`, `focus_manager()`) | `crates/flui-view/src/context/build_context.rs` |
| `setState(() => ...)` | `Element::set_state`/`set_state_scheduled`, usually reached through `StateCell<T>`/`StateHandle<T>` | `crates/flui-view/src/element/unified.rs`, `crates/flui-view/src/state_cell.rs` |
| `InheritedWidget` | `InheritedView` | `crates/flui-view/src/view/inherited.rs` |
| `Navigator` | `Navigator` / `NavigatorHandle` / `NavigatorState` | `crates/flui-widgets/src/navigator/navigator.rs` |
| `MaterialApp` | No single equivalent — `run_app()` plus `Theme`/`ThemeData` wrapping the root view | `run_app`: facade `src/lib.rs`, impl in `crates/flui-app`; `Theme`/`ThemeData`: `crates/flui-material/src/theme*.rs` |

## Notes on the divergences

- **`Navigator`, no `Router`.** FLUI has `Navigator` and named-route support (see
  [ADR-0019](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0019-navigator-routing-seam.md)
  and
  [ADR-0024](https://github.com/vanyastaff/flui/blob/main/docs/adr/ADR-0024-named-routes-seam.md)),
  but no standalone `Router`/`RouteInformationParser`-style class the way Flutter's `Router` widget
  provides. Every other "Router" hit in the codebase is unrelated pointer/event routing in
  `flui-interaction`, not navigation.
- **No `MaterialApp` equivalent.** Flutter's `MaterialApp` bundles a `Navigator`, a `Theme`, a
  title, and app-level configuration into one widget. FLUI doesn't have a single type that does
  the same — the shipped pattern (see `examples/counter.rs` and the CLI's generated `main()`) is
  `Theme::new(ThemeData::light(), <root view>)` passed to `run_app`.
- **`Layer` is a closed `enum`, not a class hierarchy** — see
  [View, Element, RenderObject](concepts/view-element-render.md).

## What this table deliberately does not claim

An earlier draft of this task mentioned a "signals" state-management system tied to an ADR-0074.
Neither exists in this repository as of this page: `docs/adr/` tops out at ADR-0073, and there is
no reactive-signal primitive anywhere in `crates/` — every "signal" hit in the source is either
gesture/pointer-signal routing (`flui-interaction`) or prose describing an event, not a state
primitive. See [State](concepts/state.md) for the three mechanisms that actually exist.
