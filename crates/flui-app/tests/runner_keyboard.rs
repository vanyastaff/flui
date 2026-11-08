//! Keyboard policy installed by the application runner's root composition.

use std::rc::Rc;

use flui_app::Application;
use flui_app::testing::HeadlessRun;
use flui_interaction::{events::Code, focus::FocusNode, testing::input::KeyEventBuilder};
use flui_platform::{
    PlatformInput,
    keyboard::{Key, KeyState, NamedKey},
};
use flui_view::ViewExt;
use flui_view::prelude::*;
use flui_widgets::{Positioned, SizedBox, Stack, interaction::Focus};

fn tab(state: KeyState) -> PlatformInput {
    PlatformInput::Keyboard(
        KeyEventBuilder::new(Code::Tab)
            .with_key(Key::Named(NamedKey::Tab))
            .with_state(state)
            .build(),
    )
}

/// The runner used by `run_app` installs `app_root`, so an unmodified
/// application receives standard Tab traversal while the runtime owns only the
/// focus-tree environment.
///
/// Red-check: remove `app_root` from `install_desktop_window`; neither key press
/// is consumed by a traversal action and neither node gains primary focus.
#[test]
fn tab_moves_focus_through_the_application_runner() {
    let first = FocusNode::with_debug_label("runner-first");
    let second = FocusNode::with_debug_label("runner-second");
    let root = Stack::new(vec![
        Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&first)))
            .left(0.0)
            .top(0.0)
            .width(10.0)
            .height(10.0)
            .into_view()
            .boxed(),
        Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&second)))
            .left(20.0)
            .top(0.0)
            .width(10.0)
            .height(10.0)
            .into_view()
            .boxed(),
    ]);
    let run = HeadlessRun::start(Application::new(move |_| root.clone()));
    run.run_owner_turn();
    let window = run
        .open_windows()
        .into_iter()
        .next()
        .expect("the application opened its main window");

    window.inject_event(tab(KeyState::Down));
    window.inject_event(tab(KeyState::Up));
    run.run_owner_turn();
    assert!(
        first.has_primary_focus(),
        "the first Tab enters the application's focus tree"
    );

    window.inject_event(tab(KeyState::Down));
    window.inject_event(tab(KeyState::Up));
    run.run_owner_turn();
    assert!(
        second.has_primary_focus(),
        "the next Tab advances through the application's focus tree"
    );

    run.join()
        .expect("the running application tears down ordinarily");
}
