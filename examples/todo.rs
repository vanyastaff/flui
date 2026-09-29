//! Todo — the step past `counter.rs`: a list instead of a single number.
//! Paired with the "Tutorial: counter → todo" page in the docs book
//! (`book/src/getting-started/tutorial-todo.md`).
//!
//! The list lives in a `Signal<Vec<Item>>`, exactly like counter.rs's
//! `Signal<usize>`: created in `init_state`, read in `build` with
//! `items.with(ctx, |list| ...)` (which rebuilds this view when the list
//! changes), and changed in place with `items.update(cx, |list| ...)` from a
//! button's press. A `Signal` is `Copy`, so every row's closures capture it
//! without a clone.
//!
//! No form/validation widget is used — a single field needs none (see
//! `examples/form.rs` for `Form` and `TextFormField`). Adding an item is a
//! theme-free `RawTextField`: pressing Enter (`RawTextField::on_submitted`)
//! or the "Add" button both call the same `add_item` helper, which pushes the
//! item and clears the field (`TextEditingController::clear`). Both
//! callbacks receive the `cx` they pass on to it.
//!
//! Run with: cargo run --example todo
//! (`tests/todo_example.rs` mounts this file's tree headless.)

use flui::prelude::*;
use flui::view::SignalError;
use flui::widgets::{SafeArea, column, row};

/// The height of one row. `ListView::new` requires a fixed item extent up
/// front (see its doc comment).
const ITEM_EXTENT: f64 = 56.0;

#[derive(Clone)]
struct Item {
    id: u64,
    text: String,
    done: bool,
}

/// Shared by the "Add" button's `on_press` and the field's
/// `on_submitted` — both end up here with the field's current text, so
/// pressing Enter and clicking Add behave identically. Empty text is a
/// no-op (nothing to add); otherwise the new item gets one past the
/// highest existing id and the field clears itself for the next entry.
fn add_item(
    cx: &mut EventCx<'_>,
    items: Signal<Vec<Item>>,
    field: &TextEditingController,
    text: &str,
) -> Result<(), SignalError> {
    if text.is_empty() {
        return Ok(());
    }
    items.update(cx, |list| {
        let id = list.last().map_or(0, |it| it.id + 1);
        list.push(Item {
            id,
            text: text.to_string(),
            done: false,
        });
    })?;
    field.clear();
    Ok(())
}

#[derive(Clone, StatelessView)]
pub(crate) struct TodoApp;

impl StatelessView for TodoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SafeArea::new().child(TodoView)
    }
}

#[derive(Clone, StatefulView)]
struct TodoView;

struct TodoState {
    items: Signal<Vec<Item>>,
    new_item: TextEditingController,
}

impl StatefulView for TodoView {
    type State = TodoState;

    fn create_state(&self) -> Self::State {
        TodoState {
            items: Signal::default(),
            new_item: TextEditingController::new(),
        }
    }
}

impl ViewState<TodoView> for TodoState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.items = ctx.signal(Vec::new());
    }

    fn build(&self, _view: &TodoView, ctx: &dyn BuildContext) -> impl IntoView {
        let items = self.items;

        let rows: Vec<BoxedView> = items.with(ctx, |list| {
            list.iter()
                .map(|item| {
                    let id = item.id;
                    let mark = if item.done { "[x]" } else { "[ ]" };

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
                    .boxed()
                })
                .collect()
        });

        let submit_field = self.new_item.clone();
        let button_field = self.new_item.clone();

        Column::new(column![
            Row::new(row![
                Text::new("New item"),
                SizedBox::width(8.0),
                // A `Row` gives its children unbounded width, and a text
                // field sizes itself to its text; the field takes what is
                // left, or an empty one is a few pixels wide.
                Expanded::new(
                    RawTextField::new(self.new_item.clone())
                        .on_submitted(move |cx, text| { add_item(cx, items, &submit_field, text) })
                ),
                RawButton::new(Text::new("Add")).on_press(move |cx| {
                    add_item(cx, items, &button_field, &button_field.text())
                }),
            ]),
            SizedBox::height(16.0),
            // A `Column` gives its children unbounded height; the list takes
            // what is left, or its rows are laid out but never reachable.
            Expanded::new(ListView::new(ITEM_EXTENT, rows)),
        ])
    }
}

fn main() {
    run_app(TodoApp);
}
