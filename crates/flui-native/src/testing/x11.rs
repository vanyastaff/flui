//! The X11 host under test: its loop, turned by hand on the thread that built
//! the platform, where a test stands in for `X11Platform::run`.

use std::time::Duration;

use crate::PlatformError;
use crate::host::Host;
use crate::owner::linux::x11::X11Loop;
use crate::platform::HostPlatform;

/// Turn the X11 loop of `platform` for `span`, each turn the one `run` takes:
/// the server's events reach the windows, the owner's turns run and the
/// frames the windows asked for are delivered. No application handler is
/// bound, so its questions go unanswered: no wake deadline arms, and a quit
/// or an exit the loop would end on ends nothing here.
///
/// # Errors
/// [`PlatformError::EventLoop`] when the X server connection fails.
pub fn run_for<H: Host<Loop = X11Loop>>(
    platform: &HostPlatform<H>,
    span: Duration,
) -> Result<(), PlatformError> {
    platform.owner().pump_for(span)
}

/// The X11 platform as [`CurrentHost::start_with`](crate::CurrentHost::start_with)
/// starts it for an application identified by `app_id`, on a runtime of its
/// own, kept as the concrete platform so [`run_for`] can turn its loop.
///
/// # Errors
/// What [`X11Platform::try_new`](crate::X11Platform::try_new) reports.
pub fn identified(app_id: &flui_foundation::AppId) -> Result<crate::X11Platform, PlatformError> {
    crate::X11Platform::try_new_with(crate::NativeExecutor::own(), None, Some(app_id))
}
