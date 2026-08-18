//! The Win32 HWND thread-affinity contract, in pure functions.
//!
//! An HWND is thread-affine: Win32 dispatches a window's messages — including
//! the `WM_DESTROY` that frees the context allocation stored in its
//! `GWLP_USERDATA` slot — on the thread that created the window, and
//! `DestroyWindow` refuses to destroy a window created by a different thread
//! (both are documented contracts: see MSDN's `DestroyWindow` remarks and the
//! "About Messages and Message Queues" thread-affinity section). The Windows
//! backend shares `Arc<WindowsWindow>` freely across threads, so every
//! dereference of the raw context pointer read back from `GWLP_USERDATA`
//! must first decide whether the calling thread may touch it at all: a
//! foreign thread's read races the owner thread's `WM_DESTROY` clear+free
//! with no synchronization, and no ordering of the read alone can close that
//! window (it is a time-of-check/time-of-use race).
//!
//! The decisions are pure rules over thread identities and slot state. They
//! live here, cfg-free, because the Win32 shells that apply them are
//! lint-only in CI (`cross-typecheck` compiles them and runs nothing) — the
//! Linux-executed suite is the only coverage these rules actually get.
//!
//! Thread identities are Win32 thread ids (`GetCurrentThreadId`,
//! `GetWindowThreadProcessId`): nonzero `u32`s, with `0` meaning "the HWND
//! no longer names a live window" (`GetWindowThreadProcessId`'s documented
//! failure value), which is why [`OWNER_GONE`] is a valid `owner_thread`
//! input and never a valid `current_thread` one.

/// The `owner_thread` value meaning the HWND no longer names a live window
/// (`GetWindowThreadProcessId` returns 0 for an invalid handle).
pub const OWNER_GONE: u32 = 0;

/// Verdict for one attempted dereference of the `GWLP_USERDATA` context
/// pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserDataVerdict {
    /// Dereferencing is sound: the caller is on the window's owning thread,
    /// the window is alive and of this backend's own class, and the slot
    /// holds a pointer. On the owning thread the deref cannot race the free:
    /// `WM_DESTROY` (the only path that frees the context) is dispatched on
    /// that same thread, serialized with the caller.
    Deref,
    /// Dereferencing must be refused; the reason says why. Callers fall back
    /// to a safe default (or report an error) instead of touching the slot.
    Refuse(UserDataRefusal),
}

/// Why a `GWLP_USERDATA` dereference was refused.
///
/// Ordered by precedence: a gone window is reported before a foreign thread,
/// which is reported before a foreign class, which is reported before an
/// empty slot — each later check is only meaningful once the earlier ones
/// pass (class and slot reads against a dead handle return defined but
/// stale-looking values, and a foreign thread must not proceed to *any*
/// use of the slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserDataRefusal {
    /// The HWND no longer names a live window (`owner_thread == 0`). The
    /// handle value may even have been recycled by the OS for an unrelated
    /// window; nothing read through it describes this wrapper's window.
    WindowGone,
    /// The calling thread is not the window's creating thread. A foreign
    /// thread's dereference races the owner thread's `WM_DESTROY` clear+free
    /// of the same allocation, so it is refused outright — this is the
    /// refusal that discharges the `Send`/`Sync` affinity obligation.
    ForeignThread,
    /// The live window behind the handle is not of this backend's window
    /// class, so its `GWLP_USERDATA` slot belongs to someone else's protocol
    /// entirely. Reachable only through OS handle recycling: this wrapper's
    /// window died and its handle value now names a foreign window.
    ForeignClass,
    /// The slot is null: the context was never installed, or `WM_DESTROY`
    /// already cleared it (it zeroes the slot before freeing the box).
    EmptySlot,
}

/// Classifies one attempted dereference of the context pointer stored in a
/// window's `GWLP_USERDATA` slot.
///
/// Inputs are the caller's OS queries, made in any order before calling:
///
/// - `owner_thread` — `GetWindowThreadProcessId(hwnd)`; `0` ([`OWNER_GONE`])
///   when the handle is dead.
/// - `current_thread` — `GetCurrentThreadId()`; never `0` (Win32 thread ids
///   are nonzero).
/// - `is_own_class` — whether the window's class name equals this backend's
///   registered class (a dead handle reports `false`).
/// - `slot` — the raw `GWLP_USERDATA` value (a dead handle reports `0`).
#[must_use]
pub fn classify_user_data_access(
    owner_thread: u32,
    current_thread: u32,
    is_own_class: bool,
    slot: isize,
) -> UserDataVerdict {
    if owner_thread == OWNER_GONE {
        UserDataVerdict::Refuse(UserDataRefusal::WindowGone)
    } else if owner_thread != current_thread {
        UserDataVerdict::Refuse(UserDataRefusal::ForeignThread)
    } else if !is_own_class {
        UserDataVerdict::Refuse(UserDataRefusal::ForeignClass)
    } else if slot == 0 {
        UserDataVerdict::Refuse(UserDataRefusal::EmptySlot)
    } else {
        UserDataVerdict::Deref
    }
}

/// How a teardown request (`close()`, or the wrapper's last `Drop`) must
/// reach the native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeardownRoute {
    /// The HWND no longer names a live window; there is nothing to destroy
    /// and nothing to post to.
    AlreadyGone,
    /// The caller is on the owning thread: `DestroyWindow` may run directly
    /// (Win32 dispatches the resulting `WM_DESTROY` synchronously on this
    /// same thread before it returns).
    DestroyDirect,
    /// The caller is on a foreign thread: `DestroyWindow` would fail there
    /// ("a thread cannot use `DestroyWindow` to destroy a window created by
    /// a different thread" — MSDN), so the close must be posted to the owner
    /// thread's message queue with `PostMessageW(WM_CLOSE)`, the documented
    /// cross-thread mechanism.
    PostClose,
}

/// Routes a teardown request by thread identity. Same input convention as
/// [`classify_user_data_access`].
#[must_use]
pub fn route_teardown(owner_thread: u32, current_thread: u32) -> TeardownRoute {
    if owner_thread == OWNER_GONE {
        TeardownRoute::AlreadyGone
    } else if owner_thread == current_thread {
        TeardownRoute::DestroyDirect
    } else {
        TeardownRoute::PostClose
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: u32 = 7;
    const FOREIGN: u32 = 8;

    #[test]
    fn owner_thread_with_live_own_class_window_and_filled_slot_may_deref() {
        assert_eq!(
            classify_user_data_access(OWNER, OWNER, true, 0x1000),
            UserDataVerdict::Deref
        );
    }

    #[test]
    fn a_dead_handle_is_refused_before_any_other_check_can_mislead() {
        // Everything else looking plausible must not matter: a dead handle's
        // class/slot reads are stale or foreign by definition.
        assert_eq!(
            classify_user_data_access(OWNER_GONE, OWNER, true, 0x1000),
            UserDataVerdict::Refuse(UserDataRefusal::WindowGone)
        );
    }

    #[test]
    fn a_foreign_thread_is_refused_even_with_a_live_window_and_filled_slot() {
        assert_eq!(
            classify_user_data_access(OWNER, FOREIGN, true, 0x1000),
            UserDataVerdict::Refuse(UserDataRefusal::ForeignThread)
        );
    }

    #[test]
    fn a_recycled_handle_naming_a_foreign_class_window_is_refused() {
        assert_eq!(
            classify_user_data_access(OWNER, OWNER, false, 0x1000),
            UserDataVerdict::Refuse(UserDataRefusal::ForeignClass)
        );
    }

    #[test]
    fn an_empty_slot_is_refused_on_the_owner_thread_too() {
        assert_eq!(
            classify_user_data_access(OWNER, OWNER, true, 0),
            UserDataVerdict::Refuse(UserDataRefusal::EmptySlot)
        );
    }

    #[test]
    fn refusal_precedence_is_gone_then_foreign_thread_then_class_then_slot() {
        // All four conditions failing at once report the highest-precedence
        // refusal, walking down as each earlier condition is repaired.
        assert_eq!(
            classify_user_data_access(OWNER_GONE, FOREIGN, false, 0),
            UserDataVerdict::Refuse(UserDataRefusal::WindowGone)
        );
        assert_eq!(
            classify_user_data_access(OWNER, FOREIGN, false, 0),
            UserDataVerdict::Refuse(UserDataRefusal::ForeignThread)
        );
        assert_eq!(
            classify_user_data_access(OWNER, OWNER, false, 0),
            UserDataVerdict::Refuse(UserDataRefusal::ForeignClass)
        );
        assert_eq!(
            classify_user_data_access(OWNER, OWNER, true, 0),
            UserDataVerdict::Refuse(UserDataRefusal::EmptySlot)
        );
    }

    #[test]
    fn teardown_routes_direct_on_owner_posted_on_foreign_skipped_when_gone() {
        assert_eq!(route_teardown(OWNER, OWNER), TeardownRoute::DestroyDirect);
        assert_eq!(route_teardown(OWNER, FOREIGN), TeardownRoute::PostClose);
        assert_eq!(
            route_teardown(OWNER_GONE, FOREIGN),
            TeardownRoute::AlreadyGone
        );
    }
}
