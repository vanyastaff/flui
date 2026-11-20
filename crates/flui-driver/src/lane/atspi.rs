//! What AT-SPI, the protocol Orca and every Linux assistive technology speak, shows of the app:
//! the accessibility tree both Linux lanes read and act through.
//!
//! - [`roles`] is the role table AT-SPI is read against, pure, so its rows run on every host.
//! - On Linux, the client (`client`) reaches the accessibility bus the session bus names
//!   (`org.a11y.Bus`), finds the app among the bus's applications by the process that owns its
//!   connection, and its window among the app's, and projects the window's tree onto the wire's
//!   nodes through the bounded walk the Windows lane's UI Automation read uses
//!   ([`walk`](crate::lane::walk)): each node's role folded by [`roles::wire_role`], its
//!   name, description, `AccessibleId` (the ADR-0240 identifier the adapter publishes), text,
//!   checked, expanded and selected states, and its extents on the screen.
//!
//! Every call waits at most [`CALL_TIMEOUT`]: a hung app fails the read with a typed timeout,
//! never a tree of defaults. AT-SPI that is off in the session (`org.a11y.Status.IsEnabled`,
//! which a screen reader turns on) is a lane the host cannot run, never an empty tree: the lane
//! does not turn it on, since that is the person's setting.

use std::time::Duration;

pub mod roles;

#[cfg(target_os = "linux")]
pub(crate) mod client;

/// How long the client waits for the bus or the app to answer one call.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);
