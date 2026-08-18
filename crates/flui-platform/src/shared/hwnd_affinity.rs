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

/// Whether a class-name buffer as filled by `GetClassNameW` names the same
/// class as `expected`.
///
/// Length conventions, pinned by the tests below because both sides are
/// subtle: `copied` is `GetClassNameW`'s return value — the number of UTF-16
/// units copied **excluding** the terminating NUL (`0` or negative means the
/// call failed); `expected` must likewise carry **no** terminating NUL (a
/// `w!(..)` literal read back through `PCWSTR::as_wide`, which measures with
/// `wcslen`, satisfies this). An `expected` that still carries its NUL can
/// never match anything `GetClassNameW` reports.
#[must_use]
pub fn class_name_matches(buffer: &[u16], copied: i32, expected: &[u16]) -> bool {
    if copied <= 0 {
        return false;
    }
    let copied = copied as usize;
    if copied > buffer.len() {
        return false;
    }
    buffer[..copied] == *expected
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
    /// The HWND names a live window that is not the one this wrapper
    /// created — the OS recycled the handle value (foreign class, or a
    /// user-data identity different from the context this wrapper
    /// installed). Destroying or posting `WM_CLOSE` would target an
    /// innocent window, so teardown must not touch the HWND at all;
    /// wrapper-side cleanup still proceeds.
    StaleHandle,
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

/// Routes a teardown request by thread identity **and window identity**.
///
/// Thread inputs follow [`classify_user_data_access`]'s convention. The
/// identity inputs guard against OS handle recycling — without them a
/// wrapper that outlived its native window would destroy (owner thread) or
/// politely close (foreign thread) whatever unrelated window the recycled
/// handle now names:
///
/// - `is_own_class` — whether the live window's class is this backend's
///   registered class.
/// - `slot` — the raw `GWLP_USERDATA` value currently stored in the window
///   (read without dereferencing, so it is safe to query from any thread).
/// - `expected_slot` — the context pointer this wrapper's constructor
///   installed (never `0`, since it came from `Box::into_raw`). A live
///   window is "ours" only while `slot == expected_slot`; a cleared slot
///   (`0`, mid-`WM_DESTROY`) or a recycled window's foreign value both
///   route [`TeardownRoute::StaleHandle`].
///
/// Residual, stated rather than claimed closed: pointer-value ABA — the
/// freed context allocation's address being reused for a *new* FLUI
/// window's context while the OS also recycles the same numeric HWND —
/// would pass this check. Both reuses must collide at once; the
/// consequence is bounded to closing a window this same process owns.
#[must_use]
pub fn route_teardown(
    owner_thread: u32,
    current_thread: u32,
    is_own_class: bool,
    slot: isize,
    expected_slot: isize,
) -> TeardownRoute {
    if owner_thread == OWNER_GONE {
        TeardownRoute::AlreadyGone
    } else if !is_own_class || slot != expected_slot {
        TeardownRoute::StaleHandle
    } else if owner_thread == current_thread {
        TeardownRoute::DestroyDirect
    } else {
        TeardownRoute::PostClose
    }
}

/// A single-threaded borrow ledger deciding when a retired window context
/// may be freed.
///
/// Win32 message dispatch re-enters: an outbound call made while a context
/// borrow is live (`SetWindowPos` inside the fullscreen toggle, a callback
/// dispatched from a `window_proc` arm) can synchronously run more
/// `window_proc` frames — including `WM_DESTROY`, if a framework callback
/// closes the window from inside the dispatch. Freeing the context in the
/// `WM_DESTROY` arm would therefore free it *under* the borrowing frames
/// further up the same stack. Instead, every borrow holds a guard counted
/// here, `WM_DESTROY` only [`retire`]s the context after clearing the slot,
/// and the free happens when the **outermost** guard releases — the same
/// recursion-deferred user-data reclaim the winit backend's wndproc uses.
///
/// Single-threaded by contract: every acquire/release/retire happens on the
/// window's owning thread (enforced by the affinity gates), which is what
/// makes a plain counter — no atomics — correct.
///
/// [`retire`]: Self::retire
#[derive(Debug)]
pub struct ContextLedger {
    guards: u32,
    retired: bool,
}

impl ContextLedger {
    /// A fresh ledger: no live borrows, not retired.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            guards: 0,
            retired: false,
        }
    }

    /// Records one live borrow of the context.
    pub fn acquire(&mut self) {
        self.guards += 1;
    }

    /// Releases one borrow. Returns `true` when the caller must now free
    /// the context allocation: this was the outermost borrow and the
    /// context has been retired.
    #[must_use]
    pub fn release(&mut self) -> bool {
        self.guards = self
            .guards
            .checked_sub(1)
            .expect("BUG: ContextLedger released more guards than were acquired");
        self.retired && self.guards == 0
    }

    /// Marks the context retired: the `GWLP_USERDATA` slot has been cleared
    /// and no new borrow can be minted. The caller is itself a live
    /// borrower (the `WM_DESTROY` frame), so the free always happens in a
    /// later [`release`](Self::release), never here.
    pub fn retire(&mut self) {
        self.retired = true;
    }
}

impl Default for ContextLedger {
    fn default() -> Self {
        Self::new()
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

    const OUR_SLOT: isize = 0x1000;

    #[test]
    fn teardown_routes_direct_on_owner_posted_on_foreign_skipped_when_gone() {
        assert_eq!(
            route_teardown(OWNER, OWNER, true, OUR_SLOT, OUR_SLOT),
            TeardownRoute::DestroyDirect
        );
        assert_eq!(
            route_teardown(OWNER, FOREIGN, true, OUR_SLOT, OUR_SLOT),
            TeardownRoute::PostClose
        );
        assert_eq!(
            route_teardown(OWNER_GONE, FOREIGN, true, OUR_SLOT, OUR_SLOT),
            TeardownRoute::AlreadyGone
        );
    }

    #[test]
    fn teardown_refuses_a_recycled_handle_on_every_identity_mismatch() {
        // A live window of a foreign class is never ours, no matter whose
        // thread asks.
        assert_eq!(
            route_teardown(OWNER, OWNER, false, OUR_SLOT, OUR_SLOT),
            TeardownRoute::StaleHandle
        );
        // Our class, but a different context identity: the handle was
        // recycled for another FLUI window.
        assert_eq!(
            route_teardown(OWNER, OWNER, true, 0x2000, OUR_SLOT),
            TeardownRoute::StaleHandle
        );
        // A cleared slot (mid-WM_DESTROY, or never installed) is not ours
        // to destroy either — and identity must also protect the posted
        // cross-thread route, not just the direct one.
        assert_eq!(
            route_teardown(OWNER, FOREIGN, true, 0, OUR_SLOT),
            TeardownRoute::StaleHandle
        );
    }

    #[test]
    fn class_name_match_pins_the_nul_exclusion_convention_on_both_sides() {
        // "Flui" as GetClassNameW leaves it: buffer holds the name, then
        // trailing NULs; the returned count EXCLUDES the terminator.
        let mut buffer = [0_u16; 8];
        buffer[..4].copy_from_slice(&[0x46, 0x6C, 0x75, 0x69]);
        let expected: &[u16] = &[0x46, 0x6C, 0x75, 0x69];

        assert!(class_name_matches(&buffer, 4, expected));

        // An `expected` that still carries its terminating NUL can never
        // match — this is the exact bug a `w!(..)` literal read back WITH
        // its terminator would cause, refusing every valid window.
        let expected_with_nul: &[u16] = &[0x46, 0x6C, 0x75, 0x69, 0x0000];
        assert!(!class_name_matches(&buffer, 4, expected_with_nul));

        // Failed call (0 or negative), prefixes, and overlong counts all
        // refuse rather than match or panic.
        assert!(!class_name_matches(&buffer, 0, expected));
        assert!(!class_name_matches(&buffer, -1, expected));
        assert!(!class_name_matches(&buffer, 3, expected));
        assert!(!class_name_matches(&buffer, 5, expected));
        assert!(!class_name_matches(&buffer, 9, expected));
    }

    #[test]
    fn ledger_frees_exactly_once_at_the_outermost_release_after_retire() {
        // Reentrant dispatch: an outer borrow (a window_proc frame or a
        // with_window_context closure) is live when a nested WM_DESTROY
        // frame retires the context. The free must wait for the OUTERMOST
        // release.
        let mut ledger = ContextLedger::new();
        ledger.acquire(); // outer frame
        ledger.acquire(); // nested WM_DESTROY frame
        ledger.retire();
        assert!(
            !ledger.release(),
            "the retiring frame's own release must not free under the outer borrow"
        );
        assert!(
            ledger.release(),
            "the outermost release after retirement must free"
        );
    }

    #[test]
    fn ledger_frees_at_the_single_frame_release_in_the_plain_destroy_case() {
        // The common, non-reentrant teardown: one WM_DESTROY frame, no
        // borrower above it.
        let mut ledger = ContextLedger::new();
        ledger.acquire();
        ledger.retire();
        assert!(ledger.release());
    }

    #[test]
    fn ledger_never_frees_while_the_context_is_not_retired() {
        let mut ledger = ContextLedger::new();
        ledger.acquire();
        ledger.acquire();
        assert!(!ledger.release());
        assert!(!ledger.release());
    }
}
