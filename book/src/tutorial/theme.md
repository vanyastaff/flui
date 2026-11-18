# Theming

Flutter counterpart: `MaterialApp`, `ThemeData`, `Theme.of`.

A design system supplies finished widgets and a theme they read. The notes application uses the
Material catalog, which `flui`'s `material` feature turns on. `MaterialApp` takes a light and a
dark `ThemeData` and picks one by its `ThemeMode`; `ThemeMode::System` follows the platform's
setting:

```rust
use flui::prelude::*;

/// A header drawn in the theme's colours, whichever theme is in force.
#[derive(Clone, StatelessView)]
struct NotesHeader;

impl StatelessView for NotesHeader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let mode = if theme.brightness().is_dark() { "dark" } else { "light" };
        let style = theme.text_theme.title_large.unwrap_or_default();
        Text::new(format!("Notes ({mode})")).style(style.with_color(theme.color_scheme.primary))
    }
}

fn app(mode: ThemeMode) -> MaterialApp {
    MaterialApp::new(Scaffold::new().body(NotesHeader))
        .theme(ThemeData::light())
        .dark_theme(ThemeData::dark())
        .theme_mode(mode)
}
# use flui::testing::{Tester, find};
# Tester::mount(app(ThemeMode::Light)).expect(find::text("Notes (light)")).to_exist();
# Tester::mount(app(ThemeMode::Dark)).expect(find::text("Notes (dark)")).to_exist();
```

- `Theme::of(ctx)` reads the nearest theme above the view and makes the view depend on it, so a
  change of theme rebuilds it.
- The catalog's widgets (`Scaffold`, `TextButton`, `TextField`, `AppBar`) read the same theme, so
  the colours come from one place.
- A theme of your own is a `ThemeData` with a different `ColorScheme`; [Themes](../cookbook/themes.md)
  covers the rest.

Next, [Testing](testing.md) checks the application the way a user would use it.
