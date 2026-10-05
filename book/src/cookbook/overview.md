# Cookbook

Choose a recipe by the behavior you want to add. Each page links to an example in the
repository and gives its command, including any required features. Run commands from the
checkout root used in [Installation](../getting-started/installation.md).

| Task | Recipe |
|------|--------|
| Validate editable text and show an error | [Forms](forms.md) |
| Load data and display loading, error, and retry states | [Async](async.md) |
| Share colors and styling through the view tree | [Themes](themes.md) |
| Animate a value over time | [Animation](animation.md) |
| Exercise a view without opening a window | [Testing](testing.md) |

For typed navigation, run `cargo run --example two_screens`. The current example roots an app in
`WidgetsApp::router` over a `#[derive(Routable)]` route enum, and its pages push and pop route
values through a `RouterHandle` (ADR-0093; see the [mapping](../mapping.md) notes).
Read its [source](https://github.com/vanyastaff/flui/blob/main/examples/two_screens.rs)
for the current composition and callbacks.

The [Notes showcase chapter](../getting-started/showcase.md) follows a source draft that combines
navigation, editing, validation, lazy rows, and retry. Its additions need integration and
execution before the walkthrough can be followed; use the chapter's source and evidence notes
to distinguish that candidate from the current two-screen example.
