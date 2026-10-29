//! `AdmitAndApply`: a test closes a window where a refusal fails the test
//! (`close_now`), and from cleanup without ever panicking
//! (`close_for_cleanup`), so a test already unwinding keeps its first failure.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_native::{HeadlessPlatform, HostWindow, Platform as _};
use flui_platform::{WindowCapabilities, WindowOptions};
use flui_testing::window_commands::AdmitAndApply as _;

/// A headless window whose window system lets the application close nothing,
/// as Android's one window.
fn a_window_that_cannot_close() -> Rc<dyn HostWindow> {
    HeadlessPlatform::new()
        .with_window_capabilities(WindowCapabilities::NONE)
        .open_window(
            WindowOptions::default(),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("a headless window opens")
}

/// Closes its window when dropped, as a test's guard does.
struct ClosesOnDrop(Rc<dyn HostWindow>);

impl Drop for ClosesOnDrop {
    fn drop(&mut self) {
        self.0.close_for_cleanup();
    }
}

fn a_refused_close_fails_the_test_only_where_asked_to() {
    let window = a_window_that_cannot_close();
    assert!(
        catch_unwind(AssertUnwindSafe(|| window.close_now())).is_err(),
        "close_now fails the test"
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| window.close_for_cleanup())).is_ok(),
        "close_for_cleanup logs instead"
    );
}

/// A guard that cleans up while the test unwinds: its refused close must not
/// panic a second time, which would abort the binary and lose the first
/// failure.
fn a_cleanup_while_unwinding_keeps_the_first_failure() {
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        let _guard = ClosesOnDrop(a_window_that_cannot_close());
        panic!("the test's own failure");
    }));
    let payload = unwound.expect_err("the test failed");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"the test's own failure")
    );
}

fn a_closed_window_is_closed_for_both() {
    let window = HeadlessPlatform::new()
        .open_window(
            WindowOptions::default(),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("a headless window opens");
    window.close_now();
    window.close_now();
    window.close_for_cleanup();
}

#[test]
fn window_commands() {
    crate::run_table(
        "window_commands",
        &[
            (
                "a_refused_close_fails_the_test_only_where_asked_to",
                a_refused_close_fails_the_test_only_where_asked_to as fn(),
            ),
            (
                "a_cleanup_while_unwinding_keeps_the_first_failure",
                a_cleanup_while_unwinding_keeps_the_first_failure as fn(),
            ),
            (
                "a_closed_window_is_closed_for_both",
                a_closed_window_is_closed_for_both as fn(),
            ),
        ],
    );
}
