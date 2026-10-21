# Cookbook

Choose a recipe by the behavior you want to add.

| Task | Recipe |
|------|--------|
| Validate editable text and show an error | [Forms](forms.md) |
| Load data and display loading, error, and retry states | [Async](async.md) |
| Share colors and styling through the view tree | [Themes](themes.md) |
| Animate a value over time | [Animation](animation.md) |
| Exercise a view without opening a window | [Testing](testing.md) |

For typed navigation, root an app in `WidgetsApp::router` over a `#[derive(Routable)]` route
enum; its pages push and pop route values through a `RouterHandle` (ADR-0093; see the
[mapping](../mapping.md) notes).
