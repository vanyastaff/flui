### Added

- `Router::on_unknown` and presentation URL ownership for the outermost `Router`: navigation intents map to typed push, replace, go and pop; nested routers keep their local stacks; unknown locations notify without changing the stack, last-page pop is a no-op, stale owners are refused and reentrant URL navigation reports `Busy` (ADR-0093 §5).
- `RoutePath` and `RouteParseError` now live in `flui-view`, with their `flui-widgets` crate-root and prelude exports preserved for typed routes and `#[derive(Routable)]`.
