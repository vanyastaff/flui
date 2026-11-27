# Testing

Flutter counterpart: `testWidgets`, `WidgetTester`, `find`.

A test of the application uses the screen as a user does: it finds what they see, acts on it,
and expects what they should see next. `flui::testing` provides it under `flui`'s `testing`
feature, which an application turns on for its tests:

```toml
[dev-dependencies]
flui = { version = "…", features = ["testing"] }
```

Here is a test of the notes screen from [Handling input](input.md); a copy of the screen's code is
hidden above it:

```rust
# use flui::prelude::*;
#
# #[derive(Clone, StatefulView)]
# struct NotesScreen;
#
# struct NotesState {
#     notes: Signal<Vec<String>>,
#     title: TextEditingController,
# }
#
# impl StatefulView for NotesScreen {
#     type State = NotesState;
#
#     fn create_state(&self, ctx: &dyn LifecycleContext) -> NotesState {
#         NotesState {
#             notes: ctx.signal(Vec::new()),
#             title: TextEditingController::new(),
#         }
#     }
# }
#
# impl ViewState<NotesScreen> for NotesState {
#     fn build(&self, _view: &NotesScreen, ctx: &dyn BuildContext) -> impl IntoView {
#         let notes = self.notes;
#         let title = self.title.clone();
#         let add = move |ctx: &mut EventContext<'_>| {
#             let text = title.text().trim().to_owned();
#             title.clear();
#             if text.is_empty() {
#                 return Ok(());
#             }
#             notes.update(ctx, |list| list.push(text.clone()))
#         };
#         let titles = notes.get(ctx);
#         let submit = add.clone();
#         Column::new((
#             Semantics::new()
#                 .label("New note")
#                 .identifier("new-note")
#                 .child(RawTextField::new(self.title.clone())
#                     // Enter in the field adds the note too.
#                     .on_submitted(move |ctx, _text| submit(ctx))),
#             RawButton::new(Text::new("Add note")).on_pressed(add),
#             Text::new(format!("Notes: {}", titles.len())),
#             Column::new(titles.into_iter().map(Text::new).collect::<Vec<Text>>()),
#         ))
#     }
# }
use flui::testing::scenario::matcher::{exists, value};
use flui::testing::scenario::{TestWindow, Tester, find};

// In an application this is the body of a `#[test] fn adds_a_note()`.
let mut t = Tester::mount_in(NotesScreen, TestWindow::new(360, 640));
t.expect(&find::text("Notes: 0"), exists());

t.type_text(&find::id("new-note"), "Groceries");
t.tap(&find::button("Add note"));

t.expect(&find::text("Groceries"), exists());
t.expect(&find::text("Notes: 1"), exists());
t.expect(&find::id("new-note"), value(""));
t.finish();
```

- `Tester::mount` mounts a view as `run_app` does, in an 800 by 600 window on a manual clock;
  `mount_in` takes a `TestWindow` of another size. No window opens and no GPU is needed.
- `find` builds a `Find` over what the screen exposes to assistive technology: `find::text` for
  a label, `find::button` for a button by its name, `find::id` for an element's `identifier`.
  It names exactly one element: two matches fail the step with both listed, and `.first()`,
  `.nth(n)` or `.all()` says which one or how many you mean.
- `tap` and `type_text` act through the window's input path, after checking that the element
  is visible and enabled, as a user would find it.
- `expect(&find, exists())` waits while the application has a frame or work due, then fails at once
  with the window's outline if what it expects is not there.
- `finish` ends the test: it runs the end checks (work left due, a soft failure never raised)
  and fails the test with its report if one does not pass.

A screen where a `Find` matches nothing is usually one whose widget publishes no label; the
failure's outline shows what the window does publish. [Testing](../cookbook/testing.md) in the
cookbook covers time, storage and the other expectations.

Next, [Platforms](platforms.md) runs the application beyond the desktop.
