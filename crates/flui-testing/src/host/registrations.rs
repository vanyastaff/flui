//! What an application registers over a headless host's built-ins, and the
//! registry a runner builds from both (ADR-0152).

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::Unsupported;
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

/// The registry a runner builds for an application on `platform`: the
/// application's overrides first (the provided storage among them), then the
/// built-ins with the host's storage, then the application's plugins.
///
/// # Panics
///
/// When the application's registrations conflict, as a run fails before its
/// platform starts.
pub(super) fn compose_registry(
    scripted: &ScriptedBuiltIns,
    platform: flui_platform::TargetPlatform,
    storage: Option<&KitStorage>,
    application: &AppRegistrations,
) -> flui_runtime::capability::CapabilityRegistry {
    let mut built_ins = scripted.registrar(platform);
    let mut overrides = flui_platform::capability::CapabilityRegistrar::new();
    for registration in &application.overrides {
        registration.register(&mut overrides);
    }
    if let Some(KitStorage { host, provided }) = storage {
        let host = Arc::clone(host);
        built_ins.provide::<StorageCapability>(
            move |_: &flui_platform::capability::ProviderContext<'_>| {
                Ok(StorageHandle::new(Arc::clone(&host)))
            },
        );
        if let Some(provided) = provided {
            let provided = Arc::clone(provided);
            overrides.provide::<StorageCapability>(
                move |_: &flui_platform::capability::ProviderContext<'_>| {
                    Ok(StorageHandle::new(Arc::clone(&provided)))
                },
            );
        }
    }
    let mut builder = flui_runtime::capability::CapabilityRegistry::builder()
        .overrides(overrides)
        .built_ins(built_ins);
    for plugin in &application.plugins {
        builder = builder.plugin(plugin.as_ref());
    }
    builder
        .build()
        .unwrap_or_else(|conflict| panic!("the application's registrations conflict: {conflict}"))
}
