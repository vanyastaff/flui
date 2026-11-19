//! The built-in asset bundle capability (ADR-0384).
//!
//! Every UI runtime of a run reads one source: the package this process runs
//! from (`flui_native::assets::package_assets`), resolved the first time a
//! widget asks, or on Android the APK's assets, which the run installs from
//! its activity before any widget can ask. Every handle the provider gives
//! out clones that source, so a cache keys what it read by one authority.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::Unsupported;
use flui_platform::assets::{AssetBundle, AssetBundleCapability};
use flui_platform::capability::{CapabilityProvider, ProviderContext};

/// The run's asset bundle, once resolved or installed.
#[derive(Clone, Default)]
pub(crate) struct AssetSlot(Rc<RefCell<Option<AssetBundle>>>);

impl AssetSlot {
    /// Install `bundle` as the run's, before any widget asks: the APK's
    /// assets on Android.
    #[cfg_attr(
        not(any(target_os = "android", test)),
        expect(dead_code, reason = "only the Android run installs its bundle")
    )]
    pub(crate) fn install(&self, bundle: AssetBundle) {
        let replaced = self.0.replace(Some(bundle));
        drop(replaced);
    }

    /// The run's bundle: the installed one, or the process's package.
    fn bundle(&self) -> AssetBundle {
        self.0
            .borrow_mut()
            .get_or_insert_with(package_assets)
            .clone()
    }
}

/// The bundle of a run that installed none.
#[cfg(not(target_os = "android"))]
fn package_assets() -> AssetBundle {
    flui_native::assets::package_assets()
}

/// The bundle of an Android run whose activity never installed its APK's:
/// none, so a widget finds `NotFound` rather than a file system path.
#[cfg(target_os = "android")]
fn package_assets() -> AssetBundle {
    flui_native::assets::no_assets()
}

/// The built-in provider over the run's [`AssetSlot`].
pub(crate) struct HostAssets {
    pub(crate) slot: AssetSlot,
}

impl CapabilityProvider<AssetBundleCapability> for HostAssets {
    fn provide(&self, _ctx: &ProviderContext<'_>) -> Result<AssetBundle, Unsupported> {
        Ok(self.slot.bundle())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_platform::assets::AssetBundle;
    use flui_platform::testing::MemoryAssets;

    use super::AssetSlot;

    /// Every handle of a run reads one source: the package resolved the
    /// first time, or the one the run installed, so a cache keys what it read
    /// by one authority.
    ///
    /// Red-check: resolve the package on every acquisition.
    #[test]
    fn every_handle_of_a_run_reads_one_source() {
        let slot = AssetSlot::default();
        assert_eq!(slot.bundle().authority(), slot.bundle().authority());

        let installed = AssetBundle::new(Arc::new(MemoryAssets::new()));
        slot.install(installed.clone());
        assert_eq!(slot.bundle().authority(), installed.authority());
    }
}
