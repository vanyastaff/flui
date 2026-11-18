//! Grants on Linux: a pick's lasting access is its path. Outside a sandbox
//! the process opens what the user can; inside a Flatpak the file chooser
//! answers a path in the document portal's folder (or the real path, where
//! the manifest exposes it), and the portal keeps that document for the
//! application across its runs, under the same path.
//!
//! The sandbox is read on a blocking pool thread (`/.flatpak-info`, with the
//! user's folders an `xdg-*` entry names), and so is every resolve, which
//! asks the file system whether the path is still there (ADR-0360 §2). In
//! the document portal's folder a document the portal no longer shows was
//! forgotten by it (the user withdrew the permission, or the file it stood
//! for is gone): `Revoked`. Anywhere else a file that is not there was moved
//! or deleted: `Missing`. A relative path is refused before any call.

use std::path::Path;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::sandbox::{Confinement, read_flatpak_info};
use flui_platform::{Unsupported, UnsupportedReason};

use super::Portal;
use super::dialogs::in_document_portal;
use super::user_dirs::named_folder;
use crate::grant::path::{Granter, file_of, reacquired};
use crate::grant::{
    Grant, GrantBackend, GrantCapability, GrantError, GrantKind, GrantResult, ResolveHold,
};
use crate::sandbox::{Sandbox, of_confinement};

/// [`GrantBackend`] over the file system, on the portal's runtime.
pub(crate) struct PortalGrants {
    portal: Rc<Portal>,
}

impl PortalGrants {
    pub(crate) fn new(portal: Rc<Portal>) -> Self {
        Self { portal }
    }
}

impl GrantBackend for PortalGrants {
    fn kind(&self) -> GrantKind {
        GrantKind::UnixPath
    }

    /// The sandbox `/.flatpak-info` describes, read off the owner thread; a
    /// portal that cannot start answers none, as its resolves would.
    fn sandbox(&self, answer: Resolver<Sandbox>) {
        if self.portal.start().is_err() {
            return;
        }
        self.portal.spawn(move |_| async move {
            let sandbox = zruntime::unblock(|| {
                of_confinement(&Confinement::of_process(
                    &read_flatpak_info(),
                    &named_folder,
                ))
            })
            .await;
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(sandbox);
        });
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
        if self.portal.start().is_err() {
            let _ = answer.resolve(Err(GrantError::Unsupported(Unsupported::of::<
                GrantCapability,
            >(
                UnsupportedReason::Unavailable,
            ))));
            return ResolveHold::new(());
        }
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        self.portal.spawn(move |_| async move {
            let in_portal = file.path().is_some_and(|path| {
                in_document_portal(path, runtime_dir.as_deref().map(Path::new))
            });
            let granter = if in_portal {
                Granter::DocumentPortal
            } else {
                Granter::User
            };
            let resolved = zruntime::unblock(move || {
                let looked = file
                    .path()
                    .map_or(Ok(()), |path| std::fs::metadata(path).map(drop));
                reacquired(file, looked, granter)
            })
            .await;
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(resolved);
        });
        // A metadata call cannot be called back; its answer is dropped
        // when nobody waits for it.
        ResolveHold::new(())
    }
}
