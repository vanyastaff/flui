### Removed

- **`flui-widgets`**: named routes. `NavigatorHandle::{route, route_keyed, on_generate_route,
  on_unknown_route, clear_routes}`, the `*_named*` doors, `push_keyed`, `GeneratedRoute`,
  `RouteRequest`, `RouteKey`, `KeyedSettings`, `NamedRouteError`, `RouteArguments` and
  `RouteSettings::{arguments, arguments_shared, argument}` are gone, and with them the
  `Send + Sync` bounds route arguments carried. An application that navigates declares its pages
  as a `#[derive(Routable)]` enum under a `Router`: a name becomes a variant with a
  `#[route("…")]` pattern, its arguments become the variant's fields, and `push_named(name)`
  becomes `RouterHandle::push(AppRoute::…)`
  ([ADR-0093](/docs/adr/ADR-0093-router-is-the-primary-navigation-api.md)).
