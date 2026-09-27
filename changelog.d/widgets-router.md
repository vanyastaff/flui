### Added

- **Typed `Router`** (ADR-0093, `flui-widgets`): `Router<R: Routable>` keeps
  navigation state as a stack of route values whose top, printed through `Routable::to_path`,
  is the current location, and places one `PageRoute` per value on a `Navigator` it builds,
  each page scoping (and, with a `semantics_label`, naming) a semantics route. A descendant
  acquires a `RouterHandle` in `init_state` with `Router::<R>::handle` — the nearest
  `Router<R>` — and drives it with `push`, `replace`, `pop` and `go(location)`; `go` derives
  the back-stack from the location's prefix chain (`Routable::back_stack`) and keeps the pages
  the two stacks share. `RoutePath` is a normalized, percent-encoded location and
  `RouteParseError` says why a location produced no route. The Router's navigator refuses
  pages pushed through its facade (a debug assertion and an already-completed result for the
  typed doors, the new `NamedRouteError::NotAddressable` for the named ones), admits pageless
  popups such as dialogs through a plain push only, follows every pop it makes, and never pops
  or removes its last page; `NavigatorHandle::pop_until` stops at a pop that is refused.
  `#[derive(Routable)]` and `WidgetsApp::router` are later steps; nothing in the framework
  builds a Router yet.
