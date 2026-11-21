//! Notes — the list, pages and theme of the book's tutorial
//! (`book/src/tutorial/`) in one window: a text field that adds a note, a keyed `ListView`
//! of notes, a page per note whose deletion is confirmed in a dialog, and a
//! Material theme that follows the platform's brightness.
//!
//! A note's page deletes its note as it leaves, and keeps showing the note
//! until it is gone.
//!
//! Run with: cargo run --example notes --features material

use std::cell::RefCell;

use flui::prelude::*;
use flui::view::SignalError;
use flui::view::ValueKey;

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

/// The root: it owns the notes, which every page reads.
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
        let add = move |ctx: &mut EventContext<'_>| -> Result<(), SignalError> {
            let text = title.text().trim().to_owned();
            if text.is_empty() {
                title.clear();
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
            })?;
            // Cleared once the note is in: a refused write keeps the typing.
            title.clear();
            Ok(())
        };
        let submit = add.clone();
        let items = notes.get(ctx).items;
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(format!("Notes: {}", items.len()))))
            .body(
                Padding::all(16.0).child(Column::new((
                    TextField::new(self.title.clone())
                        .identifier("new-note")
                        .on_submitted(move |ctx, _text| submit(ctx)),
                    TextButton::new(Text::new("Add note")).on_pressed(add),
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
        TextButton::new(Text::new(self.note.title.clone())).on_pressed(move |_ctx| {
            // Refused only once the list has left its navigator.
            let _ = router.push(NotesRoute::Note { id });
        })
    }
}

/// A note's page: back to the list, or delete the note after a
/// confirmation.
#[derive(Clone, StatefulView)]
struct NotePage {
    id: u64,
    notes: Signal<Notes>,
}

struct NotePageState {
    /// The note as the page last found it: the note is removed as the page
    /// starts to leave, and the page shows it until it is gone.
    shown: RefCell<Option<Note>>,
}

impl StatefulView for NotePage {
    type State = NotePageState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> NotePageState {
        NotePageState {
            shown: RefCell::new(None),
        }
    }
}

impl ViewState<NotePage> for NotePageState {
    fn build(&self, view: &NotePage, ctx: &dyn BuildContext) -> impl IntoView {
        let page = PageHandle::of(ctx).expect("BUG: a note is a page of the Router");
        let (id, notes) = (view.id, view.notes);
        if let Some(note) = notes.get(ctx).items.into_iter().find(|note| note.id == id) {
            *self.shown.borrow_mut() = Some(note);
        }
        let found = self.shown.borrow().clone();
        let back = page.clone();
        let back_button = TextButton::new(Text::new("Back")).on_pressed(move |_ctx| {
            let _ = back.pop();
        });
        // A path can name a note that never existed (a link, a typed URL).
        let Some(note) = found else {
            return Scaffold::new()
                .app_bar(AppBar::new().title(Text::new("No such note")))
                .body(Column::new((back_button,)));
        };
        let confirm = move |_ctx: &mut EventContext<'_>| {
            let note_page = page.clone();
            let _dialog = show_dialog::<(), _, _>(&page, move |ctx| {
                let dialog = PageHandle::of(ctx).expect("BUG: the dialog is a route");
                let (cancel, note_page) = (dialog.clone(), note_page.clone());
                AlertDialog::new()
                    .title(Text::new("Delete this note?"))
                    .actions(vec![
                        TextButton::new(Text::new("Cancel"))
                            .on_pressed(move |_ctx| {
                                let _ = cancel.pop();
                            })
                            .boxed(),
                        TextButton::new(Text::new("Delete note"))
                            .on_pressed(move |ctx| {
                                // Only the press whose pop closed the dialog
                                // deletes, so a second press does nothing; a
                                // refused write leaves the page open.
                                if let Ok(true) = dialog.pop() {
                                    notes.update(ctx, |list| list.items.retain(|n| n.id != id))?;
                                    let _ = note_page.pop();
                                }
                                Ok::<(), SignalError>(())
                            })
                            .boxed(),
                    ])
            });
        };
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(note.title)))
            .body(Column::new((
                back_button,
                TextButton::new(Text::new("Delete")).on_pressed(confirm),
            )))
    }
}
