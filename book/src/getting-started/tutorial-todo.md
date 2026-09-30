# Tutorial: counter → todo

[Installation and first run](installation.md) got the counter running. This page walks the same
shape one step further — a list instead of a single number — by comparing
[`examples/counter.rs`](https://github.com/vanyastaff/flui/blob/main/examples/counter.rs) against
[`examples/todo.rs`](https://github.com/vanyastaff/flui/blob/main/examples/todo.rs) line by line.
Both are real, compiling examples in the repository. The snippets below are excerpts from them,
trimmed to the lines that differ; `/* … */` marks what an excerpt leaves out.

```bash
cargo run --example todo
```

## What stays the same

The skeleton is identical to the counter: a `StatelessView` (`TodoApp`) that wraps a
`StatefulView` (`TodoView`) in `SafeArea::new().child(...)`, and a `ViewState` (`TodoState`) that
creates its state in `init_state` and builds a tree in `build`. Both use only the theme-free
widgets catalog, so neither needs the `material` feature. If that shape isn't familiar yet, read
[Installation and first run](installation.md) and
[View, Element, RenderObject](../concepts/view-element-render.md) first.

## What changes: `Signal<usize>` → `Signal<Vec<Item>>`

The counter's state is a `Signal<usize>`; the todo list's is a `Signal<Vec<Item>>`. A signal
holds any `'static` value, so nothing else about the state changes: it is created in `init_state`,
read in `build`, and written from a button's press through the `cx` the press callback receives.
What changes is how you read and write a value you don't want to copy — through closures:

```rust,ignore
// counter.rs
count: Signal<usize>,
self.count = ctx.signal(0);
count.update(cx, |n| *n += 1)
count.get(ctx)

// todo.rs
items: Signal<Vec<Item>>,
self.items = ctx.signal(Vec::new());
items.update(cx, |list| list.retain(|it| it.id != id))   // FnOnce(&mut T) — in place
items.with(ctx, |list| /* read list */)                   // FnOnce(&T) -> R
```

`update` takes `FnOnce(&mut T)`, so `list.push(...)`/`list.retain(...)` mutate the `Vec` in place
rather than rebuilding a whole new value. A `Signal` is `Copy`, so every closure below captures
`items` itself — no clone per row.

## Rendering the list: `ListView`

Where the counter renders one `Text`, the todo view maps every item to a row and hands the whole
`Vec` to `ListView`:

```rust,ignore
let rows: Vec<BoxedView> = items.with(ctx, |list| {
    list.iter()
        .map(|item| /* build one Row, .boxed() it */)
        .collect()
});
// ...
Expanded::new(ListView::new(ITEM_EXTENT, rows)),
```

Reading through `items.with(ctx, ...)` in `build` subscribes the view to the list, so any write
rebuilds it. `ListView::new` takes a fixed item extent and any `ViewSeq` — a `Vec<BoxedView>`
built inside the read closure is the natural fit for a runtime-sized, all-known-upfront list (as
opposed to `ListView::builder`, for a lazily-materialized one). The list sits in a `Column`,
which lays its children out with unbounded height, so `Expanded` gives it the height that is left;
without it the rows are laid out but not on screen, and a tap never reaches them.

## One row: toggle, text, delete

Each item is a `Row` (the same `row!`/`column!` macro family the counter uses, just the other
axis) of a toggle button, the item's `Text`, and a delete button:

```rust,ignore
Row::new(row![
    RawButton::new(Text::new(mark)).on_press(move |cx| {
        items.update(cx, |list| {
            if let Some(it) = list.iter_mut().find(|it| it.id == id) {
                it.done = !it.done;
            }
        })
    }),
    Text::new(item.text.clone()),
    RawButton::new(Text::new("Delete")).on_press(move |cx| {
        items.update(cx, |list| list.retain(|it| it.id != id))
    }),
])
```

Two things worth calling out:

- **The press callback returns the write's `Result`.** `RawButton::on_press` accepts a callback
  that returns `()` or a signal write's `Result`, so no `let _` is needed; a refused write is
  logged rather than dropped.
- Each item needs a stable `id` (not its position in the `Vec`) so the toggle/delete closures
  still target the right item after another item is deleted and the list reindexes. `todo.rs`
  computes it as `list.last().map_or(0, |it| it.id + 1)` when pushing — simple, and enough for an
  in-memory list that only ever grows a counter, never reuses an id.

## Adding an item: one helper, two triggers

A single field needs no `Form` (see `examples/form.rs` for `Form` and `TextFormField`). `todo.rs`
uses the theme-free `RawTextField` backed by a `TextEditingController`. Both ways of submitting a
new item — pressing Enter and clicking "Add" — end up calling the same `add_item` function, which
takes the `cx` to write with:

```rust,ignore
fn add_item(
    cx: &mut EventCx<'_>,
    items: Signal<Vec<Item>>,
    field: &TextEditingController,
    text: &str,
) -> Result<(), SignalError> {
    if text.is_empty() {
        return Ok(());
    }
    items.update(cx, |list| { /* push a new Item, one past the highest existing id */ })?;
    field.clear();
    Ok(())
}
```

```rust,ignore
Text::new("New item"),
SizedBox::width(8.0),
Expanded::new(
    RawTextField::new(self.new_item.clone()).on_submitted(move |cx, text| {
        add_item(cx, items, &submit_field, text)
    })
),
RawButton::new(Text::new("Add")).on_press(move |cx| {
    add_item(cx, items, &button_field, &button_field.text())
}),
```

Both the button's press and `RawTextField::on_submitted` receive `cx` and pass it
straight to `add_item`. The submitted callback also receives the text when Enter
is pressed while the field has focus. Returning the write's `Result` lets the
framework report a refused write on `flui::signals`; neither callback needs a
captured `WriterSource` or a manual `.report()`. The field sits in `Expanded` for the same reason as the
list: a `Row` gives its children unbounded width, and an empty field sized to its text would be
a few pixels wide. The button reads the controller's live text directly (`.text()`), since a press has no text to hand
over. `TextEditingController::clear()` empties the field after either path adds the item, so it's
ready for the next one.

## The whole file

[`examples/todo.rs`](https://github.com/vanyastaff/flui/blob/main/examples/todo.rs) is short
enough to read end to end — every snippet above is copied from it, so reading the file directly
is the fastest way to see how the pieces fit together as one `build`.
