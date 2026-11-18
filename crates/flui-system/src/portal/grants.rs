//! Grants on Linux: a pick's lasting access is its path. Outside a sandbox
//! the process opens what the user can; inside a Flatpak the file chooser
//! answers a path in the document portal's folder, and the portal keeps that
//! document for the application across its runs, under the same path.
//!
//! Resolving a grant asks the file system whether the path is still there,
//! on a blocking pool thread (ADR-0360 §2). In the document portal's folder
//! a document the portal no longer shows was forgotten by it (the user
//! withdrew the permission, or the file it stood for is gone): `Revoked`.
//! Anywhere else a file that is not there was moved or deleted: `Missing`.

use std::path::Path;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::Portal;
use super::dialogs::in_document_portal;
use crate::grant::path::{Granter, reacquired};
use crate::grant::{Grant, GrantBackend, GrantCapability, GrantError, GrantKind, GrantResult};

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

    fn resolve(&self, grant: Grant, _max_bytes: u64, answer: Resolver<GrantResult>) {
        let Some(path) = grant.path() else {
            let _ = answer.resolve(Err(GrantError::Failed(SystemFailure::MalformedAnswer)));
            return;
        };
        if self.portal.start().is_err() {
            let _ = answer.resolve(Err(GrantError::Unsupported(Unsupported::of::<
                GrantCapability,
            >(
                UnsupportedReason::Unavailable,
            ))));
            return;
        }
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        self.portal.spawn(move |_| async move {
            let granter = if in_document_portal(&path, runtime_dir.as_deref().map(Path::new)) {
                Granter::DocumentPortal
            } else {
                Granter::User
            };
            let resolved = zruntime::unblock(move || {
                let looked = std::fs::metadata(&path).map(drop);
                reacquired(path, looked, granter)
            })
            .await;
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(resolved);
        });
    }
}
