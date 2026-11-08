### Removed

- Removed the named-route registry and factories: `NavigatorHandle::route`,
  `route_keyed`, `clear_routes`, `on_generate_route` and `on_unknown_route`,
  together with `GeneratedRoute`, `RouteRequest`, `KeyedSettings` and
  `NamedRouteError`.
- Removed every named navigation door: `push_named`, `push_named_typed`,
  `push_replacement_named`, `push_replacement_named_with`, `pop_and_push_named`,
  `pop_and_push_named_with`, `push_named_and_remove_until` and `push_keyed`.
  There is no string-name replacement: applications declare typed route values
  with `Routable` and navigate them through `Router<R>` and `RouterHandle<R>`.
- Removed `RouteKey<T>`, `RouteArguments`, `RouteSettings::arguments` and the
  argument builders and accessors. Typed route values carry their own data.
  ADR-0093 §3 defines `RouterHandle::push_for_result` as the typed-result model;
  its implementation remains tracked in `docs/gaps.md` entry 43.
- `RouteSettings::name` and the route builders' `.named(...)` remain route
  labels; they do not register or resolve navigation.
