//! Which closes a host's windows let the application refuse depends on the
//! window system and the session-end support the host names, never on the
//! target the code was built for.

use flui_platform::{SessionEndSupport, TargetPlatform};
use flui_runtime::owner_turn::close_vetoes;
use flui_runtime::window_system::WindowSystem;
use flui_view::CloseReason::{Program, SessionEnd, User};

/// The same answers on every build target: a desktop's window system and
/// the platform-neutral one a headless window plays by default ask before a
/// window closes, a phone's and the browser's never do, and only a platform
/// that holds the session makes its end refusable.
///
/// Fails against a `close_vetoes` that reads the build target (on `wasm32`,
/// the platform-neutral window would ask nothing) and against one that takes
/// a page's reported support at its word (a page's end would be refusable).
#[test]
fn close_vetoes_follow_the_window_system_named() {
    let hold = Some(SessionEndSupport::Hold);
    let delay = Some(SessionEndSupport::Delay {
        max: std::time::Duration::from_secs(5),
    });
    let background = Some(SessionEndSupport::Background {
        window: std::time::Duration::from_secs(5),
    });
    let cases = [
        (
            WindowSystem::Platform(TargetPlatform::Unknown),
            hold,
            vec![User, Program, SessionEnd],
        ),
        (
            WindowSystem::Platform(TargetPlatform::Windows),
            hold,
            vec![User, Program, SessionEnd],
        ),
        (
            WindowSystem::Platform(TargetPlatform::Linux),
            delay,
            vec![User, Program],
        ),
        (
            WindowSystem::Platform(TargetPlatform::Android),
            background,
            vec![],
        ),
        (
            WindowSystem::Platform(TargetPlatform::iOS),
            background,
            vec![],
        ),
        (
            WindowSystem::Browser,
            Some(SessionEndSupport::NotifyOnly),
            vec![],
        ),
        // A page only tells, whatever its host reports: `Browser` with
        // `Hold` cannot make its session end refusable.
        (WindowSystem::Browser, hold, vec![]),
        (
            WindowSystem::Platform(TargetPlatform::MacOS),
            None,
            vec![User, Program],
        ),
    ];
    for (windows, support, expected) in cases {
        assert_eq!(
            close_vetoes(windows, support),
            expected,
            "{windows:?} with {support:?}"
        );
    }
}
