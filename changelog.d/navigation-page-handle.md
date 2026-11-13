### Added

- **`flui-widgets`**: `Router::<R>::of(ctx)` and `PageHandle::of(ctx)` resolve the nearest
  router and the route a widget is on from any build context, Flutter's `GoRouter.of(context)`
  and `ModalRoute.of(context)`. `PageHandle` has `is_current`, `can_pop`, `maybe_pop`, `pop`,
  `pop_with` and `push_popup`; it acts only on its own route, so a callback that fires after a
  popup covered its page does not pop the popup.
  A navigation made while the UI runtime builds is refused with `RouterError::DuringBuild` or
  `PageError::DuringBuild` and changes nothing
  ([ADR-0330](/docs/adr/ADR-0330-navigation-is-a-typed-router-reached-from-the-context.md)).
- **`flui-material`, `flui-cupertino`**: `MaterialApp::router` and `CupertinoApp::router`, so a
  design-system app can be rooted in a `Router`.
- An `examples/navigation.rs`: a push, a page popped with a result, and a dialog.

### Changed

- **`flui-widgets`**: `Router::handle(&dyn LifecycleContext)` is now `Router::of(&dyn
  BuildContext)`; a lifecycle context still passes. A `Router` given no `transitions` leaves its
  pages to adopt the ambient `PageTransitions` page builder, so a Router's pages under a
  `MaterialApp` run the theme's transition instead of a jump cut.
- **`flui-material`**: `show_dialog` takes the opening page's `PageHandle` and returns
  `Result<RouteResult<T>, PageError>`. `BackButton` pops the route it is on, and `AppBar`
  implies a back button from its own route's `PageHandle::can_pop`, so a covered bottom page's
  app bar shows none. `OpenContainer` opens its page as a popup over the page it sits on.
- **`flui-widgets`**: `WidgetsApp` is one type with no form parameter: `new(home)` (one page,
  on which popups open), `router(…)` or `with_builder(…)`.
- **`flui-cupertino`**: `CupertinoApp` installs the iOS parallax slide as the ambient page
  transition, so a Router's pages slide in.

### Removed

- **`flui-material`, `flui-cupertino`**: `MaterialApp::navigator`, `MaterialApp::observer`,
  `CupertinoApp::navigator` and `CupertinoApp::observer`. Root the app in `router(…)` and
  navigate through `Router::of(ctx)`.
- **`flui-widgets`**: `Navigator`, `NavigatorHandle`, `NavigatorState`, `NavigatorObserver`,
  `NavigatorRoute` and `SimpleRoute` leave the public API, with `WidgetsApp::navigator`,
  `WidgetsApp::observer`, `AppForm`, `NavigatorForm` and `RouterForm`. A page is a `Router`
  route value, a popup opens through `PageHandle::push_popup`, and a pop goes through the handle
  of the route it leaves (ADR-0330 §4).
- **`flui-cupertino`**: `cupertino_page_route`, which only a public navigator could push; a
  `CupertinoApp`'s Router pages run the same slide.
