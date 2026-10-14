//! The focus ring follows the input modality: keyboard and assistive
//! technology show it, a pointer hides it, and a stray modifier release left
//! over from switching windows changes nothing.

use flui_painting::display_list::DrawOp;
use flui_painting::styling::Color;
use flui_platform_api::keyboard::{Key, KeyRepeat, KeyState, NamedKey};
use flui_testing::{Action, ActionRequest, TreeId};
use flui_widgets::{DefaultFocusRingStyle, FocusRingStyle, RawButton, Text};

use super::{key, press, tab};
use crate::common::{LaidOut, lay_out, loose};

const STYLE: FocusRingStyle = FocusRingStyle {
    outer: Color::rgb(200, 0, 0),
    inner: Color::rgb(0, 200, 0),
    outer_width: 2.0,
    inner_width: 1.0,
};

/// A themed button labelled `Go`.
fn app() -> LaidOut {
    let mut app = lay_out(
        DefaultFocusRingStyle::new(STYLE, RawButton::new(Text::new("Go")).on_press(|_cx| {})),
        loose(400.0),
    );
    app.pump();
    app
}

/// Whether the last frame painted both bands of the ring.
fn ring_painted(app: &LaidOut) -> bool {
    let colors: Vec<Color> = app
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { paint, .. }
            | DrawOp::RRect { paint, .. }
            | DrawOp::DRRect { paint, .. }
            | DrawOp::Path { paint, .. }
            | DrawOp::Line { paint, .. } => Some(paint.color),
            _ => None,
        })
        .collect();
    let outer = colors.contains(&STYLE.outer);
    let inner = colors.contains(&STYLE.inner);
    assert_eq!(outer, inner, "the ring paints both of its bands or neither");
    outer
}

fn tab_in(app: &mut LaidOut) {
    let manager = app.focus_manager();
    assert!(
        app.enter_owner_scope(|| tab(&manager)),
        "Tab focuses the button"
    );
    app.pump();
}

/// Tab shows the ring on the focused button.
fn tab_shows_the_ring() {
    let mut app = app();
    assert!(!ring_painted(&app), "nothing is focused yet");
    tab_in(&mut app);
    assert!(ring_painted(&app), "focus Tab moved is shown");
}

/// Assistive technology moving focus shows the ring, as Tab does.
fn assistive_focus_shows_the_ring() {
    let mut app = app();
    app.enable_semantics();
    app.pump();
    let tree = app.a11y_tree().expect("semantics enabled");
    let button = tree
        .find_by_label("Go")
        .unwrap_or_else(|error| panic!("the button: {error}\n{}", tree.describe()));
    assert!(
        button.supports_action(Action::Focus),
        "the button offers assistive focus:\n{}",
        tree.describe()
    );
    app.invoke_semantics_action(ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: button.id(),
        data: None,
    })
    .expect("the focus action resolves");
    app.pump();
    app.pump();
    assert!(
        app.focus_manager().primary_focus().is_some(),
        "the button took focus"
    );
    assert!(
        ring_painted(&app),
        "focus assistive technology moved is shown"
    );
}

/// A pointer press hides the ring of a button that keeps the focus.
fn pointer_input_hides_the_ring() {
    let mut app = app();
    tab_in(&mut app);
    let manager = app.focus_manager();
    let focused = manager.primary_focus();
    // What the runtime does on every pointer-down.
    manager.note_pointer_input();
    app.pump();
    assert!(
        !ring_painted(&app),
        "focus is not shown once the pointer is in use"
    );
    assert_eq!(
        manager.primary_focus().map(|node| node.id()),
        focused.map(|node| node.id()),
        "the button keeps the focus"
    );
}

/// After a pointer press, the Alt release a window switch leaves behind and a
/// lone modifier press do not bring the ring back; the next Tab does.
fn a_stray_modifier_does_not_show_the_ring() {
    let mut app = app();
    tab_in(&mut app);
    let manager = app.focus_manager();
    manager.note_pointer_input();
    app.pump();
    let alt = Key::Named(NamedKey::Alt);
    app.enter_owner_scope(|| {
        let _ = manager.dispatch_key_event(&key(KeyState::Up, alt.clone(), KeyRepeat::First));
        let _ = press(&manager, &alt);
    });
    app.pump();
    assert!(!ring_painted(&app), "a modifier alone is not keyboard use");
    let _ = app.enter_owner_scope(|| tab(&manager));
    app.pump();
    assert!(ring_painted(&app), "Tab is");
}

/// The ring shows for focus the keyboard or assistive technology moved, and
/// not once a pointer is in use.
pub(crate) fn focus_indicator_follows_input_modality() {
    tab_shows_the_ring();
    assistive_focus_shows_the_ring();
    pointer_input_hides_the_ring();
    a_stray_modifier_does_not_show_the_ring();
}
