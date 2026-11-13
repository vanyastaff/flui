### Added

- `UiRuntime::navigate(presentation, NavigationIntent)` applies a navigation to a presentation's
  URL owner, its outermost `Router`, as Flutter's Router applies the route information the
  platform reports. A refusal is a typed `NavigationRefused` (`NoPresentation`, `NoUrlOwner`,
  `NoMatch`, `NothingToPop`, `Busy`) and leaves the stack unchanged. `HeadlessHost::navigate`
  drives it in tests.
- `Router::on_unknown(hook)`: told of a navigated path no route matches.

### Changed

- `RoutePath` and `RouteParseError` are defined in `flui-view` (`flui_view::navigation`) beside
  `NavigationIntent`, `NavigationOp`, `UrlOwner` and `NavigationRefused`; `flui-widgets` and the
  facade re-export them at the same paths as before.
