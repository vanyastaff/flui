# Cookbook

Task-oriented pages, each pointing at a real, runnable example rather than a hand-written snippet
that could drift from the actual API. Per AGENTS.md's "no invented API" rule for this book, a
cookbook page only exists for a topic that has a working example in `examples/` today — this
keeps the cookbook from promising a recipe that doesn't actually run.

Not every topic you might expect is here yet. Navigation (`Navigator`,
`crates/flui-widgets/src/navigator/navigator.rs`, ADR-0019/ADR-0024's named-route seam) is real
API, but no bundled example currently exercises it — that page will be added once one does, rather
than shipped as a stub with invented code.

- [Forms](forms.md)
- [Async](async.md)
- [Themes](themes.md)
- [Animation](animation.md)
- [Testing](testing.md)
