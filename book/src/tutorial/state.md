# Adding state

Flutter counterpart: `StatefulWidget`, `State`, `setState`.

A view whose content changes keeps its changing part in a state. A `StatefulView` names its state
type and creates it in `create_state`; the state's `build` describes the view from it. The notes
screen keeps its notes in a signal and adds one each time the button is pressed:

```rust
use flui::prelude::*;

#[derive(Clone, StatefulView)]
struct NotesScreen;

struct NotesState {
    notes: Signal<Vec<String>>,
}

impl StatefulView for NotesScreen {
    type State = NotesState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> NotesState {
        NotesState {
            notes: ctx.signal(Vec::new()),
        }
    }
}

impl ViewState<NotesScreen> for NotesState {
    fn build(&self, _view: &NotesScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let notes = self.notes;
        let titles = notes.get(ctx);
        Column::new((
            Text::new(format!("Notes: {}", titles.len())),
            Column::new(titles.into_iter().map(Text::new).collect::<Vec<Text>>()),
            RawButton::new(Text::new("Add note")).on_pressed(move |ctx| {
                notes.update(ctx, |list| {
                    let title = format!("Note {}", list.len() + 1);
                    list.push(title);
                })
            }),
        ))
    }
}
# let mut t = flui::testing::Tester::mount(NotesScreen);
# t.expect(flui::testing::find::text("Notes: 0")).to_exist();
# t.tap(&flui::testing::find::button("Add note"));
# t.tap(&flui::testing::find::button("Add note"));
# t.expect(flui::testing::find::text("Notes: 2")).to_exist();
# t.expect(flui::testing::find::text("Note 2")).to_exist();
```

- The signal is created in `create_state`, so the state is born with it; `ctx.signal` is not
  available in `build`.
- `build` reads the signal with `get(ctx)`. The read subscribes this view, so a write rebuilds
  it and nothing else.
- The button's callback receives a `ctx` and writes through it with `update`, returning the
  write's `Result`, so the button logs a refused write. `build` gets no context a write
  accepts, so a write there does not compile.
- A `Signal` is a `Copy` handle, so the callback captures its own copy with `move`.

[State](../concepts/state.md) covers the other forms of state: data shared down the tree with an
`InheritedView`, and a `ValueNotifier` that code outside the tree listens to.

Next, [Handling input](input.md) lets the user name each note.
