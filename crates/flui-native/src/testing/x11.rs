//! The X11 host under test: its loop, turned by hand on the thread that built
//! the platform, where a test stands in for `X11Platform::run`.

use std::time::Duration;

use crate::PlatformError;
use crate::host::Host;
use crate::owner::linux::x11::X11Loop;
use crate::platform::HostPlatform;

/// Turn the X11 loop of `platform` for `span`, as `run` does: the server's
/// events reach the windows, the owner's turns run and the frames the windows
/// asked for are delivered. No application handler is bound, so no wake
/// deadline is asked and no exit policy ends anything.
///
/// # Errors
/// [`PlatformError::EventLoop`] when the X server connection fails.
pub fn run_for<H: Host<Loop = X11Loop>>(
    platform: &HostPlatform<H>,
    span: Duration,
) -> Result<(), PlatformError> {
    platform.owner().pump_for(span)
}
