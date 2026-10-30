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
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use flui_platform::{
    PlatformWindow, Unsupported, UnsupportedReason,
    capability::{
        CapabilityRegistrar, CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope,
        PendingCompletions, Plugin, ProviderContext, RegisteredProvider, WindowRef,
    },
};
use flui_view::CapabilitySource;

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

    /// Whether a provider answers `capability`.
    #[must_use]
    pub fn provides(&self, capability: TypeId) -> bool {
        self.providers.contains_key(&capability)
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
        cx: &ProviderContext<'_>,
    ) -> Result<Box<dyn Any>, Unsupported> {
        match self.providers.get(&capability) {
            Some(provider) => provider.provide(cx),
            None => Err(Unsupported::named(name, UnsupportedReason::NotRegistered)),
        }
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
            .finish()
    }
}

/// Collects a [`CapabilityRegistry`]'s registrations by tier;
/// [`build`](Self::build) checks them for conflicts.
#[derive(Debug, Default)]
pub struct CapabilityRegistryBuilder {
    registrations: Vec<(ProviderTier, RegisteredProvider)>,
}

impl CapabilityRegistryBuilder {
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
        })
    }
}

/// A UI runtime's owner-job queue and the completions pending on it.
pub(crate) struct OwnerJobs {
    rx: Receiver<OwnerJob>,
    pending: PendingCompletions,
    completions: CompletionSource,
}

impl OwnerJobs {
    /// A queue whose poster wakes the owner through `wake` after each post.
    pub(crate) fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx): (Sender<OwnerJob>, _) = unbounded();
        let poster = OwnerPoster::new(move |job| {
            tx.send(job).map_err(|_| OwnerRetired)?;
            wake();
            Ok(())
        });
        let pending = PendingCompletions::new();
        Self {
            rx,
            completions: CompletionSource::new(poster, pending.clone()),
            pending,
        }
    }

    /// Where providers of this UI runtime make their completions.
    pub(crate) fn completions(&self) -> &CompletionSource {
        &self.completions
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
}

impl Drop for OwnerJobs {
    fn drop(&mut self) {
        // Retire first: a resolver answering now learns the owner is gone,
        // and every closure still waiting is dropped unrun.
        self.pending.retire();
    }
}

/// What a presentation's widgets acquire capabilities through: the UI
/// runtime's registry, its completions and the presentation's window.
pub(crate) struct PresentationCapabilities {
    pub(crate) registry: CapabilityRegistry,
    pub(crate) completions: CompletionSource,
    pub(crate) window: std::rc::Weak<dyn PlatformWindow>,
}

impl CapabilitySource for PresentationCapabilities {
    fn acquire(&self, capability: TypeId, name: &'static str) -> Result<Box<dyn Any>, Unsupported> {
        let window = self.window.upgrade().map(|window| {
            let scale = window
                .state()
                .metrics
                .map_or(1.0, |metrics| metrics.scale().get());
            WindowRef::new(window.id(), scale, window.surface_target())
        });
        let poster = self.completions.poster();
        let cx = ProviderContext::new(poster, &self.completions, window);
        self.registry.acquire(capability, name, &cx)
    }
}
