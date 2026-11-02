### Added

- **`flui-semantics`**: a change of the innermost route (a pushed, popped or replaced page, or a
  modal dialog) is told to assistive technology once, by name. An adapter whose platform has a
  notice of a new screen states `RouteChangeSupport::Native` and receives a `RouteChange` through
  `PlatformAccessibility::route_changed`. On any other platform the owner speaks the route's name
  as a polite announcement
  ([ADR-0204](/docs/adr/ADR-0204-synthetic-semantics-nodes.md) §5).
