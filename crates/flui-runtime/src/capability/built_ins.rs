//! The built-in tier of an application's registry (ADR-0152): the providers a
//! host ships, which capabilities it ships in which window system, and the
//! registry composed from them, the application's overrides and its plugins.
//! A runner and the headless host build their application's registry here,
//! with their own providers, so a headless test acquires what a run would.

use crate::window_system::WindowSystem;
use flui_interaction::text_input::{ClipboardCapability, PrimarySelectionCapability};
use flui_platform::TargetPlatform;
use flui_platform::assets::AssetBundleCapability;
use flui_platform::capability::{
    Capability, CapabilityProvider, CapabilityRegistrar, HostBridge, Plugin,
};
use flui_platform::http::HttpCapability;
use flui_view::background::BackgroundCapability;
use flui_view::persist::StorageCapability;

use super::{CapabilityConflict, CapabilityRegistry};

/// The providers a host ships for an application in one window system.
///
/// The clipboard, the background lanes, the storage and the application's
/// assets (ADR-0384) are in every window system; the primary selection only
/// where the window system has one (X11's and Wayland's, so Linux's, never a
/// browser's page); HTTP where the host was built with it; and what only one
/// host can ship (a runner's native window) through
/// [`host_specific`](Self::host_specific).
#[derive(Debug)]
pub struct BuiltIns {
    windows: WindowSystem,
    registrar: CapabilityRegistrar,
}

impl BuiltIns {
    /// The providers every window system has: `clipboard`, `background`,
    /// `storage` and `assets`, in `windows`.
    #[must_use]
    pub fn new(
        windows: WindowSystem,
        clipboard: impl CapabilityProvider<ClipboardCapability>,
        background: impl CapabilityProvider<BackgroundCapability>,
        storage: impl CapabilityProvider<StorageCapability>,
        assets: impl CapabilityProvider<AssetBundleCapability>,
    ) -> Self {
        let mut registrar = CapabilityRegistrar::new();
        registrar.provide::<ClipboardCapability>(clipboard);
        registrar.provide::<BackgroundCapability>(background);
        registrar.provide::<StorageCapability>(storage);
        registrar.provide::<AssetBundleCapability>(assets);
        Self { windows, registrar }
    }

    /// The primary selection, registered where the window system has one
    /// (Linux's; a browser's page has none): elsewhere `provider` is dropped,
    /// and a widget's acquisition is refused as not registered, as there.
    #[must_use]
    pub fn primary_selection(
        mut self,
        provider: impl CapabilityProvider<PrimarySelectionCapability>,
    ) -> Self {
        if self.windows == WindowSystem::Platform(TargetPlatform::Linux) {
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
