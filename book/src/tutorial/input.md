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
            let text = title.text().trim().to_owned();
            title.clear();
            if text.is_empty() {
                return Ok(());
            }
            notes.update(ctx, |list| list.push(text.clone()))
        };
        let titles = notes.get(ctx);
        let submit = add.clone();
        Column::new((
            RawTextField::new(self.title.clone())
                .identifier("new-note")
                // Enter in the field adds the note too.
                .on_submitted(move |ctx, _text| submit(ctx)),
            RawButton::new(Text::new("Add note")).on_pressed(add),
            Text::new(format!("Notes: {}", titles.len())),
            Column::new(titles.into_iter().map(Text::new).collect::<Vec<Text>>()),
        ))
    }
}
# use flui::testing::scenario::{Tester, find, matcher::{exists, value}};
# let mut t = Tester::mount(NotesScreen);
# t.type_text(&find::id("new-note"), "Groceries");
# t.tap(&find::button("Add note"));
# t.expect(&find::text("Groceries"), exists());
# t.expect(&find::id("new-note"), value(""));
# // A blank title adds nothing.
# t.type_text(&find::id("new-note"), "   ");
# t.tap(&find::button("Add note"));
# t.expect(&find::text("Notes: 1"), exists());
# t.expect(&find::id("new-note"), value(""));
# // Enter in the field adds what it holds.
# t.type_text(&find::id("new-note"), "Talk outline");
# t.press_keys(&["Enter"]);
# t.expect(&find::text("Talk outline"), exists());
# t.expect(&find::text("Notes: 2"), exists());
# t.expect(&find::id("new-note"), value(""));
```

- The controller is created with the state and shared by clone: the field edits it, the button's
  callback reads and clears it.
- A callback receives an `EventContext`, the `ctx` a signal write takes. The blank-title check
  returns before writing, so pressing the button with only spaces typed adds nothing.
- The field's `on_submitted` runs when the user presses Enter in it; it calls a clone of the same
  callback, since a closure whose captures are `Clone` is `Clone` too.
- `identifier` names the field for tests and assistive technology without showing a label.

The button also activates from the keyboard when it has focus, with Enter or Space.

Next, [Lists and keys](lists.md) shows the notes in a list that can grow past the window.
