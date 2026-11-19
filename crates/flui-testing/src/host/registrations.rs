//! What an application registers over a headless host's built-ins, and the
//! registry a runner builds from both (ADR-0152).

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::{Unsupported, UnsupportedReason};
use flui_view::persist::{StorageCapability, StorageHandle};

use super::{KitStorage, ScriptedBuiltIns};

/// What an application registers over the built-ins (ADR-0152): its
/// overrides, which answer in place of a built-in, and its plugins, which
/// provide what nothing else does. Shared, so a relaunch registers them again.
#[derive(Clone, Default)]
pub(crate) struct AppRegistrations {
    /// Each registers its one provider in the overrides tier.
    overrides: Vec<Rc<dyn flui_platform::capability::Plugin>>,
    plugins: Vec<Rc<dyn flui_platform::capability::Plugin>>,
}

/// One override: `C` provided by the shared `P`.
struct Override<C, P> {
    provider: Rc<P>,
    capability: std::marker::PhantomData<fn() -> C>,
}

impl<C, P> flui_platform::capability::Plugin for Override<C, P>
where
    C: flui_platform::capability::Capability,
    P: flui_platform::capability::CapabilityProvider<C>,
{
    fn register(&self, registrar: &mut flui_platform::capability::CapabilityRegistrar) {
        registrar.provide::<C>(SharedProvider(Rc::clone(&self.provider)));
    }
}

impl AppRegistrations {
    /// Nothing registered.
    pub(crate) const fn new() -> Self {
        Self {
            overrides: Vec::new(),
            plugins: Vec::new(),
        }
    }

    /// Provide `C` with `provider`, in place of the built-in and any plugin's.
    pub(crate) fn capability<C, P>(&mut self, provider: P)
    where
        C: flui_platform::capability::Capability,
        P: flui_platform::capability::CapabilityProvider<C>,
    {
        self.overrides.push(Rc::new(Override::<C, P> {
            provider: Rc::new(provider),
            capability: std::marker::PhantomData,
        }));
    }

    /// Register `plugin`'s providers after the built-ins.
    pub(crate) fn plugin(&mut self, plugin: impl flui_platform::capability::Plugin) {
        self.plugins.push(Rc::new(plugin));
    }

    /// How many overrides and plugins are registered.
    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.overrides.len(), self.plugins.len())
    }
}

/// One provider registered again by each run that shares it.
struct SharedProvider<P>(Rc<P>);

impl<C, P> flui_platform::capability::CapabilityProvider<C> for SharedProvider<P>
where
    C: flui_platform::capability::Capability,
    P: flui_platform::capability::CapabilityProvider<C>,
{
    fn provide(
        &self,
        ctx: &flui_platform::capability::ProviderContext<'_>,
    ) -> Result<C::Handle, Unsupported> {
        self.0.provide(ctx)
    }
}

/// Where a [`HeadlessHost`](super::HeadlessHost)'s registry comes from.
pub(super) enum Registry {
    /// The test's own, alone.
    Given(flui_runtime::capability::CapabilityRegistry),
    /// What a runner builds for an application on the primary window's
    /// platform ([`compose_registry`]).
    Composed(AppRegistrations),
}

/// The registry a runner builds for an application on `platform`, reading
/// `assets` (ADR-0384 §5)
/// ([`BuiltIns::compose`](flui_runtime::capability::BuiltIns::compose)): the
/// application's overrides first (the provided storage among them), then the
/// scripted built-ins a runner ships on that platform with the host's
/// storage, then the application's plugins and the
/// package plugin a test of that package names. Every registry a headless
/// host builds for itself is built here.
///
/// # Panics
///
/// When the application's registrations conflict, as a run fails before its
/// platform starts.
pub(super) fn compose_registry(
    scripted: &ScriptedBuiltIns,
    platform: flui_platform::TargetPlatform,
    assets: &flui_platform::assets::AssetBundle,
    storage: Option<&KitStorage>,
    application: &AppRegistrations,
    package: Option<&dyn flui_platform::capability::Plugin>,
) -> flui_runtime::capability::CapabilityRegistry {
    let mut overrides = flui_platform::capability::CapabilityRegistrar::new();
    for registration in &application.overrides {
        registration.register(&mut overrides);
    }
    if let Some(provided) = storage.and_then(|storage| storage.provided.as_ref()) {
        let provided = Arc::clone(provided);
        overrides.provide::<StorageCapability>(
            move |_: &flui_platform::capability::ProviderContext<'_>| {
                Ok(StorageHandle::new(Arc::clone(&provided)))
            },
        );
    }
    let host = storage.map(|storage| Arc::clone(&storage.host));
    // Without storage of its own the host answers as a runner with no data
    // directory does: the storage is not registered for this run.
    let host_storage = move |_: &flui_platform::capability::ProviderContext<'_>| match &host {
        Some(host) => Ok(StorageHandle::new(Arc::clone(host))),
        None => Err(Unsupported::of::<StorageCapability>(
            UnsupportedReason::NotRegistered,
        )),
    };
    let plugins = application.plugins.iter().map(AsRef::as_ref).chain(package);
    scripted
        .built_ins(platform, assets, host_storage)
        .compose(
            overrides,
            flui_platform::capability::HostBridge::new(),
            plugins,
        )
        .unwrap_or_else(|conflict| panic!("the application's registrations conflict: {conflict}"))
}
