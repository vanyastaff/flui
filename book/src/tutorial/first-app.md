# Your first window

Flutter counterpart: `runApp`, `StatelessWidget`.

This tutorial builds a small notes application one chapter at a time. Each sample is a complete
program; the ones that open a window are compiled but not run by the book's tests.

## A view and a window

An application is a tree of views. A view that only describes what it shows is a
`StatelessView`: a struct holding its inputs and a `build` that returns its children.

```rust,no_run
use flui::prelude::*;

#[derive(Clone, StatelessView)]
struct NotesApp;

impl StatelessView for NotesApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Center::new().child(Text::new("No notes yet"))
    }
}

fn main() -> Result<(), LaunchError> {
    run_app(NotesApp)
}
```

`run_app` opens a main window with the view at its root and runs until the window closes. It
returns a `LaunchError` when the application cannot start: the platform or its GPU is not
available, or two plugins provide the same capability.

The derive writes the `View` implementation that mounts the struct; the `impl StatelessView` is
the part you write.

## Configuring the run

`run_app(root)` is `Application::new().run(root)`. To set the window's title and size, build the
`Application` yourself and give it an `AppConfig`:

```rust,no_run
use flui::app::AppConfig;
use flui::prelude::*;

# #[derive(Clone, StatelessView)]
# struct NotesApp;
# impl StatelessView for NotesApp {
#     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
#         Text::new("No notes yet")
#     }
# }
fn main() -> Result<(), LaunchError> {
    Application::new()
        .config(AppConfig::new().title("Notes").size(480, 640))
        .run(NotesApp)
}
```

The builder also registers plugins and capability providers; each method has the bare name of
what it sets.

## One entry for every platform

On the desktop, the web and iOS, the program starts from `main`. On Android the system loads the
application as a library and calls an entry it exports. `#[flui::main]` writes that entry, so one
function starts the application everywhere. A project from `flui create` puts it in `src/lib.rs`
and calls it from `src/main.rs`:

```rust,no_run
use flui::prelude::*;

# #[derive(Clone, StatelessView)]
# struct NotesApp;
# impl StatelessView for NotesApp {
#     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
#         Text::new("No notes yet")
#     }
# }
/// Start the application in a window with [`NotesApp`] as its root.
#[flui::main]
pub fn run() -> Result<(), LaunchError> {
    run_app(NotesApp)
}

fn main() -> Result<(), LaunchError> {
    run()
}
```

Next, [Laying out a screen](layout.md) gives the window something to arrange.
