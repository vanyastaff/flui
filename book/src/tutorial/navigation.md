# Navigation

Flutter counterpart: `go_router`'s `GoRouter`, `ModalRoute.of`, `showDialog`.

An application with more than one screen names its screens as routes. The routes are an enum,
each variant with its path; a `Router` builds the page for the route on top, and the application
hands it to `WidgetsApp::router` (or a design system's `MaterialApp::router`):

```rust
use flui::prelude::*;

const NOTES: [&str; 2] = ["Groceries", "Talk outline"];

#[derive(Routable, Clone, Debug, PartialEq)]
enum NotesRoute {
    #[route("/")]
    List,
    #[route("/note/:id")]
    Note { id: usize },
}

fn app() -> WidgetsApp {
    WidgetsApp::router(Router::new(NotesRoute::List, |route: &NotesRoute, _ctx| match route {
        NotesRoute::List => NoteList.boxed(),
        NotesRoute::Note { id } => NotePage { id: *id }.boxed(),
    }))
}

/// Every note, each opening its own page.
#[derive(Clone, StatelessView)]
struct NoteList;

impl StatelessView for NoteList {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let router = Router::<NotesRoute>::of(ctx).expect("BUG: the list is a page of the Router");
        let open = |id: usize| {
            let router = router.clone();
            RawButton::new(Text::new(format!("Open {}", NOTES[id])))
                .on_pressed(move |_ctx| {
                    let _ = router.push(NotesRoute::Note { id });
                })
        };
        Column::new((open(0), open(1)))
    }
}

/// One note: back to the list, or delete it after a confirmation.
#[derive(Clone, StatelessView)]
struct NotePage {
    id: usize,
}

impl StatelessView for NotePage {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let page = PageHandle::of(ctx).expect("BUG: a note is a page of the Router");
        let back = page.clone();
        let confirm = move |_ctx: &mut EventContext<'_>| {
            let note = page.clone();
            let dialog = PopupRoute::<()>::new(move |ctx, _animation, _secondary| {
                let dialog = PageHandle::of(ctx).expect("BUG: the dialog is a route");
                let (cancel, note) = (dialog.clone(), note.clone());
                Column::new((
                    Text::new("Delete this note?"),
                    RawButton::new(Text::new("Cancel")).on_pressed(move |_ctx| {
                        let _ = cancel.pop();
                    }),
                    RawButton::new(Text::new("Delete note")).on_pressed(move |_ctx| {
                        // The dialog pops first, so the note's page is on top when it
                        // pops; the note pops only when the dialog's pop was taken.
                        if let Ok(true) = dialog.pop() {
                            let _ = note.pop();
                        }
                    }),
                ))
                .boxed()
            });
            let _ = page.push_popup(dialog);
        };
        Column::new((
            Text::new(NOTES[self.id]),
            RawButton::new(Text::new("Back")).on_pressed(move |_ctx| {
                let _ = back.pop();
            }),
            RawButton::new(Text::new("Delete")).on_pressed(confirm),
        ))
    }
}
# use flui::testing::{Tester, find};
# let mut t = Tester::mount(app());
# t.tap(&find::button("Open Talk outline"));
# t.expect(find::text("Talk outline")).to_exist();
# t.tap(&find::button("Delete"));
# t.expect(find::text("Delete this note?")).to_exist();
# t.tap(&find::button("Delete note"));
# t.pump_and_settle();
# t.expect(find::text("Delete this note?")).to_be_gone();
# t.expect(find::button("Back")).to_be_gone();
# t.expect(find::button("Open Groceries")).to_exist();
```

- A page finds its router with `Router::<R>::of(ctx)` and its own route with `PageHandle::of(ctx)`,
  in `build`. The handle moves into the callback, which runs after the build.
- `push` opens a route over the current one, and its path becomes the window's location;
  `PageHandle::pop` closes the page it was taken in.
- A dialog is a `PopupRoute` pushed with `push_popup`. It has no path, so it never enters the URL,
  and the page below stays built and visible under it.
- Each navigation call returns a `Result`: it is refused while the tree builds, or with
  `PageError::Gone` once the page has left its navigator. A press that lands while a page is on
  its way out gets `Gone`, which needs nothing done, so these callbacks drop the result. The
  dialog pops the note only when its own `pop` was taken, so a second press on a dialog already
  leaving does not pop the page under it.

Next, [Theming](theme.md) gives the application its colours.
