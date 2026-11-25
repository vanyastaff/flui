//! What the lane's `AXPress` answers ([`pressed`]): the answer every `AXPress` gives
//! (`lane::ax::pressed`, the iOS Simulator lane's too), with the lane's own refusal for a process
//! no longer trusted. It builds on every host, so its answers are pinned on Linux CI; the press
//! itself is `cfg(target_os = "macos")`.

use std::time::Duration;

use crate::error::Result;
use crate::lane::ax;
use crate::lane::macos::safety::{Permission, Refusal};
use crate::lane::walk::ProviderError;

/// What the lane's `AXPress` of the element `identifier` answered: `offered`, whether it offers
/// the action, then `perform`, the press, made only when it does, under `deadline`, with
/// `trusted` asked after a failure. A process no longer trusted is refused as the Accessibility
/// permission missing ([`Refusal::MissingPermission`]), but a press already sent and not
/// answered, or whose element went away, keeps `may_have_run` whatever the trust.
///
/// # Errors
///
/// As `lane::ax::pressed`, the untrusted refusal naming `macos_permission_accessibility`.
pub fn pressed(
    offered: std::result::Result<bool, ProviderError>,
    perform: impl FnOnce() -> std::result::Result<(), ProviderError>,
    identifier: &str,
    deadline: Duration,
    trusted: impl Fn() -> bool,
) -> Result<()> {
    ax::pressed(
        offered,
        perform,
        identifier,
        Some(deadline),
        trusted,
        || {
            Refusal::MissingPermission {
                permission: Permission::Accessibility,
            }
            .into_error()
        },
    )
}
