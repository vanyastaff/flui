use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::app::runtime::{AppRuntime, ExitPolicy};

/// A fresh clone of the loop-scoped platform wake capability — see
/// `AppRuntime::frame_wake_callback`'s doc. `APP_RUNTIME` must not be
/// currently mutably borrowed when this is called (it takes a shared
/// borrow); every call site here is either before a realm is installed or
/// after one has been taken out of the slot for dispatch.
pub(super) fn runtime_wake_callback() -> Arc<dyn Fn() + Send + Sync> {
    APP_RUNTIME.with(|slot| slot.borrow().frame_wake_callback())
}

/// The platform clipboard for [`crate::app::ui_realm::UiRealm::new`]'s
/// `clipboard` parameter. Same borrow rule as [`runtime_wake_callback`].
///
/// # Panics
///
/// If no platform clipboard is installed: [`install_owner_platform`] installs
/// it, and every runner calls that before it builds any realm.
pub(super) fn runtime_clipboard() -> Arc<dyn flui_platform::traits::Clipboard> {
    APP_RUNTIME
        .with(|slot| slot.borrow().clipboard())
        .expect("BUG: the runner installs the platform clipboard before it builds a realm")
}

/// The app's font collection for [`crate::app::ui_realm::UiRealm::new`]'s
/// `fonts` parameter (ADR-0092 §2). Same borrow rule as
/// [`runtime_wake_callback`]. The first call on a thread resolves the shared
/// engine services.
pub(super) fn runtime_font_collection() -> flui_painting::FontCollection {
    APP_RUNTIME.with(|slot| slot.borrow().font_collection())
}

/// The host's byte storage, resolved once from the run's configuration when
/// the host started (`AppRuntime::install_host_storage`). Same borrow rule
/// as [`runtime_wake_callback`].
pub(super) fn runtime_storage() -> Option<Arc<dyn flui_platform_api::Storage>> {
    APP_RUNTIME.with(|slot| slot.borrow().host_storage())
}

/// Builds a runner's realm over the runtime's shared services: `wake`, the
/// loop's `needs_redraw` flag, the platform clipboard, the app's font
/// collection and the host's byte storage. Every runner site and secondary
/// window builds its realm through this one call, so a realm cannot be
/// handed a stand-in for any of them.
pub(super) fn build_runtime_realm(
    wake: &Arc<dyn Fn() + Send + Sync>,
    window: impl Into<crate::app::presentation::PresentationWindow>,
    scale_factor: f64,
) -> Result<crate::app::ui_realm::UiRealm, crate::app::ui_realm::UiRealmError> {
    let fonts = runtime_font_collection();
    let mut host = crate::app::ui_realm::RealmHostServices::new(
        Arc::clone(wake),
        runtime_needs_redraw_handle(),
        runtime_clipboard(),
        &fonts,
        flui_scheduler::ClockSource::Platform,
    );
    if let Some(storage) = runtime_storage() {
        host = host.with_storage(storage);
    }
    crate::app::ui_realm::UiRealm::new(window, scale_factor, host)
}

/// A clone of the loop-scoped `needs_redraw` flag, for [`crate::app::ui_realm::UiRealm::new`]'s
/// `needs_redraw` parameter.
pub(super) fn runtime_needs_redraw_handle() -> Arc<AtomicBool> {
    APP_RUNTIME.with(|slot| slot.borrow().needs_redraw_handle())
}

// ============================================================================
// Loop-scoped composition root (ADR-0027, ADR-0039 §6)
// ============================================================================

thread_local! {
    /// The one loop-scoped composition root, shared by desktop, Android, and
    /// wasm. Absorbs what were, before the `AppRuntime` skeleton existed, two
    /// separate thread-locals: the transitional realm host (realm slot, queue,
    /// draining, owner thread, address cache, window registry, surface
    /// applier) and the loop-scoped `OwnerPlatform` host —
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
    /// enumeration. Real service resolution happens only when a realm is
    /// built or installed: `build_runtime_realm`, which every runner calls
    /// to build its realm, or the explicit `ensure_services` call in
    /// `install_platform_realm` below.
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
#[cfg_attr(
    any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
    expect(
        clippy::unnecessary_wraps,
        reason = "portable bootstrap keeps one fallible contract; desktop wake registration can fail, mobile only installs the owner"
    )
)]
pub(crate) fn install_owner_platform(
    owner: flui_platform::OwnerPlatform,
) -> Result<(), flui_platform::WakeRegistrationError> {
    #[cfg(any(
        target_os = "ios",
        all(not(target_os = "android"), not(target_arch = "wasm32"))
    ))]
    let owner_turn_wake: Rc<dyn Fn() -> bool> = {
        let proxy = owner.proxy();
        Rc::new(move || proxy.wake().is_ok())
    };
    #[cfg(target_os = "android")]
    let owner_turn_wake: Rc<dyn Fn() -> bool> = {
        let poke = APP_RUNTIME.with(|slot| slot.borrow().owner_turn_window_poke());
        Rc::new(move || poke())
    };
    #[cfg(target_arch = "wasm32")]
    let owner_turn_wake: Rc<dyn Fn() -> bool> = Rc::new(|| true);
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    let identity = {
        let identity = Arc::new(());
        let installed_identity = Arc::clone(&identity);
        owner.on_wake(Box::new(move || {
            if APP_RUNTIME
                .with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &installed_identity))
            {
                let _owner_callback = super::realm_dispatch::begin_owner_callback();
                super::secondary_window::drain_pending_secondary_window_completions();
                super::main_window::drive_main_window();
            }
        }))?;
        identity
    };
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    super::main_window::shutdown_main_window();
    // The platform clipboard (ADR-0038 §9) is installed with the owner, so
    // every realm a runner builds afterwards finds it (`runtime_clipboard`).
    let clipboard = owner.shared().clipboard();
    let (previous, previous_owner_turn_wake) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.owner_install_generation = state
            .owner_install_generation
            .checked_add(1)
            .expect("BUG: owner install generation exhausted");
        state.set_platform_clipboard(clipboard);
        let previous_owner_turn_wake = state.owner_turn_wake.replace(owner_turn_wake);
        let previous = state.owner_platform.replace(std::rc::Rc::new(owner));
        #[cfg(all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        ))]
        {
            state.quit_notification = crate::app::runtime::QuitNotification::Active;
            state.loop_identity = identity;
            state.pending_window_reservations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        }
        (previous, previous_owner_turn_wake)
    });
    drop(previous);
    drop(previous_owner_turn_wake);
    Ok(())
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
#[cfg_attr(
    not(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    )),
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
            if !should_exit {
                return false;
            }
            // Destructors may admit new windows. Recheck after all removed
            // realms have dropped, then fence the same ingress as senders.
            let (should_exit, removed) =
                APP_RUNTIME.with(|slot| slot.borrow_mut().should_exit(policy));
            drop(removed);
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            if should_exit {
                let ingress = APP_RUNTIME.with(|slot| slot.borrow().main_ingress.clone());
                if let Some(ingress) = ingress {
                    return ingress.try_auto_quit();
                }
            }
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

/// Install the desktop platform's one quit notification callback.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn install_platform_quit_hook() {
    let loop_identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
    let owner_thread = std::thread::current().id();
    with_owner_platform(|owner| {
        owner.shared().on_quit(Box::new(move || {
            assert_eq!(
                std::thread::current().id(),
                owner_thread,
                "BUG: platform quit must run on its owner"
            );
            if !APP_RUNTIME.with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &loop_identity)) {
                return;
            }
            super::main_window::shutdown_main_window();
            tracing::info!("Platform quit");
            super::realm_dispatch::request_quit_notification();
        }));
    });
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
#[cfg_attr(
    not(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    )),
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
pub(super) fn merge_wake_deadlines(
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
/// staying unreported while disabled: presentation lifecycle reconciliation
/// redirties the restored root and wakes the loop through the ordinary
/// `needs_redraw` channel, which
/// lets a real `WakeAction::Render` resume the retry then.
// Desktop-only, like its sole caller `bootstrap_desktop`: the mobile backends
// have no `ControlFlow`/`WaitUntil` to feed and wasm has no loop, so compiling
// it on any of them is dead code the `-D warnings` cross-target gates reject.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
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

/// Borrow-style access to the loop-scoped owner-platform capability.
/// `None` if no `OwnerPlatform` is currently installed on this thread
/// (before `on_ready`, or after the host was cleared).
///
/// Three fences (ADR-0039 §6), all landing in this one accessor:
///
/// (a) **Borrowed public access.** `pub(crate)` to `flui-app`'s `app` module,
///     never re-exported; `OwnerPlatform` isn't `Clone`, so there is no way
///     to escape this closure with a durable owned copy — every access
///     re-crosses the fence.
/// (b) **Scope rule.** Never call it from inside
///     `build`/`perform_layout`/`paint`/composite bodies. This is a free
///     function, not a `LifecycleContext` method, so no type withholds it
///     from a frame phase; (c) is the check.
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
/// its own rustdoc). The failing and passing examples share a bootstrap
/// callback and differ only in the window-opening receiver: the shared
/// residual has no `open_window`, while the owner capability does. The
/// examples compile in the doctest suite; the valid callback is not run.
///
/// ```compile_fail,E0599
/// use flui_platform::headless_platform;
///
/// let _ = headless_platform().run(Box::new(|owner| {
///     let shared = owner.shared();
///     let _ = shared.open_window(Default::default());
///     Ok(())
/// }));
/// ```
///
/// ```no_run
/// use flui_platform::headless_platform;
///
/// let _ = headless_platform().run(Box::new(|owner| {
///     let shared = owner.shared();
///     let _ = owner.open_window(Default::default());
///     Ok(())
/// }));
/// ```
///
/// # Native re-entry
///
/// The runtime owns one `Rc<OwnerPlatform>`. The accessor takes a temporary
/// internal strong reference, releases the TLS borrow, then invokes `f`.
/// Native calls may synchronously dispatch focus/close events into the runtime.
/// Public callers still receive only `&OwnerPlatform`; neither `Clone` nor
/// cross-thread ownership is added to that capability. A native operation's
/// result must separately be checked against its loop identity and admission.
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
             not be acquired from build/layout/paint (ADR-0039 §6)"
        );
    }
    let owner = APP_RUNTIME.with(|slot| slot.borrow().owner_platform.clone());
    owner.as_deref().map(f)
}

/// Unwind-safe TLS clearing. Arm this guard *before* calling
/// `Platform::run(...)` on any backend whose `run` returns (winit,
/// headless, Android, AppKit) — not inside `on_ready` — so a panic anywhere inside
/// `on_ready` or later in `run` unwinds through the guard's `Drop` and
/// cannot leak the host into whatever runs on this thread next (notably,
/// the next test). Clearing an already-empty slot is a no-op.
///
/// Web deliberately arms no guard: the host stays resident for the page's
/// lifetime (see the web runner's own comment on this). Standalone AppKit
/// stops its native loop and returns through this guard like other desktops.
///
/// # Cleanup outside borrows, and no eager resolution
///
/// `Drop` takes the owner reference out of TLS and releases it only after the
/// borrow ends. Captured native resources can therefore re-enter cleanup.
/// Just as importantly, `AppRuntime::new()` is cheap and
/// side-effect-free specifically so that a bare `.borrow_mut()` here — the
/// *first* touch of `APP_RUNTIME` on a thread whose `on_ready` panicked
/// before installing anything — can never trigger `SharedEngineServices`
/// resolution (singleton construction plus full system-font enumeration)
/// while already unwinding. A panic during that resolution, on top of the
/// panic already unwinding, would abort the process instead of propagating
/// the original failure.
#[must_use = "the guard must stay alive across the Platform::run(...) call it \
              guards, or the TLS host clears immediately instead of at loop exit"]
pub(crate) struct OwnerHostClearGuard {
    expected_generation: u64,
}

impl OwnerHostClearGuard {
    /// Own the first successful owner installation after this point. A later
    /// replacement has another generation and cannot be erased by this guard.
    /// Call immediately before `Platform::run(...)`; no-install failure leaves
    /// a previously installed owner untouched.
    pub(crate) fn arm() -> Self {
        let expected_generation = APP_RUNTIME.with(|slot| {
            slot.borrow()
                .owner_install_generation
                .checked_add(1)
                .expect("BUG: owner install generation exhausted")
        });
        Self {
            expected_generation,
        }
    }
}

impl Drop for OwnerHostClearGuard {
    fn drop(&mut self) {
        let (removed, owner_turn_wake, storage) = APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            if runtime.owner_install_generation == self.expected_generation {
                runtime.owner_turn_continuation = None;
                runtime.owner_turn_continuation_failed = false;
                runtime.owner_turn_callback_budget = None;
                runtime.owner_turn_callback_active = false;
                (
                    runtime.owner_platform.take(),
                    runtime.owner_turn_wake.take(),
                    runtime.host_storage.take(),
                )
            } else {
                (None, None, None)
            }
        });
        drop(removed);
        drop(owner_turn_wake);
        drop(storage);
    }
}
