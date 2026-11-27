# Lists and keys

Flutter counterpart: `ListView.builder`, `ValueKey`.

A list of notes can grow past the window. `ListView::builder` builds only the rows that are in
view, asking its builder for the row at each index. Each row here has state of its own, whether
it is open, and a key so that state stays with its note when a note above it is deleted:

```rust
use flui::prelude::*;
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

#[derive(Clone, StatefulView)]
struct NotesScreen;

struct NotesState {
    notes: Signal<Notes>,
    title: TextEditingController,
}

impl StatefulView for NotesScreen {
    type State = NotesState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> NotesState {
        NotesState {
            notes: ctx.signal(Notes::default()),
            title: TextEditingController::new(),
        }
    }
}

impl ViewState<NotesScreen> for NotesState {
    fn build(&self, _view: &NotesScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let notes = self.notes;
        let title = self.title.clone();
        let add = move |ctx: &mut EventContext<'_>| {
            let text = title.text();
            title.clear();
            notes.update(ctx, |list| {
                // Ids are never reused, so a key names one note for its life.
                let id = list.next_id;
                list.next_id += 1;
                list.items.push(Note { id, title: text.clone() });
            })
        };
        let items = notes.get(ctx).items;
        Column::new((
            RawTextField::new(self.title.clone()).identifier("new-note"),
            RawButton::new(Text::new("Add note")).on_pressed(add),
            Expanded::new(ListView::builder(items.len(), 48.0, move |index| {
                let note = items.get(index)?;
                Some(
                    NoteRow { key: ValueKey::new(note.id), note: note.clone(), notes }.boxed(),
                )
            })),
        ))
    }
}

/// One note: press its title to open it, or delete it.
#[derive(Clone, StatefulView)]
struct NoteRow {
    #[view(key)]
    key: ValueKey<u64>,
    note: Note,
    notes: Signal<Notes>,
}

struct NoteRowState {
    open: Signal<bool>,
}

impl StatefulView for NoteRow {
    type State = NoteRowState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> NoteRowState {
        NoteRowState { open: ctx.signal(false) }
    }
}

impl ViewState<NoteRow> for NoteRowState {
    fn build(&self, row: &NoteRow, ctx: &dyn BuildContext) -> impl IntoView {
        let open = self.open;
        let (notes, id) = (row.notes, row.note.id);
        let label = if open.get(ctx) {
            format!("{} (open)", row.note.title)
        } else {
            row.note.title.clone()
        };
        let toggle = move |ctx: &mut EventContext<'_>| open.update(ctx, |open| *open = !*open);
        let delete = move |ctx: &mut EventContext<'_>| {
            notes.update(ctx, |list| list.items.retain(|note| note.id != id))
        };
        Row::new((
            Expanded::new(RawButton::new(Text::new(label)).on_pressed(toggle)),
            RawButton::new(Text::new(format!("Delete {}", row.note.title))).on_pressed(delete),
        ))
    }
}
# use flui::testing::scenario::{Tester, find, matcher::{exists, gone}};
# let mut t = Tester::mount(NotesScreen);
# for title in ["Groceries", "Talk outline"] {
#     t.type_text(&find::id("new-note"), title);
#     t.tap(&find::button("Add note"));
# }
# t.tap(&find::button("Talk outline"));
# t.expect(&find::text("Talk outline (open)"), exists());
# t.tap(&find::button("Delete Groceries"));
# t.expect(&find::text("Groceries"), gone());
# t.expect(&find::text("Talk outline (open)"), exists());
```

- The builder returns `None` past the end of the data, and `?` on `items.get(index)` says so.
- A row's key is its note's id. When "Groceries" is deleted, "Talk outline" moves up one place;
  the key lets its element, and the `open` signal its state holds, move with it. Without the key
  the list would match rows by position, and the open row would close.
- The row writes to the list's signal, which it was given as a field: a `Signal` is a `Copy`
  handle, so passing it down is how a child changes its parent's data.

[Keys](../concepts/keys.md) has the other key types. Next, [Navigation](navigation.md) gives each
note a page of its own.
