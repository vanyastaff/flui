# Tutorial: counter → todo

[Installation and first run](installation.md) got the counter running. This page walks the same
shape one step further — a list instead of a single number — by comparing
[`examples/counter.rs`](https://github.com/vanyastaff/flui/blob/main/examples/counter.rs) against
[`examples/todo.rs`](https://github.com/vanyastaff/flui/blob/main/examples/todo.rs) line by line.
Both are real, compiling examples in the repository; nothing on this page is pseudocode.

```bash
cargo run --example todo --features material
```

## What stays the same

The skeleton is identical to the counter: a `StatelessView` (`TodoApp`) that wraps a
`StatefulView` (`TodoView`) in `Theme::new(ThemeData::light(), SafeArea::new().child(...))`, and a
`ViewState` (`TodoState`) that binds its state in `init_state` and builds a tree in `build`. If
that shape isn't familiar yet, read [Installation and first run](installation.md) and
[View, Element, RenderObject](../concepts/view-element-render.md) first.

## What changes: `StateCell` → `StateHandle`

The counter's state is `StateCell<usize>` — `usize` is `Copy`, and `StateCell<T>` requires
`T: Copy`. A todo list's state is `Vec<Item>`, which isn't `Copy`, so `StateCell` can't hold it.
The fix is `StateHandle<T>` instead — no `Copy` bound, same `bind(ctx)` contract, but reads and
mutations go through closures rather than `.get()`/`.set()`:

```rust,ignore
// counter.rs
count: StateCell<usize>,
self.count.bind(ctx);
count.update(|n| n + 1);
self.count.get()

// todo.rs
items: StateHandle<Vec<Item>>,
self.items.bind(ctx);
items.update(|list| list.push(new_item));   // FnOnce(&mut T) — in-place
self.items.with(|list| /* read list */ )    // FnOnce(&T) -> R
```

`StateHandle::update` takes `FnOnce(&mut T)`, so `list.push(...)`/`list.retain(...)` mutate the
`Vec` in place rather than rebuilding a whole new value — the same pattern
[State](../concepts/state.md) shows for `ValueNotifier`-adjacent state.

## Rendering the list: `ListView`

Where the counter renders one `Text`, the todo view maps every item to a row and hands the whole
`Vec` to `ListView`:

```rust,ignore
let rows: Vec<BoxedView> = self.items.with(|list| {
    list.iter()
        .map(|item| /* build one Row, .boxed() it */)
        .collect()
});
// ...
ListView::new(ITEM_EXTENT, rows)
```

`ListView::new` takes a fixed item extent and any `ViewSeq` — a `Vec<BoxedView>` built inside the
`.with(...)` read closure is the natural fit for a runtime-sized, all-known-upfront list (as
opposed to `ListView::builder`, for a lazily-materialized one).

## One row: checkbox, text, delete

Each item is a `Row` (the same `row!`/`column!` macro family the counter uses, just the other
axis) of a `Checkbox`, the item's `Text`, and an `IconButton` to delete it:

```rust,ignore
Row::new(row![
    Checkbox::new(item.done).on_changed(move |next| {
        toggle_items.update(|list| {
            if let Some(it) = list.iter_mut().find(|it| it.id == id) {
                it.done = next.unwrap_or(false);
            }
        });
    }),
    Text::new(item.text.clone()),
    IconButton::new(Icon::new(delete_icon_data()))
        .on_pressed(move || delete_items.update(|list| list.retain(|it| it.id != id))),
])
```

Two things worth calling out because they're easy to get wrong copying from memory rather than
from the source:

- **`Checkbox::on_changed`'s callback is `Fn(Option<bool>)`**, not `Fn(bool)`, even though
  `Checkbox::new` only ever produces `Some(_)` — unwrap with `.unwrap_or(false)`.
- Each item needs a stable `id` (not its position in the `Vec`) so the toggle/delete closures
  still target the right item after another item is deleted and the list reindexes. `todo.rs`
  computes it as `list.last().map_or(0, |it| it.id + 1)` when pushing — simple, and enough for an
  in-memory list that only ever grows a counter, never reuses an id.

## Adding an item: no form widget, no separate "hidden" duplicated logic

There is no `Form`/validated-field wrapper in this codebase — [Forms](../cookbook/forms.md)'s
validated `TextField` example is hand-rolled state, not a `Form` widget. `todo.rs` does the same,
simpler: a plain `TextField` (the **Material** one — `flui::material::TextField`. `flui::prelude`
resolves a bare `TextField` to this one whenever the `material` feature is on, explicitly shadowing
`flui-widgets`' theme-free type of the same name — see `ARCHITECTURE.md`'s
`## Mapping decisions` for why) backed by a `TextEditingController`. Both ways of submitting a new
item — pressing Enter and clicking "Add" — end up calling the same `add_item` function:

```rust,ignore
fn add_item(items: &StateHandle<Vec<Item>>, field: &TextEditingController, text: &str) {
    if text.is_empty() {
        return;
    }
    items.update(|list| { /* push a new Item, one past the highest existing id */ });
    field.clear();
}
```

```rust,ignore
TextField::new(new_item_field)
    .decoration(InputDecoration { label_text: Some("New item".to_string()), ..Default::default() })
    .on_submitted(move |text| add_item(&submit_items, &submit_field, text)),
ElevatedButton::new(Text::new("Add")).on_pressed(move || {
    let text = button_field.text();
    add_item(&button_items, &button_field, &text);
}),
```

`TextField::on_submitted(Fn(&str))` fires on a raw Enter keypress while the field has focus —
Flutter parity for `EditableText.onSubmitted`, implemented at the `flui-widgets` `EditableText`
level (this substrate has no platform IME-action-button integration, so Enter is the trigger; see
that type's own doc). The button's `on_pressed` still reads the controller's live text directly
(`.text()`), the same pattern
[`examples/material_demo`](https://github.com/vanyastaff/flui/blob/main/examples/material_demo/tree.rs)
uses for its own Submit button (see [Forms](../cookbook/forms.md)) — `on_submitted` hands the text
straight to its callback, but a plain button press has no such event to read it from.
`TextEditingController::clear()` empties the field after either path adds the item, so it's ready
for the next one.

## The whole file

[`examples/todo.rs`](https://github.com/vanyastaff/flui/blob/main/examples/todo.rs) is short
enough to read end to end — every snippet above is copied verbatim from it, so reading the file
directly is the fastest way to see how the pieces fit together as one `build`.
