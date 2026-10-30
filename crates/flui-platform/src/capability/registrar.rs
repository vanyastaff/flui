//! How a plugin, a capability crate or an application hands its providers to
//! a UI runtime (ADR-0152 §2, item 4).

use std::{any::Any, any::TypeId, fmt, rc::Rc};

use super::{Capability, CapabilityProvider, ProviderContext};
use crate::Unsupported;

/// A set of providers registered together: a capability crate's, or a
/// package's.
///
/// It lives in the contract crate, so a leaf capability crate registers its
/// providers without depending on the SDK or the runtime.
pub trait Plugin: 'static {
    /// Register this plugin's providers.
    fn register(&self, registrar: &mut CapabilityRegistrar);
}

/// The providers one source registers: a list, keyed by capability.
#[derive(Default)]
pub struct CapabilityRegistrar {
    entries: Vec<RegisteredProvider>,
}

impl CapabilityRegistrar {
    /// Nothing registered yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `provider` for the capability `C`.
    pub fn provide<C: Capability>(&mut self, provider: impl CapabilityProvider<C>) {
        self.entries.push(RegisteredProvider::new::<C>(provider));
    }

    /// The registered providers, in registration order.
    #[must_use]
    pub fn into_entries(self) -> Vec<RegisteredProvider> {
        self.entries
    }
}

impl fmt::Debug for CapabilityRegistrar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.entries).finish()
    }
}

trait ErasedProvider {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<Box<dyn Any>, Unsupported>;
}

struct Typed<C: Capability, P> {
    provider: P,
    capability: std::marker::PhantomData<fn() -> C>,
}

impl<C: Capability, P: CapabilityProvider<C>> ErasedProvider for Typed<C, P> {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<Box<dyn Any>, Unsupported> {
        self.provider
            .provide(cx)
            .map(|handle| Box::new(handle) as Box<dyn Any>)
    }
}

/// One provider with the capability it provides, as the runtime's registry
/// keeps it.
#[derive(Clone)]
pub struct RegisteredProvider {
    capability: TypeId,
    name: &'static str,
    provider: Rc<dyn ErasedProvider>,
}

impl RegisteredProvider {
    /// `provider` for the capability `C`.
    #[must_use]
    pub fn new<C: Capability>(provider: impl CapabilityProvider<C>) -> Self {
        Self {
            capability: TypeId::of::<C>(),
            name: C::NAME,
            provider: Rc::new(Typed {
                provider,
                capability: std::marker::PhantomData,
            }),
        }
    }

    /// The capability's marker type.
    #[must_use]
    pub fn capability(&self) -> TypeId {
        self.capability
    }

    /// The capability's [`NAME`](Capability::NAME).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Ask the provider for a handle, boxed; the box holds the capability's
    /// [`Handle`](Capability::Handle).
    ///
    /// # Errors
    ///
    /// What the provider answers.
    pub fn provide(&self, cx: &ProviderContext<'_>) -> Result<Box<dyn Any>, Unsupported> {
        self.provider.provide(cx)
    }
}

impl fmt::Debug for RegisteredProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredProvider")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}
