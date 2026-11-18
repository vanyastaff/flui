//! A simulated system's grants: [`ScriptedGrants`] over a [`GrantStore`]
//! that outlives the providers, as an operating system's permission store
//! outlives the process.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use rustc_hash::FxHashMap;

use crate::dialog::PickedFile;
use crate::grant::{
    Access, AccessScope, Grant, GrantBackend, GrantError, GrantKind, GrantResult, ResolveHold,
};
use crate::sandbox::Sandbox;

/// What a simulated system keeps of the grants it made: the file each
/// reaches, how, and whether it still grants it.
///
/// Cheap to clone; clones share the store. A test that drops its host and
/// registers a new [`ScriptedGrants`] over the same store is a restart: the
/// grants the first run made resolve in the second, as the system's own
/// store keeps them across the process's runs.
#[derive(Clone, Debug, Default)]
pub struct GrantStore(Rc<RefCell<Entries>>);

#[derive(Debug, Default)]
struct Entries {
    /// The id the next grant gets; ids are never handed out twice.
    next: u64,
    granted: FxHashMap<u64, Entry>,
}

#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    state: EntryState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryState {
    /// The system grants it: through a scope it starts on each resolve (a
    /// bookmark), or as the path alone.
    Granted { scoped: bool, stale: bool },
    /// The system withdrew it.
    Revoked,
    /// The file is gone.
    Missing,
}

impl GrantStore {
    /// A store that has granted nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant lasting access to `path` through a scope the system starts on
    /// each resolve, as a security-scoped bookmark is: the path is reached
    /// whether or not a sandbox reaches it.
    #[must_use]
    pub fn grant(&self, path: impl Into<PathBuf>) -> Grant {
        self.add(path.into(), true)
    }

    /// Grant lasting access to `path` as the path alone, as a desktop
    /// process keeps one: reached only where the sandbox reaches it.
    #[must_use]
    pub fn grant_path(&self, path: impl Into<PathBuf>) -> Grant {
        self.add(path.into(), false)
    }

    fn add(&self, path: PathBuf, scoped: bool) -> Grant {
        let mut entries = self.0.borrow_mut();
        let id = entries.next;
        entries.next = id
            .checked_add(1)
            .expect("BUG: a test makes fewer than u64::MAX grants");
        entries.granted.insert(
            id,
            Entry {
                path,
                state: EntryState::Granted {
                    scoped,
                    stale: false,
                },
            },
        );
        Grant::try_new(KIND, id.to_le_bytes()).expect("BUG: an id is a payload a grant holds")
    }

    /// Withdraw `grant`, as a user who revokes it or a system that prunes
    /// it does.
    pub fn revoke(&self, grant: &Grant) {
        self.set(grant, |_| EntryState::Revoked);
    }

    /// The file `grant` reaches is deleted.
    pub fn remove(&self, grant: &Grant) {
        self.set(grant, |_| EntryState::Missing);
    }

    /// The system calls `grant` stale: its next resolve hands a fresher one
    /// over, which reaches the same file.
    pub fn make_stale(&self, grant: &Grant) {
        self.set(grant, |state| match state {
            EntryState::Granted { scoped, .. } => EntryState::Granted {
                scoped,
                stale: true,
            },
            other => other,
        });
    }

    fn set(&self, grant: &Grant, change: impl FnOnce(EntryState) -> EntryState) {
        let Some(id) = id_of(grant) else {
            return;
        };
        if let Some(entry) = self.0.borrow_mut().granted.get_mut(&id) {
            entry.state = change(entry.state);
        }
    }

    /// What resolving `grant` answers, as the simulated system decides it.
    fn resolve(&self, grant: &Grant) -> GrantResult {
        // A grant this store never made is one the system does not grant.
        let entry = id_of(grant)
            .and_then(|id| self.0.borrow().granted.get(&id).cloned())
            .ok_or(GrantError::Revoked)?;
        let EntryState::Granted { scoped, stale } = entry.state else {
            return Err(match entry.state {
                EntryState::Missing => GrantError::Missing,
                _ => GrantError::Revoked,
            });
        };
        let file = PickedFile::try_new(&entry.path).expect("BUG: a test grants an absolute path");
        let renewed = stale.then(|| self.add(entry.path.clone(), scoped));
        Ok(if scoped {
            Access::scoped(file, AccessScope::new(()), renewed)
        } else {
            Access::new(file, renewed)
        })
    }
}

/// The kind a store's grants are: opaque bytes only their store reads, as a
/// bookmark is.
const KIND: GrantKind = GrantKind::AppleBookmark;

/// The id a store's grant names.
fn id_of(grant: &Grant) -> Option<u64> {
    let id: [u8; 8] = grant.payload().try_into().ok()?;
    (grant.kind() == KIND).then(|| u64::from_le_bytes(id))
}

/// Grants whose system is simulated: each resolve answers what the
/// [`GrantStore`] says, in the [`Sandbox`] the test set (none until
/// [`set_sandbox`](Self::set_sandbox)).
///
/// Cheap to clone; clones share the store and the sandbox. Set both before
/// the handle is acquired: a handle keeps the sandbox it was made in.
#[derive(Clone, Debug)]
pub struct ScriptedGrants {
    store: Rc<RefCell<GrantStore>>,
    sandbox: Rc<RefCell<Sandbox>>,
}

impl Default for ScriptedGrants {
    fn default() -> Self {
        Self {
            store: Rc::default(),
            sandbox: Rc::new(RefCell::new(Sandbox::unconfined())),
        }
    }
}

impl ScriptedGrants {
    /// The store resolves answer from.
    #[must_use]
    pub fn store(&self) -> GrantStore {
        self.store.borrow().clone()
    }

    /// Resolve from `store` from now on: an earlier run's, for a restart.
    pub fn set_store(&self, store: GrantStore) {
        *self.store.borrow_mut() = store;
    }

    /// The process runs in `sandbox`, for the handles acquired from now on.
    pub fn set_sandbox(&self, sandbox: Sandbox) {
        *self.sandbox.borrow_mut() = sandbox;
    }
}

impl GrantBackend for ScriptedGrants {
    /// A bookmark's kind: the store's grants are opaque bytes.
    fn kind(&self) -> GrantKind {
        KIND
    }

    fn sandbox(&self, answer: Resolver<Sandbox>) {
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(self.sandbox.borrow().clone());
    }

    fn resolve(&self, grant: Grant, _max_bytes: u64, answer: Resolver<GrantResult>) -> ResolveHold {
        let resolved = self.store.borrow().resolve(&grant);
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(resolved);
        ResolveHold::new(())
    }
}
