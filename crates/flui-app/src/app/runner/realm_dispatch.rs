use std::collections::VecDeque;

use flui_foundation::RealmId;
use flui_scheduler::AppLifecycleState;

use super::host::APP_RUNTIME;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::secondary_window::drain_pending_secondary_window_completions;
use crate::app::lifecycle_state::preserve_first_lifecycle_panic;
use crate::app::runtime::RealmSlot;

/// A registration-lifetime renderer-surface applier: `FnMut(size,
/// scale_factor)`. Named so [`RealmSlot`]'s `surface_applier` field
/// declaration reads plainly instead of spelling out the boxed closure type
/// inline. `pub(in crate::app)` (rather than private) so [`RealmSlot`]'s struct
/// definition in the sibling `runtime` module can name this type.
pub(in crate::app) type SurfaceApplier =
    Box<dyn FnMut(flui_types::Size<flui_types::geometry::Pixels>, f32)>;

/// Restores a taken [`SurfaceApplier`] back into its realm's slot in
/// [`APP_RUNTIME`]'s registry when dropped — including during an unwinding
/// drop, so a panic inside the applier's own call (caught by
/// `dispatch_platform_realm`'s outer `catch_unwind`) cannot permanently
/// strand resizing. Without this, the applier taken out before the call is
/// simply never restored once the call panics, and every later `Resized`
/// event for that realm finds the slot empty forever, silently coalescing at
/// the `None` arm's trace instead of ever applying again.
///
/// Addressed by [`RealmId`] (not the old bare `Option`): if the realm was
/// torn down while the applier's own call was still running, restoring into
/// a now-missing slot is a silent no-op, matching this file's existing "the
/// realm may be gone by the time a destructor runs" discipline.
#[must_use = "dropping this immediately restores the applier with no call in between"]
struct SurfaceApplierRestoreGuard {
    realm_id: RealmId,
    applier: Option<SurfaceApplier>,
}

impl SurfaceApplierRestoreGuard {
    fn call(&mut self, size: flui_types::Size<flui_types::geometry::Pixels>, scale_factor: f32) {
        if let Some(applier) = self.applier.as_mut() {
            applier(size, scale_factor);
        }
    }
}

impl Drop for SurfaceApplierRestoreGuard {
    fn drop(&mut self) {
        if let Some(applier) = self.applier.take() {
            let realm_id = self.realm_id;
            APP_RUNTIME.with(|slot| {
                if let Some(realm_slot) = slot.borrow_mut().realms.get_mut(&realm_id) {
                    realm_slot.surface_applier = Some(applier);
                }
            });
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RealmDispatcher {
    pub(super) owner_thread: std::thread::ThreadId,
    pub(super) address: flui_foundation::PresentationAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RealmDispatchError {
    WrongThread,
    /// The realm incarnation this dispatcher was minted for is gone — the
    /// common path: `realm_id`/`presentation_id` mint from one shared
    /// counter, so teardown+reinstall always changes both, and this check
    /// (realm first) catches it before the presentation half is even
    /// compared.
    StaleRealm,
    /// The realm is live and matches, but the presentation incarnation does
    /// not — reachable today only via a forged/mixed address (a dispatcher
    /// whose presentation half was swapped for another incarnation's), and,
    /// once one realm can host more than one presentation, via real
    /// presentation replacement within a live realm. Kept as its own
    /// variant now: the design-for-N contract, not dead code.
    StalePresentation,
    RealmUnavailable,
    /// Rejected because a DIFFERENT realm is currently checked out for
    /// dispatch on this thread (issue #555): dispatch is single-threaded
    /// and sequential, so a nested dispatch that targets a realm other than
    /// the one already checked out is always a bug, never a legitimate
    /// concurrent-realm scenario — reachable only if something bypasses the
    /// defer-to-idle discipline
    /// [`crate::app::runtime::AppRuntime::request_realm_install`]/[`crate::app::runtime::AppRuntime::request_realm_uninstall`]
    /// exist to make unnecessary. A `debug_assert!` at the same call site
    /// makes this loud in debug builds; this variant is the release-mode
    /// fallback that still refuses instead of silently nesting.
    NestedCrossRealmDispatchRejected,
}

/// Typed, closed cross-thread payload (ADR-0037 §3): every routable
/// platform-to-UI event. Compile-time evidence that this is a real `Send`
/// boundary: `static_assertions::assert_impl_all!` is checked in this
/// module's own tests. If `PlatformInput` ever
/// stopped being `Send`, that must be fixed in `flui-platform` itself,
/// never worked around here.
// `pub(in crate::app)` because `RealmTask::Event` (also `pub(in crate::app)`, for
// `AppRuntime`'s sake) carries this type in a field the compiler considers
// reachable at that same visibility.
// The window-event variants (`WindowFocus`, `WindowVisibility`,
// `AppearanceChanged`, `WindowHover`) are produced only by the desktop
// runner's `on_window_event` wiring; the mobile and web runners drive their
// lifecycle from platform callbacks instead, so those variants are
// unconstructed there.
#[cfg_attr(
    all(
        not(test),
        // Only android and iOS drop the window-event variants: the web runner
        // constructs `WindowFocus`/`WindowHover` through the browser's
        // visibility/focus signals, so on wasm32 they are live.
        any(target_os = "android", target_os = "ios")
    ),
    expect(
        dead_code,
        reason = "window-event variants are produced only by the desktop runner"
    )
)]
pub(in crate::app) enum PlatformToUi {
    Input(flui_platform::traits::PlatformInput),
    Resized {
        size: flui_types::Size<flui_types::geometry::Pixels>,
        scale_factor: f32,
    },
    /// Window focus changed (winit's `WindowEvent::Focused`, or the
    /// equivalent per-backend signal; same source as the deleted `Active`
    /// variant this one replaces). Feeds the `(visible, focused)` ->
    /// `AppLifecycleState` derivation below, alongside
    /// [`WindowVisibility`](Self::WindowVisibility).
    WindowFocus(bool),
    /// Reversible native execution eligibility for one presentation.
    #[cfg_attr(
        any(target_arch = "wasm32", target_os = "android"),
        expect(
            dead_code,
            reason = "these runners retain their existing host lifecycle transport"
        )
    )]
    WindowExecution(flui_platform::WindowExecutionState),
    /// Addressed logical content-view safe area.
    ///
    /// The iOS runner is the only producer; this module's own tests construct
    /// it directly to pin the addressed-write contract (a report for a
    /// presentation closed before delivery is dropped, not a panic).
    #[cfg_attr(
        not(any(test, target_os = "ios")),
        expect(
            dead_code,
            reason = "safe-area reports are produced only by the UIKit runner"
        )
    )]
    SafeAreaChanged(flui_types::geometry::EdgeInsets),
    /// Window visibility/occlusion changed (winit's `WindowEvent::Occluded`,
    /// negated — see `PlatformWindow::on_visibility_status_change`).
    ///
    /// Combined with [`WindowFocus`](Self::WindowFocus) via
    /// the addressed presentation's lifecycle reconciliation. The realm
    /// scheduler derives its aggregate from all live presentations.
    // Not yet constructed on wasm32: `run_web` only wires `WindowFocus` —
    // no occlusion signal for the web backend yet (see run_web's comment at
    // its `on_active_status_change` registration).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    WindowVisibility(bool),
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    SynchronizeLifecycle,
    #[cfg(any(target_os = "ios", target_os = "android", target_arch = "wasm32"))]
    Shutdown,
    /// The OS light/dark appearance changed (winit's `ThemeChanged`, or the
    /// equivalent per-backend signal). Republishes
    /// `MediaQueryData::platform_brightness` through the root media query.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    AppearanceChanged(flui_platform::WindowAppearance),
    /// The pointer entered (`true`) or left (`false`) the window (winit's
    /// `CursorEntered`/`CursorLeft`, via
    /// `PlatformWindow::on_hover_status_change`). Leave sweeps the addressed
    /// presentation's hover state — `MouseRegion::on_exit` fires and the
    /// cursor resets; without this a widget hovered at the moment the cursor
    /// crosses the window edge keeps its hover visuals forever. Enter is a
    /// no-op today: the next `CursorMoved` re-primes hover from a fresh hit
    /// test on its own.
    // Constructed by both the desktop and web bootstraps' registrations, so
    // every backend whose `PlatformWindow` dispatches
    // `on_hover_status_change` routes here; a backend that never fires the
    // callback simply never constructs the event.
    WindowHover(bool),
    /// Drive a lifecycle target that requires owner-local realm cleanup (most
    /// notably Detached during platform shutdown).
    Lifecycle(AppLifecycleState),
}

/// One queued unit of owner-thread work: a typed cross-thread
/// [`PlatformToUi`] event, the co-located frame pump, or a request to close
/// one presentation out of this realm's forest. `Frame` and
/// `ClosePresentation` are both deliberately KEPT OUT of the cross-thread
/// [`PlatformToUi`] vocabulary above, but for two different reasons — and
/// this enum, `RealmTask` itself, never crosses a thread either way: it is
/// owner-thread-only end to end (`AppRuntime`'s per-realm queue lives in
/// owner-thread-only `RealmSlot` storage, drained only from
/// [`dispatch_platform_realm`] on that same thread), the same as it was
/// before `ClosePresentation` existed — `Frame`'s `Box<dyn FnOnce(&UiRealm)>`
/// alone already makes the enum `!Send` in the general case, so nothing
/// about adding `ClosePresentation` changes that.
///
/// `Frame` carries an owner-local closure, which a cross-thread payload must
/// never do (ADR-0037 §3 forbids `Box<dyn FnOnce()>` on that boundary) — its
/// exclusion is load-bearing today. `ClosePresentation`'s payload alone
/// (`PresentationId`, a plain `Copy` id) happens to satisfy `Send` in
/// isolation, same as [`PlatformToUi`]'s own fields do — but that is a
/// property of the ID type, not a claim about this enum or this variant:
/// `ClosePresentation` is excluded from `PlatformToUi` because
/// [`dispatch_platform_realm`]'s own drain loop must special-case it (see
/// below), not because its payload could not cross a thread if some later
/// slice needed that.
///
/// `ClosePresentation` is handled specially by [`dispatch_platform_realm`]'s
/// own drain loop, never by [`RealmTask::run`]: closing a presentation needs
/// `&mut UiRealm` (removing it from the forest), which only exists for the
/// brief window the realm sits checked out of `APP_RUNTIME` as an owned
/// local — exactly the window the drain loop already has open, and the
/// reason this variant exists instead of giving `PresentationForest`
/// interior mutability to reach the same `&mut` from behind `run`'s shared
/// `&UiRealm` receiver.
// `pub(in crate::app)` (rather than private) so `AppRuntime`'s `queue` field, defined
// in the sibling `runtime` module, can name this type.
pub(in crate::app) enum RealmTask {
    Event(PlatformToUi),
    Frame(Box<dyn FnOnce(&crate::app::ui_realm::UiRealm)>),
    ClosePresentation(flui_foundation::PresentationId),
}

impl RealmTask {
    /// Runs an `Event`/`Frame` task against the realm's shared capabilities.
    ///
    /// `presentation_id` is the [`flui_foundation::PresentationId`] the
    /// enqueueing [`RealmDispatcher`] was addressed to — stamped onto this
    /// task's queue entry at enqueue time (`dispatch_platform_realm`), since
    /// a realm's queue is shared across every presentation it hosts (issue
    /// #555 the addressed-routing slice). `Self::Frame`'s closure ignores it (the
    /// frame pump is realm-wide, not presentation-addressed); only
    /// `Self::Event` threads it through to [`PlatformToUi::run`].
    ///
    /// # Panics
    /// Panics if called with `Self::ClosePresentation` — that variant never
    /// reaches this method: [`dispatch_platform_realm`]'s drain loop matches
    /// it out before calling `run`, since it needs `&mut UiRealm` instead of
    /// this method's `&UiRealm` receiver. Not reachable from any other
    /// caller — `run` has exactly one call site.
    fn run(
        self,
        realm: &crate::app::ui_realm::UiRealm,
        presentation_id: flui_foundation::PresentationId,
    ) {
        match self {
            Self::Event(event) => event.run(realm, presentation_id),
            Self::Frame(run) => run(realm),
            Self::ClosePresentation(id) => unreachable!(
                "BUG: RealmTask::ClosePresentation({id:?}) reached RealmTask::run -- \
                 dispatch_platform_realm's drain loop must match this variant out before \
                 calling run, so it can call close_presentation_entered with &mut UiRealm \
                 instead"
            ),
        }
    }
}

impl PlatformToUi {
    /// `presentation_id` is the exact presentation this event was stamped
    /// for at enqueue time (see [`RealmTask::run`]'s doc). `Input` delivers
    /// to it through [`crate::app::ui_realm::UiRealm::handle_input_addressed`];
    /// every other variant (`Resized`/`WindowFocus`/`WindowVisibility`/
    /// `Lifecycle`) is still realm-wide, not yet per-presentation-addressed
    /// — a stated, named gap (not silent): resize/lifecycle addressing
    /// across a genuine N>1 forest is future work, out of this hop-2 slice's
    /// bounded scope (input/IME/semantics/keyboard/redraw), except
    /// `WindowFocus(true)`, which DOES use `presentation_id` to update
    /// [`crate::app::ui_realm::UiRealm::notify_presentation_focus_gained`] — see
    /// that arm below.
    fn run(
        self,
        realm: &crate::app::ui_realm::UiRealm,
        presentation_id: flui_foundation::PresentationId,
    ) {
        match self {
            Self::Input(input) => realm.handle_input_addressed(presentation_id, input),
            Self::Resized { size, scale_factor } => {
                // Take the applier out of THIS realm's slot, release the
                // borrow, call it, then restore it — never call through a
                // live borrow, so a reentrant TLS access from inside the
                // applier (e.g. a nested dispatch enqueuing further work)
                // cannot hit an already-mutably-borrowed `RefCell` panic. If
                // the slot is ever found empty here (no applier installed
                // yet, or already cleared by teardown) this skips with a
                // trace instead of unwrapping/panicking; surface application
                // then coalesces onto the next real applier install.
                let realm_id = realm.realm_id();
                let applier = APP_RUNTIME.with(|slot| {
                    slot.borrow_mut()
                        .realms
                        .get_mut(&realm_id)
                        .and_then(|realm_slot| realm_slot.surface_applier.take())
                });
                match applier {
                    Some(applier) => {
                        // The guard restores the applier on drop
                        // unconditionally — including if `call` below
                        // panics and the drop runs during unwind — so a
                        // caught panic in the applier never permanently
                        // strands resizing.
                        let mut guard = SurfaceApplierRestoreGuard {
                            realm_id,
                            applier: Some(applier),
                        };
                        guard.call(size, scale_factor);
                    }
                    None => {
                        tracing::debug!(
                            "realm resize: surface applier slot is empty; surface application \
                             coalesces onto the next real applier install"
                        );
                    }
                }
                realm.set_device_pixel_ratio(scale_factor);
                // Addressed write: dropped when the presentation this resize
                // was stamped for is gone by delivery time — see
                // `UiRealm::media_query_for`. Everything else in this arm
                // (surface applier, device pixel ratio, redraw) is realm-wide
                // and runs either way.
                if let Some(source) = realm.media_query_for(presentation_id) {
                    source.update(|data| {
                        data.size = size;
                        data.device_pixel_ratio = scale_factor;
                    });
                } else {
                    tracing::debug!(
                        ?presentation_id,
                        "realm resize: addressed presentation is gone; no media query to resize"
                    );
                }
                realm.request_redraw();
                tracing::trace!(?size, scale_factor, "realm resize committed");
            }
            Self::SafeAreaChanged(insets) => {
                if let Some(source) = realm.media_query_for(presentation_id) {
                    source.update(|data| data.padding = insets);
                } else {
                    tracing::debug!(
                        ?presentation_id,
                        "safe-area report: addressed presentation is gone; no media query to pad"
                    );
                }
                realm.request_redraw();
            }
            Self::WindowFocus(focused) => realm.update_window_focus(presentation_id, focused),
            Self::WindowExecution(state) => realm.update_window_execution(presentation_id, state),
            Self::WindowHover(inside) => {
                realm.handle_window_hover_addressed(presentation_id, inside);
            }
            Self::AppearanceChanged(appearance) => {
                use flui_platform::WindowAppearance;
                let brightness = match appearance {
                    WindowAppearance::Dark | WindowAppearance::VibrantDark => {
                        flui_types::platform::Brightness::Dark
                    }
                    WindowAppearance::Light | WindowAppearance::VibrantLight => {
                        flui_types::platform::Brightness::Light
                    }
                };
                if let Some(source) = realm.media_query_for(presentation_id) {
                    source.update(|data| {
                        data.platform_brightness = brightness;
                    });
                } else {
                    tracing::debug!(
                        ?presentation_id,
                        "appearance change: addressed presentation is gone; no media query to \
                         brighten"
                    );
                }
                realm.request_redraw();
            }
            Self::WindowVisibility(visible) => {
                realm.update_window_visibility(presentation_id, visible);
            }
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            Self::SynchronizeLifecycle => realm.synchronize_window_lifecycle(),
            #[cfg(any(target_os = "ios", target_os = "android", target_arch = "wasm32"))]
            Self::Shutdown => realm.stop_presentations(),
            Self::Lifecycle(new) => {
                #[cfg(all(
                    not(target_os = "android"),
                    not(target_os = "ios"),
                    not(target_arch = "wasm32")
                ))]
                APP_RUNTIME.with(|slot| slot.borrow_mut().main_host_lifecycle = new);
                realm.update_host_lifecycle(new);
            }
        }
    }
}

/// Installs `applier` as `realm_id`'s registration-lifetime renderer-surface
/// applier, replacing (never stacking) any previously-installed one for that
/// SAME realm — a sibling realm's own applier is untouched. Call once per
/// realm install, alongside `install_platform_realm` (Android/web; the desktop
/// and iOS bootstraps use [`install_realm_alongside`]), from each backend's
/// bootstrap — never from inside a frame/event dispatch.
///
/// `realm_id` not being resident here is always a caller bug, never a
/// legitimate race: this is called synchronously, immediately alongside the
/// realm's own install, so the slot must already exist. Loud rather than a
/// silent no-op, because the failure mode otherwise is silent forever — that
/// realm's `Resized` events would coalesce onto a `None` applier for its
/// entire lifetime with nothing ever pointing at why.
pub(super) fn install_surface_applier(
    realm_id: RealmId,
    applier: impl FnMut(flui_types::Size<flui_types::geometry::Pixels>, f32) + 'static,
) {
    APP_RUNTIME.with(|slot| {
        if let Some(realm_slot) = slot.borrow_mut().realms.get_mut(&realm_id) {
            realm_slot.surface_applier = Some(Box::new(applier));
        } else {
            debug_assert!(
                false,
                "BUG: install_surface_applier called for realm {realm_id:?}, which is not \
                 resident in the registry -- call this once, synchronously, immediately \
                 alongside install_platform_realm/install_realm_alongside, never after"
            );
            tracing::error!(
                ?realm_id,
                "install_surface_applier: realm not found in the registry -- this realm's \
                 resize handling is silently lost for its whole lifetime"
            );
        }
    });
}

/// Installs `realm` as the SOLE hosted realm on this thread, minting its
/// dispatcher's address by registering `window` in the single
/// [`crate::app::window_registry::WindowRegistry`] authority — the registry is
/// the sole mint path for a routable [`flui_foundation::PresentationAddress`];
/// no caller of this function ever names the platform-internal native-handle
/// key type itself.
///
/// This is the legacy single-primary-realm entry point every backend's
/// bootstrap still calls exactly once: it clears the ENTIRE realm registry
/// (every hosted realm, not only one) before inserting the fresh one —
/// exactly the behavior the single `Option<UiRealm>` slot this registry
/// replaces used to have, since that slot could only ever hold one realm at
/// all. A second, non-displacing realm (a genuinely independent window
/// alongside this one) is installed through
/// [`install_realm_alongside`] instead.
#[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
pub(super) fn install_platform_realm(
    realm: crate::app::ui_realm::UiRealm,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> RealmDispatcher {
    let owner_thread = std::thread::current().id();
    let address = flui_foundation::PresentationAddress {
        realm_id: realm.realm_id(),
        presentation_id: realm.presentation_id(),
    };
    let displaced = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Every realm hosted here may already be installed — a reinstall
        // without an intervening `teardown_platform_realm` (the
        // panic-recovery path: a mid-`on_ready` failure leaves the old
        // registry/queue/applier/window-registry mappings in place, and
        // bootstrap tries again on the same thread). Remove every registry
        // mapping addressed to EACH displaced realm — not just the window
        // being installed now — in this same borrow, before registering the
        // new window: otherwise a displaced realm's own window(s) survive as
        // dead entries no later teardown ever reaches (this legacy entry
        // point is the only one that clears the whole registry at once).
        let mut removed_window_mappings = 0;
        let displaced = state.realms.clear();
        for (displaced_id, _) in &displaced {
            removed_window_mappings += state.registry.remove_realm(*displaced_id).len();
        }
        state.registry.register_window(window, address);

        if !displaced.is_empty() {
            tracing::warn!(
                displaced_realms = displaced.len(),
                new_address = ?address,
                removed_window_mappings,
                "install_platform_realm: replacing realm(s) that were never torn down"
            );
        }
        state.realms.insert(
            address.realm_id,
            RealmSlot {
                realm: Some(realm),
                queue: VecDeque::new(),
                draining: false,
                address,
                surface_applier: None,
            },
        );
        state.owner_thread = Some(owner_thread);
        // Defensive: a reinstall-without-teardown only reaches this path
        // when the displaced incarnation's own dispatch never restored its
        // slot's `realm` (the panic-recovery scenario this function's doc
        // already documents) — `dispatched_scheduler`/`dispatched_realm_id`,
        // if the displaced incarnation left either stashed, belong to that
        // dead incarnation and must not leak into the fresh one's fence-(c)
        // reads.
        state.dispatched_scheduler = None;
        state.dispatched_realm_id = None;
        // Explicit, known-point resolution: a realm is actually being
        // installed, so this thread genuinely needs `SharedEngineServices`
        // -- unlike `install_owner_platform`, which every backend calls
        // (including `run_direct`, which never installs a realm and never
        // needs these services). Idempotent (`ensure_services` caches), so
        // it does not matter whether a prior realm on this thread already
        // triggered it.
        let _ = state.ensure_services();
        // Same known-point discipline for the loop-scoped execution
        // services (issue #557): resolved here (host-injected if the
        // bootstrap stashed `AppConfig::executors`, default pools
        // otherwise), never ambiently. Cheap — default pools start worker
        // threads on first background spawn, not here.
        let _ = state.ensure_execution();
        // And for the service registry (issue #558): a PRIOR loop's
        // teardown closed its admission; this loop hosting a realm reopens
        // it so config-declared services can start. Running services are
        // untouched — mid-loop reinstalls (hot-restart, panic recovery)
        // find admission already open and their services still owned.
        #[cfg(not(target_arch = "wasm32"))]
        state.reopen_lifecycles();
        displaced
    });
    // Destructors may re-enter platform/framework code (the same invariant
    // `teardown_platform_realm` honors) — drop only after the TLS borrow
    // above has released.
    drop(displaced);
    RealmDispatcher {
        owner_thread,
        address,
    }
}

/// Installs `realm` ALONGSIDE whatever is already hosted, never displacing a
/// sibling — the multi-realm counterpart to `install_platform_realm`'s
/// (Android/web-only, hence not linked)
/// legacy single-primary-realm replace semantics. Requests window
/// registration and the registry insertion TOGETHER, through
/// [`crate::app::runtime::AppRuntime::request_realm_install`] (never registers
/// the window separately/eagerly — see that method's own doc for the gap a
/// two-step sequence would leave open), so an install requested while
/// another realm is checked out for dispatch (a frame callback opening a
/// second window) defers to loop idle instead of installing mid-dispatch.
///
/// `Err(RegistryError::WindowAlreadyMapped)` only when applied immediately
/// (no dispatch/visit in flight) and `window`'s id already maps to a live
/// entry — refused, never silently re-routed onto whichever sibling realm
/// already owns that id (`WindowRegistry::register_window`'s replace
/// semantics is exactly the wrong tool for two realms meant to coexist). A
/// deferred install that later collides is traced and dropped instead (see
/// `AppRuntime::drain_pending_realm_mutations`), since the caller has
/// already returned by the time a deferred mutation applies.
///
/// Production caller: [`open_secondary_window`](super::secondary_window::open_secondary_window) under
/// [`crate::app::runtime::WindowPolicy::SeparateRealms`] — the embedder-facing
/// seam issue #555 adds. Also exercised directly by this module's own
/// tests.
///
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "open_secondary_window (its production caller) is desktop-only -- android/wasm32 \
                  have no caller outside this module's own tests"
    )
)]
pub(super) fn install_realm_alongside(
    realm: crate::app::ui_realm::UiRealm,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> Result<RealmDispatcher, crate::app::window_registry::RegistryError> {
    let owner_thread = std::thread::current().id();
    let address = flui_foundation::PresentationAddress {
        realm_id: realm.realm_id(),
        presentation_id: realm.presentation_id(),
    };
    let window = std::sync::Arc::clone(window);
    let result = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.owner_thread.get_or_insert(owner_thread);
        let _ = state.ensure_services();
        state.request_realm_install(
            address.realm_id,
            RealmSlot {
                realm: Some(realm),
                queue: VecDeque::new(),
                draining: false,
                address,
                surface_applier: None,
            },
            window,
        )
    });
    // On a refused collision, `result` carries the rejected `RealmSlot` (and
    // the `UiRealm` it owns) back out of the TLS borrow above -- dropped
    // only here, after that borrow has released, never inside it (see
    // `AppRuntime::apply_install`'s own doc for why).
    match result {
        Ok(()) => Ok(RealmDispatcher {
            owner_thread,
            address,
        }),
        Err((error, rejected_slot)) => {
            drop(rejected_slot);
            Err(error)
        }
    }
}

/// Errors from [`install_presentation_alongside`]. A dedicated type rather
/// than folding into [`RealmDispatchError`]: none of that enum's variants
/// mean "the realm is fine, but this specific window id collided" —
/// mislabeling that as `RealmUnavailable` (the pre-fix shape) told a caller
/// the wrong thing about what actually went wrong.
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
        reason = "install_presentation_alongside (its only producer) is desktop-only -- \
                  android/wasm32 have no caller outside this module's own tests"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum InstallPresentationError {
    /// `dispatcher`'s realm no longer exists (a newer realm replaced it, or
    /// it was already torn down).
    #[error("the realm this dispatcher was minted for no longer exists")]
    RealmUnavailable,
    /// A dispatch or hot-restart visit is currently in flight on this
    /// thread; see this function's own doc for why that is a named,
    /// stated gap rather than a defer-to-idle path.
    #[error("a dispatch or hot-restart visit is in flight on this thread")]
    DispatchInFlight,
    /// `dispatcher`'s realm is live, but `dispatcher.address` itself is no
    /// longer registered in `WindowRegistry` — the presentation it was
    /// minted for closed since, even though a DIFFERENT presentation kept
    /// the realm alive. The same authorization check
    /// `dispatch_platform_realm` runs (`registry.contains_address`), applied
    /// here too: a caller must hold a dispatcher whose exact address is
    /// CURRENTLY live to authorize installing another presentation
    /// alongside it, not merely one whose realm happens to still exist.
    #[error("the presentation this dispatcher was minted for is no longer registered")]
    StalePresentation,
    /// `window`'s id was already registered to a (possibly different)
    /// address — practically unreachable for a freshly opened window, but
    /// a real, distinct failure mode from `RealmUnavailable`: the realm
    /// itself is perfectly fine.
    #[error("window is already registered: {0}")]
    WindowAlreadyMapped(#[from] crate::app::window_registry::RegistryError),
}

/// Installs another presentation into `dispatcher`'s realm, alongside
/// whatever it already hosts — the addressed-routing counterpart to
/// [`install_realm_alongside`] (which installs a second REALM instead of a
/// second presentation of the SAME realm). This is the production entry
/// point [`flui_runtime::presentation_forest::PresentationForest`]'s doc
/// points to: the forest's former `len()<=1` ratchet lifted (issue #555)
/// specifically so this function has somewhere real to install into.
///
/// `window` becomes the fresh presentation's own native window. Ordering is
/// load-bearing: the presentation is
/// [`assembled`](crate::app::ui_realm::UiRealm::assemble_presentation) but NOT
/// yet installed into the forest, then its `WindowRegistry` mapping is
/// minted, and ONLY on success is it
/// [`installed`](crate::app::ui_realm::UiRealm::install_presentation) — so a
/// registration failure leaves nothing forest-resident to roll back (the
/// assembled-but-uninstalled `PresentationState` is simply dropped), never
/// a presentation the forest holds with no registry entry of its own. This
/// is the single-native-window-map-authority invariant this function
/// exists to uphold: no hosted presentation is ever forest-resident
/// without a mapping.
///
/// # Errors
///
/// [`InstallPresentationError::RealmUnavailable`] if `dispatcher`'s realm no
/// longer exists. [`InstallPresentationError::StalePresentation`] if the
/// realm survives but `dispatcher.address` itself is no longer a live
/// registered address (its own presentation closed, even though a sibling
/// kept the realm alive) — the same authorization
/// `dispatch_platform_realm` requires of every dispatched task, applied to
/// this mutation too. [`InstallPresentationError::WindowAlreadyMapped`] if
/// `window`'s id is somehow already registered (practically unreachable: a
/// freshly opened window has a fresh id by construction) — nothing is
/// installed into the forest in this case.
/// [`InstallPresentationError::DispatchInFlight`] if a dispatch or
/// hot-restart visit is currently in flight on this thread: **named gap,
/// not a silent one** — unlike [`install_realm_alongside`]/
/// [`uninstall_platform_realm`], this path does not yet defer to loop idle
/// through `AppRuntime::pending_realm_mutations`; [`open_secondary_window`](super::secondary_window::open_secondary_window)
/// (its production caller, under
/// [`crate::app::runtime::WindowPolicy::SharedRealm`]) never calls this from
/// inside a dispatched callback, so there is nothing forcing that
/// generalization today. Extending the deferral queue to
/// presentation-install requests is follow-up work, not silently skipped:
/// this refuses loudly (a `debug_assert!` in debug builds) rather than
/// corrupting `AppRuntime` state.
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
        reason = "open_secondary_window (its production caller) is desktop-only -- android/wasm32 \
                  have no caller outside this module's own tests"
    )
)]
pub(super) fn install_presentation_alongside(
    dispatcher: RealmDispatcher,
    window: impl Into<crate::app::presentation::PresentationWindow>,
) -> Result<RealmDispatcher, InstallPresentationError> {
    let presentation_window = window.into();
    let realm_id = dispatcher.address.realm_id;
    let owner_thread = dispatcher.owner_thread;
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.dispatched_realm_id.is_some() || state.iterating_all_realms {
            debug_assert!(
                false,
                "BUG: install_presentation_alongside called while a dispatch/hot-restart visit \
                 is in flight -- this path has no defer-to-idle queue yet (a named, stated gap, \
                 not a silent one); call only from outside any dispatched callback"
            );
            tracing::error!(
                ?realm_id,
                "rejecting install_presentation_alongside while a dispatch is in flight"
            );
            return Err(InstallPresentationError::DispatchInFlight);
        }
        if !state.realms.contains_key(&realm_id) {
            return Err(InstallPresentationError::RealmUnavailable);
        }
        // Authorization check (mirrors `dispatch_platform_realm`'s own):
        // the realm existing is not enough -- `dispatcher.address` itself
        // must still be a LIVE registered address. A realm survives its
        // sole non-primary presentation closing (a sibling keeps it
        // alive), so a caller could otherwise still hold a dispatcher for
        // that now-closed presentation and use it to authorize installing
        // yet another one alongside -- checked BEFORE borrowing
        // `state.realms` mutably below, exactly like `dispatch_platform_
        // realm` checks `state.registry` before `state.realms.get_mut`.
        if !state.registry.contains_address(dispatcher.address) {
            tracing::debug!(
                ?dispatcher,
                "rejecting install_presentation_alongside: the dispatcher's own presentation is \
                 no longer registered, even though its realm survives"
            );
            return Err(InstallPresentationError::StalePresentation);
        }
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence just checked above via contains_key");
        let Some(realm) = realm_slot.realm.as_mut() else {
            return Err(InstallPresentationError::RealmUnavailable);
        };
        // Assemble WITHOUT installing yet -- see this function's own doc
        // for why the ordering matters. `presentation` is dropped (no
        // forest membership, so nothing to roll back) if registration
        // below fails.
        let window = std::sync::Arc::clone(presentation_window.window());
        let presentation = realm.assemble_presentation(presentation_window);
        let address = flui_foundation::PresentationAddress {
            realm_id,
            presentation_id: presentation.id(),
        };
        state.registry.try_register_window(&window, address)?;
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence checked above, and nothing between here and there removed it");
        let realm = realm_slot
            .realm
            .as_mut()
            .expect("BUG: presence checked above, and nothing between here and there took it");
        realm.install_presentation(presentation);
        Ok(RealmDispatcher {
            owner_thread,
            address,
        })
    })
}

/// Uninstalls exactly one realm — tearing down a whole [`WindowPolicy::
/// SeparateRealms`]/`SharedRealm` group at once, unconditionally, regardless
/// of how many presentations it still hosts — without disturbing any other
/// hosted realm. Requests the removal through [`crate::app::runtime::AppRuntime::
/// request_realm_uninstall`], so a request arriving mid-dispatch or
/// mid-hot-restart-visit defers to loop idle instead of mutating the
/// registry another operation is still walking.
///
/// No production embedder call site: an ordinary window closing always goes
/// through [`close_this_window`]/[`close_presentation`] instead, which
/// reduces to exactly this same effect only when the closing presentation is
/// its realm's sole one — calling this directly from a window's own close
/// handler would tear down an entire `SharedRealm` group out from under a
/// still-open sibling window, which is precisely the bug a prior revision of
/// this function's own caller had. Exercised directly by this module's own
/// tests (which construct scenarios `close_this_window` cannot, e.g. forcibly
/// tearing down a realm that still hosts more than one presentation, to pin
/// this function's own "whole group, unconditionally" contract in isolation).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no production embedder call site -- an ordinary window close goes through \
                  close_this_window/close_presentation instead, which reduces to this same \
                  effect only for a realm's sole presentation; exercised by this module's own \
                  tests"
    )
)]
fn uninstall_platform_realm(realm_id: RealmId) {
    let removed = APP_RUNTIME.with(|slot| slot.borrow_mut().request_realm_uninstall(realm_id));
    // Destructors may re-enter platform/framework code — drop only after the
    // TLS borrow above has released.
    drop(removed);
}

/// Requests that one presentation be closed and removed from `dispatcher`'s
/// realm — a single window closing out of a realm that hosts more than one,
/// without tearing down the realm itself (contrast [`uninstall_platform_realm`],
/// which removes a whole realm). Request-shaped, like every other realm-map
/// mutation in this module: this function only enqueues
/// [`RealmTask::ClosePresentation`] and (if the realm is currently idle)
/// drives the drain loop that runs it — it never runs the six teardown
/// steps itself, and never returns anything about their outcome beyond
/// whether the request was accepted for this dispatcher (see
/// [`dispatch_platform_realm`]'s own `Err` variants).
///
/// The six steps run at Idle inside [`crate::app::ui_realm::UiRealm::close_presentation_entered`],
/// which only [`dispatch_platform_realm`]'s own drain loop calls, and only
/// with the realm checked out as an owned local — so a dispose callback the
/// closed presentation runs mid-teardown sees the exact same
/// `dispatched_realm_id` TLS state (and therefore the exact same
/// install/uninstall deferral discipline) any other dispatched frame or
/// event callback sees. Production contract: async-at-Idle only — a caller
/// needing the six steps to have actually run before proceeding must drive
/// the dispatch loop to idle itself (a synchronous bypass would defeat the
/// whole point of routing this through the dispatch seam).
///
/// Production caller: [`close_this_window`] — the single `on_close` wiring
/// point every window this crate opens uses (`run_desktop`'s primary,
/// [`open_secondary_window`](super::secondary_window::open_secondary_window)'s new window under either [`WindowPolicy`](crate::app::runtime::WindowPolicy)).
/// This is the SAME entry point regardless of whether `id` turns out to be
/// its realm's sole presentation (routes to a full realm uninstall above) or
/// one of several (removes just this one, siblings and realm survive) —
/// #555 closes with this slice; there is no further slice deferring this.
/// Also exercised directly by this module's own tests.
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "close_this_window (its one production caller) is desktop-only -- \
                  android/wasm32 have no caller outside this module's own tests"
    )
)]
fn close_presentation(
    dispatcher: RealmDispatcher,
    id: flui_foundation::PresentationId,
) -> Result<(), RealmDispatchError> {
    dispatch_platform_realm(dispatcher, RealmTask::ClosePresentation(id))
}

/// Closes exactly the window `dispatcher` addresses — the single production
/// `on_close` wiring point for every window this crate opens (`run_desktop`'s
/// primary, both [`open_secondary_window`](super::secondary_window::open_secondary_window) policies). Routes through
/// [`close_presentation`], which correctly reduces to a full realm uninstall
/// when `dispatcher`'s presentation is its realm's ONLY one (a
/// [`WindowPolicy::SeparateRealms`](crate::app::runtime::WindowPolicy::SeparateRealms) window, or the last surviving
/// presentation of a [`WindowPolicy::SharedRealm`](crate::app::runtime::WindowPolicy::SharedRealm) group), or removes just
/// that one presentation while its realm and any sibling presentation
/// survive otherwise — never [`uninstall_platform_realm`] directly, which
/// would tear down an ENTIRE `SharedRealm` group out from under a still-open
/// sibling window.
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "its production callers (run_desktop, open_secondary_window) are desktop-only \
                  -- android/wasm32 have no caller outside this module's own tests"
    )
)]
pub(super) fn close_this_window(dispatcher: RealmDispatcher) {
    if let Err(error) = close_presentation(dispatcher, dispatcher.address.presentation_id) {
        tracing::warn!(?dispatcher, ?error, "close_this_window: dispatch refused");
    }
}

pub(super) fn dispatch_platform_realm(
    dispatcher: RealmDispatcher,
    event: RealmTask,
) -> Result<(), RealmDispatchError> {
    if std::thread::current().id() != dispatcher.owner_thread {
        tracing::error!(?dispatcher, "rejecting realm callback on non-owner thread");
        return Err(RealmDispatchError::WrongThread);
    }
    let realm_id = dispatcher.address.realm_id;
    let checked_out = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Normative compare order (ADR-0037): realm first, then
        // presentation. `realm_id`/`presentation_id` mint from one shared
        // counter, so teardown+reinstall always changes both and the realm
        // check fires first on the common path.
        if state.realms.is_empty() {
            tracing::debug!(
                ?dispatcher,
                "dropping realm callback: no realm installed (not yet ready, or already torn down)"
            );
            return Err(RealmDispatchError::RealmUnavailable);
        }
        if !state.realms.contains_key(&realm_id) {
            tracing::debug!(
                ?dispatcher,
                "dropping realm callback: a newer realm replaced the one it was dispatched for"
            );
            return Err(RealmDispatchError::StaleRealm);
        }
        // Presentation check (issue #555's addressed-routing slice): membership in the SAME
        // `WindowRegistry` authority `dispatch_platform_realm`'s own
        // production window callbacks are resolved through, not equality
        // against `realm_slot.address` (which tracks only ONE presentation —
        // this realm's primary). A realm hosting N presentations has N live
        // addresses at once; a dispatcher naming any one of them, as long as
        // its exact address is still registered, is live -- one closed
        // (unregistered) since the dispatcher was minted, or a forged/mixed
        // address, is `StalePresentation` either way. Checked BEFORE
        // borrowing `realm_slot` mutably below (the registry is a sibling
        // field on the same `state`, so an immutable read here and a mutable
        // `realms` borrow next cannot overlap).
        if !state.registry.contains_address(dispatcher.address) {
            tracing::debug!(
                ?dispatcher,
                "dropping realm callback: presentation incarnation mismatch within the live realm"
            );
            return Err(RealmDispatchError::StalePresentation);
        }
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence just checked above via contains_key");
        // Same-realm reentrancy: this realm is already draining (mid its
        // own drain loop below) or checked out (by its own dispatch, or by
        // a `for_each_installed_realm` visit) -- always safe to enqueue and
        // return early; the ongoing drain loop, or the next legitimate
        // dispatch once the realm is restored, picks the event up.
        if realm_slot.draining || realm_slot.realm.is_none() {
            realm_slot
                .queue
                .push_back((dispatcher.address.presentation_id, event));
            return Ok(None);
        }
        // Nested cross-realm dispatch guard (issue #555), checked BEFORE
        // enqueuing `event` anywhere: reaching here means THIS realm is
        // neither draining nor checked out (both ruled out just above), so
        // a `dispatched_realm_id` naming a DIFFERENT realm can only mean a
        // nested dispatch was attempted while that other realm's task is
        // still running on this same thread — the one case
        // `request_realm_install`/`request_realm_uninstall`'s defer-to-idle
        // discipline exists to make structurally unreachable in production.
        // Checking this BEFORE the enqueue below is load-bearing, not
        // cosmetic: enqueuing `event` first and rejecting after would leave
        // it stuck in this realm's queue forever (nothing else ever removes
        // a rejected event), silently delivered to the next LEGITIMATE
        // dispatch instead of the genuine rejection this error reports.
        // Caught loudly in debug builds; release builds still refuse rather
        // than nest.
        if let Some(dispatched_realm_id) = state.dispatched_realm_id {
            debug_assert!(
                false,
                "BUG: nested cross-realm dispatch: realm {realm_id:?} dispatched while realm \
                 {dispatched_realm_id:?} is still checked out on this thread -- installs/\
                 uninstalls must defer to loop idle instead of nesting"
            );
            tracing::error!(
                ?realm_id,
                ?dispatched_realm_id,
                "rejecting nested cross-realm dispatch"
            );
            return Err(RealmDispatchError::NestedCrossRealmDispatchRejected);
        }
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence checked above");
        realm_slot
            .queue
            .push_back((dispatcher.address.presentation_id, event));
        let first = realm_slot
            .queue
            .pop_front()
            .expect("BUG: event was enqueued before starting realm dispatch");
        realm_slot.draining = true;
        let realm = realm_slot.realm.take();
        // Stash a clone of the checked-out realm's scheduler (and its
        // identity) BEFORE it leaves this slot: `installed_realm_phase`
        // (with_owner_platform's fence (c)) reads `dispatched_scheduler` as
        // its fallback whenever no OTHER realm's dispatch is in flight, which
        // is exactly the state this call is about to create for the entire
        // duration of the dispatched task below — otherwise the fence goes
        // blind for every real production frame, not merely when no realm is
        // installed at all. `UpdateScheduler::clone` is one `Arc::clone` (see
        // `flui-scheduler`'s single-`Arc` handle shape), not a second
        // scheduler.
        state.dispatched_scheduler = realm.as_ref().map(|realm| realm.scheduler().clone());
        state.dispatched_realm_id = Some(realm_id);
        Ok(realm.map(|realm| (realm, first)))
    })?;
    let Some((mut realm, first)) = checked_out else {
        return Ok(());
    };

    // Never hold the TLS RefCell borrow across user/platform callbacks. Catch
    // only to restore the host invariants; the original panic is resumed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut next = Some(first);
        while let Some((task_presentation_id, event)) = next {
            // `ClosePresentation` is matched out here, before it would
            // otherwise reach `RealmTask::run`'s `&UiRealm` receiver: closing
            // a presentation removes it from the forest (`&mut UiRealm`),
            // which is only available in this exact window -- `realm` sits
            // here as an owned local, checked out of `APP_RUNTIME` for the
            // whole dispatched task, so `&mut` falls out naturally instead of
            // needing interior mutability on the forest itself.
            match event {
                RealmTask::ClosePresentation(id) => {
                    if realm.is_sole_presentation(id) {
                        // Reentrant events must fail admission before terminal observers run.
                        APP_RUNTIME.with(|slot| slot.borrow_mut().registry.remove_realm(realm_id));
                        APP_RUNTIME
                            .with(|slot| slot.borrow().close_requests())
                            .forget_realm(realm_id);
                        // Closing the realm's ONLY presentation IS closing
                        // the realm. Dispatch Detached FIRST, through this
                        // exact realm, before requesting the uninstall --
                        // shutdown must cancel any in-flight pointer
                        // sequence whose platform Up/Cancel will never
                        // arrive, and must notify lifecycle observers,
                        // before the realm and its `UpdateScheduler` are gone.
                        // This is the same reason `on_quit`'s own Detached
                        // dispatch exists (`run_desktop`'s bootstrap),
                        // generalized to per-realm teardown instead of only
                        // process-wide quit: a realm closing because its one
                        // window closed is exactly as "detached" as one
                        // closing because the whole process quit, and
                        // `on_quit`'s own dispatch now frequently finds this
                        // realm already gone (a harmless, traced no-op --
                        // see that callback's doc). Uses `PlatformToUi::
                        // Lifecycle(..).run` directly (the same private
                        // helper an ordinary `RealmTask::Event(PlatformToUi::
                        // Lifecycle(..))` dispatches through below) rather
                        // than re-queuing another task, since `realm` is
                        // already the exact owned local that method needs.
                        let notification =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                realm.stop_presentations();
                            }));

                        // Closing the realm's ONLY presentation IS closing
                        // the realm -- routing it through
                        // close_presentation_entered would leave an empty
                        // forest behind (every other method on this realm,
                        // starting with `primary()`, assumes one always
                        // exists). Request a full realm uninstall through
                        // the SAME deferral machinery every other
                        // realm-map mutation already uses:
                        // `dispatched_realm_id` is `Some` for this whole
                        // checkout, so this defers to `dispatch_platform_
                        // realm`'s own tail (below), which runs
                        // `apply_uninstall` only after `realm` is restored
                        // to its slot -- `apply_uninstall` already removes
                        // every one of this realm's `WindowRegistry`
                        // entries before dropping the `RealmSlot` (see its
                        // own doc), so step 1 is covered by the SAME path
                        // a whole-realm close always used.
                        let displaced = APP_RUNTIME
                            .with(|slot| slot.borrow_mut().request_realm_uninstall(realm_id));
                        drop(displaced);
                        if let Err(payload) = notification {
                            std::panic::resume_unwind(payload);
                        }
                    } else {
                        // Step 1: unregister exactly THIS presentation's own
                        // window mapping -- never a sibling's, and never
                        // every window this realm owns (`WindowRegistry::
                        // remove_realm` would be wrong here). `UiRealm`
                        // itself has no access to the registry (AppRuntime
                        // is its single authority, ADR-0037 §2), so this
                        // runs here, in the one caller that does, before
                        // handing off to close_presentation_entered's
                        // steps 2-6. Without this, a stale platform event
                        // for the closed presentation's original window
                        // would still resolve to its now-dead
                        // PresentationId instead of being refused.
                        let address = flui_foundation::PresentationAddress {
                            realm_id,
                            presentation_id: id,
                        };
                        let unregistered = APP_RUNTIME
                            .with(|slot| slot.borrow_mut().registry.remove_presentation(address));
                        drop(unregistered);
                        // Same step for the close-request router (issue
                        // #558): this presentation can no longer be asked
                        // about, nor closed programmatically, once its
                        // routable address is gone.
                        APP_RUNTIME
                            .with(|slot| slot.borrow().close_requests())
                            .forget(address);

                        // Re-stamp this realm's tracked routable address
                        // (`RealmSlot::address`) to the surviving primary
                        // BEFORE running `id`'s own teardown/dispose hooks
                        // -- not after. `close_presentation_entered`'s step
                        // 2-3 (below) can run a dispose hook that re-enters
                        // this exact function with a dispatcher still
                        // bearing `id`; ordering the re-stamp first means
                        // that reentrant dispatch's `StalePresentation`
                        // check (at the top of this function) already
                        // compares against the NEW primary and correctly
                        // refuses it right there. Re-stamping AFTER
                        // disposal instead would leave a window where that
                        // same reentrant dispatch still compares equal
                        // (both sides still `id`), gets ACCEPTED by the
                        // stale check, and is merely enqueued behind the
                        // same-realm reentrancy guard -- only to run a
                        // moment later, in this very drain loop, against
                        // whatever survives the close: silently
                        // misaddressed rather than refused. See
                        // `UiRealm::primary_id_excluding`'s own doc for why
                        // this is computable before the removal happens.
                        // Kept as an `if let` rather than an unwrap: this
                        // branch runs only when `is_sole_presentation(id)` was
                        // `false` above, which rules out a forest holding just
                        // `id` but not an EMPTY one — `is_sole_presentation`
                        // is `false` for a forest of none as well. A `None`
                        // here is therefore not reachable for a realm that
                        // still hosts something, and the arm simply skips the
                        // re-stamp for one that does not.
                        if let Some(surviving_primary_id) = realm.primary_id_excluding(id) {
                            APP_RUNTIME.with(|slot| {
                                if let Some(realm_slot) =
                                    slot.borrow_mut().realms.get_mut(&realm_id)
                                {
                                    realm_slot.address.presentation_id = surviving_primary_id;
                                }
                            });
                        }

                        realm.close_presentation_entered(id);
                    }
                }
                other => realm.enter(|realm| other.run(realm, task_presentation_id)),
            }
            next = APP_RUNTIME.with(|slot| {
                slot.borrow_mut()
                    .realms
                    .get_mut(&realm_id)
                    .and_then(|realm_slot| realm_slot.queue.pop_front())
            });
        }
    }));
    let removed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // The slot may be gone entirely if a NESTED, non-dispatch teardown
        // (e.g. `teardown_platform_realm`'s full-registry clear, called
        // reentrantly from inside the just-run task) already removed it —
        // that dropped this realm's queue/applier already, so there is
        // nothing left to restore; just let `realm` fall out of scope below.
        if let Some(realm_slot) = state.realms.get_mut(&realm_id) {
            realm_slot.realm = Some(realm);
            realm_slot.draining = false;
        }
        // Cleared unconditionally in this same restore block, which runs
        // whether or not the dispatched task above panicked (the panic, if
        // any, is only resumed after this restore completes below) — the
        // fence-(c) fallback must not survive past the dispatch it was
        // stashed for.
        state.dispatched_scheduler = None;
        state.dispatched_realm_id = None;
        // Applies any realm-map mutation this realm's own task requested
        // (e.g. a dispose callback uninstalling itself, or a frame callback
        // installing a second realm) — deferred above precisely because
        // `dispatched_realm_id` was `Some` for the whole task, and safe to
        // apply now that it is cleared.
        let removed = state.drain_pending_realm_mutations();
        // A realm that leaves the map HERE left it after the exit-policy hook
        // had already been consulted and answered "don't exit".
        //
        // That is reachable, and was a tracked gap: a window closed
        // REENTRANTLY from inside a dispatched callback requests its own
        // realm's uninstall, which — being a same-realm dispatch — only
        // enqueues. `window.close()`'s `notify_closed` then consults the hook
        // while the registry is still non-empty, gets a veto, and returns;
        // the uninstall applies moments later, right above, with nothing left
        // to re-ask. The exit was missed, not merely delayed.
        //
        // So re-ask, through the platform's own coalesced owner-thread
        // request rather than any new machinery.
        //
        // Armed whenever the drain yielded a slot, which is NOT the same as
        // "every mutation": a successful `Install` yields none and arms
        // nothing, while `removed` carries both an applied `Uninstall`'s slot
        // AND a REJECTED install's — `drain_pending_realm_mutations` routes a
        // window-id collision through the same bucket. Only the first
        // actually shrinks the realm map; the second is a spurious arm on an
        // already-logged error path. Left that way deliberately: the seam's
        // own contract makes a spurious request a no-op — windows still open,
        // or a hook still vetoing, decide nothing — so paying for it is
        // cheaper than teaching the drain to report which kind it applied.
        //
        // Cloned out here and fired below, outside this borrow: the hook
        // borrows `APP_RUNTIME` itself.
        #[cfg(not(target_arch = "wasm32"))]
        let reevaluate_exit = (!removed.is_empty())
            .then(|| state.exit_policy_reevaluation_notifier())
            .flatten();
        #[cfg(target_arch = "wasm32")]
        let reevaluate_exit: Option<std::sync::Arc<dyn Fn() + Send + Sync>> = None;
        (removed, reevaluate_exit)
    });
    let (removed, reevaluate_exit) = removed;
    // Destructors may re-enter platform/framework code — drop only after the
    // TLS borrow above has released.
    let mut first_panic = result.err();
    drop_removed_realms(removed, &mut first_panic);
    // Fired after the borrow AND after those destructors: the hook this wakes
    // borrows `APP_RUNTIME`, and a realm dropped by `removed` must be gone
    // before the policy is asked whether anything is left.
    if let Some(reevaluate) = reevaluate_exit {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reevaluate())).err();
        preserve_first_lifecycle_panic(&mut first_panic, failure, "exit policy reevaluation");
    }
    // Applies any `open_secondary_window` Pending-arm completion this
    // realm's own task resolved (see `drain_pending_secondary_window_
    // completions`'s own doc for why this must run only now, after the
    // checkout state above is cleared) — same "drain regardless of panic"
    // discipline as `drain_pending_realm_mutations` above, for the same
    // reason: a resolved secondary window should not sit unwired forever
    // just because this dispatch's own task panicked for an unrelated
    // reason. This function itself (`dispatch_platform_realm`) compiles on
    // every backend (only `not(target_os = "ios")`), but the drain helper
    // and everything it touches exist only on the desktop-only
    // `open_secondary_window` family's own cfg -- gate the call site to
    // match, not the function.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        let notification =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(drain_quit_notification)).err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            notification,
            "deferred quit notification",
        );
        let completions = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            drain_pending_secondary_window_completions,
        ))
        .err();
        preserve_first_lifecycle_panic(&mut first_panic, completions, "secondary completion drain");
        let main = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            super::main_window::drive_main_window,
        ))
        .err();
        preserve_first_lifecycle_panic(&mut first_panic, main, "main window drain");
    }
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
    Ok(())
}

/// Release each removed realm independently: one user destructor must not
/// unwind through another removed realm or skip the deferred quit notification.
fn drop_removed_realms(
    removed: Vec<RealmSlot>,
    first_panic: &mut Option<Box<dyn std::any::Any + Send>>,
) {
    for realm in removed {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(realm))).err();
        preserve_first_lifecycle_panic(first_panic, failure, "removed realm cleanup");
    }
}

/// Hot-restart's own iteration primitive (issue #555): visits every
/// currently-installed realm, in mount order, running `f` against each one
/// OUTSIDE the `APP_RUNTIME` borrow — the same checkout/restore discipline
/// [`dispatch_platform_realm`] uses for a single addressed realm.
///
/// What `f` may safely do, precisely (not "any `APP_RUNTIME`-touching
/// function" — the visited realm counts as dispatched, see below, so the
/// same restrictions a dispatched task's own closure has apply here too):
/// dispatch back to the SAME realm being visited (enqueues via the ordinary
/// same-realm reentrant path, never recurses); request a realm-map
/// install/uninstall (defers rather than applies immediately — see
/// [`crate::app::runtime::AppRuntime::request_realm_install`]'s own doc — applied
/// only once every realm has been visited, so the set of realms visited
/// never shifts mid-iteration); or call [`crate::app::runtime::AppRuntime::should_exit`]
/// (also defers its own drain while this visit is in flight, for the same
/// reason). Dispatching to a DIFFERENT, sibling realm is NOT safe: it now
/// trips `dispatch_platform_realm`'s nested-cross-realm-dispatch guard (see
/// the next paragraph), a deliberate behavior change, not an oversight.
///
/// Each visited realm is ALSO stashed into `dispatched_scheduler`/
/// `dispatched_realm_id` for the duration of its own call to `f` — the same
/// fields `dispatch_platform_realm` stashes for a dispatched task — so
/// `with_owner_platform`'s fence (c) stays able to see the visited realm's
/// own scheduler phase for the whole time it sits checked out of `realms`,
/// exactly as it does for a real dispatch. A side effect of that stash is
/// exactly the restriction stated above: `dispatch_platform_realm` treats a
/// visited realm as "checked out" indistinguishably from a dispatched one,
/// so a nested dispatch to a sibling realm from inside `f` is rejected by
/// the same nested-cross-realm-dispatch guard a nested dispatch from inside
/// another dispatch would be.
///
/// A panic inside `f` is caught, not propagated past this function's own
/// cleanup: the visited realm is restored to its slot, `iterating_all_realms`
/// is cleared, and any mutation deferred during the visit (including one
/// requested by the very call that panicked) is drained, all BEFORE the
/// panic resumes. Skipping any of that on a panicking visit would strand the
/// visited realm's slot at `realm: None` forever and wedge
/// `iterating_all_realms` at `true` forever — which, after
/// `drain_pending_realm_mutations`'s own guard against draining while a
/// visit is nominally still in flight, would silently defer every future
/// realm-map mutation for the rest of the process.
///
/// Quit notification reuses this checkout discipline. Its visitor catches each
/// realm's lifecycle panic so all siblings still receive the notification.
#[cfg(any(
    test,
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    )
))]
fn for_each_installed_realm(mut f: impl FnMut(&crate::app::ui_realm::UiRealm)) {
    let ids = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        debug_assert!(
            !state.iterating_all_realms,
            "BUG: reentrant for_each_installed_realm"
        );
        state.iterating_all_realms = true;
        state.realms.keys()
    });

    let mut panic_payload = None;
    for id in ids {
        let realm = APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm = state
                .realms
                .get_mut(&id)
                .and_then(|realm_slot| realm_slot.realm.take())?;
            // Mirrors `dispatch_platform_realm`'s own checkout stash: while
            // `realm` sits outside `realms` for the extent of `f` below,
            // fence (c) must still be able to read ITS phase, not go blind.
            state.dispatched_scheduler = Some(realm.scheduler().clone());
            state.dispatched_realm_id = Some(id);
            Some(realm)
        });
        let Some(realm) = realm else {
            // Removed by an earlier realm's own visit before this iteration
            // reached it -- unreachable under the defer-to-idle discipline
            // (a mutation requested mid-visit only applies AFTER the whole
            // visit completes), kept as a defensive skip rather than an
            // `expect`.
            continue;
        };

        // Never hold the TLS RefCell borrow across `f` -- the same
        // discipline `dispatch_platform_realm` follows for a dispatched
        // task. Catch only to restore the checked-out realm and this
        // visit's own bookkeeping; the original panic resumes once every
        // realm has been handled (visited, or left untouched after a
        // panic stops the walk).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&realm)));
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            if let Some(realm_slot) = state.realms.get_mut(&id) {
                realm_slot.realm = Some(realm);
            }
            state.dispatched_scheduler = None;
            state.dispatched_realm_id = None;
        });
        if let Err(payload) = result {
            panic_payload = Some(payload);
            break;
        }
    }

    let removed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.iterating_all_realms = false;
        state.drain_pending_realm_mutations()
    });
    drop_removed_realms(removed, &mut panic_payload);
    // Same rationale as `dispatch_platform_realm`'s own tail: a visited
    // realm's frame callback may have resolved an `open_secondary_window`
    // Pending completion via `UpdateScheduler::drive_async_tasks`, which cannot
    // complete mid-visit for the same reason it cannot complete
    // mid-dispatch (`iterating_all_realms` holds this thread's checkout
    // state just as `dispatched_realm_id` does). The visitor is available
    // in desktop builds and tests; secondary completion is desktop-only.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        let notification =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(drain_quit_notification)).err();
        preserve_first_lifecycle_panic(
            &mut panic_payload,
            notification,
            "visited quit notification",
        );
        let completions = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            drain_pending_secondary_window_completions,
        ))
        .err();
        preserve_first_lifecycle_panic(
            &mut panic_payload,
            completions,
            "visited secondary completion drain",
        );
    }

    if let Some(payload) = panic_payload {
        std::panic::resume_unwind(payload);
    }
}

/// Close admission now; notify once all currently checked-out realm state returns.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn request_quit_notification() {
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.quit_notification == crate::app::runtime::QuitNotification::Active {
            state.quit_notification = crate::app::runtime::QuitNotification::Requested;
        }
    });
    drain_quit_notification();
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn drain_quit_notification() {
    use crate::app::runtime::QuitNotification;
    let ready = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.quit_notification != QuitNotification::Requested
            || state.dispatched_realm_id.is_some()
            || state.iterating_all_realms
        {
            return false;
        }
        state.quit_notification = QuitNotification::Notifying;
        true
    });
    if !ready {
        return;
    }
    let mut first_panic = None;
    let cancel = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        super::secondary_window::cancel_pending_secondary_windows,
    ))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, cancel, "pending window cancellation");
    let visit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for_each_installed_realm(|realm| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                realm.enter(|realm| {
                    realm.stop_presentations();
                });
            }))
            .err();
            preserve_first_lifecycle_panic(&mut first_panic, result, "realm quit notification");
        });
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, visit, "quit visitor cleanup");
    APP_RUNTIME.with(|slot| slot.borrow_mut().quit_notification = QuitNotification::Notified);
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

/// Drains the per-frame owner-inbox commands and reports whether the drain
/// itself asked for a redraw.
///
/// Every platform's frame callback must call this exactly once per wake, at
/// the Idle frame boundary — before the dirty gate, and before any
/// early-return fast path a platform's frame callback takes (e.g. Android's
/// hot-reload plugin scene) — never inside the frame transaction below.
/// Running it unconditionally on every wake is what keeps
/// `UiCommandSender`'s bounded inbox draining: a wake that skips the drain
/// lets the inbox fill until it hard-errors, and a coalesced redraw request
/// that nothing consumes never wakes the loop again (`take_redraw_request`
/// only flips back to `false` once observed here).
pub(super) fn drain_owner_inbox(realm: &crate::app::ui_realm::UiRealm) -> bool {
    let report = realm.drain_commands();
    if report != crate::app::ui_realm::DrainReport::default() {
        tracing::trace!(?report, "owner inbox drained");
    }
    realm.take_redraw_request()
}

/// Per-pool grace deadline for joining running background work at full
/// loop-exit teardown. Bounds a hung compute job's ability to wedge process
/// exit; running work that finishes sooner ends shutdown sooner (the
/// deadline is a cap, not a sleep).
#[cfg(not(target_arch = "wasm32"))]
const EXECUTION_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Deadline for the staged SERVICE shutdown (issue #558) that runs just
/// before the pools close: every service is cancelled first, then joined
/// against this one shared deadline — the bounded flush window in which a
/// service writes its final state. A service that ignores cancellation is
/// reported (`DeadlineExceeded`) and force-abandoned by the pool shutdown
/// that follows; it cannot wedge process exit past this deadline plus the
/// per-pool grace above.
#[cfg(not(target_arch = "wasm32"))]
const SERVICE_SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// Full loop-exit teardown: drop every hosted realm, close-request
/// registration, service and execution pool, and the platform clipboard.
///
/// Reached from each backend's loop exit — `run_desktop`/`run_android` after
/// `Platform::run` returns, and iOS from `applicationWillTerminate:`, which is
/// the only pre-exit signal a `UIApplicationMain` loop that never returns can
/// offer.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn teardown_platform_realm() {
    let realms = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Registry removal first (ADR-0037 §2): stop new routing before the
        // queued old-generation events below are dropped, and before the
        // registry is cleared. The teardown real read: assert the removed
        // entries include the address EACH realm installed — `remove_realm`
        // removes every window mapped to that realm, not just the first, so
        // a one-realm-many-windows install still leaves nothing behind.
        //
        // Every hosted realm tears down here, not just one: this runs from
        // `run_desktop`/`run_android` after their respective
        // `platform.run(...)` returns, i.e. the WHOLE loop is exiting, so
        // every realm this thread ever hosted goes with it.
        let realms = state.realms.clear();
        for (id, realm_slot) in &realms {
            let removed = state.registry.remove_realm(*id);
            debug_assert!(
                removed
                    .iter()
                    .any(|(_, removed_address)| *removed_address == realm_slot.address),
                "BUG: window_registry teardown read did not include the installed address"
            );
        }
        // Close-request registrations go with the loop, not with any one
        // realm (issue #558). Per-realm teardown already drops a realm's
        // own entries, but an explicit platform quit, or a bootstrap that
        // wired a window and then failed before installing its realm,
        // leaves entries no realm removal ever names — and this same
        // `AppRuntime` serves a SECOND `Platform::run` on this thread, so a
        // survivor would be consulted by the next loop's windows.
        state.close_requests().clear();
        state.owner_thread = None;
        state.dispatched_scheduler = None;
        state.dispatched_realm_id = None;
        state.iterating_all_realms = false;
        debug_assert!(
            !state.has_pending_realm_mutations(),
            "BUG: realm-map mutations still pending at full loop-exit teardown -- \
             dispatch_platform_realm and for_each_installed_realm must drain \
             unconditionally in their own tails"
        );
        realms
    });
    // Destructors may re-enter platform/framework code. Drop only after the
    // TLS borrow and incarnation identity have been released.
    drop(realms);

    // Service-lifecycle shutdown (issue #558) BEFORE the pools close: the
    // registry cancels every application service cooperatively and joins
    // each against one shared deadline — the flush window in which a
    // service persists its final state. Ordering is load-bearing: the
    // execution shutdown below cancels the pools' root token and
    // hard-drops any future still running at its next await point, so a
    // service joined AFTER that would lose its flush window every time —
    // pinned by `teardown_gives_services_their_flush_window_before_the_pools_close`.
    let report = APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .shutdown_lifecycles(SERVICE_SHUTDOWN_DEADLINE)
    });
    let incomplete: Vec<&'static str> = report
        .entries
        .iter()
        .filter(|entry| entry.outcome != crate::app::lifecycle::ServiceShutdownOutcome::Completed)
        .map(|entry| entry.name)
        .collect();
    if incomplete.is_empty() {
        tracing::debug!(
            services = report.entries.len(),
            "application services shut down cleanly"
        );
    } else {
        tracing::warn!(
            services = report.entries.len(),
            ?incomplete,
            "some application services did not complete by the shutdown deadline"
        );
    }

    // Execution-services shutdown (issue #557): the whole loop is exiting,
    // so stop background admission, cancel outstanding work, join running
    // work bounded by a per-pool grace deadline, and CLEAR the slot — a
    // second platform loop hosted on this same thread later (an embedder
    // running `run_app` twice in one process) must re-resolve fresh
    // services at its own realm install, not inherit an instance whose
    // admission is permanently closed. Loop-scoped like the clipboard
    // below — hot-restart never reaches this function, so a reinstalled
    // realm keeps its pools. A teardown path that skips this (panic
    // mid-teardown) still tears the pools down non-blockingly via
    // `ExecutionServices`' own `Drop`.
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .shutdown_execution(EXECUTION_SHUTDOWN_GRACE);
    });

    // ADR-0038 §9's install/teardown symmetry: the event loop has exited (this
    // runs from both `run_desktop` and `run_android`, after their respective
    // `platform.run(...)` returns), so drop the platform clipboard now rather
    // than let a live platform resource (arboard on X11 owns a live X11
    // connection) sit pinned behind `AppRuntime` for the rest of the
    // process's life. `Drop for AppRuntime` is the last-resort third clear
    // if this explicit path is ever skipped (a panic mid-teardown, for
    // instance) — see that impl's doc.
    let released = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state.clear_platform_clipboard();
        state.clear_redraw_window()
    });
    // Ordinarily `None` already: the window-close path released this pin
    // (`release_redraw_window_for`) while the event loop was still alive,
    // which is the order the platform teardown contract wants. Dropped here
    // outside the TLS borrow for the paths that never closed a window (an
    // OS-level quit with the window still open).
    drop(released);
}

#[path = "realm_dispatch/tests.rs"]
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
mod realm_dispatch_tests;
