# Cookbook

Task-oriented pages, each pointing at a real, runnable example rather than a hand-written snippet
that could drift from the actual API. A cookbook page only exists for a topic that has a working
example in `examples/` today, so the cookbook never promises a recipe that doesn't run.

Not every topic you might expect has a page yet. Navigation has a runnable example but no page:
`cargo run --example two_screens` (`examples/two_screens.rs`) roots an app in
`WidgetsApp::router` over a `#[derive(Routable)]` route enum, and its pages push and pop route
values through a `RouterHandle` (ADR-0093; see the [mapping](../mapping.md) notes).

- [Forms](forms.md)
- [Async](async.md)
- [Themes](themes.md)
- [Animation](animation.md)
- [Testing](testing.md)
