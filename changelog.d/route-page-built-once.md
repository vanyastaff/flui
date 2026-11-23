### Changed

- A route's page builder runs once per route state, not on every frame of its transition: each
  tick rebuilds only the transitions around the page, as Flutter's modal route caches its page.
  A builder that read `primary.value()` directly now reads it once; a page that follows the
  animation wraps the part that moves in an `AnimatedView` or a transition widget. The builder's
  `ctx` now sits below the route's hero and pop scopes. In the `app_scenarios` bench of
  `flui-material`, the `navigation` scenario costs 81% fewer instructions under callgrind.
