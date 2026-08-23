#[cfg(not(target_os = "ios"))]
use std::sync::Arc;
#[cfg(not(target_os = "ios"))]
use std::sync::atomic::AtomicBool;

#[cfg(not(target_os = "ios"))]
use crate::app::runtime::{AppRuntime, ExitPolicy};

/// A fresh clone of the loop-scoped platform wake capability — see
/// `AppRuntime::frame_wake_callback`'s doc. `APP_RUNTIME` must not be
/// currently mutably borrowed when this is called (it takes a shared
/// borrow); every call site here is either before a realm is installed or
/// after one has been taken out of the slot for dispatch.
#[cfg(not(target_os = "ios"))]
pub(super) fn runtime_wake_callback() -> Arc<dyn Fn() + Send + Sync> {
    APP_RUNTIME.with(|slot| slot.borrow().frame_wake_callback())
}

/// A clone of the loop-scoped `needs_redraw` flag, for [`crate::app::ui_realm::UiRealm::new`]'s
/// `needs_redraw` parameter.
#[cfg(not(target_os = "ios"))]
pub(super) fn runtime_needs_redraw_handle() -> Arc<AtomicBool> {
    APP_RUNTIME.with(|slot| slot.borrow().needs_redraw_handle())
}

// ============================================================================
// Loop-scoped composition root (ADR-0027, ADR-0039 §6)
// ============================================================================

#[cfg(not(target_os = "ios"))]
thread_local! {
    /// The one loop-scoped composition root, shared by desktop, Android, and
    /// wasm. Absorbs what were, before the `AppRuntime` skeleton existed, two
    /// separate thread-locals: the transitional realm host (realm slot, queue,
    /// draining, owner thread, address cache, window registry, surface
    /// applier, visible/focused) and the loop-scoped `OwnerPlatform` host —
    /// see [`AppRuntime`]'s own module doc for why one struct correctly
    /// carries both invariants. The platform callback surface still
    /// requires `Send`, so the `!Send` realm this holds remains in owner TLS
    /// until that seam is retired (ADR-0027 follow-up 5); access is only
    /// through the stamped FIFO dispatcher below and the fenced
    /// `with_owner_platform` accessor.
    ///
    /// `AppRuntime::new()` is cheap and side-effect-free (no singleton
    /// resolution) precisely so merely *touching* this thread-local -- for
    /// any reason, including `OwnerHostClearGuard::drop` firing during an
    /// unwind on a thread that never reached platform init -- can never
    /// itself trigger singleton construction or full system-font
    /// enumeration. Real service resolution happens only via the explicit
    /// `ensure_services` call in `install_platform_realm` below, when a
    /// realm is actually installed.
    pub(super) static APP_RUNTIME: std::cell::RefCell<AppRuntime> =
        std::cell::RefCell::new(AppRuntime::new());
}

/// Installs `owner` in the loop-scoped host. Call once, at the top of each
/// backend's `on_ready` callback (ADR-0039 §6) — every `run_*` entry point
/// in this module does so immediately after minting/receiving its
/// `OwnerPlatform`.
///
/// Deliberately does NOT resolve `SharedEngineServices`: `run_direct`
/// installs an owner platform and opens a window but never installs a
/// `UiRealm` (no widget tree, no painting/semantics/scheduler singleton
/// reach at all), so resolving here would pay for singleton construction
/// and full system-font enumeration on a path that can never consume
/// either. `install_platform_realm` is the one call site that resolves —
/// every realm-hosting backend goes through it, `run_direct` never does.
#[cfg(not(target_os = "ios"))]
pub(crate) fn install_owner_platform(owner: flui_platform::OwnerPlatform) {
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut().owner_platform = Some(owner);
    });
}

/// Installs the exit-policy hook this thread's `AppRuntime` consults instead
/// of letting a platform backend decide, alone, whether "every window this
/// backend tracks just closed" means "exit" — the live-loop wiring `issue
/// #555`'s `ExitPolicy`/`AppRuntime::should_exit` names as its own
/// deliberately-deferred follow-up (see `ExitPolicy`'s own doc). Call once,
/// immediately after [`install_owner_platform`], from every backend that
/// wants this: today, that is `run_desktop` only — Android/web bootstraps
/// never call this (their platforms don't override
/// [`flui_platform::traits::Platform::set_exit_policy_hook`] either, so it
/// would be inert there anyway; wiring the hook itself into those backends
/// is unrelated to whether flui-app installs it).
///
/// The hook itself re-enters `APP_RUNTIME` only when the PLATFORM calls it
/// later (a window closing) — never synchronously from this function, which
/// only registers it — so this does not violate `with_owner_platform`'s "no
/// host re-entry" rule.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "run_desktop (its one caller) is desktop-only -- android/wasm32 bootstraps \
                  never call this"
    )
)]
pub(super) fn install_exit_policy_hook(policy: ExitPolicy) {
    let shared = with_owner_platform(|owner| {
        owner.shared().set_exit_policy_hook(Box::new(move || {
            let (should_exit, removed) =
                APP_RUNTIME.with(|slot| slot.borrow_mut().should_exit(policy));
            drop(removed);
            should_exit
        }));
        owner.shared()
    });

    // The hook above is otherwise only consulted when a window closes, so
    // a veto owed to a running keep-alive service (issue #558) would be
    // PERMANENT once the last window is gone — nothing left to close,
    // nothing to re-ask, and the process would linger forever. Close that
    // loop: when a keep-alive service reports its exit (on whatever worker
    // thread it ran on), request the platform's coalesced, owner-thread
    // re-consultation of the same hook. Spurious fires are harmless by
    // contract (windows still open, or the hook still vetoing, are
    // no-ops), and backends without the mechanism default it to inert.
    // Registered outside the `with_owner_platform` borrow: the notifier
    // installation touches `APP_RUNTIME`, which must never run while the
    // owner-platform host is checked out. `None` (no owner platform on
    // this thread) means the hook install above was a no-op too — nothing
    // to wire. Not compiled on wasm32, where the lifecycle layer (and so
    // the notifier seam) does not exist.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(shared) = shared {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .set_lifecycle_exit_notifier(std::sync::Arc::new(move || {
                    shared.request_exit_policy_reevaluation();
                }));
        });
    }
    #[cfg(target_arch = "wasm32")]
    drop(shared);
}

/// Installs the wall-clock-wake hook this thread's platform
/// backend consults, once per idle iteration, for the earliest instant it
/// should wake at instead of blocking forever — see
/// [`flui_platform::traits::Platform::set_wake_deadline_hook`]'s doc for the
/// full contract. Call once, alongside [`install_exit_policy_hook`], from
/// every backend that wants this: today, that is `run_desktop` only —
/// Android/web bootstraps never call this (their platforms don't override
/// that trait method either, so it would be inert there anyway; see
/// `DeviceRecoveryBackoff`'s own doc for what that means for a device-
/// recovery deadline specifically on Android).
///
/// `secondary_deadline` merges an additional wake-deadline SOURCE into this
/// hook's answer (the earliest of the two wins) without this function
/// needing to know what produces it — `bootstrap_desktop` passes a closure
/// over its own `DeviceRecoveryBackoff`, so a device stuck retrying under
/// backoff still gets `ControlFlow::WaitUntil`'s efficient wait instead of
/// this hook silently only ever answering the realm's own deadline. Kept
/// generic (not `DeviceRecoveryBackoff`-typed) so this function's own `cfg`
/// gate can stay as broad as it already is (`not(ios)`, wider than that
/// type's `not(ios), not(wasm32)`) without needing a matching narrow gate
/// here too — only the (already desktop-only) call site's closure captures
/// the concrete backoff type.
///
/// The hook itself re-enters `APP_RUNTIME` only when the PLATFORM calls it
/// later (`about_to_wait`) — never synchronously from this function, which
/// only registers it — so this does not violate `with_owner_platform`'s "no
/// host re-entry" rule. Read-only (`AppRuntime::next_wake` takes `&self`),
/// unlike `install_exit_policy_hook`'s `&mut self` — no deferred-mutation
/// drain needed here, since computing a wake deadline never touches the
/// realm registry itself.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "run_desktop (its one caller) is desktop-only -- android/wasm32 bootstraps \
                  never call this"
    )
)]
pub(super) fn install_wake_deadline_hook(
    secondary_deadline: impl Fn() -> Option<web_time::Instant> + Send + Sync + 'static,
) {
    with_owner_platform(|owner| {
        owner.shared().set_wake_deadline_hook(Box::new(move || {
            let realm_deadline = APP_RUNTIME.with(|slot| slot.borrow().next_wake());
            merge_wake_deadlines(realm_deadline, secondary_deadline())
        }));
    });
}

/// The earlier of two optional wake deadlines, treating `None` as "no
/// opinion" rather than as a value that could win a `min` against a real
/// deadline — the same fold `AppRuntime::next_wake` (`runtime.rs`) itself
/// uses across realms, pulled out here as its own named, unit-tested
/// function because it is exactly what [`install_wake_deadline_hook`]'s
/// entire non-blocking desktop design now rests on: this whole module's
/// device-recovery deadline reaches the platform's `ControlFlow::WaitUntil`
/// only through this fold correctly picking the earlier of the realm's own
/// deadline and the secondary (device-recovery) one, and correctly leaving
/// the realm's deadline untouched when the secondary source has nothing
/// pending.
#[cfg(not(target_os = "ios"))]
fn merge_wake_deadlines(
    a: Option<web_time::Instant>,
    b: Option<web_time::Instant>,
) -> Option<web_time::Instant> {
    [a, b].into_iter().flatten().min()
}

/// Whether an armed device-recovery deadline should actually be reported to
/// [`install_wake_deadline_hook`]'s secondary-source closure this call —
/// pulled out as its own pure function for the same reason `frame_is_dirty`
/// was: the real closure captures `APP_RUNTIME` state no unit test can drive
/// directly, so the decision this function makes is tested here in
/// isolation instead, and the closure calls this rather than reimplementing
/// it (round 6's own lesson about what happens to a predicate reimplemented
/// in two places).
///
/// `frames_enabled == false` suppresses the deadline unconditionally,
/// regardless of how soon it is due: while frames are disabled
/// (`AppLifecycleState::Hidden`/`Paused`/`Detached`), the frame closure's
/// `WakeAction::PumpAsync` arm returns before `render_frame_with_device_
/// recovery` ever runs, so nothing on that path would consume a reported
/// deadline — reporting it anyway hands `about_to_wait` the SAME past
/// instant on every idle iteration once it comes due, which is
/// `WinitApp::new_events`'s own named `WaitUntil(past)` busy-spin, forced by
/// this hook instead of a stale realm deadline. The deadline is not lost by
/// staying unreported while disabled: frames re-enabling already redirties
/// the root unconditionally (`UiRealm::redirty_root_for_frames_reenable`),
/// which wakes the loop through the ordinary `needs_redraw` channel and
/// lets a real `WakeAction::Render` resume the retry then.
// Desktop-only, like its sole caller `bootstrap_desktop`: wasm has no
// `ControlFlow`/`WaitUntil` to feed and iOS has no bootstrap here, so
// compiling it on either target is dead code the `-D warnings` wasm gate
// rejects.
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
pub(super) fn desktop_secondary_wake_deadline(
    next_attempt_at: Option<web_time::Instant>,
    frames_enabled: bool,
) -> Option<web_time::Instant> {
    if frames_enabled {
        next_attempt_at
    } else {
        None
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod desktop_secondary_wake_deadline_tests {
    use web_time::Instant;

    use super::desktop_secondary_wake_deadline;

    #[test]
    fn an_armed_deadline_is_reported_while_frames_are_enabled() {
        let deadline = Instant::now();
        assert_eq!(
            desktop_secondary_wake_deadline(Some(deadline), true),
            Some(deadline)
        );
    }

    #[test]
    fn an_armed_deadline_is_suppressed_while_frames_are_disabled() {
        let deadline = Instant::now();
        assert_eq!(
            desktop_secondary_wake_deadline(Some(deadline), false),
            None,
            "a deadline nothing can act on must not be reported -- reporting it would hand \
             `about_to_wait` the same past instant forever (the PumpAsync arm never consumes \
             it), the exact WaitUntil(past) busy-spin `WinitApp::new_events` names, one layer \
             up from where round 4 fixed the equivalent hole on the Render path"
        );
    }

    #[test]
    fn no_armed_deadline_stays_none_either_way() {
        assert_eq!(desktop_secondary_wake_deadline(None, true), None);
        assert_eq!(desktop_secondary_wake_deadline(None, false), None);
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod merge_wake_deadlines_tests {
    use std::time::Duration;

    use web_time::Instant;

    use super::merge_wake_deadlines;

    #[test]
    fn picks_the_earlier_of_two_present_deadlines_either_order() {
        let now = Instant::now();
        let earlier = now + Duration::from_millis(16);
        let later = now + Duration::from_secs(1);

        assert_eq!(
            merge_wake_deadlines(Some(earlier), Some(later)),
            Some(earlier),
            "realm-earlier, secondary-later"
        );
        assert_eq!(
            merge_wake_deadlines(Some(later), Some(earlier)),
            Some(earlier),
            "realm-later, secondary-earlier -- order must not matter"
        );
    }

    #[test]
    fn a_missing_secondary_leaves_the_realm_deadline_untouched() {
        let deadline = Instant::now() + Duration::from_millis(16);
        assert_eq!(
            merge_wake_deadlines(Some(deadline), None),
            Some(deadline),
            "no device-recovery deadline pending must not suppress a real realm deadline"
        );
    }

    #[test]
    fn a_missing_realm_deadline_leaves_the_secondary_deadline_untouched() {
        let deadline = Instant::now() + Duration::from_millis(16);
        assert_eq!(
            merge_wake_deadlines(None, Some(deadline)),
            Some(deadline),
            "no realm deadline pending must not suppress a real device-recovery deadline"
        );
    }

    #[test]
    fn both_absent_is_absent() {
        assert_eq!(
            merge_wake_deadlines(None, None),
            None,
            "neither source has an opinion -- the platform must fall back to its \
             unconditional Wait, not a spurious WaitUntil(anything)"
        );
    }
}

/// Borrow-style access to the loop-scoped owner-platform capability.
/// `None` if no `OwnerPlatform` is currently installed on this thread
/// (before `on_ready`, or after the host was cleared).
///
/// Three fences (ADR-0039 §6), all landing in this one accessor:
///
/// (a) **Borrow, not clone.** `pub(crate)` to `flui-app`'s `app` module,
///     never re-exported; `OwnerPlatform` isn't `Clone`, so there is no way
///     to escape this closure with a durable owned copy — every access
///     re-crosses the fence.
/// (b) **Static scan.** This function's name carries the scanner token
///     `owner_platform`, so `scripts/check-frame-capability-scope.sh`
///     (trigger #22) mechanically rejects any call from inside
///     `build`/`perform_layout`/`paint`/composite bodies, across every
///     crate the scanner sweeps.
/// (c) **Runtime backstop.** `debug_assert!`s that the installed realm's own
///     scheduler (`AppRuntime::installed_realm_phase`) is not inside the
///     frame transaction. "Not inside a frame phase" per the ADR means
///     `TransientCallbacks`/`MidFrameMicrotasks`/`PersistentCallbacks` are
///     forbidden; `Idle` and `PostFrameCallbacks` are allowed (legitimate
///     ADR-0021-style post-frame work); `None` (truly no realm installed,
///     and none currently dispatched — see `AppRuntime::dispatched_scheduler`)
///     holds vacuously. `installed_realm_phase` reads through to the
///     checked-out realm's scheduler for the entire extent of a
///     `dispatch_platform_realm` call, not only the resident-realm case, so
///     this fence is load-bearing during a real dispatched production frame,
///     not merely when the realm sits untouched in the slot. This fence is
///     still **vacuous on binding-local frame paths**: headless/test
///     bindings drive their own binding-local `UpdateScheduler`, never a realm
///     installed into `APP_RUNTIME`, so fences (a) and (b) are the
///     load-bearing ones there — stated here, not hidden.
///
/// # `owner.shared()`'s method list is a compile-time fence too
///
/// Code reached through this accessor that isn't owner-thread-affine goes
/// through [`OwnerPlatform::shared`](flui_platform::OwnerPlatform::shared),
/// which returns `SharedPlatform` — a type whose method list IS the fence
/// (no owner-affine method, e.g. `open_window`, is ever added to it; see
/// its own rustdoc). This
/// `compile_fail` doctest is CI-run evidence for that fence: `flui-app`
/// dev/normal-depends on `flui-platform` and its own doc tests DO run under
/// `just test-doc`, unlike the equivalent illustration inside
/// `flui-platform` itself (excluded from that gate, `justfile:177`).
///
/// ```compile_fail,E0599
/// use flui_platform::headless_platform;
///
/// let _ = headless_platform().run(Box::new(|owner| {
///     let shared = owner.shared();
///     // `SharedPlatform` has no `open_window` — it stays owner-affine on
///     // `OwnerPlatform` only. Fails with "no method named `open_window`
///     // found for struct `SharedPlatform`" (E0599).
///     let _ = shared.open_window(Default::default());
///     Ok(())
/// }));
/// ```
///
/// # No host re-entry
///
/// `f` runs while this function holds an immutable `APP_RUNTIME.borrow()`.
/// Since `AppRuntime` folded the realm-facing state and `owner_platform`
/// into ONE thread-local `RefCell` (they were two disjoint cells before), `f`
/// must never call back into any function that touches this same cell:
/// `dispatch_platform_realm`,
/// `install_platform_realm`, `teardown_platform_realm`,
/// `install_surface_applier`, or `install_owner_platform` itself. Any of
/// those does `slot.borrow_mut()` while this borrow is still live, which is
/// a guaranteed `BorrowMutError` panic — in every build, not only debug
/// (`RefCell`'s borrow tracking is not a `debug_assert`). No production
/// caller does this today (`f` closures only call owner-affine `Platform`
/// methods), so this is a documented invariant with a regression pin
/// (`with_owner_platform_reentering_dispatch_panics` below), not a runtime
/// guard.
#[cfg(not(target_os = "ios"))]
pub(crate) fn with_owner_platform<R>(
    f: impl FnOnce(&flui_platform::OwnerPlatform) -> R,
) -> Option<R> {
    #[cfg(debug_assertions)]
    {
        // A sequential, separate `.with()` borrow -- released before the
        // real one below opens -- so this never re-enters the same
        // `RefCell`. `None` (no realm installed on this thread) is vacuous
        // but truthful: no realm means no frame transaction can be in
        // flight here, so the asserted property holds trivially.
        let phase = APP_RUNTIME.with(|slot| slot.borrow().installed_realm_phase());
        debug_assert!(
            !matches!(
                phase,
                Some(
                    flui_scheduler::SchedulerPhase::TransientCallbacks
                        | flui_scheduler::SchedulerPhase::MidFrameMicrotasks
                        | flui_scheduler::SchedulerPhase::PersistentCallbacks
                )
            ),
            "BUG: with_owner_platform called while the installed realm's scheduler is inside \
             the frame transaction (phase {phase:?}) -- owner_platform must \
             not be acquired from build/layout/paint (ADR-0039 §6, trigger #22)"
        );
    }
    APP_RUNTIME.with(|slot| slot.borrow().owner_platform.as_ref().map(f))
}

/// Unwind-safe TLS clearing. Arm this guard *before* calling
/// `Platform::run(...)` on any backend whose `run` returns (winit,
/// headless, Android) — not inside `on_ready` — so a panic anywhere inside
/// `on_ready` or later in `run` unwinds through the guard's `Drop` and
/// cannot leak the host into whatever runs on this thread next (notably,
/// the next test). Clearing an already-empty slot is a no-op.
///
/// Web deliberately arms no guard: the host stays resident for the page's
/// lifetime (see the web runner's own comment on this). macOS is moot:
/// `run` never returns there (`terminate:` exits the process).
///
/// # No host re-entry, and no eager resolution
///
/// `Drop` touches only `owner_platform` (`self.owner_platform.take()`) —
/// never `dispatch_platform_realm`/`install_platform_realm`/
/// `teardown_platform_realm`/`install_surface_applier`, all of which would
/// re-borrow the same `APP_RUNTIME` cell this drop already holds mutably
/// (see `with_owner_platform`'s own "No host re-entry" doc for the general
/// rule). Just as importantly, `AppRuntime::new()` is cheap and
/// side-effect-free specifically so that a bare `.borrow_mut()` here — the
/// *first* touch of `APP_RUNTIME` on a thread whose `on_ready` panicked
/// before installing anything — can never trigger `SharedEngineServices`
/// resolution (singleton construction plus full system-font enumeration)
/// while already unwinding. A panic during that resolution, on top of the
/// panic already unwinding, would abort the process instead of propagating
/// the original failure.
#[cfg(not(target_os = "ios"))]
#[must_use = "the guard must stay alive across the Platform::run(...) call it \
              guards, or the TLS host clears immediately instead of at loop exit"]
pub(crate) struct OwnerHostClearGuard {
    _private: (),
}

#[cfg(not(target_os = "ios"))]
impl OwnerHostClearGuard {
    /// Arms the guard. Call immediately before `Platform::run(...)`.
    pub(crate) fn arm() -> Self {
        Self { _private: () }
    }
}

#[cfg(not(target_os = "ios"))]
impl Drop for OwnerHostClearGuard {
    fn drop(&mut self) {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().owner_platform.take();
        });
    }
}
