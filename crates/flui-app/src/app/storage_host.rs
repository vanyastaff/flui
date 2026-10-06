//! The byte storage the host hands every realm it builds.

use std::sync::Arc;

use flui_platform_api::Storage;

use super::AppConfig;

/// The storage `config` asks for, which every realm the runner builds hands
/// its widgets through `LifecycleContext::storage`: `None` when the
/// application configured none, or where no file store exists.
///
/// Not yet wired: a configured directory is reported and no storage is
/// given.
pub(crate) fn host_storage(config: &AppConfig) -> Option<Arc<dyn Storage>> {
    #[cfg(feature = "persist")]
    if let Some(name) = config.storage_dir {
        tracing::warn!(
            storage_dir = name.as_str(),
            "a storage directory is configured, but no file store is wired yet; \
             widgets get no storage"
        );
    }
    #[cfg(not(feature = "persist"))]
    let _ = config;
    None
}
