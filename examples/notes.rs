//! Notes — the application the book's tutorial builds, chapter by chapter, in
//! one program: a text field and a keyed `ListView` of notes, a page per note
//! with a confirmation dialog, and a Material theme that follows the platform's
//! brightness.
//!
//! Each piece is explained, and checked headless, in the book's tutorial
//! (`book/src/tutorial/`).
//!
//! Run with: cargo run --example notes --features material

use flui::prelude::*;
use flui::view::key::ValueKey;

#[derive(Clone, Default)]
struct Notes {
    next_id: u64,
    items: Vec<Note>,
}

#[derive(Clone)]
struct Note {
    id: u64,
    title: String,
}

#[derive(Routable, Clone, Debug, PartialEq)]
enum NotesRoute {
    #[route("/")]
    List,
    #[route("/note/:id")]
    Note { id: u64 },
}

fn main() -> Result<(), LaunchError> {
    Application::new()
        .config(AppConfig::new().title("Notes").size(480, 640))
        .run(NotesApp)
}

/// The root: it owns the notes, which every page reads and writes.
#[derive(Clone, StatefulView)]
struct NotesApp;

struct NotesAppState {
    notes: Signal<Notes>,
}

impl StatefulView for NotesApp {
    type State = NotesAppState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> NotesAppState {
        NotesAppState {
            notes: ctx.signal(Notes::default()),
        }
    }
}

impl ViewState<NotesApp> for NotesAppState {
    fn build(&self, _view: &NotesApp, _ctx: &dyn BuildContext) -> impl IntoView {
        let notes = self.notes;
        let router = Router::new(
            NotesRoute::List,
            move |route: &NotesRoute, _ctx| match route {
                NotesRoute::List => NoteList { notes }.boxed(),
                NotesRoute::Note { id } => NotePage { id: *id, notes }.boxed(),
            },
        );
        MaterialApp::router(router)
            .theme(ThemeData::light())
            .dark_theme(ThemeData::dark())
            .theme_mode(ThemeMode::System)
    }
}

/// The list: a field to add a note, and every note, each opening its page.
#[derive(Clone, StatefulView)]
struct NoteList {
    notes: Signal<Notes>,
}

struct NoteListState {
    title: TextEditingController,
}

impl StatefulView for NoteList {
    type State = NoteListState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> NoteListState {
        NoteListState {
            title: TextEditingController::new(),
        }
    }
}

impl ViewState<NoteList> for NoteListState {
    fn build(&self, view: &NoteList, ctx: &dyn BuildContext) -> impl IntoView {
        let notes = view.notes;
        let title = self.title.clone();
        let add = move |ctx: &mut EventContext<'_>| {
            let text = title.text().trim().to_owned();
            title.clear();
            if text.is_empty() {
                return Ok(());
            }
            notes.update(ctx, |list| {
                // Ids are never reused, so a key names one note for its life.
                let id = list.next_id;
                list.next_id += 1;
                list.items.push(Note {
                    id,
                    title: text.clone(),
                });
            })
        };
        let submit = add.clone();
        let items = notes.get(ctx).items;
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(format!("Notes: {}", items.len()))))
            .body(
                Padding::all(16.0).child(Column::new((
                    RawTextField::new(self.title.clone())
                        .identifier("new-note")
                        .on_submitted(move |ctx, _text| submit(ctx)),
                    RawButton::new(Text::new("Add note")).on_pressed(add),
                    Expanded::new(ListView::builder(items.len(), 48.0, move |index| {
                        let note = items.get(index)?;
                        Some(
                            NoteRow {
                                key: ValueKey::new(note.id),
                                note: note.clone(),
                            }
                            .boxed(),
                        )
                    })),
                ))),
            )
    }
}

/// One note in the list; pressing it opens the note's page.
#[derive(Clone, StatelessView)]
struct NoteRow {
    #[view(key)]
    key: ValueKey<u64>,
    note: Note,
}

impl StatelessView for NoteRow {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let router = Router::<NotesRoute>::of(ctx).expect("BUG: the list is a page of the Router");
        let id = self.note.id;
        RawButton::new(Text::new(self.note.title.clone())).on_pressed(move |_ctx| {
            // Refused only once the list has left its navigator.
            let _ = router.push(NotesRoute::Note { id });
        })
    }
}

/// A note's page: back to the list, or delete the note after a confirmation.
#[derive(Clone, StatelessView)]
struct NotePage {
    id: u64,
    notes: Signal<Notes>,
}

impl StatelessView for NotePage {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let page = PageHandle::of(ctx).expect("BUG: a note is a page of the Router");
        let (notes, id) = (self.notes, self.id);
        let title = notes
            .get(ctx)
            .items
            .into_iter()
            .find(|note| note.id == id)
            .map_or_else(|| "Deleted".to_owned(), |note| note.title);
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
                    RawButton::new(Text::new("Delete note")).on_pressed(move |ctx| {
                        // The note's page pops only when the dialog's pop
                        // was taken, so a second press pops nothing.
                        if let Ok(true) = dialog.pop() {
                            let _ = note.pop();
                            return notes.update(ctx, |list| list.items.retain(|n| n.id != id));
                        }
                        Ok(())
                    }),
                ))
                .boxed()
            });
            let _ = page.push_popup(dialog);
        };
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(title)))
            .body(Column::new((
                RawButton::new(Text::new("Back")).on_pressed(move |_ctx| {
                    let _ = back.pop();
                }),
                RawButton::new(Text::new("Delete")).on_pressed(confirm),
            )))
    }
}
