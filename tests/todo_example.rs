//! The `todo` example, mounted headless: `#[path]` includes the exact tree
//! `examples/todo.rs` runs, so a layout or wiring defect in the example
//! fails here instead of only in a real window.
//!
//! Covers what the pointer-and-semantics tests of `RawButton` cannot: the
//! new-item field takes the row's remaining width, and its Enter handler,
//! the one write opened through a held `WriterSource`, adds the item.

#[path = "../examples/todo.rs"]
#[allow(dead_code, reason = "the example's `main` is not called here")]
mod todo;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_widgets::testing::{LaidOut, lay_out, tight};
use flui_widgets::{MediaQuery, MediaQueryData};

const WIDTH: f64 = 480.0;

/// The example's root under the `MediaQuery` a running app publishes above
/// it (its `SafeArea` reads one).
fn mount() -> LaidOut {
    lay_out(
        MediaQuery::new(MediaQueryData::default(), todo::TodoApp),
        tight(WIDTH, 640.0),
    )
}

fn key(code: Code, key: Key) -> KeyEvent {
    KeyEventBuilder::new(code)
        .with_key(key)
        .with_state(KeyState::Down)
        .build()
}

/// The new-item field's render object.
fn field(app: &LaidOut) -> flui_foundation::RenderId {
    app.find_by_render_type("RenderEditable")
}

/// Tap the centre of the field, which requests its focus.
fn tap_field(app: &mut LaidOut) {
    let field = field(app);
    let offset = app.absolute_offset(field);
    let size = app.size(field);
    let x = offset.dx + size.width / 2.0;
    let y = offset.dy + size.height / 2.0;
    app.dispatch_pointer_down(x, y);
    app.dispatch_pointer_up(x, y);
    app.tick();
}

fn type_text(app: &LaidOut, text: &str) {
    for ch in text.chars() {
        app.focus_manager()
            .dispatch_key_event(&key(Code::KeyA, Key::Character(ch.to_string())));
    }
}

#[test]
fn the_new_item_field_takes_the_rows_remaining_width() {
    let app = mount();

    let width = app.size(field(&app)).width;
    let add = app.size(app.find_text("Add").expect("the Add button's label"));
    let label = app.size(app.find_text("New item").expect("the field's label"));
    assert!(
        width > (WIDTH / 2.0),
        "the empty field is {width}px wide in a {WIDTH}px row (label {}px, Add {}px)",
        label.width,
        add.width,
    );
}

#[test]
fn enter_in_the_field_adds_the_item_and_clears_the_field() {
    let mut app = mount();
    assert!(app.find_text("milk").is_none());

    tap_field(&mut app);
    type_text(&app, "milk");
    app.tick();
    assert!(app.find_text("milk").is_none(), "typing alone adds nothing");

    let consumed = app
        .focus_manager()
        .dispatch_key_event(&key(Code::Enter, Key::Named(NamedKey::Enter)));
    app.tick();

    assert!(consumed, "the focused field consumes Enter");
    assert!(
        app.find_text("milk").is_some(),
        "Enter adds the typed text as a row"
    );
    assert_eq!(
        app.render_property(field(&app), "text").as_deref(),
        Some(""),
        "the field clears after the item is added"
    );
}
