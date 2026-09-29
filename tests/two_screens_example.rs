//! The `two_screens` example, mounted headless: `#[path]` includes the exact
//! tree `examples/two_screens.rs` runs, so its derived routes, its
//! `WidgetsApp::router` root and its buttons are driven through the real
//! pipeline instead of only in a window.

#[path = "../examples/two_screens.rs"]
#[allow(dead_code, reason = "the example's `main` is not called here")]
mod two_screens;

use std::time::Duration;

use flui::animation::Vsync;
use flui::widgets::VsyncScope;
use flui_testing::widgets::{LaidOut, lay_out_animated, tight};

fn mount() -> LaidOut {
    let vsync = Vsync::new();
    lay_out_animated(
        VsyncScope::new(vsync.clone(), two_screens::TwoScreensApp),
        tight(480.0, 640.0),
        vsync,
    )
}

/// Pump well past the 300 ms default page transition.
fn settle(app: &mut LaidOut) {
    for _ in 0..10 {
        app.pump_for(Duration::from_millis(50));
    }
}

fn laid_out(app: &LaidOut, text: &str) -> bool {
    app.find_text(text)
        .is_some_and(|id| app.try_size(id).is_some_and(|size| size.width > 0.0))
}

/// Tap the middle of the text `label`.
fn tap(app: &mut LaidOut, label: &str) {
    let id = app
        .find_text(label)
        .unwrap_or_else(|| panic!("{label:?} is on screen"));
    let origin = app.absolute_offset(id);
    let size = app.size(id);
    let x = origin.dx + size.width / 2.0;
    let y = origin.dy + size.height / 2.0;
    app.dispatch_pointer_down(x, y);
    app.dispatch_pointer_up(x, y);
    settle(app);
}

#[test]
fn two_screens_opens_a_note_and_comes_back() {
    let mut app = mount();
    settle(&mut app);
    assert!(laid_out(&app, "/"), "Home shows its location");
    assert!(laid_out(&app, "Home"));

    tap(&mut app, "Open");
    assert!(laid_out(&app, "/note/1"), "the Note shows its location");
    assert!(laid_out(&app, "Note 1"));

    tap(&mut app, "Back");
    // Home stays laid out under the Note, so only the popped page's absence
    // tells that the pop happened.
    assert!(
        app.find_text("/note/1").is_none(),
        "the Note's location left"
    );
    assert!(app.find_text("Note 1").is_none(), "the Note page left");
    assert!(laid_out(&app, "Home"), "Home is shown again");
}
