//! Grants on Windows: a desktop process, packaged in MSIX with full trust
//! or not packaged at all, opens what the user can, so a pick's lasting
//! access is its path, and resolving it asks the file system whether the
//! path is still there, on a shell thread (ADR-0360 §2). A path that is not
//! absolute, or that names a share, a pipe or a device, is refused before
//! any file system call.
//!
//! FLUI's window and dialogs are Win32's, which run as a desktop process; a
//! process in an AppContainer would need the future-access list
//! (`StorageApplicationPermissions.FutureAccessList`) to reach a pick after
//! a restart, which is not built: there the file system refuses the path
//! and the grant answers `Revoked`.

use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::{Unsupported, UnsupportedReason};

use super::ShellThreads;
use crate::grant::path::{Granter, file_of, reacquired};
use crate::grant::{
    Grant, GrantBackend, GrantCapability, GrantError, GrantKind, GrantResult, ResolveHold,
};
use crate::sandbox::Sandbox;

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
    fn takes(&self, grant: &Grant) -> bool {
        grant.kind() == Some(GrantKind::WindowsPath)
    }

    /// A desktop process has no sandbox.
    fn sandbox(&self, answer: Resolver<Sandbox>) {
        // The owner retiring first leaves nothing waiting.
        let _ = answer.resolve(Sandbox::unconfined());
    }

    fn resolve(&self, grant: Grant, _max_bytes: u64, answer: Resolver<GrantResult>) -> ResolveHold {
        let file = match file_of(&grant) {
            Ok(file) => file,
            Err(refusal) => {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(Err(refusal));
                return ResolveHold::new(());
            }
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
                let looked = file
                    .path()
                    .map_or(Ok(()), |path| std::fs::metadata(path).map(drop));
                reacquired(file, looked, Granter::User)
            },
        );
        // A metadata call cannot be called back; its answer is dropped
        // when nobody waits for it.
        ResolveHold::new(())
    }
}
