# Handling input

Flutter counterpart: `TextField`, `TextEditingController`, `onPressed`.

A note needs a title the user types. A text field edits the text of a `TextEditingController`;
the state that owns the controller reads it when the user adds the note:

```rust
use flui::prelude::*;

#[derive(Clone, StatefulView)]
struct NotesScreen;

struct NotesState {
    notes: Signal<Vec<String>>,
    title: TextEditingController,
}

impl StatefulView for NotesScreen {
    type State = NotesState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> NotesState {
        NotesState {
            notes: ctx.signal(Vec::new()),
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
            if text.trim().is_empty() {
                return Ok(());
            }
            title.clear();
            notes.update(ctx, |list| list.push(text.clone()))
        };
        Column::new((
            RawTextField::new(self.title.clone()).identifier("new-note"),
            RawButton::new(Text::new("Add note")).on_pressed(add),
            Column::new(notes.get(ctx).into_iter().map(Text::new).collect::<Vec<Text>>()),
        ))
    }
}
# use flui::testing::{Tester, find};
# let mut t = Tester::mount(NotesScreen);
# t.enter_text(&find::id("new-note"), "Groceries");
# t.tap(&find::button("Add note"));
# t.expect(find::text("Groceries")).to_exist();
# t.tap(&find::button("Add note"));
# t.expect(find::text("Groceries")).to_have_count(1);
```

- The controller is created with the state and shared by clone: the field edits it, the button's
  callback reads and clears it.
- A callback receives an `EventContext`, the `ctx` a signal write takes. The empty-title check
  returns before writing, so pressing the button with nothing typed changes nothing.
- `identifier` names the field for tests and assistive technology without showing a label.

A pointer is not the only way to press: the button also activates from the keyboard when it has
focus, and the field's `on_submitted` runs when the user presses Enter in it.

Next, [Lists and keys](lists.md) shows the notes in a list that can grow past the window.
