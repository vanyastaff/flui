### Added

- **`flui-testing`**: `flui_testing::widgets::SharedSignal<T>`, a value a test and the view reading it both hold. The view binds it in `init_state` (a `Signal` in its element's graph) and reads it in `build`; the test sets it between frames and reads it back.

### Removed

- **`flui-view`**: `StateCell<T>` and `StateHandle<T>`, with their `flui::prelude` and `flui_widgets::prelude` re-exports. Local and app state is a `Signal<T>`: create it with `ctx.signal(value)` in `init_state`, read it with `signal.get(ctx)` in `build`, and write it from an event callback with `signal.update(cx, |value| ..)` (ADR-0074, ADR-0086). A controller that lives outside the tree stays a `Listenable`.
