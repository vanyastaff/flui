//! Notes — the list, pages and theme of the book's tutorial
//! (`book/src/tutorial/`) in one window: a text field that adds a note, a keyed `ListView`
//! of notes, a page per note whose deletion is confirmed in a dialog, and a
//! Material theme that follows the platform's brightness.
//!
//! The note's page pops with whether it was deleted, and the row that opened
//! it removes the note, so the page never shows a note that is gone while it
//! leaves.
//!
//! Run with: cargo run --example notes --features material

use std::cell::RefCell;
use std::rc::Rc;

use flui::prelude::*;
use flui::view::key::ValueKey;
use flui::view::{AsyncDriver, SignalError, TaskToken, WriterSource};

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
                                notes,
                            }
                            .boxed(),
                        )
                    })),
                ))),
            )
    }
}

/// One note in the list. Pressing it opens the note's page and waits for
/// the page's answer, which belongs to this note: the key keeps the wait
/// with its note when others are added or removed.
#[derive(Clone, StatefulView)]
struct NoteRow {
    #[view(key)]
    key: ValueKey<u64>,
    note: Note,
    notes: Signal<Notes>,
}

struct NoteRowState {
    writer: WriterSource,
    driver: Option<AsyncDriver>,
    /// The task awaiting the open page's answer; dropping it cancels it.
    waiting: Rc<RefCell<Option<TaskToken>>>,
}

impl StatefulView for NoteRow {
    type State = NoteRowState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> NoteRowState {
        NoteRowState {
            writer: ctx.writer_source(),
            driver: ctx.async_driver(),
            waiting: Rc::default(),
        }
    }
}

impl ViewState<NoteRow> for NoteRowState {
    fn build(&self, view: &NoteRow, ctx: &dyn BuildContext) -> impl IntoView {
        let router = Router::<NotesRoute>::of(ctx).expect("BUG: the list is a page of the Router");
        let (id, notes) = (view.note.id, view.notes);
        let (writer, driver) = (self.writer.clone(), self.driver.clone());
        let waiting = Rc::clone(&self.waiting);
        TextButton::new(Text::new(view.note.title.clone())).on_pressed(move |_ctx| {
            // Refused only once the list has left its navigator.
            let (Ok(answer), Some(driver)) = (
                router.push_for_result::<bool>(NotesRoute::Note { id }),
                &driver,
            ) else {
                return;
            };
            let writer = writer.clone();
            let task = driver.spawn_local(Box::pin(async move {
                if let Ok(Some(true)) = answer.await {
                    // Refused only once the UI runtime has closed.
                    let _ = writer.write(|ctx| {
                        notes.update(ctx, |list| list.items.retain(|note| note.id != id))
                    });
                }
            }));
            *waiting.borrow_mut() = Some(task);
        })
    }
}

/// A note's page: back to the list, or delete the note after a
/// confirmation, popping with `true`.
#[derive(Clone, StatefulView)]
struct NotePage {
    id: u64,
    notes: Signal<Notes>,
}

struct NotePageState {
    /// The note as the page last found it: the list removes the note as the
    /// page starts to leave, and the page shows it until it is gone.
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
        let id = view.id;
        if let Some(note) = view
            .notes
            .get(ctx)
            .items
            .into_iter()
            .find(|note| note.id == id)
        {
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
                            .on_pressed(move |_ctx| {
                                // The page answers only when the dialog's pop
                                // was taken, so a second press answers nothing.
                                if let Ok(true) = dialog.pop() {
                                    let _ = note_page.pop_with(true);
                                }
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
