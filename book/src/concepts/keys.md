# Keys

Flutter counterpart: `Key`, `ValueKey`, `ObjectKey`, `GlobalKey`.

Keys control how the reconciler matches a new `View` against an existing `Element` when a parent
rebuilds. Without one, matching falls back to position and type, which breaks state when a list
of children is reordered or filtered.

The key types live in `flui::view::key`: `ValueKey`, `UniqueKey` and the `ViewKey` trait, plus two
widget-layer keys. A view holds its key in a field marked `#[view(key)]`, which the
`StatelessView` and `StatefulView` derives read: the field is a key, an `Option` of one, or an
`Rc<dyn ViewKey>` or `Box<dyn ViewKey>`.

```rust
# use flui::prelude::*;
use flui::view::key::ValueKey;

#[derive(Clone, StatelessView)]
struct TodoRow {
    #[view(key)]
    key: ValueKey<u64>,
    title: String,
}

impl StatelessView for TodoRow {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.title.clone())
    }
}

let row = TodoRow { key: ValueKey::new(7), title: "Buy milk".to_owned() };
# let _ = row;
```

The two widget-layer key types:

- `ObjectKey` is keyed by a value's identity, for reordering a list of items each backed by a
  distinct object.
- `GlobalKey<T>` is unique across the whole tree and reaches an element's state from outside the
  normal parent-to-child data flow. Use it sparingly: it breaks locality.

```rust
use std::rc::Rc;
use flui::view::key::{GlobalKey, ObjectKey};

struct Note { title: String }

let note = Rc::new(Note { title: "Groceries".to_owned() });
// Equal for the same `Rc`, different for another `Note` with the same title.
let row_key = ObjectKey::new(&note);
# let _ = (row_key, &note.title);

// A key that will name one `NoteState` wherever it is mounted.
struct NoteState;
let editor_key: GlobalKey<NoteState> = GlobalKey::new();
assert_eq!(editor_key.with_current_state(|_state| ()), None, "not mounted yet");
```
