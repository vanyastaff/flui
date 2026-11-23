//! How a window system lets the session end, and the budget its `Ending` phase has.

#[cfg(doc)]
use super::HeadlessHost;

use flui_runtime::window_system::WindowSystem;

/// How long [`HeadlessHost::end_session`]'s `Ending` phase writes what the
/// documents published (ADR-0260).
pub use flui_view::__runtime::SESSION_END_BUDGET;

/// How `windows` let the session end, as their host reports it: `Background`
/// on Android and iOS, `Delay` on Linux (logind), `Hold` on Windows, macOS
/// and the platform-neutral window, as the native headless platform does, and
/// `NotifyOnly` in a browser. A duration names the session end's budget.
pub(super) fn session_end_support_of(windows: WindowSystem) -> flui_platform::SessionEndSupport {
    use flui_platform::{SessionEndSupport, TargetPlatform};
    match windows {
        WindowSystem::Platform(TargetPlatform::Android | TargetPlatform::iOS) => {
            SessionEndSupport::Background {
                window: SESSION_END_BUDGET,
            }
        }
        WindowSystem::Platform(TargetPlatform::Linux) => SessionEndSupport::Delay {
            max: SESSION_END_BUDGET,
        },
        WindowSystem::Browser => SessionEndSupport::NotifyOnly,
        _ => SessionEndSupport::Hold,
    }
}
