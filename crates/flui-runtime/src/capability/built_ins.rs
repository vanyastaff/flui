//! The built-in tier of an application's registry (ADR-0152): the providers a
//! host ships, which capabilities it ships on which platform, and the
//! registry composed from them, the application's overrides and its plugins.
//! A runner and the headless host build their application's registry here,
//! with their own providers, so a headless test acquires what a run would.

use flui_interaction::text_input::{ClipboardCapability, PrimarySelectionCapability};
use flui_platform::TargetPlatform;
use flui_platform::capability::{
    Capability, CapabilityProvider, CapabilityRegistrar, HostBridge, Plugin,
};
use flui_platform::http::HttpCapability;
use flui_view::background::BackgroundCapability;
use flui_view::persist::StorageCapability;

use super::{CapabilityConflict, CapabilityRegistry};

/// The providers a host ships for an application on one platform.
///
/// The clipboard, the background lanes and the storage are on every
/// platform; the primary selection only where the platform has one (X11's
/// and Wayland's, so Linux); HTTP where the host was built with it; and
/// what only one host can ship (a runner's native window) through
/// [`host_specific`](Self::host_specific).
#[derive(Debug)]
pub struct BuiltIns {
    platform: TargetPlatform,
    registrar: CapabilityRegistrar,
}

impl BuiltIns {
    /// The providers every platform has: `clipboard`, `background` and
    /// `storage`, on `platform`.
    #[must_use]
    pub fn new(
        platform: TargetPlatform,
        clipboard: impl CapabilityProvider<ClipboardCapability>,
        background: impl CapabilityProvider<BackgroundCapability>,
        storage: impl CapabilityProvider<StorageCapability>,
    ) -> Self {
        let mut registrar = CapabilityRegistrar::new();
        registrar.provide::<ClipboardCapability>(clipboard);
        registrar.provide::<BackgroundCapability>(background);
        registrar.provide::<StorageCapability>(storage);
        Self {
            platform,
            registrar,
        }
    }

    /// The primary selection, registered where the platform has one (Linux):
    /// elsewhere `provider` is dropped, and a widget's acquisition is refused
    /// as not registered, as on that platform.
    #[must_use]
    pub fn primary_selection(
        mut self,
        provider: impl CapabilityProvider<PrimarySelectionCapability>,
    ) -> Self {
        if self.platform == TargetPlatform::Linux {
            self.registrar
                .provide::<PrimarySelectionCapability>(provider);
        }
        self
    }

    /// The HTTP client, for a host built with it.
    #[must_use]
    pub fn http(mut self, provider: impl CapabilityProvider<HttpCapability>) -> Self {
        self.registrar.provide::<HttpCapability>(provider);
        self
    }

    /// A provider only this host can ship, such as a runner's native window,
    /// whose capability names a type of the host's own crate.
    #[must_use]
    pub fn host_specific<C: Capability>(mut self, provider: impl CapabilityProvider<C>) -> Self {
        self.registrar.provide::<C>(provider);
        self
    }

    /// The application's registry: `overrides` first, then these built-ins,
    /// then each of `plugins`, with `host` offered to every provider.
    ///
    /// A plugin's `register` is user code, so the caller holds no borrow of
    /// its own state.
    ///
    /// # Errors
    ///
    /// The first [`CapabilityConflict`] among them.
    pub fn compose<'p>(
        self,
        overrides: CapabilityRegistrar,
        host: HostBridge,
        plugins: impl IntoIterator<Item = &'p dyn Plugin>,
    ) -> Result<CapabilityRegistry, CapabilityConflict> {
        let mut builder = CapabilityRegistry::builder()
            .overrides(overrides)
            .built_ins(self.registrar)
            .host(host);
        for plugin in plugins {
            builder = builder.plugin(plugin);
        }
        builder.build()
    }
}
