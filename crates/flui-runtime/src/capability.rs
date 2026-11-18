//! The UI runtime's side of the capability seam (ADR-0152): the registry its
//! presentations acquire platform capabilities from, and the owner-job queue a
//! provider's results come back to the owner thread through.
//!
//! - [`CapabilityRegistry`] holds, for each capability, the one provider that
//!   answers it: the application's override, else the built-in, else the one
//!   plugin that registered it. It is an owner-thread value, built once by the
//!   composition root and handed to every UI runtime it constructs
//!   ([`RuntimeHostServices::with_capabilities`](crate::ui_runtime::RuntimeHostServices::with_capabilities)).
//!   It answers [`UnsupportedReason::NotRegistered`] itself and keeps no
//!   handle: every acquisition asks the provider again.
//! - The owner-job queue is unbounded, apart from the bounded command inbox:
//!   a provider's answer that backpressure refused would leave its
//!   [`Completion`](flui_platform::capability::Completion) pending forever.
//!   The runtime drains it at every Idle drain, after the inbox, and retires
//!   its pending completions when it drops.

use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    fmt,
    rc::Rc,
    sync::Arc,
};

use crossbeam_channel::{Receiver, SendError, Sender, unbounded};
use flui_foundation::panic::FirstPanic;
use flui_platform::{
    PlatformWindow, Unsupported, UnsupportedReason,
    capability::{
        Capability, CapabilityRegistrar, CompletionSource, HostBridge, OwnerJob, OwnerPoster,
        OwnerRetired, OwnerScope, PendingCompletions, Plugin, ProviderContext, RegisteredProvider,
        WindowRef,
    },
};
use flui_view::CapabilitySource;
use parking_lot::Mutex;

mod built_ins;

pub use built_ins::BuiltIns;

/// Which registration a provider came from, in precedence order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ProviderTier {
    /// The application's own provider, which replaces any other.
    Override,
    /// The provider the framework ships for the platform.
    BuiltIn,
    /// A plugin's provider: one plugin per capability.
    Plugin,
}

impl fmt::Display for ProviderTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Override => "application override",
            Self::BuiltIn => "built-in provider",
            Self::Plugin => "plugin",
        })
    }
}

/// Two registrations the registry cannot choose between, found when it is
/// built, before any window opens.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CapabilityConflict {
    /// One capability registered twice in the same tier: two plugins, two
    /// overrides, or two built-ins.
    #[error("capability `{name}` is registered twice as a {tier}")]
    Duplicate {
        /// The capability's `NAME`.
        name: &'static str,
        /// The tier both registrations are in.
        tier: ProviderTier,
    },
    /// Two different capabilities share one `NAME`, so a refusal could not
    /// say which of them it is about.
    #[error("two different capabilities are both named `{name}`")]
    SharedName {
        /// The name both declare.
        name: &'static str,
    },
    /// A capability's `NAME` is not a `ResultKey` (1 to 64 bytes of
    /// printable ASCII), under which the Android host keys its provider's
    /// requests for results.
    #[error("capability `{name}` has a name that is not a result key: {reason}")]
    InvalidName {
        /// The name.
        name: &'static str,
        /// Why it is not a key.
        reason: flui_platform::capability::InvalidResultKey,
    },
}

/// The providers a UI runtime acquires platform capabilities from, keyed by
/// capability marker.
///
/// Owner-thread and cheap to clone: the composition root builds one and
/// hands a clone to each UI runtime. [`CapabilityRegistry::empty`] answers
/// every acquisition with [`UnsupportedReason::NotRegistered`].
#[derive(Clone, Default)]
pub struct CapabilityRegistry {
    providers: Rc<HashMap<TypeId, RegisteredProvider>>,
    /// What the running host offers providers beyond the window; the
    /// composition root fills it for each run's host.
    host: HostBridge,
}

impl CapabilityRegistry {
    /// A registry with no provider.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Start a registry from its three tiers.
    #[must_use]
    pub fn builder() -> CapabilityRegistryBuilder {
        CapabilityRegistryBuilder::default()
    }

    /// Ask the provider of `capability` for a handle, as the erased
    /// `capability_erased` entry point does.
    ///
    /// # Errors
    ///
    /// [`UnsupportedReason::NotRegistered`], naming `name`, when no provider
    /// answers the capability; otherwise the provider's own refusal,
    /// unchanged.
    pub fn acquire(
        &self,
        capability: TypeId,
        name: &'static str,
        ctx: &ProviderContext<'_>,
    ) -> Result<Box<dyn Any>, Unsupported> {
        match self.providers.get(&capability) {
            Some(provider) => provider.provide(ctx),
            None => Err(Unsupported::named(name, UnsupportedReason::NotRegistered)),
        }
    }

    /// What the running host offers providers beyond the window, which every
    /// acquisition's [`ProviderContext`] reads.
    #[must_use]
    pub(crate) fn host(&self) -> &HostBridge {
        &self.host
    }
}

impl fmt::Debug for CapabilityRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names: Vec<_> = self
            .providers
            .values()
            .map(RegisteredProvider::name)
            .collect();
        names.sort_unstable();
        f.debug_struct("CapabilityRegistry")
            .field("capabilities", &names)
            .field("host", &self.host)
            .finish()
    }
}

/// Collects a [`CapabilityRegistry`]'s registrations by tier;
/// [`build`](Self::build) checks them for conflicts.
#[derive(Debug, Default)]
pub struct CapabilityRegistryBuilder {
    registrations: Vec<(ProviderTier, RegisteredProvider)>,
    host: HostBridge,
}

impl CapabilityRegistryBuilder {
    /// The bridge through which the composition root offers what each run's
    /// host has beyond the window (its Android Activity); without it, no
    /// provider is offered any.
    #[must_use]
    pub fn host(mut self, host: HostBridge) -> Self {
        self.host = host;
        self
    }

    /// The application's overrides.
    #[must_use]
    pub fn overrides(mut self, registrar: CapabilityRegistrar) -> Self {
        self.push(ProviderTier::Override, registrar);
        self
    }

    /// The framework's built-in providers.
    #[must_use]
    pub fn built_ins(mut self, registrar: CapabilityRegistrar) -> Self {
        self.push(ProviderTier::BuiltIn, registrar);
        self
    }

    /// What `plugin` registers.
    #[must_use]
    pub fn plugin(mut self, plugin: &dyn Plugin) -> Self {
        let mut registrar = CapabilityRegistrar::new();
        plugin.register(&mut registrar);
        self.push(ProviderTier::Plugin, registrar);
        self
    }

    fn push(&mut self, tier: ProviderTier, registrar: CapabilityRegistrar) {
        self.registrations.extend(
            registrar
                .into_entries()
                .into_iter()
                .map(|provider| (tier, provider)),
        );
    }

    /// The registry: per capability, the provider of the highest tier.
    ///
    /// # Errors
    ///
    /// The first [`CapabilityConflict`] in registration order, tiers in
    /// precedence order: a capability registered twice in one tier, or two
    /// capabilities that share a `NAME`. Registering a capability in more
    /// than one tier is not a conflict; the higher tier answers.
    pub fn build(mut self) -> Result<CapabilityRegistry, CapabilityConflict> {
        // Stable: within a tier, registration order decides which conflict
        // is reported first.
        self.registrations.sort_by_key(|(tier, _)| *tier);
        let mut names: HashMap<&'static str, TypeId> = HashMap::new();
        let mut registered: HashSet<(ProviderTier, TypeId)> = HashSet::new();
        let mut chosen: HashMap<TypeId, RegisteredProvider> = HashMap::new();
        for (tier, provider) in self.registrations {
            let (capability, name) = (provider.capability(), provider.name());
            if let Err(reason) = flui_platform::capability::ResultKey::try_new(name) {
                return Err(CapabilityConflict::InvalidName { name, reason });
            }
            if *names.entry(name).or_insert(capability) != capability {
                return Err(CapabilityConflict::SharedName { name });
            }
            // Checked in every tier, answered or not: two plugins conflict
            // even where a built-in answers in their place.
            if !registered.insert((tier, capability)) {
                return Err(CapabilityConflict::Duplicate { name, tier });
            }
            // Tiers arrive in precedence order: the first one holds it.
            chosen.entry(capability).or_insert(provider);
        }
        Ok(CapabilityRegistry {
            providers: Rc::new(chosen),
            host: self.host,
        })
    }
}

/// A UI runtime's owner-job queue and the completions pending on it.
pub(crate) struct OwnerJobs {
    rx: Receiver<OwnerJob>,
    /// The poster's way in. Retirement takes it under the lock, so a post
    /// either lands before the final drain or is refused with
    /// [`OwnerRetired`]; none is accepted and then dropped unrun.
    admission: Admission,
    pending: PendingCompletions,
    completions: CompletionSource,
    locals: flui_platform::capability::RuntimeLocals,
}

type Admission = Arc<Mutex<Option<Sender<OwnerJob>>>>;

impl OwnerJobs {
    /// A queue whose poster wakes the owner through `wake` after each post.
    pub(crate) fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx): (Sender<OwnerJob>, _) = unbounded();
        let admission: Admission = Arc::new(Mutex::new(Some(tx)));
        let sink = Arc::clone(&admission);
        let poster = OwnerPoster::new(move |job| {
            // A refused job is dropped after the lock is released: its
            // captures may run user code.
            let refused = match sink.lock().as_ref() {
                Some(tx) => tx.send(job).err().map(SendError::into_inner),
                None => Some(job),
            };
            if let Some(job) = refused {
                drop(job);
                return Err(OwnerRetired);
            }
            wake();
            Ok(())
        });
        let pending = PendingCompletions::new();
        Self {
            rx,
            admission,
            completions: CompletionSource::new(poster, pending.clone()),
            locals: flui_platform::capability::RuntimeLocals::new(),
            pending,
        }
    }

    /// Where providers of this UI runtime make their completions.
    pub(crate) fn completions(&self) -> &CompletionSource {
        &self.completions
    }

    /// What providers keep for this UI runtime (ADR-0152 §5).
    pub(crate) fn locals(&self) -> &flui_platform::capability::RuntimeLocals {
        &self.locals
    }

    /// The jobs posted before this call, each run by `run` with the owner's
    /// scope; `run` may contain a job's panic. Jobs posted meanwhile wait for
    /// the next drain.
    pub(crate) fn drain(&self, mut run: impl FnMut(OwnerJob, &OwnerScope<'_>)) -> usize {
        // Bounded by the pre-read length, as the command inbox is: a job that
        // posts another cannot extend this drain.
        let queued = self.rx.len();
        let scope = OwnerScope::new(&self.pending);
        let mut ran = 0;
        for _ in 0..queued {
            let Ok(job) = self.rx.try_recv() else {
                break;
            };
            ran += 1;
            run(job, &scope);
        }
        ran
    }

    /// Whether a job waits for the next drain.
    pub(crate) fn has_queued(&self) -> bool {
        !self.rx.is_empty()
    }

    /// The UI runtime retires: its pending completions end, then the jobs
    /// still queued are dropped unrun, each under its own catch, then what
    /// providers kept for the runtime (`RuntimeLocals`) ends, last, so a
    /// completion's closure dropped above still finds its provider's state.
    /// Returns the first panic for the runtime's teardown to keep with its
    /// own first failure; a later panic is discarded, and every queued job is
    /// still dropped (ADR-0281 §3).
    #[must_use = "the first panic of a retirement belongs to the runtime's teardown"]
    pub(crate) fn retire(&self) -> FirstPanic {
        // Close admission first: a post that races this retirement is either
        // queued before the drain below or refused, never accepted and lost.
        drop(self.admission.lock().take());
        let mut first = FirstPanic::new();
        if let Some(payload) = self.pending.retire() {
            first.record(payload);
        }
        while let Ok(job) = self.rx.try_recv() {
            // A queued job's drop runs contained so the drain finishes.
            first.contain(|| drop(job));
        }
        if let Some(payload) = self.locals.retire() {
            first.record(payload);
        }
        first
    }
}

impl Drop for OwnerJobs {
    fn drop(&mut self) {
        // The UI runtime's teardown retires the queue before it resumes its
        // first failure; this is the backstop for a queue dropped without it.
        self.retire().resume();
    }
}

/// What the composition root acquires capabilities through for itself, outside
/// every UI runtime: the run's registry, an owner-job queue of its own and no
/// window (ADR-0338).
///
/// The host keeps state of its own in an application's capabilities (a window's
/// remembered placement in its storage), so it acquires them from the same
/// registry its UI runtimes do, the application's override included, before any
/// UI runtime exists. A provider sees a [`ProviderContext`] whose
/// [`window`](ProviderContext::window) is `None`; a capability that needs a
/// window answers [`UnsupportedReason::NoWindow`] itself, as for a widget. What
/// a provider posts back runs when the owner calls [`run_posted`](Self::run_posted)
/// at its turn, never inline in the post.
///
/// Owner-thread: it holds the registry, an `Rc`. [`retire`](Self::retire) ends
/// its pending completions and refuses later posts; dropping it unretired
/// retires it and resumes the first panic of that.
pub struct HostCapabilities {
    registry: CapabilityRegistry,
    jobs: OwnerJobs,
}

impl HostCapabilities {
    /// A scope over `registry` whose poster calls `wake` after each post, from
    /// whatever thread posted: the owner then takes the job at its next turn.
    #[must_use]
    pub fn new(registry: CapabilityRegistry, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            registry,
            jobs: OwnerJobs::new(wake),
        }
    }

    /// The handle of capability `C`, from the provider the registry holds for
    /// it, asked now: the registry keeps no handle.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] naming `C`:
    /// [`NotRegistered`](UnsupportedReason::NotRegistered) when nothing
    /// provides it, otherwise what the provider answers. A provider's panic
    /// is not caught here; the caller decides what it costs.
    pub fn acquire<C: Capability>(&self) -> Result<C::Handle, Unsupported> {
        let handle = acquire_with(
            &self.registry,
            self.jobs.completions(),
            self.jobs.locals(),
            None,
            TypeId::of::<C>(),
            C::NAME,
        )?
        .downcast::<C::Handle>()
        .expect("BUG: the registry keys a provider by the capability whose handle it makes");
        Ok(*handle)
    }

    /// Run the jobs providers posted before this call, each contained, so one
    /// that panics leaves the rest to run. Returns the first panic; a later
    /// one is discarded (ADR-0281).
    #[must_use = "the first panic of the posted jobs belongs to the owner's turn"]
    pub fn run_posted(&self) -> FirstPanic {
        let mut first = FirstPanic::new();
        self.jobs.drain(|job, scope| {
            first.contain(|| job.run(scope));
        });
        first
    }

    /// The host retires the scope: its pending completions end, a job still
    /// queued is dropped unrun and a later post is refused with
    /// [`OwnerRetired`]. Returns the first panic of that.
    #[must_use = "the first panic of a retirement belongs to the host's teardown"]
    pub fn retire(&self) -> FirstPanic {
        self.jobs.retire()
    }
}

impl fmt::Debug for HostCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCapabilities")
            .field("registry", &self.registry)
            .field("posted", &self.jobs.has_queued())
            .finish_non_exhaustive()
    }
}

/// What a presentation's widgets acquire capabilities through: the UI
/// runtime's registry, its completions and the presentation's window.
pub(crate) struct PresentationCapabilities {
    pub(crate) registry: CapabilityRegistry,
    pub(crate) completions: CompletionSource,
    pub(crate) locals: flui_platform::capability::RuntimeLocals,
    pub(crate) window: std::rc::Weak<dyn PlatformWindow>,
    /// Whether the window is still open, handed to its providers.
    pub(crate) lifetime: flui_platform::capability::WindowLifetime,
}

impl CapabilitySource for PresentationCapabilities {
    fn acquire(&self, capability: TypeId, name: &'static str) -> Result<Box<dyn Any>, Unsupported> {
        let window = self
            .window
            .upgrade()
            .map(|window| WindowRef::new(&window, self.lifetime.clone()));
        acquire_with(
            &self.registry,
            &self.completions,
            &self.locals,
            window,
            capability,
            name,
        )
    }
}

/// Ask `registry`'s provider of `capability` in the context one owner gives
/// its providers: its completions, what providers keep for it, and the window
/// it acquires for, if any. The one acquisition a UI runtime's presentation
/// and the host's own scope share.
fn acquire_with(
    registry: &CapabilityRegistry,
    completions: &CompletionSource,
    locals: &flui_platform::capability::RuntimeLocals,
    window: Option<WindowRef>,
    capability: TypeId,
    name: &'static str,
) -> Result<Box<dyn Any>, Unsupported> {
    let ctx = ProviderContext::new(
        completions.poster(),
        completions,
        locals,
        registry.host(),
        window,
    );
    registry.acquire(capability, name, &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_post_after_retirement_is_refused_not_queued() {
        let jobs = OwnerJobs::new(|| {});
        let poster = jobs.completions().poster().clone();
        assert!(!jobs.retire().is_held());

        assert_eq!(poster.post(|| {}), Err(OwnerRetired));
        assert!(!jobs.has_queued(), "a refused job is not left in the queue");
    }
}
