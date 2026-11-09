//! The runner installs the catalog's keyboard bindings above every root it
//! attaches: an application run through `flui_app::testing`, its main window
//! and a secondary one, moves focus with Tab and Shift+Tab. The UI runtime
//! installs no key bindings, so a runner that attached its root without
//! `flui_widgets::app_root` leaves Tab unhandled.

use std::rc::Rc;

use flui_app::app::{AppConfig, Application, ExitPolicy, WindowPolicy};
use flui_app::testing::HeadlessRun;
use flui_interaction::focus::FocusNode;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_platform::{EventTime, PlatformInput};
use flui_view::ViewExt as _;
use flui_widgets::{Column, Focus, SizedBox};

/// One window's two fields, the first focused when it mounts.
#[derive(Clone)]
struct Fields {
    first: Rc<FocusNode>,
    second: Rc<FocusNode>,
}

impl Fields {
    fn new(window: &str) -> Self {
        Self {
            first: FocusNode::with_debug_label(format!("{window} first")),
            second: FocusNode::with_debug_label(format!("{window} second")),
        }
    }

    fn root(&self) -> Column {
        let field = |node: &Rc<FocusNode>, autofocus: bool| {
            Focus::new(SizedBox::new(100.0, 20.0))
                .focus_node(Rc::clone(node))
                .autofocus(autofocus)
                .boxed()
        };
        Column::new(vec![field(&self.first, true), field(&self.second, false)])
    }

    /// Whether the first field (`false`) or the second (`true`) has the
    /// primary focus; `None` when neither does.
    fn focused(&self) -> Option<bool> {
        match (
            self.first.has_primary_focus(),
            self.second.has_primary_focus(),
        ) {
            (true, false) => Some(false),
            (false, true) => Some(true),
            _ => None,
        }
    }
}

/// Press and release Tab in `window`, with `modifiers` held.
fn tab(window: &flui_native::MockWindow, modifiers: Modifiers) {
    for state in [KeyState::Down, KeyState::Up] {
        let _ = window.inject_event(PlatformInput::Keyboard(
            KeyEvent::new(
                state,
                Key::Named(NamedKey::Tab),
                Code::Tab,
                EventTime::from_nanos(0),
            )
            .with_modifiers(modifiers),
        ));
    }
}

/// Red-check: attach a runner's root without `app_root` — Tab leaves the
/// first field focused.
pub(crate) fn tab_moves_focus_in_every_window_a_runner_installs() {
    let main = Fields::new("main");
    let secondary = Fields::new("secondary");
    let main_root = main.root();
    let secondary_root = secondary.root();
    let application = Application::new(move |_| main_root.clone())
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed))
        .on_ready(move |_| {
            drop(
                flui_app::open_window(AppConfig::new(), WindowPolicy::Isolated, secondary_root)
                    .expect("an isolated window opens on the headless platform"),
            );
        });
    let run = HeadlessRun::start(application);
    run.run_owner_turn();
    let windows = run.open_windows();
    assert_eq!(windows.len(), 2, "both windows are open");
    run.run_owner_turn();
    assert_eq!(
        main.focused(),
        Some(false),
        "the main window's first field autofocuses"
    );
    assert_eq!(
        secondary.focused(),
        Some(false),
        "the secondary window's first field autofocuses"
    );

    // Each window's Tab moves its own fields; which mock window is which is
    // the platform's order, so the pair that moved names it.
    for window in &windows {
        let before = (main.focused(), secondary.focused());
        window.simulate_focus(true);
        tab(window, Modifiers::NONE);
        run.run_owner_turn();
        let after = (main.focused(), secondary.focused());
        assert_ne!(before, after, "Tab moved focus in its window");
    }
    assert_eq!(
        main.focused(),
        Some(true),
        "Tab reached the main window's second field"
    );
    assert_eq!(
        secondary.focused(),
        Some(true),
        "Tab reached the secondary window's second field"
    );

    for window in &windows {
        window.simulate_focus(true);
        tab(window, Modifiers::SHIFT);
        run.run_owner_turn();
    }
    assert_eq!(
        main.focused(),
        Some(false),
        "Shift+Tab moves back in the main window"
    );
    assert_eq!(
        secondary.focused(),
        Some(false),
        "Shift+Tab moves back in the secondary window"
    );
    drop(run);
}
