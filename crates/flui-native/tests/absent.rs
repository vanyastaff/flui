//! A position a host's table leaves out (`Absent`) answers "not here" with its
//! reason wherever its vocabulary can say so, and hands a window a bridge
//! that is never active and keeps nothing it is given.

use std::sync::Arc;

use flui_native::host::{Absent, Accessibility, Displays, Window, position};
use flui_native::{OpenWindowError, PlatformError};
use flui_platform::{UnsupportedReason, WindowOptions};
use flui_semantics::platform::AccessibilityHandler;

struct Handler;

impl AccessibilityHandler for Handler {
    fn activation_changed(&self, _: bool) {}

    fn action_requested(&self, _: accesskit::ActionRequest) {}

    fn assistive_focus_changed(&self, _: Option<accesskit::NodeId>) {}
}

/// A host with no window position opens none, and says why.
fn an_absent_window_position_opens_no_window_on_this_platform() {
    let opened = <Absent<position::Window> as Window<()>>::open(
        &(),
        &Arc::new(()),
        WindowOptions::default(),
        flui_testing::window_handler::RecordingWindowHandler::new(),
    );
    match opened {
        Err(OpenWindowError::Unsupported(absent)) => {
            assert_eq!(absent.reason, UnsupportedReason::NotOnThisPlatform);
            assert!(
                !absent.is_retryable(),
                "a host without windows never gains one"
            );
        }
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(_) => panic!("an absent window position opened a window"),
    }
}

/// A host with no display source reads none, and says why.
fn an_absent_display_position_reads_no_displays_on_this_platform() {
    match <Absent<position::Displays> as Displays<()>>::snapshot(&()) {
        Err(PlatformError::Unsupported(absent)) => {
            assert_eq!(absent.reason, UnsupportedReason::NotOnThisPlatform);
        }
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(snapshot) => panic!("an absent display source read {snapshot:?}"),
    }
}

/// A window of a host with no accessibility integration holds a bridge that
/// is never active and keeps no handler the runtime hands it.
fn an_absent_accessibility_position_hands_out_a_bridge_that_is_never_active() {
    let bridge = <Absent<position::Accessibility> as Accessibility<()>>::for_window(&());
    let handler: Arc<dyn AccessibilityHandler> = Arc::new(Handler);
    let attachment = bridge
        .attach(Arc::clone(&handler))
        .expect("an absent bridge accepts an attachment without retaining it");
    assert!(!attachment.is_active());
    assert_eq!(
        Arc::strong_count(&handler),
        1,
        "the bridge keeps no handler"
    );
    bridge.focus_changed(true);
    bridge.detach();
    let attachment = bridge
        .attach(handler)
        .expect("an absent bridge remains attachable after teardown");
    assert!(!attachment.is_active(), "nothing it is told activates it");
}

#[test]
fn absent_positions_say_not_here() {
    crate::run_table(
        "absent_positions_say_not_here",
        &[
            (
                "an_absent_window_position_opens_no_window_on_this_platform",
                an_absent_window_position_opens_no_window_on_this_platform as fn(),
            ),
            (
                "an_absent_display_position_reads_no_displays_on_this_platform",
                an_absent_display_position_reads_no_displays_on_this_platform as fn(),
            ),
            (
                "an_absent_accessibility_position_hands_out_a_bridge_that_is_never_active",
                an_absent_accessibility_position_hands_out_a_bridge_that_is_never_active as fn(),
            ),
        ],
    );
}
