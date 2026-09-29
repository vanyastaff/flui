//! Owner-thread affinity — the runtime floor under ADR-0039's capability model.
//!
//! Native platform backends (`AppKit`, Win32) own an event loop whose OS
//! objects are thread-affine, yet the `Platform` trait is `Send + Sync` with
//! `&self` methods — nothing stops a worker thread from calling `open_window`
//! on macOS, which is undefined-behavior-class misuse of `AppKit`. The full
//! fix is the `!Send` `OwnerPlatform` capability (ADR-0039); this module is
//! the runtime backstop beneath it: a recordable owner identity plus a debug
//! assertion backends place at the entry of their affine operations.
//!
//! It lives in `flui-foundation` — not `flui-platform` — so its behavior is
//! exercised by a test suite CI actually runs (`flui-platform`'s suite is
//! excluded from the CI gate; see AGENTS.md). The backend wiring is
//! compile-checked by cross-typecheck and verified locally on the native OS.

use std::sync::OnceLock;
use std::thread::{self, ThreadId};

/// Records which thread owns a platform event loop and lets affine
/// operations assert they run there.
///
/// Lifecycle: constructed unbound (`const`-initializable, so it can sit in a
/// backend struct or a `static`); bound once via [`bind_current`] at the
/// backend's loop-entry point (`Platform::run`, or the constructor where the
/// backend documents a main-thread requirement); consulted via
/// [`debug_assert_owner`] at each affine operation's entry.
///
/// Before binding, [`debug_assert_owner`] is deliberately a no-op: pre-run
/// flows (e.g. Win32 examples opening a window before `run()`) stay legal
/// until the `OwnerPlatform` capability moves them into `on_ready`
/// (ADR-0039), and an unbound affinity has no owner to check against.
///
/// [`bind_current`]: Self::bind_current
/// [`debug_assert_owner`]: Self::debug_assert_owner
#[derive(Debug)]
pub struct OwnerAffinity {
    owner: OnceLock<ThreadId>,
}

impl OwnerAffinity {
    /// An unbound affinity. `const`, so backends can hold one without an
    /// `Option` dance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            owner: OnceLock::new(),
        }
    }

    /// Binds the calling thread as the owner.
    ///
    /// Idempotent from the owner thread (re-entering `run`-adjacent setup is
    /// harmless). Binding from a *different* thread after an owner exists is
    /// a bug — the loop cannot move threads — and trips a `debug_assert!`;
    /// in release the original owner is kept and the attempt is traced.
    pub fn bind_current(&self) {
        let current = thread::current().id();
        let bound = *self.owner.get_or_init(|| current);
        if bound != current {
            debug_assert!(
                false,
                "BUG: OwnerAffinity re-bind from {current:?}, but the owner \
                 is {bound:?} — an event loop cannot change threads"
            );
            tracing::error!(
                ?bound,
                ?current,
                "OwnerAffinity re-bind from a foreign thread ignored"
            );
        }
    }

    /// The bound owner thread, if any.
    #[must_use]
    pub fn owner(&self) -> Option<ThreadId> {
        self.owner.get().copied()
    }

    /// Whether the calling thread is the bound owner. `false` when unbound.
    #[must_use]
    pub fn is_owner(&self) -> bool {
        self.owner() == Some(thread::current().id())
    }

    /// `debug_assert!` that the caller is the bound owner; a no-op when no
    /// owner is bound yet (see the type docs for why).
    ///
    /// `op` names the violated operation in the panic message and trace.
    pub fn debug_assert_owner(&self, op: &'static str) {
        let Some(bound) = self.owner() else {
            return;
        };
        let current = thread::current().id();
        if bound != current {
            tracing::error!(
                op,
                ?bound,
                ?current,
                "thread-affine platform operation called off the owner thread"
            );
            debug_assert!(
                false,
                "BUG: `{op}` called from {current:?}, but the platform event \
                 loop is owned by {bound:?} — thread-affine OS APIs must be \
                 reached through the owner thread (ADR-0039)"
            );
        }
    }
}

impl Default for OwnerAffinity {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the violation is a debug_assert; release keeps the owner and only traces"
    )]
    fn foreign_rebind_is_rejected_and_original_owner_kept() {
        let affinity = std::sync::Arc::new(OwnerAffinity::new());
        affinity.bind_current();
        let original = affinity.owner();
        let foreign_view = std::sync::Arc::clone(&affinity);
        let panicked = std::thread::spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                foreign_view.bind_current();
            }))
            .is_err()
        })
        .join()
        .expect("worker thread must complete");
        assert!(panicked, "foreign re-bind must trip the debug assertion");
        assert_eq!(
            affinity.owner(),
            original,
            "the original owner must survive a rejected re-bind",
        );
    }
}
