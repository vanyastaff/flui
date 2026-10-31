//! The application's capability providers (ADR-0152): the built-in ones,
//! which this crate supplies as the composition root, and the registry a run
//! builds from them, the application's overrides and its plugins.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Weak},
};

use flui_interaction::{
    ClipboardBackend, ClipboardCapability, ClipboardHandle, PrimaryClaimId, PrimaryClaims,
    PrimarySelectionBackend, PrimarySelectionCapability, PrimarySelectionHandle, SelectionText,
};
use flui_native::{NativeClipboard, NativePrimarySelection};
use flui_platform::executor::{ComputeJob, Executor, IoFuture, SpawnError};
use flui_platform::{
    Storage, Unsupported, UnsupportedReason,
    capability::{
        Capability, CapabilityProvider, CapabilityRegistrar, Plugin, ProviderContext, Resolver,
    },
};
use flui_runtime::capability::{CapabilityConflict, CapabilityRegistry};
use flui_runtime::execution::ExecutionServices;
use flui_view::background::{Background, BackgroundCapability};
use flui_view::persist::{StorageCapability, StorageHandle};

/// The slot the owner platform's clipboard is installed in for a loop's life.
pub(crate) type ClipboardSlot = Rc<RefCell<Option<Rc<dyn NativeClipboard>>>>;

/// The capability providers an application registers before it runs
/// (ADR-0152): its overrides, which replace a built-in provider, and its
/// plugins, which provide what neither the application nor the framework
/// does.
///
/// Built on the owner thread and handed to the run through
/// [`AppLaunch::capabilities`](crate::AppLaunch::capabilities) or
/// [`Application`](crate::Application)'s own builder methods. Two overrides or
/// two plugins for one capability, or two capabilities sharing a name, fail
/// the run with a [`CapabilityConflict`] before the platform starts.
///
/// ```rust,ignore
/// let capabilities = AppCapabilities::new()
///     .capability::<ClipboardCapability>(MyClipboard::default())
///     .plugin(MyPlugin);
/// ```
#[derive(Default)]
pub struct AppCapabilities {
    overrides: CapabilityRegistrar,
    plugins: Vec<Box<dyn Plugin>>,
}

impl AppCapabilities {
    /// No override and no plugin: the run gets the built-in providers alone.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Provide the capability `C` with `provider`, in place of the built-in
    /// provider and of any plugin's. A widget acquires it with
    /// `cx.capability::<C>()` in a lifecycle hook.
    #[must_use = "the builder returns the updated registrations; assign or chain it"]
    pub fn capability<C: Capability>(mut self, provider: impl CapabilityProvider<C>) -> Self {
        self.overrides.provide::<C>(provider);
        self
    }

    /// Register the providers of `plugin`, for the capabilities neither the
    /// application nor the framework provides.
    #[must_use = "the builder returns the updated registrations; assign or chain it"]
    pub fn plugin(mut self, plugin: impl Plugin) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// The registry of a run: these overrides, then the built-ins over
    /// `slots`, then the plugins. A plugin's `register` is user code, so
    /// the caller holds no borrow of the application runtime.
    ///
    /// # Errors
    ///
    /// The first [`CapabilityConflict`] among them.
    pub(crate) fn into_registry(
        self,
        slots: &CapabilitySlots,
    ) -> Result<CapabilityRegistry, CapabilityConflict> {
        let mut builder = CapabilityRegistry::builder()
            .overrides(self.overrides)
            .built_ins(built_ins(slots));
        for plugin in &self.plugins {
            builder = builder.plugin(plugin.as_ref());
        }
        builder.build()
    }
}

impl std::fmt::Debug for AppCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCapabilities")
            .field("overrides", &self.overrides)
            .field("plugins", &self.plugins.len())
            .finish()
    }
}

/// The registry a run with no override and no plugin has.
pub(crate) fn built_in_registry(slots: &CapabilitySlots) -> CapabilityRegistry {
    CapabilityRegistry::builder()
        .built_ins(built_ins(slots))
        .build()
        .expect("BUG: the built-in providers name each capability once")
}

/// The providers this crate ships.
fn built_ins(slots: &CapabilitySlots) -> CapabilityRegistrar {
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<BackgroundCapability>(HostBackground {
        slot: Rc::clone(&slots.execution),
    });
    #[cfg(feature = "http")]
    registrar.provide::<flui_platform::http::HttpCapability>(super::http_host::HostHttp::new(
        Rc::clone(&slots.execution),
    ));
    registrar.provide::<StorageCapability>(HostStorageProvider {
        slot: slots.storage.clone(),
    });
    registrar.provide::<ClipboardCapability>(PlatformClipboard {
        slot: Rc::clone(&slots.clipboard),
    });
    // The primary selection is X11's and Wayland's alone.
    #[cfg(target_os = "linux")]
    registrar.provide::<PrimarySelectionCapability>(PlatformPrimarySelection {
        slot: Rc::clone(&slots.primary_selection),
    });
    registrar
}

/// The built-in clipboard: the owner platform's, while one is installed.
struct PlatformClipboard {
    slot: ClipboardSlot,
}

impl CapabilityProvider<ClipboardCapability> for PlatformClipboard {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        let native = self.slot.borrow().clone().ok_or_else(|| {
            // Between loops: no owner platform is installed.
            Unsupported::of::<ClipboardCapability>(UnsupportedReason::Unavailable)
        })?;
        Ok(ClipboardHandle::new(
            NativeBackend(native),
            cx.completions().clone(),
        ))
    }
}

/// A handle's view of the host's clipboard.
struct NativeBackend(Rc<dyn NativeClipboard>);

impl ClipboardBackend for NativeBackend {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        self.0.read_text(answer);
    }

    fn write_text(&self, text: String) {
        self.0.write_text(text);
    }
}

/// The slot the owner platform's primary selection is installed in for a
/// loop's life, with the claims made over it; empty between loops and
/// where the platform has none.
pub(crate) type PrimarySelectionSlot = Rc<RefCell<Option<NativePrimary>>>;

/// The owner platform's primary selection and the claims every handle over
/// it shares, so a claim from one widget makes another's stale.
#[derive(Clone)]
pub(crate) struct NativePrimary {
    native: Rc<dyn NativePrimarySelection>,
    claims: Rc<RefCell<PrimaryClaims>>,
}

impl NativePrimary {
    pub(crate) fn new(native: Rc<dyn NativePrimarySelection>) -> Self {
        Self {
            native,
            claims: Rc::default(),
        }
    }
}

/// The slots the built-in providers read the owner platform from.
#[derive(Clone, Default)]
pub(crate) struct CapabilitySlots {
    pub(crate) storage: StorageSlot,
    pub(crate) execution: ExecutionSlot,
    pub(crate) clipboard: ClipboardSlot,
    #[cfg_attr(
        not(target_os = "linux"),
        expect(
            dead_code,
            reason = "only the Linux hosts register a primary selection"
        )
    )]
    pub(crate) primary_selection: PrimarySelectionSlot,
}

/// The built-in primary selection: the owner platform's, on the platforms
/// that have one. Elsewhere nothing registers it, and an acquisition is
/// refused as not registered.
#[cfg_attr(
    not(target_os = "linux"),
    expect(dead_code, reason = "only the Linux hosts have a primary selection")
)]
struct PlatformPrimarySelection {
    slot: PrimarySelectionSlot,
}

impl CapabilityProvider<PrimarySelectionCapability> for PlatformPrimarySelection {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<PrimarySelectionHandle, Unsupported> {
        let native = self.slot.borrow().clone().ok_or_else(|| {
            // Between loops, or a host of this platform without one (winit,
            // a compositor without the protocol).
            Unsupported::of::<PrimarySelectionCapability>(UnsupportedReason::Unavailable)
        })?;
        Ok(PrimarySelectionHandle::new(
            native,
            cx.completions().clone(),
        ))
    }
}

impl PrimarySelectionBackend for NativePrimary {
    /// The host's offer holds a second reference to `text`; the claims keep
    /// theirs until the handle takes it back, so the host letting its offer
    /// go never runs the captures' `Drop`.
    fn claim(
        &self,
        text: Rc<dyn SelectionText>,
    ) -> (PrimaryClaimId, Option<Rc<dyn SelectionText>>) {
        let offered = Rc::clone(&text);
        let (id, replaced) = self.claims.borrow_mut().claim(text);
        if self.claims.borrow().is_current(id) {
            self.native.offer(Rc::new(move || offered.text()));
        }
        (id, replaced)
    }

    fn release(&self, claim: PrimaryClaimId) -> Option<Rc<dyn SelectionText>> {
        let released = self.claims.borrow_mut().release(claim);
        if released.is_some() {
            // A no-op once another client selected: the host owns nothing.
            self.native.withdraw();
        }
        released
    }

    /// A claim another client's selection ended stays in the claims until
    /// its handle releases it, which hands its captures back to be dropped
    /// there.
    fn is_current(&self, claim: PrimaryClaimId) -> bool {
        self.native.owns() && self.claims.borrow().is_current(claim)
    }

    fn read_text(&self, answer: Resolver<Option<String>>) {
        self.native.read_text(answer);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// What the adapter offered the host.
    type Offered = Rc<dyn Fn() -> Option<String>>;

    /// A host's primary selection that records what the adapter asks of it.
    #[derive(Default)]
    struct Recorded {
        offered: RefCell<Option<Offered>>,
        withdrawn: Cell<u32>,
    }

    impl NativePrimarySelection for Recorded {
        fn offer(&self, text: Rc<dyn Fn() -> Option<String>>) {
            self.offered.replace(Some(text));
        }

        /// Counts only a withdrawal of a selection the host still holds, as
        /// the trait says a withdrawal after another client selected does
        /// nothing.
        fn withdraw(&self) {
            if self.offered.replace(None).is_some() {
                self.withdrawn.set(self.withdrawn.get() + 1);
            }
        }

        fn owns(&self) -> bool {
            self.offered.borrow().is_some()
        }

        fn read_text(&self, answer: Resolver<Option<String>>) {
            drop(answer);
        }
    }

    impl Recorded {
        /// Another client selects.
        fn lose(&self) {
            self.offered.replace(None);
        }

        fn text(&self) -> Option<String> {
            let offered = self.offered.borrow().clone();
            offered.and_then(|text| text())
        }
    }

    /// Two widgets' claims over one host selection: the newer makes the
    /// older stale, a stale claim's release withdraws nothing, another
    /// client selecting ends both, and the text is asked only when read.
    #[test]
    fn claims_over_one_host_selection_share_their_currency() {
        let native = Rc::new(Recorded::default());
        let primary = NativePrimary::new(Rc::clone(&native) as Rc<dyn NativePrimarySelection>);
        let asked = Rc::new(Cell::new(0));
        let (first, _) = primary.claim(Rc::new({
            let asked = Rc::clone(&asked);
            move || {
                asked.set(asked.get() + 1);
                Some("first".to_owned())
            }
        }));
        assert_eq!(asked.get(), 0, "a claim copies nothing");
        assert_eq!(native.text().as_deref(), Some("first"));
        assert_eq!(asked.get(), 1);

        let (second, replaced) = primary.claim(Rc::new(|| Some("second".to_owned())));
        assert_eq!(
            replaced.and_then(|text| text.text()).as_deref(),
            Some("first"),
            "the replaced claim's text comes back to the handle"
        );
        assert!(!primary.is_current(first));
        assert!(primary.release(first).is_none());
        assert_eq!(native.withdrawn.get(), 0, "a stale claim withdraws nothing");
        assert!(primary.is_current(second));

        native.lose();
        assert!(!primary.is_current(second), "another client selected");
        assert_eq!(
            primary
                .release(second)
                .and_then(|text| text.text())
                .as_deref(),
            Some("second"),
            "a lost claim's text comes back at its release, not when it is lost"
        );
        let (third, _) = primary.claim(Rc::new(|| Some("third".to_owned())));
        assert!(primary.is_current(third));
        assert!(primary.release(third).is_some());
        assert_eq!(
            native.withdrawn.get(),
            1,
            "the current claim's release withdraws"
        );
        assert!(!native.owns());
    }

    /// The built-in lanes run work while the run's services live, and refuse
    /// as shut down once they are gone, without keeping them alive.
    #[test]
    fn the_loop_executor_refuses_once_the_services_are_gone() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let executors = flui_runtime::execution::DeterministicExecutors::new();
        let services = Arc::new(ExecutionServices::with_host(executors.host_executors()));
        let executor = LoopExecutor(Arc::downgrade(&services));
        let ran = Arc::new(AtomicBool::new(false));
        let job_ran = Arc::clone(&ran);
        executor
            .spawn_compute(Box::new(move || job_ran.store(true, Ordering::SeqCst)))
            .expect("live services admit the job");
        let _ = executors.run_until_idle();
        assert!(ran.load(Ordering::SeqCst));

        drop(services);
        assert_eq!(
            executor.spawn_compute(Box::new(|| {})),
            Err(SpawnError::ShuttingDown)
        );
        assert_eq!(
            executor.spawn_io(Box::pin(async {})),
            Err(SpawnError::ShuttingDown)
        );
    }
}

/// The host's byte storage for a loop's life, or why the run has none:
/// [`NotRegistered`](UnsupportedReason::NotRegistered) until a run installs
/// its storage and when it has no data directory to name,
/// [`NotOnThisPlatform`](UnsupportedReason::NotOnThisPlatform) when the
/// platform has no data root.
#[derive(Clone)]
pub(crate) struct StorageSlot {
    storage: Rc<RefCell<Result<Arc<dyn Storage>, Unsupported>>>,
    /// Whether this run has said why a widget got no storage.
    explained: Rc<Cell<bool>>,
}

impl Default for StorageSlot {
    fn default() -> Self {
        Self {
            storage: Rc::new(RefCell::new(Err(Unsupported::of::<StorageCapability>(
                UnsupportedReason::NotRegistered,
            )))),
            explained: Rc::new(Cell::new(false)),
        }
    }
}

impl StorageSlot {
    /// Install the run's storage, or the reason it has none.
    pub(crate) fn install(&self, storage: Result<Arc<dyn Storage>, Unsupported>) {
        let replaced = self.storage.replace(storage);
        self.explained.set(false);
        drop(replaced);
    }

    /// Release the run's storage at loop exit, leaving none installed.
    pub(crate) fn take(&self) -> Option<Arc<dyn Storage>> {
        self.storage
            .replace(Err(Unsupported::of::<StorageCapability>(
                UnsupportedReason::NotRegistered,
            )))
            .ok()
    }

    /// The installed storage, if the run has one.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    pub(crate) fn storage(&self) -> Option<Arc<dyn Storage>> {
        self.storage.borrow().as_ref().ok().map(Arc::clone)
    }
}

/// The built-in storage: the host's, under the run's data directory.
struct HostStorageProvider {
    slot: StorageSlot,
}

impl CapabilityProvider<StorageCapability> for HostStorageProvider {
    fn provide(&self, _cx: &ProviderContext<'_>) -> Result<StorageHandle, Unsupported> {
        let refusal = match &*self.slot.storage.borrow() {
            Ok(storage) => return Ok(StorageHandle::new(Arc::clone(storage))),
            Err(refusal) => *refusal,
        };
        // The first widget refused for want of a directory says how to name
        // one, once a run; a debug build only, since a release build that
        // keeps nothing is the author's choice.
        if cfg!(debug_assertions)
            && refusal.reason == UnsupportedReason::NotRegistered
            && !self.slot.explained.replace(true)
        {
            tracing::warn!(
                "a widget asked for storage, but the application has no identifier: set \
                 `AppIdentity::with_app_id` (`flui build` does from flui.toml) or \
                 `AppConfig::with_storage_dir`"
            );
        }
        Err(refusal)
    }
}

/// The slot the run's execution services are reachable through once the
/// host resolves them; held weakly, so a handle never keeps them alive past
/// the loop's shutdown.
pub(crate) type ExecutionSlot = Rc<RefCell<Option<Weak<ExecutionServices>>>>;

/// The built-in background lanes: the run's execution services, inside
/// their admission windows and shutdown.
struct HostBackground {
    slot: ExecutionSlot,
}

impl CapabilityProvider<BackgroundCapability> for HostBackground {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<Background, Unsupported> {
        let services = self.slot.borrow().clone().ok_or_else(|| {
            // Before the host installs a UI runtime it has no lanes.
            Unsupported::of::<BackgroundCapability>(UnsupportedReason::Unavailable)
        })?;
        Ok(Background::new(
            Arc::new(LoopExecutor(services)),
            cx.completions().clone(),
        ))
    }
}

/// The run's execution services, refusing as shut down once they are gone.
struct LoopExecutor(Weak<ExecutionServices>);

impl Executor for LoopExecutor {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        self.0
            .upgrade()
            .ok_or(SpawnError::ShuttingDown)?
            .spawn_compute(job)
    }

    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        self.0
            .upgrade()
            .ok_or(SpawnError::ShuttingDown)?
            .spawn_io(future)
    }
}
