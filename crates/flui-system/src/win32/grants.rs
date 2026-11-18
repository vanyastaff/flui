//! Grants on Windows: a desktop process, packaged in MSIX with full trust
//! or not packaged at all, opens what the user can, so a pick's lasting
//! access is its path, and resolving it asks the file system whether the
//! path is still there, on a shell thread (ADR-0360 §2).
//!
//! FLUI's window and dialogs are Win32's, which run as a desktop process; a
//! process in an AppContainer would need the future-access list
//! (`StorageApplicationPermissions.FutureAccessList`) to reach a pick after
//! a restart, which is not built: there the file system refuses the path
//! and the grant answers `Revoked`.

use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::ShellThreads;
use crate::grant::path::{Granter, reacquired};
use crate::grant::{Grant, GrantBackend, GrantCapability, GrantError, GrantKind, GrantResult};

/// [`GrantBackend`] over the file system, on the runtime's shell threads.
pub(crate) struct WindowsGrants {
    threads: Rc<ShellThreads>,
}

impl WindowsGrants {
    pub(crate) fn new(threads: Rc<ShellThreads>) -> Self {
        Self { threads }
    }
}

impl GrantBackend for WindowsGrants {
    fn kind(&self) -> GrantKind {
        GrantKind::WindowsPath
    }

    fn resolve(&self, grant: Grant, _max_bytes: u64, answer: Resolver<GrantResult>) {
        let Some(path) = grant.path() else {
            // A Windows grant of an odd length was not made here.
            let _ = answer.resolve(Err(GrantError::Failed(SystemFailure::MalformedAnswer)));
            return;
        };
        self.threads.answer(
            "flui-system grant",
            answer,
            || {
                Err(GrantError::Unsupported(Unsupported::of::<GrantCapability>(
                    UnsupportedReason::Unavailable,
                )))
            },
            move || {
                let looked = std::fs::metadata(&path).map(drop);
                reacquired(path, looked, Granter::User)
            },
        );
    }
}
