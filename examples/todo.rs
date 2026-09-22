//! Todo — the step past `counter.rs`: a list instead of a single number.
//! Paired with the "Tutorial: counter → todo" page in the docs book
//! (`book/src/getting-started/tutorial-todo.md`).
//!
//! The one piece of counter.rs's shape that does NOT carry over: a `Vec` is
//! not `Copy`, so it can't live in a `StateCell<T>` (`T: Copy`). This
//! example uses `StateHandle<Vec<Item>>` instead — same `bind(ctx)` in
//! `init_state`, but reads go through `.with(|list| ...)` and mutation is
//! in-place via `.update(|list| ...)` rather than `.get()`/`.set()`.
//!
//! No form/validation widget is used — none exists in this codebase yet.
//! Adding an item is a plain `TextField` (Material's, not `flui-widgets`'
//! theme-free one — see the import below) read at "Add"-press time, the
//! same "read a `TextEditingController` from a button's `on_pressed`"
//! pattern `examples/material_demo` uses for its own Submit button. There
//! is no `on_submitted`/enter-to-add: `TextField` doesn't have one yet.
//!
//! Run with: cargo run --example todo --features material

use flui::material::{
    Checkbox, ElevatedButton, IconButton, InputDecoration, TextField, Theme, ThemeData,
};
use flui::prelude::*;
use flui::widgets::{Icon, IconData, SafeArea, column, row};

/// Material Icons "delete" glyph — a real codepoint in the Material Icons
/// font, but not one already used anywhere in this codebase (every existing
/// `IconData::new(...)` call here is for a different glyph); the
/// `IconData::new(cp).with_font_family("Material Icons")` *pattern* is
/// copied from `examples/material_demo`'s icon-button helpers.
fn delete_icon_data() -> IconData {
    IconData::new(0xE872).with_font_family("Material Icons")
}

/// A visual approximation of Material's standard list-tile height, not a
/// value read from this codebase — `ListView::new` requires a fixed item
/// extent up front (see its doc comment) and this repo has no shared
/// "list tile height" constant to reuse yet.
const ITEM_EXTENT: f32 = 56.0;

#[derive(Clone)]
struct Item {
    id: u64,
    text: String,
    done: bool,
}

#[derive(Clone, StatelessView)]
struct TodoApp;

impl StatelessView for TodoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), SafeArea::new().child(TodoView))
    }
}

#[derive(Clone, StatefulView)]
struct TodoView;

struct TodoState {
    items: StateHandle<Vec<Item>>,
    new_item: TextEditingController,
}

impl StatefulView for TodoView {
    type State = TodoState;

    fn create_state(&self) -> Self::State {
        TodoState {
            items: StateHandle::new(Vec::new()),
            new_item: TextEditingController::new(),
        }
    }
}

impl ViewState<TodoView> for TodoState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.items.bind(ctx);
    }

    fn build(&self, _view: &TodoView, _ctx: &dyn BuildContext) -> impl IntoView {
        let items = self.items.clone();
        let new_item_field = self.new_item.clone();

        let rows: Vec<BoxedView> = self.items.with(|list| {
            list.iter()
                .map(|item| {
                    let id = item.id;
                    let toggle_items = items.clone();
                    let delete_items = items.clone();

                    Row::new(row![
                        Checkbox::new(item.done).on_changed(move |next| {
                            toggle_items.update(|list| {
                                if let Some(it) = list.iter_mut().find(|it| it.id == id) {
                                    it.done = next.unwrap_or(false);
                                }
                            });
                        }),
                        Text::new(item.text.clone()),
                        IconButton::new(Icon::new(delete_icon_data())).on_pressed(move || {
                            delete_items.update(|list| list.retain(|it| it.id != id))
                        }),
                    ])
                    .boxed()
                })
                .collect()
        });

        let add_item_field = self.new_item.clone();

        Column::new(column![
            Row::new(row![
                TextField::new(new_item_field).decoration(InputDecoration {
                    label_text: Some("New item".to_string()),
                    ..Default::default()
                }),
                ElevatedButton::new(Text::new("Add")).on_pressed(move || {
                    let text = add_item_field.text();
                    if text.is_empty() {
                        return;
                    }
                    items.update(|list| {
                        let id = list.last().map_or(0, |it| it.id + 1);
                        list.push(Item {
                            id,
                            text: text.clone(),
                            done: false,
                        });
                    });
                }),
            ]),
            SizedBox::height(16.0),
            ListView::new(ITEM_EXTENT, rows),
        ])
    }
}

fn main() {
    run_app(TodoApp);
}
