//! AppBinding - transitional process service host.
//!
//! This coordinates the process-scoped services that have not yet moved into
//! ADR-0027 ownership domains. Widget-tree state lives in `UiRealm`, not here.
//!
//! # Flutter Equivalence
//!
//! ```dart
//! // Flutter's combined binding
//! class WidgetsFlutterBinding extends BindingBase
//!     with GestureBinding, SchedulerBinding, ServicesBinding,
//!          SemanticsBinding, PaintingBinding, RendererBinding,
//!          WidgetsBinding { }
//! ```
//!
//! In Rust, we compose the bindings as owned fields instead of mixins.
//!
//! # Architecture
//!
//! ```text
//! AppBinding (transitional process host)
//!   ├── renderer: RendererBinding      (render tree, pipeline)
//!   ├── gestures: GestureBinding       (hit testing, pointer coalescing)
//!   └── scheduler: Scheduler           (frame callbacks)
//!
//! UiRealm (owner-affine)
//!   └── widgets: WidgetsBinding        (element tree, build)
//! ```

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use flui_animation::Vsync;
use flui_engine::{EngineError, RasterBackend};
use flui_foundation::HasInstance;
use flui_interaction::{
    ClientToken, ImeEventCallback, OpaqueWindowHandle, TextInputRegistry, binding::GestureBinding,
    routing::FocusManager,
};
use flui_layer::Scene;
use flui_platform::traits::{PlatformInput, PlatformWindow};
use flui_rendering::constraints::BoxConstraints;
use flui_scheduler::Scheduler;
use flui_types::{
    HapticFeedback, Size,
    geometry::{Bounds, Pixels, px},
};
use flui_view::View;
use flui_widgets::VsyncScope;
use parking_lot::{Mutex, RwLock};

use crate::{
    app::lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState, PlatformLifecycle},
    bindings::RenderingFlutterBinding,
};

/// Transitional process service host.
///
/// AppBinding coordinates the specialized process services that remain during
/// the ADR-0027 migration:
/// - [`RendererBinding`](crate::bindings::RendererBinding) - Manages render tree and pipeline
/// - [`GestureBinding`] - Manages hit testing, pointer coalescing, and gestures
/// - [`Scheduler`] - Manages frame scheduling
///
/// The runner-owned `UiRealm` separately owns the element tree, BuildOwner,
/// and widget build phase.
///
/// # Input Handling
///
/// Platform events enter through [`handle_input()`](Self::handle_input):
/// - Pointer events → `GestureBinding::handle_pointer_event()` (with
///   coalescing)
/// - Keyboard events → `FocusManager::dispatch_key_event()`
///
/// # Thread affinity
///
/// AppBinding is a transitional owner-thread host accessed via `instance()`.
/// It is thread-local during the ADR-0027 migration so owner-local gesture and
/// widget state do not have to implement `Send + Sync` just to satisfy a
/// process-global static.
pub struct AppBinding {
    /// Renderer binding (render tree, layout/paint phases)
    renderer: RwLock<RenderingFlutterBinding>,

    /// Gesture binding (input handling, hit testing, pointer coalescing)
    gestures: GestureBinding,

    /// Whether a redraw is needed
    needs_redraw: Arc<AtomicBool>,

    /// Whether the app is initialized
    initialized: AtomicBool,

    /// Total frames rendered successfully
    frames_rendered: AtomicU64,

    /// Frames dropped due to surface errors
    frames_dropped: AtomicU64,

    /// Shared pipeline owner for elements (wrapped in Arc for sharing)
    /// This is the same PipelineOwner as in RendererBinding, but wrapped
    /// for sharing with elements that need `Arc<RwLock<PipelineOwner>>`.
    shared_pipeline_owner: Arc<RwLock<flui_rendering::pipeline::PipelineOwner>>,

    /// Application lifecycle state tracker.
    lifecycle: Mutex<DefaultLifecycle>,

    /// Active platform window (set during run_desktop).
    active_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>>,

    /// Controller registry for implicit animations (VsyncScope-driven).
    ///
    /// Wrapped in a `Mutex` so `set_vsync` can replace the shared `Arc` handle
    /// through `&self` (AppBinding is a `'static` singleton).  The `Mutex` is
    /// only locked to read the handle (`vsync_slot.lock().clone()`) or replace
    /// it (`set_vsync`); the per-frame `tick_all` / `has_running` calls then
    /// operate on the cloned `Vsync` handle — which shares the inner
    /// `Arc<Mutex<VsyncInner>>` — so there is no extra lock on the hot path
    /// beyond what `Vsync` itself already takes.
    ///
    /// The controllers here are DISJOINT from the global `Scheduler`'s ticker
    /// set: implicit controllers use a private throwaway `Scheduler`
    /// (`flui-widgets/src/animated/implicitly_animated.rs:54`), not the one
    /// `AppBinding::scheduler()` exposes.  There is therefore NO double-advance
    /// when both `Scheduler::handle_draw_frame` and `vsync.tick_all` run in
    /// the same frame.
    vsync_slot: Mutex<Vsync>,

    /// Wall-clock origin for the production `now_secs` computation.
    ///
    /// `now_secs()` = `start.elapsed().as_secs_f64()`.  Stored once in `new`
    /// so all frames share a single monotonically-increasing origin instead of
    /// each calling `Instant::now()` independently (which drifts between the
    /// Vsync tick and the Scheduler).
    start: web_time::Instant,

    /// Test-only injectable clock, stored as the f64 bits in a u64 atomic.
    ///
    /// When set (non-zero bit pattern), `now_secs()` returns this value
    /// instead of `start.elapsed()`.  Set via `set_now_secs_for_test`; reset
    /// by storing `0u64`.  This keeps `draw_frame`/`render_frame` signatures
    /// stable between production and test code.
    #[cfg(test)]
    now_secs_override: AtomicU64,
}

#[derive(Clone)]
struct FrameWakeHandle {
    needs_redraw: Arc<AtomicBool>,
    active_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>>,
}

impl FrameWakeHandle {
    fn wake_frame(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
        let window = self.active_window.lock().as_ref().cloned();
        if let Some(window) = window {
            window.request_redraw();
            tracing::trace!("wake_frame: platform window request_redraw sent");
        }
    }

    fn into_callback(self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(move || self.wake_frame())
    }
}

/// Bridges [`TextInputRegistry`] (`flui-interaction`) client attach/detach
/// to the platform's `PlatformTextInput` capability (`flui-platform`), so
/// attaching an IME client automatically enables platform IME composition
/// and detaching automatically disables it. Without this bridge, every call
/// site (PR2's `EditableText`) would need to re-derive the
/// enable-on-attach/disable-only-if-still-active rule itself — and could get
/// the stale-detach guard wrong.
///
/// `flui-interaction` cannot depend on `flui-platform` (see
/// `TextInputRegistry`'s module doc), so `flui-app` — which depends on both
/// — is where this wiring has to live.
struct ImeBackend;

impl ImeBackend {
    fn attach(window: &Arc<dyn PlatformWindow>, callback: ImeEventCallback) -> ClientToken {
        let token = TextInputRegistry::global()
            .attach(OpaqueWindowHandle::new(Arc::clone(window)), callback);
        if let Some(text_input) = window.text_input() {
            text_input.set_ime_allowed(true);
        }
        token
    }

    fn detach(window: Option<&Arc<dyn PlatformWindow>>, token: ClientToken) {
        if !TextInputRegistry::global().detach(token) {
            return;
        }
        if let Some(text_input) = window.and_then(|window| window.text_input()) {
            text_input.set_ime_allowed(false);
        }
    }
}

/// A `'static`, `Arc`-cloneable handle onto exactly the state
/// [`ImeBackend::attach`]/[`ImeBackend::detach`] need — the active-window
/// slot — without borrowing `&AppBinding` itself.
///
/// [`UiRealm::bind_to_app`](super::ui_realm::UiRealm::bind_to_app) installs
/// this (via [`AppBinding::text_input_platform_bridge`]) into the
/// [`flui_interaction::TextInputHandle`] capability, instead of a closure
/// that re-resolves [`AppBinding::instance`] on every call. That distinction
/// is load-bearing, not stylistic: `bind_to_app` is also called from test
/// code with a standalone `AppBinding::new()` instance (`UiRealm::for_test`,
/// isolated from the process-wide singleton on purpose), and a closure
/// hard-coding `AppBinding::instance()` would silently attach to the WRONG
/// binding there — the singleton, which never had `set_window` called on it
/// in that test — rather than the specific `app` `bind_to_app` was given.
/// Cloning the `Arc<Mutex<..>>` active-window slot ties the installed handle
/// to the correct binding either way, production singleton or test instance.
#[derive(Clone)]
pub(crate) struct TextInputPlatformBridge {
    active_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>>,
}

impl TextInputPlatformBridge {
    pub(crate) fn attach(&self, callback: ImeEventCallback) -> Option<ClientToken> {
        let window = self.active_window.lock().as_ref().cloned()?;
        Some(ImeBackend::attach(&window, callback))
    }

    pub(crate) fn detach(&self, token: ClientToken) {
        let window = self.active_window.lock().as_ref().cloned();
        ImeBackend::detach(window.as_ref(), token);
    }

    /// Tell the platform IME where to draw its candidate window (ADR-0032).
    /// Clones the active window's `Arc<dyn PlatformTextInput>` out from
    /// under the lock before calling through — the same clone-then-call
    /// discipline [`AppBinding::perform_haptic_feedback`] follows, so a slow
    /// or reentrant backend call never holds `active_window` for anyone
    /// else. Silent no-op with no active window, or a backend with no
    /// `PlatformTextInput` capability.
    pub(crate) fn set_cursor_area(&self, area: Bounds<Pixels>) {
        let window = self.active_window.lock().as_ref().cloned();
        if let Some(text_input) = window.and_then(|window| window.text_input()) {
            text_input.set_ime_cursor_area(area);
        }
    }
}

/// A `'static`, `Arc`-cloneable handle onto exactly the state
/// [`HotReloadBridge::apply`] needs — the shared pipeline owner and the
/// `needs_redraw` flag — without borrowing `&AppBinding` itself.
///
/// [`UiRealm::bind_to_app`](super::ui_realm::UiRealm::bind_to_app) installs
/// this (via [`AppBinding::hot_reload_bridge`]) so `UiRealm::drain_commands`'s
/// `UiCommand::HotReload` arm applies against the app the realm was actually
/// bound to. This is the same stale-binding class
/// [`TextInputPlatformBridge`] exists to prevent for IME callbacks: hitting
/// `AppBinding::instance()` directly from that arm would silently reassemble
/// the process-wide singleton's tree instead of a standalone test instance's
/// (`UiRealm::for_test`) — wrong under `for_test`, and a trap for a future
/// multi-realm binding.
#[derive(Clone)]
pub(crate) struct HotReloadBridge {
    pipeline: Arc<RwLock<flui_rendering::pipeline::PipelineOwner>>,
    needs_redraw: Arc<AtomicBool>,
}

impl HotReloadBridge {
    /// Reassembles the element and render trees and requests a redraw.
    /// Mirrors [`AppBinding::perform_hot_reload_entered`] but through the
    /// bound app's own handles rather than `&AppBinding`.
    pub(crate) fn apply(
        &self,
        realm: &super::ui_realm::UiRealm,
        tier: flui_hot_reload::HotReloadTier,
    ) {
        use flui_hot_reload::HotReloadTier;

        match tier {
            HotReloadTier::HotReload => {
                realm.widgets().perform_reassemble();
                self.pipeline.write().reassemble();
                self.needs_redraw.store(true, Ordering::Relaxed);
                tracing::info!("Hot reload applied — element and render trees reassembled");
            }
            HotReloadTier::HotRestart => {
                tracing::warn!(
                    "HotRestart requested — root remount not yet implemented; \
                     falling back to reassemble (state may be stale)"
                );
                self.apply(realm, HotReloadTier::HotReload);
            }
            HotReloadTier::FullRestart => {
                tracing::debug!("FullRestart is handled by the CLI process supervisor");
            }
        }
    }
}

/// Outcome of one build+layout+paint pass, distinguishing "nothing was
/// dirty" from "the pipeline failed" — both produce no layer tree, but only
/// the latter must force a retry rather than being treated as a settled,
/// up-to-date frame (see [`AppBinding::render_frame_entered`]'s retry gate).
enum FramePaintOutcome {
    /// A fresh layer tree was painted and turned into a `Scene`.
    Painted(Arc<Scene>),
    /// Nothing was dirty this frame; no new content to composite.
    Idle,
    /// The build/layout/paint transaction failed (e.g. a render object
    /// panicked and was caught by `catch_unwind`); the frame was dropped and
    /// must be retried.
    Errored,
}

impl AppBinding {
    /// Create a new, standalone `AppBinding` — distinct from
    /// [`AppBinding::instance()`]'s process-singleton. `pub(crate)` (not
    /// private) specifically so `UiRealm`'s own tests (a sibling module) can
    /// build one to bind a realm against, the same way this module's tests
    /// do — see `UiRealm::for_test`.
    pub(crate) fn new() -> Self {
        // Ensure the global Scheduler singleton is initialized
        let _ = Scheduler::instance();

        let needs_redraw = Arc::new(AtomicBool::new(false));
        let active_window = Arc::new(Mutex::new(None));
        let wake_handle = FrameWakeHandle {
            needs_redraw: Arc::clone(&needs_redraw),
            active_window: Arc::clone(&active_window),
        };

        // Create shared pipeline owner first (elements need Arc access)
        let shared_pipeline_owner =
            Arc::new(RwLock::new(flui_rendering::pipeline::PipelineOwner::new()));

        // Idle-wake wiring: a dirty mark (mark_needs_layout /
        // add_node_needing_paint) fires this callback so a quiescent
        // event loop produces the frame — without it, work scheduled
        // while the app is idle (an async image decode, a timer-driven
        // setState) would sit in the dirty queues until some unrelated
        // input forced a redraw.
        //
        // Lock order is safe: the callback fires while the CALLER holds
        // the pipeline-owner lock, and `wake_frame` acquires only the
        // `active_window` leaf Mutex — never the owner, never `widgets`.
        // The callback captures only a small Send + Sync wake capability, not
        // `AppBinding` itself. That matters after ADR-0027 made
        // `AppBinding::instance()` thread-local: a worker-thread dirty mark
        // must wake the owner binding, not resolve/create a worker-local TLS
        // binding and set redraw state there.
        let visual_wake = wake_handle.clone();
        shared_pipeline_owner
            .write()
            .set_on_need_visual_update(move || visual_wake.wake_frame());

        // Animation-wake wiring: scheduling a frame callback (a ticker
        // tick) fires this hook on the scheduler's false→true
        // `frame_scheduled` transition (Flutter parity:
        // `SchedulerBinding.scheduleFrame` → platform `scheduleFrame`).
        // Without it an AnimationController only advances on frames some
        // OTHER source produces — after the first idle frame the ticker
        // starves and the animation freezes.
        //
        // Deliberately NOT `wake_handle.into_callback()`: `Scheduler::instance()`
        // is itself a thread-local singleton, so this hook is a shared resource
        // between every `AppBinding` constructed on this thread — including a
        // throwaway `AppBinding::new()` a test builds alongside the real
        // singleton. A hook closing over ONE construction's `wake_handle` would
        // get silently overwritten by the next `new()` call, leaving whichever
        // binding built the hook first (often the real singleton) with a dead
        // ticker and no diagnostic. Resolving `AppBinding::instance()` fresh on
        // every fire instead makes reinstallation idempotent — every `new()`
        // call installs the same semantic hook — and always wakes the ONE
        // binding `instance()` actually resolves to on this thread, regardless
        // of how many throwaway bindings were constructed after it. Same
        // lock-safety argument as the visual-update hook above: `wake_frame`
        // touches only the `active_window` leaf Mutex, never re-entering here.
        Scheduler::instance().set_on_frame_scheduled(Some(Arc::new(|| {
            AppBinding::instance().wake_frame();
        })));

        // Create RendererBinding sharing the SAME PipelineOwner
        let renderer =
            RenderingFlutterBinding::new_with_pipeline(Arc::clone(&shared_pipeline_owner));

        Self {
            renderer: RwLock::new(renderer),
            gestures: GestureBinding::new(),
            needs_redraw,
            initialized: AtomicBool::new(false),
            frames_rendered: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            shared_pipeline_owner,
            lifecycle: Mutex::new(DefaultLifecycle::new()),
            active_window,
            vsync_slot: Mutex::new(Vsync::new()),
            start: web_time::Instant::now(),
            #[cfg(test)]
            now_secs_override: AtomicU64::new(0),
        }
    }

    /// Get the singleton instance.
    ///
    /// Creates the owner-thread instance on first call. The leaked allocation
    /// preserves the historical `&'static Self` API while avoiding a
    /// process-global `Sync` requirement for owner-local interaction state.
    pub fn instance() -> &'static Self {
        thread_local! {
            static INSTANCE: &'static AppBinding = {
            tracing::info!("Initializing AppBinding");
            Box::leak(Box::new(AppBinding::new()))
            };
        }

        INSTANCE.with(|binding| *binding)
    }

    pub(crate) fn frame_wake_callback(&self) -> Arc<dyn Fn() + Send + Sync> {
        self.wake_handle().into_callback()
    }

    fn wake_handle(&self) -> FrameWakeHandle {
        FrameWakeHandle {
            needs_redraw: Arc::clone(&self.needs_redraw),
            active_window: Arc::clone(&self.active_window),
        }
    }

    /// Check if the binding is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Relaxed)
    }

    // ========================================================================
    // Renderer Binding Access
    // ========================================================================

    /// Get read access to RendererBinding.
    pub fn renderer(&self) -> parking_lot::RwLockReadGuard<'_, RenderingFlutterBinding> {
        // PORT-CHECK-OK-SP6: AppBinding renderer accessor; pre-existing SP-6
        self.renderer.read()
    }

    /// Get write access to RendererBinding.
    pub fn renderer_mut(&self) -> parking_lot::RwLockWriteGuard<'_, RenderingFlutterBinding> {
        // PORT-CHECK-OK-SP6: AppBinding renderer_mut accessor; pre-existing SP-6
        self.renderer.write()
    }

    // ========================================================================
    // Widgets Binding Access
    // ========================================================================

    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// # Root bootstrap
    ///
    /// Forwards to [`flui_view::WidgetsBinding::attach_root_widget`] — the
    /// single root-bootstrap path. Every runner entry point
    /// (`runner.rs::run_desktop`/`run_android`/`run_web`) calls
    /// [`Self::attach_root_widget_with_size`] (this method's sized sibling),
    /// not a separate hand-rolled wiring; there is exactly one element-tree
    /// ownership model (the by-id, slab-resident `ElementTree`).
    ///
    /// # Implicit-animation auto-wrap
    ///
    /// The binding automatically wraps `view` in a
    /// [`VsyncScope`] backed by [`Self::vsync()`] before
    /// handing it to the element tree. This means every implicitly-animated widget
    /// below the root (`AnimatedOpacity`, `AnimatedContainer`, …) registers its
    /// controller into the binding's vsync registry without any app-author
    /// boilerplate — the binding ticks that registry once per frame
    /// (before the frame's build phase).
    ///
    /// ## Invariant — the binding owns the root scope
    ///
    /// The binding wraps with `self.vsync()` and ticks **that same registry**.
    /// To supply a custom registry, call [`set_vsync`](Self::set_vsync) **before**
    /// this method: the binding will then wrap with the custom registry and tick
    /// it. Never mount a second `VsyncScope` at the root with a *different*
    /// registry — the binding would tick its own while descendants register into
    /// the other, leaving them frozen.
    ///
    /// A `VsyncScope` nested deeper in the tree with its own registry shadows the
    /// root scope for that subtree and is **not** ticked by the binding; the app
    /// must drive it. Nesting with the **same** registry (i.e. the binding's
    /// `vsync()`) is harmless.
    ///
    /// # Errors
    ///
    /// Forwards every [`AttachError`](flui_view::AttachError) the
    /// underlying [`flui_view::WidgetsBinding::attach_root_widget`] returns —
    /// notably [`AttachError::AlreadyAttached`](flui_view::AttachError::AlreadyAttached)
    /// when a root widget is already mounted. Callers MUST handle the
    /// `Result`: an earlier version logged the error and swallowed it,
    /// which hid `AlreadyAttached` (and any future variant added under
    /// the enum's `#[non_exhaustive]` cover) from the caller.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "desktop/mobile runners use the sized attach variant"
        )
    )]
    pub(crate) fn attach_root_widget<V>(
        &self,
        realm: &super::ui_realm::UiRealm,
        view: &V,
    ) -> Result<(), flui_view::AttachError>
    where
        V: View + Clone + 'static,
    {
        realm.enter(|realm| self.attach_root_widget_entered(realm, view))
    }

    fn attach_root_widget_entered<V>(
        &self,
        realm: &super::ui_realm::UiRealm,
        view: &V,
    ) -> Result<(), flui_view::AttachError>
    where
        V: View + Clone + 'static,
    {
        // Auto-wrap: inject a VsyncScope carrying the binding's registry so
        // every implicitly-animated widget below can register its controller
        // without any app-author boilerplate. VsyncScope is an InheritedView
        // with no render object, so the render/hit-test root is unchanged.
        let wrapped = VsyncScope::new(self.vsync(), view.clone());
        let widgets = realm.widgets();
        widgets.attach_root_widget(&wrapped)?;
        self.initialized.store(true, Ordering::Relaxed);
        self.request_redraw();
        tracing::debug!("Root widget attached");
        Ok(())
    }

    /// Attach a root widget sizing the root view to an explicit logical
    /// `width` × `height` — the platform window's surface size.
    ///
    /// Identical to [`attach_root_widget`](Self::attach_root_widget) except the
    /// root [`RenderView`](flui_rendering::view::RenderView) is born at the real
    /// window size instead of the 800×600 fallback. This is the runner's
    /// bootstrap entry point.
    ///
    /// See [`attach_root_widget`](Self::attach_root_widget) for the
    /// implicit-animation auto-wrap invariant.
    ///
    /// # Errors
    ///
    /// Forwards every [`AttachError`](flui_view::AttachError) from
    /// [`flui_view::WidgetsBinding::attach_root_widget_with_size`].
    pub(crate) fn attach_root_widget_with_size<V>(
        &self,
        realm: &super::ui_realm::UiRealm,
        view: &V,
        width: f32,
        height: f32,
    ) -> Result<(), flui_view::AttachError>
    where
        V: View + Clone + 'static,
    {
        realm.enter(|realm| self.attach_root_widget_with_size_entered(realm, view, width, height))
    }

    fn attach_root_widget_with_size_entered<V>(
        &self,
        realm: &super::ui_realm::UiRealm,
        view: &V,
        width: f32,
        height: f32,
    ) -> Result<(), flui_view::AttachError>
    where
        V: View + Clone + 'static,
    {
        // Auto-wrap: same VsyncScope injection as attach_root_widget.
        let wrapped = VsyncScope::new(self.vsync(), view.clone());
        let widgets = realm.widgets();
        widgets.attach_root_widget_with_size(&wrapped, width, height)?;
        self.initialized.store(true, Ordering::Relaxed);
        self.request_redraw();
        tracing::debug!(width, height, "Root widget attached (sized)");
        Ok(())
    }

    /// Apply a hot reload at the given tier (Flutter parity entry point).
    ///
    /// - [`flui_hot_reload::HotReloadTier::HotReload`]: `perform_reassemble` on widgets + render pipeline.
    /// - [`flui_hot_reload::HotReloadTier::HotRestart`]: detach + re-attach root (Phase B — not yet wired).
    /// - [`flui_hot_reload::HotReloadTier::FullRestart`]: no-op here; use `flui run` process restart.
    ///
    /// Apply reload while the realm owner is at an Idle commit point.
    ///
    /// Delegates to [`HotReloadBridge::apply`] against THIS app's own
    /// pipeline/redraw handles — the single implementation both this method
    /// and `UiRealm::drain_commands`'s hot-reload arm share.
    pub(crate) fn perform_hot_reload_entered(
        &self,
        realm: &super::ui_realm::UiRealm,
        tier: flui_hot_reload::HotReloadTier,
    ) {
        self.hot_reload_bridge().apply(realm, tier);
    }

    // ========================================================================
    // Render Pipeline Access (for elements)
    // ========================================================================

    /// Get the Arc to RenderPipelineOwner for sharing with elements.
    ///
    /// This is used by RootRenderElement to insert RenderObjects into the tree.
    /// Elements need `Arc<RwLock<PipelineOwner>>` for concurrent access.
    pub fn render_pipeline_arc(&self) -> Arc<RwLock<flui_rendering::pipeline::PipelineOwner>> {
        // PORT-CHECK-OK-SP6: AppBinding render_pipeline_arc accessor; pre-existing SP-6
        Arc::clone(&self.shared_pipeline_owner)
    }

    /// Get read access to RenderPipelineOwner.
    pub fn render_pipeline(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, flui_rendering::pipeline::PipelineOwner> {
        self.shared_pipeline_owner.read()
    }

    /// Get write access to RenderPipelineOwner.
    pub fn render_pipeline_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, flui_rendering::pipeline::PipelineOwner> {
        self.shared_pipeline_owner.write()
    }

    // ========================================================================
    // Gesture Binding Access
    // ========================================================================

    /// Get the gesture binding.
    pub fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    // ========================================================================
    // Scheduler Access
    // ========================================================================

    /// Get the scheduler singleton.
    pub fn scheduler(&self) -> &'static Scheduler {
        Scheduler::instance()
    }

    // ========================================================================
    // Vsync — implicit-animation controller registry
    // ========================================================================

    /// A clone of the shared controller registry for implicit animations.
    ///
    /// `Vsync` is `Arc`-backed; cloning is two atomic increments — cheap.
    /// App code constructs a `VsyncScope` from this clone so every
    /// implicitly-animated widget below registers its controller here.  The
    /// production frame driver ticks all registered running controllers once
    /// per frame (before the build phase) and keeps the frame loop alive until
    /// the last one completes.
    ///
    /// This mirrors `HeadlessBinding::vsync` exactly; the two binding types are
    /// interchangeable at the `VsyncScope` boundary.
    pub fn vsync(&self) -> Vsync {
        self.vsync_slot.lock().clone()
    }

    /// Replace this binding's registry with a pre-existing shared `Vsync`.
    ///
    /// Use when a `VsyncScope` was built before the binding's registry was
    /// acquired (the scope needs the handle to pass to descendants, and the
    /// binding must drive that same registry).  Call before any controller is
    /// registered so no registration is stranded on the discarded registry.
    /// Mirrors `HeadlessBinding::adopt_vsync`.
    ///
    /// # Customizing the auto-wrapped registry
    ///
    /// `attach_root_widget` auto-wraps the root in a
    /// [`VsyncScope`] backed by `self.vsync()` and the
    /// frame driver ticks **that same registry**. To supply a custom registry,
    /// call `set_vsync(custom)` **before** `attach_root_widget` so the binding
    /// wraps with and ticks the same handle:
    ///
    /// ```ignore
    /// let custom = Vsync::new();
    /// binding.set_vsync(custom.clone());
    /// binding.attach_root_widget(&root)?;
    /// // binding.draw_frame ticks `custom` each frame.
    /// ```
    ///
    /// Never mount a second `VsyncScope` at the root with a *different* registry
    /// — the binding ticks its own while descendants register into the other,
    /// leaving implicit animations frozen.
    pub fn set_vsync(&self, vsync: Vsync) {
        *self.vsync_slot.lock() = vsync;
    }

    /// Whether at least one registered controller is currently running.
    ///
    /// The production frame driver calls this after `tick_all` to decide
    /// whether to request the next frame (continuation check).  Returns `true`
    /// while any implicit animation is in flight; `false` once all have
    /// settled, so the window quiesces cleanly without infinite redraw.
    pub fn has_vsync_running(&self) -> bool {
        self.vsync_slot.lock().has_running()
    }

    /// Current virtual seconds for the Vsync tick.
    ///
    /// Production: `self.start.elapsed().as_secs_f64()` — one monotonic origin
    /// shared across the Vsync tick and all frame accounting, so there is no
    /// clock drift between the two.
    ///
    /// Tests: the injected override (`set_now_secs_for_test`) takes precedence,
    /// allowing deterministic animation stepping with no wall-clock reads.
    fn now_secs(&self) -> f64 {
        #[cfg(test)]
        {
            let bits = self.now_secs_override.load(Ordering::Relaxed);
            if bits != 0 {
                return f64::from_bits(bits);
            }
        }
        self.start.elapsed().as_secs_f64()
    }

    /// Inject a deterministic virtual `now_secs` for test frames.
    ///
    /// `draw_frame` uses this value for `tick_all` instead of the real wall
    /// clock.  Only compiled in `#[cfg(test)]` builds; production signatures
    /// are unaffected.
    ///
    /// Store `0.0` to clear and revert to wall-clock (internally the sentinel
    /// is `0u64`; a caller supplying exactly `0.0` gets the smallest positive
    /// subnormal instead, which is negligible for any animation test).
    #[cfg(test)]
    pub fn set_now_secs_for_test(&self, secs: f64) {
        let bits = secs.to_bits();
        // 0u64 bits is the "not set" sentinel; bump to 1 (subnormal ~5e-324 s)
        // so a caller who genuinely wants t=0 gets a value indistinguishable
        // from it for any practical animation duration.
        let stored = if bits == 0 { 1u64 } else { bits };
        self.now_secs_override.store(stored, Ordering::Relaxed);
    }

    /// Clear the test clock override, reverting to wall-clock time.
    #[cfg(test)]
    pub fn clear_now_secs_for_test(&self) {
        self.now_secs_override.store(0, Ordering::Relaxed);
    }

    // ========================================================================
    // Lifecycle Management
    // ========================================================================

    /// Get the current lifecycle state.
    pub fn lifecycle_state(&self) -> LifecycleState {
        self.lifecycle.lock().state()
    }

    /// Transition the lifecycle via an event.
    ///
    /// Delegates to [`DefaultLifecycle::handle_event`] and logs the transition.
    pub fn transition_lifecycle(&self, event: LifecycleEvent) {
        self.lifecycle.lock().handle_event(event);
        tracing::debug!(?event, state = ?self.lifecycle_state(), "Lifecycle transition");
    }

    /// Check if the lifecycle state allows rendering.
    pub fn should_render(&self) -> bool {
        self.lifecycle.lock().should_render()
    }

    // ========================================================================
    // Window Access
    // ========================================================================

    /// Store the active platform window.
    ///
    /// Called by the runner after all callbacks have been registered.
    pub fn set_window(&self, window: Box<dyn PlatformWindow>) {
        self.set_shared_window(Arc::from(window));
    }

    /// Store an already shared platform window.
    ///
    /// The web runner clones this owner into asynchronous WebGPU
    /// initialization so the native canvas handle outlives every surface
    /// operation, including early-return paths during app startup.
    pub(crate) fn set_shared_window(&self, window: Arc<dyn PlatformWindow>) {
        *self.active_window.lock() = Some(window);
        tracing::debug!("Active window stored in AppBinding");
    }

    /// Access the active window.
    ///
    /// Calls the provided function with a reference to the window.
    /// Returns `None` if no window is set.
    pub fn with_window<R>(&self, f: impl FnOnce(&dyn PlatformWindow) -> R) -> Option<R> {
        self.active_window.lock().as_ref().map(|w| f(w.as_ref()))
    }

    // ========================================================================
    // IME (text input)
    // ========================================================================

    /// Attach an IME client on the active window, via
    /// [`TextInputRegistry::global`]. Enables platform IME composition
    /// (`PlatformTextInput::set_ime_allowed(true)`) through the window's
    /// `text_input()` capability, if the backend supports one.
    ///
    /// Returns `None` if there is no active window yet (the caller attached
    /// before `set_window`/`set_shared_window` ran).
    pub fn attach_text_input(&self, callback: ImeEventCallback) -> Option<ClientToken> {
        self.text_input_platform_bridge().attach(callback)
    }

    /// Detach the IME client identified by `token`.
    ///
    /// The registry side always runs; the platform `set_ime_allowed(false)`
    /// call additionally requires `token` to still be the registry's active
    /// client (see [`TextInputRegistry::detach`]'s stale-token guard) — that
    /// guard is what keeps a replaced field's dispose/blur handler from
    /// disabling IME for the field that replaced it.
    pub fn detach_text_input(&self, token: ClientToken) {
        self.text_input_platform_bridge().detach(token);
    }

    /// Tell the platform IME where to draw its candidate window, in
    /// window-root-space logical pixels (ADR-0032) — `flui-widgets`'
    /// `EditableText` post-frame loop is the only production caller, via
    /// the `TextInputHandle::set_cursor_area` capability
    /// `UiRealm::bind_to_app` installs.
    ///
    /// Routes through `text_input_platform_bridge` rather than reading
    /// `self.active_window` directly, for the same per-instance-targeting
    /// reason [`attach_text_input`](Self::attach_text_input) does (see
    /// `TextInputPlatformBridge`'s doc) — but follows
    /// [`perform_haptic_feedback`](Self::perform_haptic_feedback)'s exact
    /// clone-the-capability-out-of-the-lock-then-call-outside-it discipline
    /// (see `TextInputPlatformBridge::set_cursor_area`). Silent no-op with
    /// no active window yet, or a backend with no `PlatformTextInput`
    /// capability.
    pub fn set_ime_cursor_area(&self, area: Bounds<Pixels>) {
        self.text_input_platform_bridge().set_cursor_area(area);
    }

    /// A `'static`, `Arc`-cloneable handle onto this specific binding's
    /// active-window slot — see [`TextInputPlatformBridge`]'s doc for why
    /// [`UiRealm::bind_to_app`](super::ui_realm::UiRealm::bind_to_app)
    /// installs THIS instead of a closure that re-resolves
    /// [`AppBinding::instance`].
    pub(crate) fn text_input_platform_bridge(&self) -> TextInputPlatformBridge {
        TextInputPlatformBridge {
            active_window: Arc::clone(&self.active_window),
        }
    }

    /// A cloneable capability onto this app's pipeline + redraw flag, for
    /// [`UiRealm::bind_to_app`](super::ui_realm::UiRealm::bind_to_app) to
    /// install so hot-reload commands drained later apply against THIS app,
    /// not whichever `AppBinding::instance()` resolves to at drain time. See
    /// [`HotReloadBridge`]'s doc for why that distinction matters.
    pub(crate) fn hot_reload_bridge(&self) -> HotReloadBridge {
        HotReloadBridge {
            pipeline: Arc::clone(&self.shared_pipeline_owner),
            needs_redraw: Arc::clone(&self.needs_redraw),
        }
    }

    // ========================================================================
    // Haptics
    // ========================================================================

    /// Perform haptic feedback on the active window, via
    /// [`PlatformWindow::haptics`].
    ///
    /// Silent no-op — no panic, no error — when there is no active window
    /// yet, or the active window's backend has no [`PlatformHaptics`]
    /// capability (desktop winit targets, for instance). This mirrors
    /// Flutter's own `HapticFeedback` degradation contract: every call is
    /// fire-and-forget best-effort, with no availability-discovery API to
    /// check first (see `flui_types::HapticFeedback`'s module doc).
    ///
    /// `perform` runs *after* `with_window`'s `active_window` guard is
    /// dropped — only the cheap `Arc` clone out of `haptics()` happens
    /// under the lock, matching `TextInputPlatformBridge::attach`'s
    /// clone-then-release shape. A backend whose `perform` blocks or
    /// re-enters the binding must not stall every other window accessor
    /// for the duration of one haptic call.
    ///
    /// [`PlatformHaptics`]: flui_platform::traits::PlatformHaptics
    pub fn perform_haptic_feedback(&self, feedback: HapticFeedback) {
        let haptics = self.with_window(|window| window.haptics()).flatten();
        if let Some(haptics) = haptics {
            haptics.perform(feedback);
        }
    }

    // ========================================================================
    // Frame Management
    // ========================================================================

    /// Request a redraw.
    pub fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Wake the platform event loop so the next frame is rendered.
    ///
    /// Sets the `needs_redraw` atomic flag **and** calls
    /// `PlatformWindow::request_redraw()` on the active window so a
    /// quiescent winit / platform event loop wakes up and fires the
    /// `on_request_frame` callback.
    ///
    /// # Deadlock-safety
    ///
    /// This method acquires only `self.active_window` (a leaf `Mutex`)
    /// and never touches realm widget state. It is safe to
    /// call from any context, including from inside a `build_scope`
    /// callback that is executing while `AppBinding::widgets` is held —
    /// the two locks are disjoint.
    pub fn wake_frame(&self) {
        self.wake_handle().wake_frame();
    }

    /// Check if a redraw is needed.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark the frame as rendered.
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// Get total frames rendered successfully.
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered.load(Ordering::Relaxed)
    }

    /// Get frames dropped due to surface errors.
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped.load(Ordering::Relaxed)
    }

    /// Draw a frame and return Scene for GPU rendering.
    ///
    /// This executes the complete rendering pipeline:
    /// 1. Build phase (WidgetsBinding) - rebuild dirty elements
    /// 2. Layout phase - compute sizes
    /// 3. Paint phase - generate display lists
    /// 4. Create Scene from LayerTree
    ///
    /// Returns `Some(Scene)` if a new scene was produced, or cached scene
    /// otherwise.
    #[cfg(test)]
    pub(crate) fn draw_frame(
        &self,
        realm: &super::ui_realm::UiRealm,
        constraints: BoxConstraints,
    ) -> Option<Arc<Scene>> {
        match realm.enter(|realm| self.draw_frame_entered(realm, constraints)) {
            FramePaintOutcome::Painted(scene) => Some(scene),
            FramePaintOutcome::Idle | FramePaintOutcome::Errored => None,
        }
    }

    fn draw_frame_entered(
        &self,
        realm: &super::ui_realm::UiRealm,
        constraints: BoxConstraints,
    ) -> FramePaintOutcome {
        // Vsync tick — MUST precede the build phase (Phase 1).
        //
        // Implicit-animation controllers registered via a `VsyncScope` use a
        // private throwaway `Scheduler` (see `flui-widgets/src/animated/
        // implicitly_animated.rs:54`), NOT the global `Scheduler` that
        // `AppBinding::scheduler()` exposes.  That disjoint set means:
        //   (a) there is NO double-advance: `tick_all` here and
        //       `Scheduler::handle_draw_frame` in the runner both run in the
        //       same frame but each drives a non-overlapping controller set.
        //   (b) `tick_all` is NOT idempotent across calls in the same frame —
        //       safety rests entirely on disjointness, enforced by the private
        //       Scheduler invariant in `ImplicitController`.
        //
        // The tick MUST happen before `build_scope` (Phase 1) so a controller
        // that crosses its target this frame marks its `AnimatedView` dirty
        // via `notify_listeners` → `BuildOwner` external inbox, and that dirty
        // entry is drained by the same `build_scope` below (same-frame
        // rebuild).  A tick AFTER build would delay the rebuild by one frame.
        let now = self.now_secs();
        {
            let vsync = self.vsync_slot.lock().clone();
            vsync.tick_all(now);

            // Frame continuation: if any controller is still running after
            // this tick, request the NEXT frame so the runner gate
            // (`runner.rs:225`: `needs_redraw() || has_pending_work()`) stays
            // TRUE for the full animation duration.  Without this, after the
            // first frame `mark_rendered` clears `needs_redraw` and the build
            // heap drains — no running-controller signal keeps the gate open —
            // and the animation freezes mid-transition.
            //
            // Once the last controller completes, `has_running()` is `false`,
            // `wake_frame` is NOT called, and the window idles (quiescence).
            if vsync.has_running() {
                self.wake_frame();
            }
        }

        // The async-driver step used to live HERE. It moved into
        // `Scheduler::handle_begin_frame`'s mid-frame slot.
        //
        // Why: this method is the pipeline, and the pipeline runs in the
        // scheduler's `PersistentCallbacks` phase — where `drive_async_tasks`
        // debug-asserts it must never poll. Keeping the call here made it
        // impossible to run post-frame callbacks *after* the pipeline, which is
        // the ordering `HeroController` needs. One mid-frame poll per frame, on
        // the right `Scheduler` instance is now enforced by the scheduler itself,
        // for both bindings.
        //
        // Consequence, stated plainly: calling `draw_frame` **outside**
        // `Scheduler::drive_frame` polls no async tasks. Every frame driver goes
        // through `drive_frame`.

        // Phase 1: Build (WidgetsBinding)
        {
            let w = realm.widgets();
            if w.has_pending_builds() {
                w.draw_frame();
            }
        }

        // Phase 2 & 3: Layout, Compositing, Paint, Semantics through the
        // typestate-driven orchestrator. The four `flush_*` calls are gone;
        // `run_frame` is the single entry point and the layer tree comes
        // back as its second return value.
        //
        // `run_frame` returns
        // `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)`. The
        // owner always comes back at Idle, so we always restore it. If
        // the frame errored (e.g. a render object panicked and was
        // caught by `catch_unwind`), we log via tracing and drop the
        // frame -- the owner is still usable for the next call.
        let mut pipeline_errored = false;
        let (layer_tree, link_registry) = {
            {
                // The window's constraints ARE the root constraints — without
                // this, frame 1 has neither cached state nor root_constraints
                // and run_layout drops the root dirty entry (blank window).
                // set_root_constraints marks the root dirty only on CHANGE,
                // so the per-frame call is idempotent and resize-correct.
                self.shared_pipeline_owner
                    .write()
                    .set_root_constraints(Some(constraints));
            }
            // The shared layout<->build fixpoint settles every build-during-layout
            // node before paint, then delegates to `PipelineOwner::run_frame`.
            // `HeadlessBinding::pump_frame` calls the SAME
            // `BuildOwner::run_frame_with_layout_builders`; a builder that settles
            // headlessly but not on screen would be a silent correctness bug, so
            // neither frame path may hand-roll the loop. A plain `run_frame` when
            // no `LayoutBuilder` is mounted.
            //
            // The owner is threaded by lock: the helper restores it and frees the
            // write guard before each `build_scope`, which mounts render objects
            // through that same lock. Holding the guard here would deadlock the
            // first time a builder mounts a child.
            let result = realm
                .widgets()
                .run_frame_with_layout_builders(&self.shared_pipeline_owner);
            // Taken alongside the layer tree so `Scene::with_links` (below)
            // gets the SAME frame's leader/follower registry — resolving a
            // `Layer::Follower` position against a stale or empty registry
            // would silently misposition tooltips/dropdowns.
            let link_registry = self.shared_pipeline_owner.write().take_link_registry();
            match result {
                Ok(layer_tree) => (layer_tree, link_registry),
                Err(e) => {
                    tracing::error!(error = ?e, "draw_frame: pipeline failed, dropping frame");
                    pipeline_errored = true;
                    (None, link_registry)
                }
            }
        };

        // Production↔headless convergence point: service lazy-sliver child
        // requests accumulated by `run_frame`'s layout pass. This is the
        // production equivalent of `HeadlessBinding::pump_frame` step 6.
        // Lock order: `widgets` (brief write for the split-borrow) → `pipeline`
        // (brief write inside `service_child_requests` to drain the two pending
        // Vecs). The `run_frame` pipeline write-guard above is fully released
        // before this call — no nested write.
        //
        // NOTE: The Vsync tick (implicit-animation controllers) does NOT belong
        // here.  It runs before Phase 1 (build), not after `run_frame`, so that
        // a controller that completes this frame marks its `AnimatedView` dirty
        // before `build_scope` drains the inbox — enabling a same-frame
        // rebuild.  See the Vsync tick block at the top of `draw_frame`.
        {
            let w = realm.widgets();
            w.service_child_requests(&self.shared_pipeline_owner);
        }

        // Phase 4: Create Scene from LayerTree
        let size = constraints.constrain(Size::ZERO);
        let frame_number = self.frames_rendered.load(Ordering::Relaxed) + 1;

        if let Some(layer_tree) = layer_tree {
            // Create scene from layer tree. `Scene` is `Send` (auto-derived
            // from `LayerTree` + `LinkRegistry` + `Vec<CompositionCallback>`
            // whose payload is `FnOnce() + Send + 'static`) but is *not*
            // `Sync` because the `FnOnce + Send` callback payload itself is
            // not `Sync`. Making `Scene: Sync` requires either dropping the
            // composition-callback list or relaxing it to `Fn + Send + Sync`
            // — tracked under the engine composition redesign. Until then,
            // the binding thread is the sole reader of this `Arc<Scene>`,
            // so the lint is suppressed with an honest justification.
            let root = layer_tree.root();
            // `with_links` (not `Scene::new`, which always builds an empty
            // registry) — `link_registry` is this SAME frame's paint-phase
            // byproduct, letting the engine resolve `Layer::Follower`
            // positions against the leaders that were just composed.
            let scene = Scene::with_links(
                size,
                layer_tree,
                root,
                link_registry.unwrap_or_default(),
                frame_number,
            );
            #[expect(
                clippy::arc_with_non_send_sync,
                reason = "Scene: Send but !Sync due to CompositionCallback (FnOnce + Send + 'static, no Sync). Sole reader is the binding thread; relaxing the callback bound is tracked under the engine composition redesign."
            )]
            let arc = Arc::new(scene);
            FramePaintOutcome::Painted(arc)
        } else if pipeline_errored {
            // No layer tree because `run_frame_with_layout_builders` errored
            // above, not because nothing was dirty — the caller must retry
            // rather than treat this as a settled, up-to-date frame.
            FramePaintOutcome::Errored
        } else {
            // No new layer tree, and no error: nothing was dirty this frame.
            FramePaintOutcome::Idle
        }
    }

    /// Render while the platform dispatcher already owns the realm entry.
    /// This keeps scheduler callbacks and the full build/layout/paint/raster
    /// transaction under one activation instead of creating a nested scope.
    ///
    /// Returns whether the frame reached `present()` — needed for the
    /// runner's no-present fallback throttle (see `runner.rs`'s
    /// `no_present_fallback_pace`): Fifo present blocks every PRESENTED
    /// frame at display cadence, but a frame that never presents (nothing
    /// dirty, no damage, occluded surface, surface lost) carries no such
    /// pacing signal. No caller currently consumes the painted [`Scene`]
    /// itself (the GPU-side `render_scene` call already owns presentation),
    /// so this returns the presented flag alone rather than reintroducing
    /// an unused `Option<Arc<Scene>>`.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn render_frame_entered<R: RasterBackend>(
        &self,
        realm: &super::ui_realm::UiRealm,
        renderer: &mut R,
    ) -> bool {
        // 1. Flush coalesced pointer moves (GestureBinding handles coalescing)
        self.gestures.flush_pending_moves();

        // 1b. Advance recognizer deadlines so a held-still pointer past its
        //     timeout (e.g. long press) fires without a further input event.
        self.gestures.tick_deadlines();

        // 2. Draw frame (build + layout + paint → Scene). The surface
        // reports PHYSICAL pixels; the framework lays out in LOGICAL
        // pixels — the paint root's DPR transform bridges back. A
        // physical-sized layout at DPR 2 would paint everything double
        // size (the "red box covers a quarter of the window" bug).
        let (width, height) = renderer.size();
        let dpr = self.shared_pipeline_owner.read().device_pixel_ratio();
        let constraints =
            BoxConstraints::tight(Size::new(px(width as f32 / dpr), px(height as f32 / dpr)));
        let outcome = self.draw_frame_entered(realm, constraints);

        // 3. Render scene to GPU
        let mut presented = false;
        // Forces `wake_frame()` instead of `mark_rendered()` below for any
        // frame that was dropped rather than settled — a pipeline error, or
        // a recoverable GPU error below — so `needs_redraw` stays armed AND
        // an actual wake is scheduled. Without this, a dropped frame on an
        // otherwise-quiescent event loop (no animation, no further input)
        // never gets retried: the loop falls back to `ControlFlow::Wait`
        // and the UI stays stale until the next external event.
        let mut retry_needed = matches!(outcome, FramePaintOutcome::Errored);
        if let FramePaintOutcome::Painted(ref scene) = outcome
            && scene.has_content()
        {
            // The pipeline painted a FRESH scene this frame, so the
            // on-screen content is stale. The engine's damage tracker is
            // only marked by resize/surface-create paths; without this
            // mark, `render_scene` early-returns on "no damage" and every
            // animation frame is silently dropped — the screen then only
            // updates on resize. Until fine-grained damage from the layer
            // diff lands, a new scene is a full repaint.
            renderer.mark_full_repaint();
            match renderer.render_scene(scene) {
                Ok(did_present) => {
                    presented = did_present;
                    if did_present {
                        self.frames_rendered.fetch_add(1, Ordering::Relaxed);
                        tracing::trace!(
                            frame = scene.frame_number(),
                            total = self.frames_rendered.load(Ordering::Relaxed),
                            "Frame rendered successfully"
                        );
                    } else {
                        // No damage / occluded: `render_scene` skipped
                        // `present()` without error. Not counted as a
                        // rendered frame — no pixel reached the screen.
                        tracing::trace!(
                            frame = scene.frame_number(),
                            "Frame skipped: no damage or surface occluded (no present)"
                        );
                    }
                }
                Err(EngineError::SurfaceLost) => {
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    retry_needed = true;
                    tracing::debug!("Surface lost; frame dropped — retry armed via wake_frame()");
                }
                Err(EngineError::DeviceLost) => {
                    // GPU device lost (TDR / driver crash / GPU switch). Recovery
                    // requires rebuilding the entire GPU context asynchronously; it
                    // is handled by the platform runner after `render_frame` returns.
                    // `render_frame` itself has no async context and no raw window
                    // handle, so it must not attempt recovery here.
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        "GPU device lost — recovery will be attempted by the platform runner"
                    );
                }
                Err(EngineError::SurfaceValidation) => {
                    // Surface misconfig (wgpu Validation). Drop this frame and
                    // log; reconfiguration is NOT automatic — it requires an
                    // external trigger (window resize / surface recreate).
                    // `render_scene` only reconfigures in the Outdated/Lost arm,
                    // so without such a trigger this would drop + error-log
                    // every frame. We do not retry blindly: re-reconfiguring the
                    // same bad config would re-validate and loop forever.
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::error!(
                        "Surface validation error — surface misconfig; external reconfigure required"
                    );
                }
                Err(e) => {
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::error!(error = ?e, "Render error (non-recoverable this frame)");
                }
            }
        }

        // 4. Mark rendered — unless this frame was dropped rather than
        // settled, in which case `wake_frame()` re-arms `needs_redraw` AND
        // schedules an actual platform wake (see `retry_needed` above).
        if retry_needed {
            self.wake_frame();
        } else {
            self.mark_rendered();
        }

        presented
    }

    /// Check if there is pending work.
    pub(crate) fn has_pending_work(&self, realm: &super::ui_realm::UiRealm) -> bool {
        realm.widgets().has_pending_builds() || self.shared_pipeline_owner.read().has_dirty_nodes()
    }

    // ========================================================================
    // Input Handling
    // ========================================================================

    /// Handle a platform input event.
    ///
    /// This is the single entry point for all input from the platform layer.
    /// Routes pointer events to `GestureBinding`, keyboard events to
    /// `FocusManager`, and IME composition/commit events to
    /// `TextInputRegistry`.
    ///
    /// Pointer events are coalesced by `GestureBinding` — high-frequency move
    /// events are stored and flushed once per frame via
    /// `flush_pending_moves()` in `render_frame()`.
    pub fn handle_input(&self, input: PlatformInput) {
        match input {
            PlatformInput::Ime(ime_event) => {
                TextInputRegistry::global().dispatch(&ime_event);
                self.request_redraw();
            }
            PlatformInput::Pointer(pointer_event) => {
                self.gestures
                    .handle_pointer_event(&pointer_event, |position| {
                        // A single canonical `HitTestResult` flows through both
                        // rendering traversal and gesture dispatch: the
                        // rendering crate re-exports
                        // `flui_interaction::routing::HitTestResult`, so the
                        // same instance crosses both layers without conversion
                        // (no per-hit bridge that could silently drop targets).
                        use flui_rendering::binding::RendererBinding;
                        let renderer = self.renderer.read();
                        let mut result = flui_interaction::routing::HitTestResult::new();
                        let offset = flui_types::Offset::new(position.dx, position.dy);
                        renderer.hit_test_in_view(&mut result, offset, 0);
                        if !result.is_empty() {
                            tracing::debug!(hits = result.len(), "Hit test found targets");
                        }
                        result
                    });
                self.request_redraw();
            }
            PlatformInput::Keyboard(keyboard_event) => {
                FocusManager::global().dispatch_key_event(&keyboard_event);
                self.request_redraw();
            }
        }
    }
}

impl std::fmt::Debug for AppBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBinding")
            .field("initialized", &self.initialized.load(Ordering::Relaxed))
            .field("needs_redraw", &self.needs_redraw.load(Ordering::Relaxed))
            .field("vsync", &self.vsync_slot.lock().clone())
            .field("renderer", &*self.renderer.read())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    // Required to call `ctx.get::<T, _>(...)` on `&dyn BuildContext`; the method
    // lives on the `BuildContextExt` extension trait.
    use flui_view::BuildContextExt as _;

    #[test]
    fn test_singleton() {
        let binding1 = AppBinding::instance();
        let binding2 = AppBinding::instance();
        assert!(std::ptr::eq(binding1, binding2));
    }

    /// Idle-wake wiring smoke test: a dirty mark on the shared
    /// pipeline owner must reach `AppBinding::wake_frame` through the
    /// visual-update notifier set in `AppBinding::new`, flipping
    /// `needs_redraw` so the platform loop produces a frame.
    #[test]
    fn dirty_mark_fires_wake_via_notifier() {
        let binding = AppBinding::instance();

        let id = binding.shared_pipeline_owner.write().insert(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        )
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        binding
            .shared_pipeline_owner
            .write()
            .clear_all_dirty_nodes();
        binding.mark_rendered();

        binding.shared_pipeline_owner.write().mark_needs_layout(id);
        assert!(
            binding.needs_redraw(),
            "an owner dirty mark must wake the binding via the \
             visual-update notifier wired in AppBinding::new",
        );
    }

    #[test]
    fn cross_thread_dirty_handle_wakes_owner_binding_not_worker_tls() {
        let binding = AppBinding::new();
        binding.mark_rendered();

        let handle = binding.shared_pipeline_owner.read().handle();
        std::thread::spawn(move || {
            handle
                .request_mark_dirty(
                    flui_foundation::RenderId::new(1),
                    0,
                    flui_rendering::pipeline::DirtyKind::Paint,
                )
                .expect("dirty request should enqueue");
        })
        .join()
        .expect("worker thread should not panic");

        assert!(
            binding.needs_redraw(),
            "cross-thread dirty requests must wake the owner binding captured \
             during AppBinding construction, not resolve a worker-local TLS binding"
        );
    }

    #[test]
    fn test_needs_redraw() {
        let binding = AppBinding::instance();

        binding.mark_rendered();
        assert!(!binding.needs_redraw());

        binding.request_redraw();
        assert!(binding.needs_redraw());

        binding.mark_rendered();
        assert!(!binding.needs_redraw());
    }

    #[test]
    fn test_renderer_initialized() {
        let binding = AppBinding::instance();
        // Verify the renderer sub-binding is accessible (created during
        // AppBinding::new)
        let _renderer = binding.renderer();
    }

    /// Minimal leaf view/element so a headless `attach_root_widget` has
    /// something to mount without pulling in a widget crate.
    #[derive(Clone)]
    struct LeafView;

    impl flui_view::RenderView for LeafView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) {
            *render_object = flui_objects::RenderSizedBox::shrink();
        }
    }

    impl View for LeafView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    /// Root view with owner-local state. This deliberately does not implement
    /// `Send` or `Sync`; the app root is mounted on the UI owner thread.
    #[derive(Clone)]
    struct OwnerLocalLeafView {
        creates: Rc<Cell<usize>>,
    }

    impl flui_view::RenderView for OwnerLocalLeafView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            self.creates.set(self.creates.get() + 1);
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) {
            *render_object = flui_objects::RenderSizedBox::shrink();
        }
    }

    impl View for OwnerLocalLeafView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    fn test_realm(app: &AppBinding) -> super::super::ui_realm::UiRealm {
        super::super::ui_realm::UiRealm::for_test(app)
    }

    /// E2/E3 regression: `AppBinding` hands its shared `PipelineOwner` to the
    /// `WidgetsBinding` it owns, so `attach_root_widget` actually bootstraps
    /// the root render tree. Without that wiring the root mounts with no
    /// PipelineOwner, no `RenderView` is created, and the window renders
    /// nothing — the shared owner's root id stays `None`.
    #[test]
    fn attach_root_widget_bootstraps_shared_render_tree() {
        let app = AppBinding::new();
        let realm = test_realm(&app);
        realm
            .enter(|realm| app.attach_root_widget(realm, &LeafView))
            .expect("attach succeeds");
        assert!(
            app.shared_pipeline_owner.read().root_id().is_some(),
            "AppBinding must pass its PipelineOwner to the widgets binding so the \
             root render tree bootstraps; without it the window renders nothing",
        );
    }

    /// Root-hop parent-link regression: after a standard `AppBinding`
    /// bootstrap (`attach_root_widget` + a build/layout/paint `draw_frame`),
    /// the mounted leaf's render node must have a working parent link back
    /// to the root, not just the root's child-list entry.
    ///
    /// The two link directions were previously written asymmetrically:
    /// `RenderBehavior::on_mount` set both when the leaf mounted, but
    /// `RootRenderElement`'s `ElementBase::render_id` fell through to the
    /// trait default (`None`) instead of the struct's own render id (root.rs
    /// carried a correct *inherent* `render_id()` that the trait method
    /// never delegated to). `ElementTree::reorder_render_children_after_build`
    /// reads `render_id()` through `&dyn ElementBase` — the trait method —
    /// while walking the tree to compute each render node's desired parent;
    /// seeing `None` for the root, it treated the root as parentless-of-render
    /// and propagated that past it, corrupting the leaf's desired parent to
    /// `None` and overwriting the correct link `on_mount` had set. The
    /// child-list entry was never touched by that bug, so layout/paint/hit-test
    /// (which only walk downward) rendered fine while every upward walk
    /// (`transform_to`, `local_to_global`, hero/overlay positioning) silently
    /// failed at the very first hop.
    #[test]
    fn transform_to_resolves_through_the_root_hop_after_standard_bootstrap() {
        let app = AppBinding::new();
        let realm = test_realm(&app);
        realm
            .enter(|realm| app.attach_root_widget(realm, &LeafView))
            .expect("attach succeeds");
        let _ = app.draw_frame(
            &realm,
            flui_rendering::constraints::BoxConstraints::tight(flui_types::Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            )),
        );

        let owner = app.shared_pipeline_owner.read();
        let root_id = owner.root_id().expect("root id set by attach_root_widget");
        let root_node = owner
            .render_tree()
            .get(root_id)
            .expect("root render node resolves");
        let leaf_id = *root_node
            .children()
            .first()
            .expect("LeafView must have mounted one render child under the root");

        // The downward link always worked (layout/paint only walk down) —
        // assert it too, so a regression that breaks BOTH directions still
        // fails loudly instead of looking like a pass on this half.
        assert_eq!(
            owner
                .render_tree()
                .get(leaf_id)
                .and_then(flui_rendering::storage::RenderNode::parent),
            Some(root_id),
            "the leaf's render node must carry a parent link back to the root"
        );

        let transform = owner.transform_to(leaf_id, root_id);
        assert!(
            transform.is_some(),
            "transform_to(leaf, root) must resolve through the root hop; None means the \
             ancestor walk broke at the very first step (accessors.rs's `parent(current)?`)"
        );
        assert_eq!(
            transform,
            Some(flui_types::Matrix4::IDENTITY),
            "LeafView (RenderSizedBox::shrink(), zero offset) composes to the identity \
             transform into root space"
        );
    }

    #[test]
    fn attach_root_widget_accepts_owner_local_root_state() {
        static_assertions::assert_not_impl_any!(OwnerLocalLeafView: Send, Sync);

        let app = AppBinding::new();
        let realm = test_realm(&app);
        let creates = Rc::new(Cell::new(0));
        let root = OwnerLocalLeafView {
            creates: Rc::clone(&creates),
        };

        realm
            .enter(|realm| app.attach_root_widget(realm, &root))
            .expect("owner-local root attaches");

        assert!(app.shared_pipeline_owner.read().root_id().is_some());
    }

    // ========================================================================
    // E0a — wake_frame
    // ========================================================================

    /// `wake_frame` must set `needs_redraw` even when no window is stored
    /// (the window lock is a leaf that is independently optional).
    #[test]
    fn wake_frame_sets_needs_redraw_without_window() {
        // Use a fresh binding rather than the singleton so this test does not
        // race with other tests' redraw state.
        let binding = AppBinding::new();
        binding.mark_rendered();
        assert!(!binding.needs_redraw(), "precondition: no redraw pending");

        // No window installed — wake_frame must still set the atomic.
        binding.wake_frame();

        assert!(
            binding.needs_redraw(),
            "wake_frame must set needs_redraw even without an active window"
        );
    }

    /// `wake_frame` must call `PlatformWindow::request_redraw` when a window
    /// is installed, and must NOT acquire `widgets` or `inner`.
    ///
    /// A minimal inline mock records how many times `request_redraw` was
    /// called without touching any binding lock.
    #[test]
    fn wake_frame_calls_platform_request_redraw() {
        use std::sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        };

        use flui_platform::traits::PlatformWindow;
        use flui_types::geometry::{DevicePixels, Pixels, Size, device_px, px};

        struct CountingWindow {
            redraw_count: Arc<AtomicU32>,
        }

        impl PlatformWindow for CountingWindow {
            fn physical_size(&self) -> Size<DevicePixels> {
                Size::new(device_px(800), device_px(600))
            }
            fn logical_size(&self) -> Size<Pixels> {
                Size::new(px(800.0), px(600.0))
            }
            fn scale_factor(&self) -> f64 {
                1.0
            }
            fn request_redraw(&self) {
                self.redraw_count.fetch_add(1, Ordering::Relaxed);
            }
            fn is_focused(&self) -> bool {
                false
            }
            fn is_visible(&self) -> bool {
                true
            }
            // Trait default impls cover the remaining callback-registration
            // methods; only the required methods above need bodies.
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let redraw_count = Arc::new(AtomicU32::new(0));
        let window = CountingWindow {
            redraw_count: Arc::clone(&redraw_count),
        };

        let binding = AppBinding::new();
        binding.mark_rendered();
        binding.set_window(Box::new(window));

        binding.wake_frame();

        assert!(binding.needs_redraw(), "wake_frame must set needs_redraw");
        assert_eq!(
            redraw_count.load(Ordering::Relaxed),
            1,
            "wake_frame must call PlatformWindow::request_redraw exactly once"
        );
    }

    #[test]
    fn input_dispatches_through_the_exposed_gesture_binding() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use flui_interaction::PointerId;
        use flui_interaction::events::{PointerType, make_down_event_for_id};
        use flui_interaction::routing::{HitTestResult, PointerRouteHandler};
        use flui_types::geometry::{Offset, Pixels};

        // A handler registered on the gesture binding the public accessor
        // exposes must observe an event dispatched through that same binding —
        // proving registration and dispatch share ONE authoritative gesture
        // binding / arena, with no separate global instance to diverge.
        let app = AppBinding::instance();

        // A test-local pointer id keeps this isolated from other tests that
        // share the `AppBinding` singleton (its arena / router / hit-test maps),
        // and a per-pointer route is scoped to that id (unlike a global handler).
        let pointer = PointerId::new(9001).expect("nonzero pointer id");

        let fired = Arc::new(AtomicBool::new(false));
        let f = fired.clone();
        let handler: PointerRouteHandler = Rc::new(move |_| f.store(true, Ordering::Relaxed));
        app.gestures().pointer_router().add_route(pointer, handler);

        // Dispatch straight through the accessor-exposed binding via the
        // explicit-result path, which bypasses hit testing (no renderer lock,
        // no simultaneous-pointer cap that other tests could exhaust).
        let event = make_down_event_for_id(
            pointer,
            Offset::new(Pixels(10.0), Pixels(10.0)),
            PointerType::Touch,
        );
        app.gestures()
            .handle_pointer_event_with_result(&event, &HitTestResult::new());

        assert!(
            fired.load(Ordering::Relaxed),
            "the binding AppBinding::gestures() exposes must dispatch the event it is handed"
        );

        // Shared process singleton — clean up this pointer's route + arena entry.
        app.gestures().pointer_router().remove_all_routes(pointer);
        app.gestures().sweep_arena(pointer);
    }

    // ========================================================================
    // service_child_requests wiring tests
    // ========================================================================

    /// Wiring test: `AppBinding::draw_frame` must invoke
    /// `WidgetsBinding::service_child_requests`, which drains the pipeline's
    /// `pending_child_requests` buffer. We verify the drain happened by:
    ///   1. Seeding one request via `push_pending_child_request_for_test`
    ///      (`#[cfg(test)]` helper on `PipelineOwner`).
    ///   2. Running one `draw_frame`.
    ///   3. Asserting the buffer is now empty — `take_pending_child_requests`
    ///      was called, proving the wiring is present.
    ///
    /// Without the `service_child_requests` call at ~line 460 of `draw_frame`,
    /// `take_pending_child_requests` is never called and the buffer remains
    /// non-empty after the frame. The test is RED without step-2 and GREEN with
    /// it; no root attach is needed.
    #[test]
    fn draw_frame_invokes_service_child_requests() {
        // A fresh binding so we avoid the singleton root-attach collision.
        let binding = AppBinding::new();
        let realm = test_realm(&binding);

        // Insert a dummy render object to obtain a valid RenderId (the pending
        // buffer stores `(RenderId, index)` pairs — any valid id works).
        let sliver_id = binding.shared_pipeline_owner.write().insert(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        )
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);

        // Seed one pending child-build request. The seeding helper is gated
        // behind flui-rendering's `testing` feature (enabled for this crate's
        // dev builds) — the test-only mirror of `SubtreeArena::request_child_build`.
        binding
            .shared_pipeline_owner
            .write()
            .push_pending_child_request_for_test(sliver_id, 0);

        // Verify the seed is present (precondition).
        {
            let mut guard = binding.shared_pipeline_owner.write();
            let drained = guard.take_pending_child_requests();
            assert_eq!(drained.len(), 1, "seed must be present before draw_frame");
            // Re-push so draw_frame sees it.
            guard.push_pending_child_request_for_test(sliver_id, 0);
        }

        // Run one draw_frame.  No root render object is attached (fresh binding)
        // so no scene is produced, but the service path must still be traversed.
        let _ = binding.draw_frame(
            &realm,
            flui_rendering::constraints::BoxConstraints::tight(flui_types::Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            )),
        );

        // After draw_frame the pending buffer must be empty — drained by
        // `service_child_requests`.  Without the wiring the buffer is never
        // drained and this take returns a non-empty vec.
        let remaining = binding
            .shared_pipeline_owner
            .write()
            .take_pending_child_requests();
        assert!(
            remaining.is_empty(),
            "draw_frame must drain pending_child_requests via service_child_requests; \
             {} request(s) remained undrained — wiring is absent",
            remaining.len(),
        );
    }

    // ========================================================================
    // Async-driver ownership (historically coordinated between layers)
    // ========================================================================

    /// Serializes the tests that drive the process-global `Scheduler::instance()`.
    ///
    /// CI runs nextest, which gives each test its own process, so this is belt and
    /// braces there. Plain `cargo test` (a stated gate for this crate) runs them on
    /// threads in one process, where two tests each opening a scheduler frame on the
    /// singleton would interleave — the same class of hazard `SEMANTICS_TEST_LOCK`
    /// guards in `flui-app` (AGENTS.md, "Testing quirks").
    static SINGLETON_FRAME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The production frame path polls the async driver **exactly once**, on the
    /// `Scheduler::instance()` singleton, in the mid-frame slot — and the pipeline
    /// runs afterwards, in the persistent slot.
    ///
    /// Replaces `draw_frame_invokes_the_async_driver_step`, which pinned the poll
    /// *inside* `draw_frame`. That location was the bug: `drive_async_tasks`
    /// debug-asserts it never runs during `PersistentCallbacks`, which is exactly
    /// the phase the pipeline must occupy for post-frame callbacks to observe its
    /// layout. The scheduler owns the step now. Historically this
    /// real invariant — one mid-frame poll per frame, on the right instance — is
    /// what this asserts.
    #[test]
    fn the_production_frame_polls_the_singletons_async_driver_once_before_the_pipeline() {
        let _serialized = SINGLETON_FRAME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let scheduler = flui_scheduler::Scheduler::instance();

        let polls = Arc::new(AtomicUsize::new(0));
        let polls_for_task = Arc::clone(&polls);
        let _token = scheduler.spawn_local(Box::pin(async move {
            polls_for_task.fetch_add(1, Ordering::Release);
        }));
        assert_eq!(
            polls.load(Ordering::Acquire),
            0,
            "spawn must not poll inline"
        );

        let polled_before_pipeline = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&polled_before_pipeline);
        let polls_probe = Arc::clone(&polls);

        scheduler.drive_frame(flui_scheduler::Instant::now(), || {
            // The pipeline's slot. The driver poll already happened, in
            // `handle_begin_frame`'s mid-frame slot.
            flag.store(polls_probe.load(Ordering::Acquire) == 1, Ordering::Release);
            let _ = binding.draw_frame(
                &realm,
                flui_rendering::constraints::BoxConstraints::tight(flui_types::Size::new(
                    flui_types::geometry::px(800.0),
                    flui_types::geometry::px(600.0),
                )),
            );
        });

        assert!(
            polled_before_pipeline.load(Ordering::Acquire),
            "the async driver must be polled before the pipeline runs"
        );
        assert_eq!(
            polls.load(Ordering::Acquire),
            1,
            "exactly one driver poll per frame"
        );
    }

    /// `draw_frame` no longer polls the driver itself. Stated as a test so the
    /// call cannot quietly come back and re-break the phase invariant.
    #[test]
    fn draw_frame_does_not_poll_the_async_driver_itself() {
        let _serialized = SINGLETON_FRAME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use std::sync::atomic::{AtomicBool, Ordering};

        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let ran = Arc::new(AtomicBool::new(false));
        let ran_for_task = Arc::clone(&ran);
        let _token = flui_scheduler::Scheduler::instance().spawn_local(Box::pin(async move {
            ran_for_task.store(true, Ordering::Release);
        }));

        let _ = binding.draw_frame(
            &realm,
            flui_rendering::constraints::BoxConstraints::tight(flui_types::Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            )),
        );

        assert!(
            !ran.load(Ordering::Acquire),
            "the driver step belongs to Scheduler::handle_begin_frame, not to the pipeline"
        );
    }

    /// **The production-path acceptance test.** A post-frame callback on the
    /// `Scheduler::instance()` singleton observes the geometry `AppBinding`'s real
    /// pipeline committed **in the same frame**.
    ///
    /// This is the production twin of
    /// `flui-binding`'s `post_frame_callback_runs_after_layout_in_the_same_pumped_frame`.
    /// The runner drains the post-frame queue
    /// `render_frame`, so the callback saw the previous frame's layout.
    ///
    /// No GPU: `draw_frame(constraints)` is the pipeline; `render_frame` only adds
    /// the raster submission on top of it.
    #[test]
    fn production_post_frame_callback_observes_this_frames_committed_layout() {
        let _serialized = SINGLETON_FRAME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use std::sync::atomic::{AtomicUsize, Ordering};

        use flui_rendering::prelude::Leaf;
        use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, PaintCx, RenderBox};
        use flui_types::Size;
        use flui_types::geometry::px;

        #[derive(Debug, Default)]
        struct FixedBox;
        impl flui_foundation::Diagnosticable for FixedBox {}
        impl RenderBox for FixedBox {
            type Arity = Leaf;
            type ParentData = BoxParentData;
            fn perform_layout(
                &mut self,
                _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
            ) -> Size {
                Size::new(px(40.0), px(24.0))
            }
            fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
        }

        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let pipeline = binding.render_pipeline_arc();

        let root = {
            let mut owner = pipeline.write();
            let root = owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(FixedBox));
            owner.set_root_id(Some(root));
            root
        };

        assert_eq!(
            pipeline.read().box_size(root),
            None,
            "nothing is laid out before the first frame"
        );

        let observed = Arc::new(RwLock::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_cb = Arc::clone(&observed);
        let calls_cb = Arc::clone(&calls);
        let pipeline_cb = Arc::clone(&pipeline);

        let scheduler = flui_scheduler::Scheduler::instance();
        scheduler.add_post_frame_callback(Box::new(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            *observed_cb.write() = pipeline_cb.read().box_size(root);
        }));

        scheduler.drive_frame(flui_scheduler::Instant::now(), || {
            let _ = binding.draw_frame(
                &realm,
                flui_rendering::constraints::BoxConstraints::new(
                    px(0.0),
                    px(200.0),
                    px(0.0),
                    px(200.0),
                ),
            );
        });

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            *observed.read(),
            Some(Size::new(px(40.0), px(24.0))),
            "the production post-frame callback must observe THIS frame's layout"
        );
    }

    /// A second `AppBinding::new()` (a throwaway test binding, constructed
    /// alongside the real thread-local singleton) must not steal the
    /// process-global `Scheduler::instance()`'s animation wake hook away
    /// from the singleton — it used to, because `AppBinding::new()` closed
    /// the hook over that ONE construction's own `wake_handle`, and every
    /// later construction on this thread overwrote it.
    ///
    /// Drives the real mechanism end to end: `Scheduler::instance().request_frame()`
    /// firing `on_frame_scheduled` (the same hook a running `AnimationController`
    /// uses) after a throwaway binding was constructed, asserting the hook
    /// still reaches `AppBinding::instance()` (the real singleton) rather
    /// than the throwaway.
    ///
    /// Red-check: revert `AppBinding::new()`'s scheduler-hook line back to
    /// `wake_handle.into_callback()` and this fails — the throwaway
    /// binding's construction below steals the hook, so the real
    /// singleton's `needs_redraw` never flips.
    #[test]
    fn a_second_binding_does_not_steal_the_real_singletons_scheduler_wake_hook() {
        let _serialized = SINGLETON_FRAME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Ensure the real singleton exists on this thread (installs the hook,
        // if some earlier test in this process has not already done so) and
        // start from a known, settled state.
        let real = AppBinding::instance();
        let scheduler = flui_scheduler::Scheduler::instance();
        if scheduler.is_frame_scheduled() {
            scheduler.drive_frame(flui_scheduler::Instant::now(), || {});
        }
        real.mark_rendered();

        // Construct a throwaway binding — pre-fix, this call would silently
        // rebind the scheduler's wake hook to ITS OWN (windowless, about to
        // be dropped) wake handle instead of the real singleton's.
        let _throwaway = AppBinding::new();

        scheduler.request_frame();

        assert!(
            real.needs_redraw(),
            "constructing a second AppBinding must not steal the scheduler's \
             animation wake hook away from the real singleton — an animation \
             driven through Scheduler::instance() would otherwise freeze with \
             no diagnostic the first time a test built an extra binding",
        );
    }

    // ========================================================================
    // layout-builder seam wiring test
    // ========================================================================

    /// Wiring test: `AppBinding::draw_frame` must run the shared layout<->build
    /// fixpoint (`BuildOwner::run_frame_with_layout_builders`), not a bare
    /// `PipelineOwner::run_frame`.
    ///
    /// This test plants a registry entry by hand rather than mounting a real
    /// `LayoutBuilder`, so it stays a pure wiring test of the frame path. `service_layout_builders` prunes entries
    /// whose element and render node do not exist, on every pass, before
    /// anything is built; that prune is the observable side effect.
    ///
    /// `flui-binding` carries the mirror test for `HeadlessBinding::pump_frame`.
    /// If either frame path stopped calling the shared helper, exactly one of
    /// the two would fail — which is the headless↔production divergence this
    /// pair exists to catch.
    #[test]
    fn draw_frame_invokes_the_layout_builder_seam() {
        let binding = AppBinding::new();
        let realm = test_realm(&binding);

        // Plant a stale entry: neither the element nor the render node exists.
        realm.widgets().with_build_owner_mut(|owner| {
            let _cell = owner.register_layout_builder_for_test(
                flui_foundation::RenderId::new(1),
                flui_foundation::ElementId::new(1),
            );
            assert_eq!(owner.layout_builder_count(), 1);
        });

        let _ = binding.draw_frame(
            &realm,
            flui_rendering::constraints::BoxConstraints::tight(flui_types::Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            )),
        );

        realm.widgets().with_build_owner_mut(|owner| {
            assert_eq!(
                owner.layout_builder_count(),
                0,
                "draw_frame must run service_layout_builders (via the shared \
                 run_frame_with_layout_builders helper), which prunes the stale entry"
            );
        });
    }

    /// Wake-gate contract: after a frame marks a render node dirty (simulating
    /// what `service_child_requests` does to a sliver after building new
    /// children), `has_pending_work()` must return `true` so the runner gate
    /// (`runner.rs:225`: `needs_redraw() || has_pending_work()`) schedules the
    /// settling frame.
    ///
    /// Also asserts the quiescence direction: once no nodes are dirty,
    /// `has_pending_work()` is `false` and the app can go idle.
    ///
    /// # Wake-gate invariant
    ///
    /// The settling frame survives because `layout` marks the sliver dirty
    /// (`has_dirty_nodes`), NOT because the pending-request buffer is non-empty
    /// (`has_pending_work` does not consult `pending_child_requests` or
    /// `pending_retain_bands`). A future change that emits a child request
    /// WITHOUT calling `mark_needs_layout` would strand the settling frame —
    /// this test documents and guards that invariant.
    #[test]
    fn wake_gate_schedules_settling_frame_after_dirty_mark() {
        let binding = AppBinding::new();
        let realm = test_realm(&binding);

        // `mark_rendered` puts `needs_redraw` in a known state so the
        // has_pending_work assertion is insulated from any prior redraw state
        // set by other singleton tests sharing the same process.
        binding.mark_rendered();
        assert!(!binding.needs_redraw(), "precondition: needs_redraw clear");

        // Insert a node and mark it dirty — this is what service_child_requests
        // does to a sliver after building new children.
        let node_id = binding.shared_pipeline_owner.write().insert(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        )
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        binding
            .shared_pipeline_owner
            .write()
            .clear_all_dirty_nodes();
        // Confirm quiescence baseline: nothing dirty yet.
        assert!(
            !binding.has_pending_work(&realm),
            "baseline: no pending work after clearing dirty nodes",
        );

        // Mark the node dirty (as service_child_requests does after building children).
        binding
            .shared_pipeline_owner
            .write()
            .mark_needs_layout(node_id);

        // The runner gate reads has_pending_work(); it must be true now.
        assert!(
            binding.has_pending_work(&realm),
            "a dirty layout node must make has_pending_work() true so the runner \
             schedules the settling frame; this is the invariant that lazy-list \
             settling depends on (NOT the pending_child_requests buffer)",
        );

        // Once all dirty nodes are cleared (settled frame ran layout), the app
        // must go idle — no infinite redraw.
        binding
            .shared_pipeline_owner
            .write()
            .clear_all_dirty_nodes();
        assert!(
            !binding.has_pending_work(&realm),
            "after clearing dirty nodes has_pending_work() must be false so a \
             settled lazy-list app does not loop forever",
        );
    }

    // ========================================================================
    // Vsync wiring tests (production frame continuation)
    // ========================================================================
    //
    // All tests below use `AppBinding::new()` (fresh non-singleton binding),
    // register controllers DIRECTLY into the binding's Vsync (no widget tree,
    // no root-attach, no flui-widgets dep), and inject time via
    // `set_now_secs_for_test`.  `--test-threads=1` is enforced workspace-wide
    // (see AGENTS.md) so there are no singleton collision races.
    //
    // Panel constraint (harsh-critic): the "continuation" test must be RED
    // without the `has_running() → wake_frame()` call in `draw_frame` and
    // GREEN with it.  The "value advances" test alone passes while a real
    // window freezes — it does NOT prove the fix.

    /// Helper: construct a fresh `AnimationController` with a private
    /// `Scheduler` (not the global singleton) so tests are isolated and can
    /// be safely parallelised if policy ever allows it.
    fn make_controller(duration_ms: u64) -> flui_animation::AnimationController {
        use std::{sync::Arc, time::Duration};

        flui_animation::AnimationController::new(
            Duration::from_millis(duration_ms),
            Arc::new(flui_scheduler::Scheduler::new()),
        )
    }

    /// Helper: a tight 800×600 constraint for `draw_frame` calls that need
    /// no real geometry.
    fn test_constraints() -> flui_rendering::constraints::BoxConstraints {
        flui_rendering::constraints::BoxConstraints::tight(flui_types::Size::new(
            flui_types::geometry::px(800.0),
            flui_types::geometry::px(600.0),
        ))
    }

    /// V1 — **Frame continuation (the key test)**
    ///
    /// A running controller registered in the binding's Vsync must keep the
    /// runner gate (`needs_redraw() || has_pending_work()`) schedulable
    /// (true) across every mid-animation frame, and the gate must go idle
    /// (false) once the controller completes.
    ///
    /// Without the `has_running() → wake_frame()` call added to `draw_frame`,
    /// `mark_rendered` clears `needs_redraw` after the first frame, the build
    /// heap drains, and neither `needs_redraw` nor `has_pending_work` stays
    /// true — the runner gate returns false and the animation freezes.  This
    /// test is RED without step-3 of the implementation plan and GREEN with it.
    #[test]
    fn vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle() {
        use flui_animation::{Animation, AnimationStatus};

        // Fresh non-singleton binding so this test does not race with others
        // that share the `AppBinding::instance()` singleton's redraw state.
        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let vsync = binding.vsync();

        // 100 ms controller, registered directly (no widget tree).
        let controller = make_controller(100);
        vsync.register(controller.clone());
        controller.forward().expect("fresh controller forwards");

        let constraints = test_constraints();

        // --- Frame at t=0.0s: anchor frame ---
        // `tick_all` at t=0 anchors the run start; controller value should
        // be ~0.  After draw_frame `wake_frame` is called (controller is still
        // running at t=0 since `tick_at(0.0)` keeps it at start), so
        // `needs_redraw` is set.
        binding.set_now_secs_for_test(0.0);
        binding.mark_rendered(); // known state before the frame
        let _ = binding.draw_frame(&realm, constraints);

        assert!(
            binding.needs_redraw() || binding.has_pending_work(&realm),
            "V1: the runner gate must be open after an anchor frame — a running \
             controller (100 ms, t=0) must schedule the next frame via wake_frame",
        );

        // --- Frame at t=0.05s: mid-animation ---
        binding.set_now_secs_for_test(0.05);
        binding.mark_rendered();
        let _ = binding.draw_frame(&realm, constraints);

        let gate_mid = binding.needs_redraw() || binding.has_pending_work(&realm);
        assert!(
            gate_mid,
            "V1: runner gate must remain open at t=0.05s (controller still running \
             at ~50%% progress); gate was false — animation would freeze here without \
             the continuation wiring",
        );

        let mid_value = controller.value();
        assert!(
            mid_value > 0.1 && mid_value < 0.95,
            "V1: sanity — controller is mid-run at t=50ms (value={mid_value})",
        );

        // --- Frame at t=0.2s: beyond the 100 ms duration, controller completes ---
        binding.set_now_secs_for_test(0.20);
        binding.mark_rendered();
        let _ = binding.draw_frame(&realm, constraints);

        assert_eq!(
            controller.status(),
            AnimationStatus::Completed,
            "V1: controller must be Completed after t=200ms (duration=100ms)",
        );

        // Once every controller settles, `has_running()` is false, so `draw_frame`
        // does NOT call `wake_frame` — and the runner gate must therefore be CLOSED
        // (the window quiesces; no infinite redraw after animations finish).
        //
        // We assert the GATE itself, not just the Vsync source. `mark_rendered()`
        // at line 1347 cleared `needs_redraw` BEFORE this settle frame, and this
        // fresh binding has no widget tree, so the only thing that could re-set
        // `needs_redraw` during the frame is the Vsync continuation's `wake_frame`.
        // `!needs_redraw()` therefore genuinely proves the Vsync path did NOT wake
        // — it would be RED if the continuation wrongly woke a settled controller.
        assert!(
            !binding.needs_redraw(),
            "V1: the runner gate must be CLOSED after settle — a completed \
             controller must NOT schedule another frame (window quiesces)",
        );
        assert!(
            !binding.has_vsync_running(),
            "V1: has_vsync_running() must be false once the controller completes",
        );

        controller.dispose();
    }

    /// V2 — **Value advances across injected-time frames**
    ///
    /// The controller's value must change across successive `draw_frame` calls
    /// with increasing virtual time.  This documents that `tick_all` in
    /// `draw_frame` actually advances the controller; it does NOT by itself
    /// prove the window stays alive (V1 does that).
    #[test]
    fn vsync_value_advances_across_frames() {
        use flui_animation::Animation;

        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let vsync = binding.vsync();
        let controller = make_controller(200);
        vsync.register(controller.clone());
        controller.forward().expect("fresh controller forwards");

        let constraints = test_constraints();

        // Frame 1 — anchor at t=0.
        binding.set_now_secs_for_test(0.0);
        let _ = binding.draw_frame(&realm, constraints);
        let v0 = controller.value();

        // Frame 2 — t=0.1 s → 50 % of a 200 ms run.
        binding.set_now_secs_for_test(0.10);
        let _ = binding.draw_frame(&realm, constraints);
        let v1 = controller.value();

        assert!(
            v1 > v0,
            "V2: controller value must increase across frames as virtual time advances \
             (v0={v0}, v1={v1}); tick_all is not running before draw_frame",
        );
        assert!(
            (v1 - 0.5).abs() < 0.05,
            "V2: at t=100ms / 200ms run the value should be near 0.5 (got {v1})",
        );

        controller.dispose();
    }

    /// V3 — **Exactly-once-per-frame** (no double-advance)
    ///
    /// A single `draw_frame` call must call `tick_all` exactly once — not
    /// zero times (animation stalls) and not twice (double-advance).  We
    /// verify this by checking that a 100 ms controller is NOT completed after
    /// a frame at t=50 ms (would only complete early if double-ticked past
    /// 100 ms) and IS completed after a frame at t=150 ms.
    ///
    /// The disjoint-set invariant (Vsync controllers NEVER appear in the
    /// global Scheduler's ticker set) is also the safety argument for why a
    /// single `tick_all` here does not double-advance anything: the two sets
    /// are non-overlapping by construction (implicit controllers carry a
    /// private throwaway Scheduler, not the global singleton).
    #[test]
    fn vsync_tick_exactly_once_per_frame() {
        use flui_animation::{Animation, AnimationStatus};

        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let vsync = binding.vsync();
        let controller = make_controller(100);
        vsync.register(controller.clone());
        controller.forward().expect("fresh controller forwards");

        let constraints = test_constraints();

        // Anchor frame at t=0.
        binding.set_now_secs_for_test(0.0);
        let _ = binding.draw_frame(&realm, constraints);

        // At t=0.05s (50 ms into a 100 ms run): NOT yet complete.
        // If `tick_all` were called twice, elapsed would appear as ~100 ms
        // and the controller would snap to Completed — failing this assert.
        binding.set_now_secs_for_test(0.05);
        let _ = binding.draw_frame(&realm, constraints);
        assert_ne!(
            controller.status(),
            AnimationStatus::Completed,
            "V3: controller must NOT be complete at t=50ms (100ms duration); \
             a double-tick would falsely advance it to completion",
        );

        // At t=0.15s (150 ms, past the 100 ms duration): must complete.
        binding.set_now_secs_for_test(0.15);
        let _ = binding.draw_frame(&realm, constraints);
        assert_eq!(
            controller.status(),
            AnimationStatus::Completed,
            "V3: controller must be Completed after t=150ms (duration=100ms)",
        );

        controller.dispose();
    }

    // ========================================================================
    // A-series — auto-wrap VsyncScope tests (core5-vsync-autowrap)
    // ========================================================================
    //
    // These tests verify that `attach_root_widget*` auto-injects a `VsyncScope`
    // so an implicitly-animated widget below the root registers its controller
    // into the binding's vsync WITHOUT any app-author boilerplate.
    //
    // All three use a fresh `AppBinding::new()` (not the singleton) to avoid
    // re-attach collisions.  `--test-threads=1` is enforced workspace-wide.

    // -----------------------------------------------------------------------
    // Test helper: `VsyncProbeView` — a `StatefulView` whose `init_state`
    // reads the ambient `VsyncScope` and registers a caller-supplied
    // `AnimationController` into it, then calls `forward()` so the controller
    // is running and ticking is observable.
    //
    // Placed here (module scope, inside `mod tests`) so all A-series tests share it.
    // -----------------------------------------------------------------------
    use flui_view::{IntoView, StatefulView, ViewState};

    /// Test-local view that captures the auto-injected `VsyncScope` in
    /// `init_state`, registers a caller-supplied controller, and starts it
    /// running so subsequent `draw_frame` calls advance its value.
    ///
    /// Used exclusively to verify the auto-wrap chain; not a real widget.
    #[derive(Clone)]
    struct VsyncProbeView {
        /// The controller to register via the `VsyncScope` found in `init_state`.
        controller_to_register: flui_animation::AnimationController,
    }

    /// State for `VsyncProbeView`.
    struct VsyncProbeState {
        controller: flui_animation::AnimationController,
    }

    impl StatefulView for VsyncProbeView {
        type State = VsyncProbeState;

        fn create_state(&self) -> Self::State {
            VsyncProbeState {
                controller: self.controller_to_register.clone(),
            }
        }
    }

    impl ViewState<VsyncProbeView> for VsyncProbeState {
        fn init_state(&mut self, ctx: &dyn flui_view::BuildContext) {
            // Mirror the VsyncScope LOOKUP path every real implicit widget uses
            // (`animated_opacity.rs:91` etc.): read the VsyncScope provided by
            // `attach_root_widget`'s auto-wrap and register our controller in it.
            // (The real widgets also store the `VsyncRegistration` to unregister
            // on `dispose`; this probe drops it — harmless, the registry is owned
            // by an isolated `AppBinding::new()` that drops at test end.)
            if let Some(vsync) =
                ctx.get::<flui_widgets::VsyncScope, _>(|scope| scope.vsync().clone())
            {
                vsync.register(self.controller.clone());
                // Start the controller so `has_running()` / value-advance is
                // observable in A2 — without forward() the controller stays
                // Dismissed and tick_all is a no-op for it.
                self.controller.forward().ok();
            }
        }

        fn build(
            &self,
            _view: &VsyncProbeView,
            _ctx: &dyn flui_view::BuildContext,
        ) -> impl IntoView {
            // Leaf — no children to build; only the probe's init_state matters.
            LeafView
        }
    }

    impl View for VsyncProbeView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }

    /// Construct a `VsyncProbeView` with a fresh 200 ms controller on an
    /// isolated (non-global) `Scheduler`. Returns the view and the controller
    /// handle so tests can inspect value / dispose after the frame.
    fn make_vsync_probe() -> (VsyncProbeView, flui_animation::AnimationController) {
        use std::{sync::Arc, time::Duration};
        let controller = flui_animation::AnimationController::new(
            Duration::from_millis(200),
            Arc::new(flui_scheduler::Scheduler::new()),
        );
        let view = VsyncProbeView {
            controller_to_register: controller.clone(),
        };
        (view, controller)
    }

    /// A1 — **Auto-wrap causes registration (the key red→green test)**
    ///
    /// `attach_root_widget` must inject a `VsyncScope` so a descendant widget's
    /// `init_state` (which calls `ctx.get::<VsyncScope, _>(...)`) finds it and
    /// registers its controller into `binding.vsync()`.
    ///
    /// **Without** the auto-wrap: no `VsyncScope` above the root →
    /// `ctx.get::<VsyncScope>()` returns `None` → registration never happens →
    /// `binding.vsync().len()` stays `0` after a build pass. This is genuinely RED
    /// on a build of this file that omits the `VsyncScope::new(...)` wrap.
    ///
    /// **With** the auto-wrap: scope is present → `init_state` registers →
    /// `len() > 0`. The assertion is causal: the build pass (draw_frame Phase 1)
    /// is what runs `init_state`; the check before draw_frame is `0` in both worlds.
    #[test]
    fn a1_autowrap_causes_registration_after_build_pass() {
        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let (probe, controller) = make_vsync_probe();

        binding
            .attach_root_widget(&realm, &probe)
            .expect("a fresh AppBinding must accept its first root widget");

        // Before draw_frame: init_state has not run → no registration yet.
        assert!(
            binding.vsync().is_empty(),
            "A1 precondition: controller must not be registered before the first build pass",
        );

        // draw_frame Phase 1 (build_scope) triggers mount → init_state → registration.
        let _ = binding.draw_frame(&realm, test_constraints());

        assert!(
            !binding.vsync().is_empty(),
            "A1: after a build pass the controller registered in init_state must appear \
             in binding.vsync(); empty means the auto-injected VsyncScope was absent \
             — the wrap in attach_root_widget is missing or broken",
        );

        controller.dispose();
    }

    /// A2 — **End-to-end tick: auto-wrap → register → tick → value advances**
    ///
    /// After the build pass runs `init_state` (which reads the auto-injected
    /// `VsyncScope` and registers + starts the controller), `draw_frame` calls
    /// `tick_all` before each build phase. The controller's value must advance
    /// across successive frames, proving the full chain: attach_root_widget
    /// auto-wrap → VsyncScope in tree → `init_state` registers →
    /// `tick_all` in draw_frame advances the value.
    ///
    /// ## Tick-anchor ordering
    ///
    /// `tick_all` runs BEFORE `build_scope` inside each `draw_frame`. The
    /// controller is registered in `init_state`, which executes during `build_scope`
    /// of frame 1. Therefore:
    ///
    /// - Frame 1 (t=0.0): `tick_all(0.0)` fires with no controller yet;
    ///   build_scope runs → registration + `forward()`. Value stays at `0`.
    /// - Frame 2 (t=0.1): `tick_all(0.1)` sees the new run-generation, anchors
    ///   `run_start = 0.1`, elapsed = 0 → value stays near `0` (anchor frame).
    /// - Frame 3 (t=0.2): `tick_all(0.2)` computes elapsed = 0.1 s on a 200 ms
    ///   controller → ~50 % progress → value advances to ~0.5.
    ///
    /// This 3-frame sequence is the minimal proof of the end-to-end chain.
    #[test]
    fn a2_autowrap_end_to_end_tick_advances_controller_value() {
        use flui_animation::Animation as _;

        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        let (probe, controller) = make_vsync_probe();
        binding
            .attach_root_widget(&realm, &probe)
            .expect("a fresh AppBinding must accept its first root widget");

        // Frame 1 (t=0.0): build pass runs init_state → registration + forward().
        // tick_all fires before build_scope, so the controller is not yet known; value = 0.
        binding.set_now_secs_for_test(0.0);
        let _ = binding.draw_frame(&realm, test_constraints());
        assert!(
            !binding.vsync().is_empty(),
            "A2 precondition: controller must be registered after the first build pass",
        );

        // Frame 2 (t=0.1): tick_all observes the new run-generation and sets run_start=0.1;
        // elapsed = 0.1 - 0.1 = 0 → this is the anchor frame; value stays near 0.
        binding.set_now_secs_for_test(0.1);
        let _ = binding.draw_frame(&realm, test_constraints());
        let value_after_anchor = controller.value();

        // Frame 3 (t=0.2): elapsed = 0.2 - 0.1 = 0.1 s on a 200 ms run → ~50 % progress.
        // If the chain is intact, value must be strictly above the anchor-frame value.
        binding.set_now_secs_for_test(0.2);
        let _ = binding.draw_frame(&realm, test_constraints());
        let value_at_50_percent = controller.value();

        assert!(
            value_at_50_percent > value_after_anchor,
            "A2: controller value must advance from the anchor frame to t=0.2 s \
             (anchor={value_after_anchor}, t=200ms={value_at_50_percent}); \
             equal values mean tick_all did not reach the registered controller — \
             either the VsyncScope was absent (auto-wrap broken) or tick_all was skipped",
        );

        controller.dispose();
    }

    /// A3 — **No-animation root: auto-wrap registers nothing itself**
    ///
    /// A root with no implicitly-animated widget (plain `LeafView`) must leave
    /// `binding.vsync()` empty after a build pass. The auto-injected `VsyncScope`
    /// is present in the tree but passively provides; no self-registration occurs.
    /// This verifies the wrapper is inert for apps that don't use implicit animations.
    #[test]
    fn a3_no_animation_root_vsync_stays_empty_after_build_pass() {
        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        binding
            .attach_root_widget(&realm, &LeafView)
            .expect("a fresh AppBinding must accept its first root widget");

        // Build pass runs — VsyncScope is mounted but LeafView has no init_state
        // that reads it, so no registration occurs.
        let _ = binding.draw_frame(&realm, test_constraints());

        assert!(
            binding.vsync().is_empty(),
            "A3: a root with no implicitly-animated widgets must not register anything \
             into binding.vsync(); the VsyncScope wrapper must not self-register",
        );
    }

    /// V4 — **No-animation app idles cheaply**
    ///
    /// An empty `Vsync` (no registered controllers) must make `tick_all` and
    /// `has_running()` both no-ops that do NOT flip `needs_redraw` via the
    /// Vsync path.  The runner gate can go idle between frames when nothing
    /// is running.
    #[test]
    fn vsync_empty_does_not_keep_gate_open() {
        let binding = AppBinding::new();
        let realm = test_realm(&binding);
        // No controllers registered.
        assert!(binding.vsync().is_empty(), "precondition: Vsync is empty");

        let constraints = test_constraints();
        binding.set_now_secs_for_test(1.0);
        binding.mark_rendered(); // clear any redraw flag

        // `draw_frame` must not set `needs_redraw` through the Vsync path when
        // no controllers are registered.
        let _ = binding.draw_frame(&realm, constraints);

        assert!(
            !binding.has_vsync_running(),
            "V4: has_vsync_running() must be false when no controllers are registered",
        );
        // `needs_redraw` may be set by OTHER paths (the pipeline-owner dirty hook
        // fires when the new binding's PipelineOwner is touched).  We assert only
        // the Vsync-specific gate: has_vsync_running is false.
    }

    /// `render_frame_entered`'s retry gate: a frame dropped mid-render (surface
    /// lost) must not be treated as settled. Drives `render_frame_entered`
    /// directly against a scripted `RasterBackend` — no GPU device required —
    /// covering the error arm the audit found had zero tests.
    mod frame_retry_semantics {
        use flui_types::geometry::{Pixels, Rect};

        use super::*;

        /// A `RasterBackend` whose `render_scene` outcome is fixed at
        /// construction, so a test can force the exact arm
        /// `render_frame_entered` must handle without a real GPU device.
        struct ScriptedRasterBackend {
            /// Consumed on the first `render_scene` call — a second call
            /// within one of these single-frame tests would be a bug.
            outcome: Option<Result<bool, EngineError>>,
            render_scene_calls: u32,
        }

        impl ScriptedRasterBackend {
            fn new(outcome: Result<bool, EngineError>) -> Self {
                Self {
                    outcome: Some(outcome),
                    render_scene_calls: 0,
                }
            }
        }

        impl RasterBackend for ScriptedRasterBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                self.render_scene_calls += 1;
                self.outcome
                    .take()
                    .expect("render_scene called more than once in a single-frame test")
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
            }
            fn mark_dirty(&mut self, _rect: Rect<Pixels>) {}
            fn mark_full_repaint(&mut self) {}
            fn has_damage(&self) -> bool {
                true
            }
            fn size(&self) -> (u32, u32) {
                (800, 600)
            }
            fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
                Ok(())
            }
        }

        /// Mounts a root so the pipeline actually paints a non-empty scene —
        /// the SurfaceLost/success arms under test only run inside
        /// `render_frame_entered`'s `scene.has_content()` gate.
        fn mount_root(app: &AppBinding) -> super::super::super::ui_realm::UiRealm {
            let realm = test_realm(app);
            realm
                .enter(|realm| app.attach_root_widget(realm, &LeafView))
                .expect("attach succeeds");
            realm
        }

        /// Red-check: replace the `retry_needed` branch with an unconditional
        /// `self.mark_rendered()` (the pre-fix shape) and this fails —
        /// `needs_redraw` comes back `false` after a dropped `SurfaceLost`
        /// frame, so nothing would ever re-drive a static UI back to life.
        #[test]
        fn surface_lost_keeps_needs_redraw_armed_for_a_retry() {
            let app = AppBinding::new();
            let realm = mount_root(&app);
            let mut backend = ScriptedRasterBackend::new(Err(EngineError::SurfaceLost));

            app.mark_rendered(); // known state before the frame
            let presented = app.render_frame_entered(&realm, &mut backend);

            assert!(!presented, "a SurfaceLost frame never reaches present()");
            assert_eq!(
                backend.render_scene_calls, 1,
                "precondition: the mounted scene actually reached render_scene"
            );
            assert!(
                app.needs_redraw(),
                "a dropped SurfaceLost frame must re-arm needs_redraw so the next wake \
                 actually retries — 'will retry next frame' must be a mechanism, not \
                 just a comment"
            );
        }

        /// Control case: a successful, presented frame must still clear
        /// `needs_redraw`, exactly as before this fix.
        #[test]
        fn a_successful_frame_still_clears_needs_redraw() {
            let app = AppBinding::new();
            let realm = mount_root(&app);
            let mut backend = ScriptedRasterBackend::new(Ok(true));

            app.request_redraw(); // simulate the wake that scheduled this frame
            let presented = app.render_frame_entered(&realm, &mut backend);

            assert!(presented, "Ok(true) means render_scene reached present()");
            assert!(
                !app.needs_redraw(),
                "a successfully presented frame must clear needs_redraw, same as \
                 before this fix"
            );
        }
    }

    /// `AppBinding::attach_text_input`/`detach_text_input` (the `ImeBackend`
    /// bridge) end-to-end against a headless window backed by
    /// `flui_platform::FakeTextInput`, plus `handle_input`'s
    /// `PlatformInput::Ime` → `TextInputRegistry` dispatch.
    mod ime_binding_bridge {
        use std::cell::RefCell;

        use flui_types::ImeEvent;

        use super::*;

        /// `TextInputRegistry::global()` is a thread-local singleton
        /// (matching `FocusManager`'s shape). Rust's default test harness
        /// reuses worker threads across tests, so two of these tests could
        /// otherwise observe each other's leftover attach/detach state on a
        /// shared thread — the same class of hazard `SEMANTICS_TEST_LOCK`
        /// guards against in `renderer_binding.rs` (AGENTS.md, "Testing
        /// quirks"). Held for each test's duration.
        static TEXT_INPUT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        /// A headless window plus a live handle to the same
        /// `FakeTextInput` its `PlatformWindow::text_input()` returns, so a
        /// test can drive the binding and assert exactly what the platform
        /// side recorded.
        fn headless_window_with_ime() -> (
            Box<dyn PlatformWindow>,
            Arc<dyn flui_platform::traits::PlatformTextInput>,
        ) {
            let platform = flui_platform::headless_platform();
            let window = platform
                .open_window(flui_platform::traits::WindowOptions::default())
                .expect("headless platform always opens a window");
            let text_input = window
                .text_input()
                .expect("headless backend supports PlatformTextInput");
            (window, text_input)
        }

        fn fake_text_input(
            text_input: &Arc<dyn flui_platform::traits::PlatformTextInput>,
        ) -> &flui_platform::FakeTextInput {
            text_input
                .as_any()
                .downcast_ref::<flui_platform::FakeTextInput>()
                .expect("the headless backend's PlatformTextInput is a FakeTextInput")
        }

        /// Attach records `set_ime_allowed(true)`; preedit/commit events
        /// routed through `handle_input` reach the attached client with the
        /// exact delivered strings; detach from the still-active token
        /// records `set_ime_allowed(false)`.
        #[test]
        fn attach_dispatch_and_active_detach_round_trip_through_the_platform() {
            let _guard = TEXT_INPUT_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let (window, text_input) = headless_window_with_ime();
            let fake = fake_text_input(&text_input);

            let binding = AppBinding::new();
            binding.set_window(window);

            let received = Rc::new(RefCell::new(Vec::new()));
            let sink = Rc::clone(&received);
            let token = binding
                .attach_text_input(Rc::new(move |event: &ImeEvent| {
                    sink.borrow_mut().push(event.clone());
                }))
                .expect("attach_text_input must succeed once a window is set");

            assert_eq!(
                fake.last_ime_allowed(),
                Some(true),
                "attach must enable platform IME composition"
            );

            binding.handle_input(PlatformInput::Ime(ImeEvent::Preedit {
                text: "ni".to_string(),
                cursor: Some((0, 2)),
            }));
            binding.handle_input(PlatformInput::Ime(ImeEvent::Commit("你好".to_string())));

            assert_eq!(
                received.borrow().as_slice(),
                [
                    ImeEvent::Preedit {
                        text: "ni".to_string(),
                        cursor: Some((0, 2)),
                    },
                    ImeEvent::Commit("你好".to_string()),
                ],
                "handle_input must deliver the exact ImeEvent payload to the attached client"
            );

            binding.detach_text_input(token);
            assert_eq!(
                fake.last_ime_allowed(),
                Some(false),
                "detaching the active token must disable platform IME composition"
            );
        }

        /// The stale-detach race named in `TextInputRegistry`'s module doc:
        /// field A attaches, field B attaches (replacing A), and A's
        /// now-stale detach must record NOTHING on the platform side —
        /// only B's later, active-token detach may disable IME.
        #[test]
        fn a_stale_detach_records_nothing_on_the_platform() {
            let _guard = TEXT_INPUT_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let (window, text_input) = headless_window_with_ime();
            let fake = fake_text_input(&text_input);

            let binding = AppBinding::new();
            binding.set_window(window);

            let token_a = binding
                .attach_text_input(Rc::new(|_event: &ImeEvent| {}))
                .expect("attach_text_input must succeed once a window is set");
            assert_eq!(fake.ime_allowed_calls(), vec![true]);

            let token_b = binding
                .attach_text_input(Rc::new(|_event: &ImeEvent| {}))
                .expect("attach_text_input must succeed once a window is set");
            assert_eq!(
                fake.ime_allowed_calls(),
                vec![true, true],
                "attach-replaces still enables IME for the new client"
            );

            binding.detach_text_input(token_a);
            assert_eq!(
                fake.ime_allowed_calls(),
                vec![true, true],
                "a stale detach (token_a, already replaced by token_b) records nothing"
            );

            binding.detach_text_input(token_b);
            assert_eq!(
                fake.ime_allowed_calls(),
                vec![true, true, false],
                "the active token's detach still disables IME"
            );
        }

        /// End-to-end proof of the literal ADR-0030 claim `flui-widgets`'
        /// own `editable_text::tests` cannot make on their own (they wire
        /// `TextInputHandle` straight to `TextInputRegistry::global()`, with
        /// no `flui-app`/`PlatformWindow` involved — see that module's
        /// doc): a real, mounted `flui_widgets::EditableText`, focused
        /// through the same `FocusManager` singleton production keyboard
        /// routing uses, attaches through `AppBinding::attach_text_input`
        /// (via the `BuildContext::text_input_handle` capability
        /// `UiRealm::bind_to_app` installs) and actually toggles the
        /// platform's `set_ime_allowed` on the `FakeTextInput` — not merely
        /// the registry.
        ///
        /// Red-check: skip `UiRealm::bind_to_app`'s `set_text_input_handle`
        /// call — `EditableText::init_state`'s `ctx.text_input_handle()`
        /// then returns `None`, no attach happens, and this test's first
        /// assertion fails (`fake.last_ime_allowed()` stays `None`).
        #[test]
        fn a_mounted_editable_text_toggles_platform_ime_on_focus_and_blur() {
            let _guard = TEXT_INPUT_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            FocusManager::global().unfocus();

            let (window, text_input) = headless_window_with_ime();
            let fake = fake_text_input(&text_input);

            let binding = AppBinding::new();
            binding.set_window(window);
            let realm = test_realm(&binding);

            let controller = flui_widgets::TextEditingController::new();
            realm
                .enter(|realm| {
                    binding.attach_root_widget(
                        realm,
                        &flui_widgets::EditableText::new(controller.clone()),
                    )
                })
                .expect("attach succeeds");
            let _ = binding.draw_frame(&realm, test_constraints());

            let node_id = controller
                .focus_node_id()
                .expect("a mounted, enabled EditableText publishes its focus node");

            FocusManager::global().request_focus(node_id);
            assert_eq!(
                fake.last_ime_allowed(),
                Some(true),
                "focusing a mounted EditableText must attach through AppBinding \
                 and enable platform IME composition"
            );

            FocusManager::global().unfocus();
            assert_eq!(
                fake.last_ime_allowed(),
                Some(false),
                "blurring the field must detach and disable platform IME composition"
            );
        }

        /// `AppBinding::set_ime_cursor_area` (ADR-0032) reaches the active
        /// window's `PlatformTextInput` capability — the real-path proof
        /// `flui-widgets`' own IME cursor-area tests cannot make on their
        /// own (their harness wires `TextInputHandle` straight to an
        /// in-crate recorder, with no `flui-app`/`PlatformWindow` involved;
        /// see `test_harness`'s `mount_with_ime` doc). This exercises
        /// exactly the plumbing `UiRealm::bind_to_app`'s installed third
        /// closure calls through: `AppBinding::set_ime_cursor_area` ->
        /// `TextInputPlatformBridge::set_cursor_area` ->
        /// `PlatformTextInput::set_ime_cursor_area`.
        ///
        /// Deliberately does not mount a widget tree: this test proves the
        /// bridge FORWARDS an already-computed `Bounds` to the platform, not
        /// that a real `EditableText` computes the right one — that's the
        /// widget-level geometry/dedupe/lifecycle coverage
        /// (`flui-widgets`' `editable_text` module carries it). A bare
        /// `Bounds::new(...)` exercises the forwarding path with less setup
        /// and no render-tree dependency. (`AppBinding::attach_root_widget`'s
        /// `RootRenderElement` bootstrap does correctly connect a mounted
        /// subtree's render root under its `RenderViewAdapter` node in both
        /// directions — see `transform_to_resolves_through_the_root_hop_after_standard_bootstrap`
        /// above — so mounting one here would work; it is simply
        /// unnecessary for what this test asserts.)
        ///
        /// Red-check: turning `AppBinding::set_ime_cursor_area` into a
        /// no-op (dropping the `text_input_platform_bridge().
        /// set_cursor_area` call) makes `fake.cursor_area_calls()` stay
        /// empty.
        #[test]
        fn set_ime_cursor_area_reaches_the_active_windows_platform_capability() {
            let (window, text_input) = headless_window_with_ime();
            let fake = fake_text_input(&text_input);

            let binding = AppBinding::new();
            binding.set_window(window);

            let area = Bounds::new(
                flui_types::Point::new(px(10.0), px(20.0)),
                Size::new(px(2.0), px(18.0)),
            );
            binding.set_ime_cursor_area(area);

            assert_eq!(
                fake.cursor_area_calls(),
                vec![area],
                "set_ime_cursor_area must call through to the active window's \
                 PlatformTextInput::set_ime_cursor_area with the exact area"
            );
        }

        /// No active window yet (the loop's first tick fires before
        /// `set_window` ran) is a silent no-op — the same degradation
        /// contract `perform_haptic_feedback` documents, not a panic.
        #[test]
        fn set_ime_cursor_area_with_no_active_window_is_a_silent_no_op() {
            let binding = AppBinding::new();
            binding.set_ime_cursor_area(Bounds::new(
                flui_types::Point::new(px(0.0), px(0.0)),
                Size::new(px(1.0), px(1.0)),
            ));
        }
    }

    /// `AppBinding::perform_haptic_feedback` end-to-end against a headless
    /// window backed by `flui_platform::FakeHaptics`, plus the two silent
    /// no-op degradation cases (no active window; a window with no
    /// `PlatformHaptics` capability).
    mod haptics_binding_bridge {
        use super::*;

        /// A headless window plus a live handle to the same `FakeHaptics`
        /// its `PlatformWindow::haptics()` returns, so a test can drive
        /// the binding and assert exactly what the platform side recorded.
        fn headless_window_with_haptics() -> (
            Box<dyn PlatformWindow>,
            Arc<dyn flui_platform::traits::PlatformHaptics>,
        ) {
            let platform = flui_platform::headless_platform();
            let window = platform
                .open_window(flui_platform::traits::WindowOptions::default())
                .expect("headless platform always opens a window");
            let haptics = window
                .haptics()
                .expect("headless backend supports PlatformHaptics");
            (window, haptics)
        }

        fn fake_haptics(
            haptics: &Arc<dyn flui_platform::traits::PlatformHaptics>,
        ) -> &flui_platform::FakeHaptics {
            haptics
                .as_any()
                .downcast_ref::<flui_platform::FakeHaptics>()
                .expect("the headless backend's PlatformHaptics is a FakeHaptics")
        }

        /// A minimal `PlatformWindow` implementing only the trait's
        /// non-default methods, so `haptics()` falls through to the trait
        /// default (`None`) — the "backend with no haptics capability"
        /// case `perform_haptic_feedback` must degrade against silently.
        struct BareWindow;

        impl PlatformWindow for BareWindow {
            fn physical_size(
                &self,
            ) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
                flui_types::geometry::Size::default()
            }

            fn logical_size(&self) -> flui_types::geometry::Size<flui_types::Pixels> {
                flui_types::geometry::Size::default()
            }

            fn scale_factor(&self) -> f64 {
                1.0
            }

            fn request_redraw(&self) {}

            fn is_focused(&self) -> bool {
                false
            }

            fn is_visible(&self) -> bool {
                true
            }
        }

        /// Real-path proof: `perform_haptic_feedback` reads the binding's
        /// active window and calls through to its `PlatformHaptics`.
        ///
        /// Red-check: turning `perform_haptic_feedback` into a no-op
        /// (dropping the `with_window`/`perform` call) makes this fail —
        /// `fake.calls()` would stay empty.
        #[test]
        fn perform_haptic_feedback_reaches_the_active_windows_platform_capability() {
            let (window, haptics) = headless_window_with_haptics();
            let fake = fake_haptics(&haptics);

            let binding = AppBinding::new();
            binding.set_window(window);

            binding.perform_haptic_feedback(HapticFeedback::SelectionClick);

            assert_eq!(
                fake.calls(),
                vec![HapticFeedback::SelectionClick],
                "perform_haptic_feedback must call through to the active \
                 window's PlatformHaptics::perform"
            );
        }

        /// No active window yet (attached before `set_window` ran) is a
        /// silent no-op — no panic, nothing recorded anywhere to assert
        /// against beyond "this returned normally".
        #[test]
        fn perform_haptic_feedback_with_no_active_window_is_a_silent_no_op() {
            let binding = AppBinding::new();
            binding.perform_haptic_feedback(HapticFeedback::Vibrate);
        }

        /// An active window whose backend has no `PlatformHaptics`
        /// capability (desktop winit's shape, reproduced here without a
        /// real display) is also a silent no-op — Flutter's own
        /// `HapticFeedback` degradation contract, not a panic.
        #[test]
        fn perform_haptic_feedback_on_a_window_without_haptics_is_a_silent_no_op() {
            let binding = AppBinding::new();
            binding.set_window(Box::new(BareWindow));

            binding.perform_haptic_feedback(HapticFeedback::MediumImpact);
        }
    }
}
