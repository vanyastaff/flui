# Laying out a screen

Flutter counterpart: `Column`, `Row`, `Padding`, `Expanded`, `SizedBox`.

A screen is views arranged by layout views. Each layout view hands its children constraints and
places them in the space they take ([Layout](../concepts/layout.md) has the protocol). The notes
screen is a header above a list of note cards:

```rust
use flui::prelude::*;

/// One note in the list: its title, and how many words it has.
#[derive(Clone, StatelessView)]
struct NoteCard {
    title: String,
    words: usize,
}

impl StatelessView for NoteCard {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Padding::symmetric(16.0, 8.0).child(Row::new((
            // The title takes whatever width the count leaves.
            Expanded::new(Text::new(self.title.clone())),
            SizedBox::width(8.0),
            Text::new(format!("{} words", self.words)),
        )))
    }
}

#[derive(Clone, StatelessView)]
struct NotesScreen;

impl StatelessView for NotesScreen {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Column::new((
            Padding::all(16.0).child(Text::new("Notes")),
            NoteCard { title: "Groceries".to_owned(), words: 12 },
            NoteCard { title: "Talk outline".to_owned(), words: 240 },
        ))
    }
}
# let mut t = flui::testing::Tester::mount(NotesScreen);
# // The window is 800 wide: both counts end 16 from its right edge, whatever
# // the length of the title beside them.
# let short = t.rect(&flui::testing::find::text("12 words"));
# let long = t.rect(&flui::testing::find::text("240 words"));
# assert_eq!(short.x + short.width, 784.0);
# assert_eq!(long.x + long.width, 784.0);
```

- `Column` and `Row` lay their children out along one axis, given as a tuple. A `Row` takes all
  the width its parent allows, so each card spans the window.
- `Expanded` gives its child the space its siblings leave on the main axis, so the word count
  stays at the right edge however long the title is, and a long title wraps before it.
- `Padding` and `SizedBox` make space: one around a child, the other between two.

A view is a plain struct, so a card is reused by building it with other inputs. The list here is
fixed; [Adding state](state.md) makes it grow.
