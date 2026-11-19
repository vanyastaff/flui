//! [`ApkAssets`]: the assets of an APK, through `AAssetManager`.

use std::ffi::CString;
use std::io::Read;
use std::sync::Arc;

use android_activity::ndk::asset::AssetManager;
use flui_platform::assets::{AssetFuture, AssetKey, AssetLoadError, AssetSource};

use super::PACKAGE_DIR;

/// Reads asset `key` from the APK's `assets/flui_assets/<key>` with
/// `AAssetManager_open` in streaming mode (ADR-0384 §3).
///
/// `AAsset` is not `Send`, so a read opens, checks and reads its asset in the
/// first poll of its future, on whatever lane polls it: the length is checked
/// against the limit before anything is read, and no more than the limit is
/// read even if the asset reports less than it holds.
pub struct ApkAssets {
    manager: Arc<AssetManager>,
}

impl ApkAssets {
    /// A source over `manager`, the activity's asset manager
    /// (`AndroidApp::asset_manager`).
    #[must_use]
    pub fn new(manager: AssetManager) -> Self {
        Self {
            manager: Arc::new(manager),
        }
    }
}

impl AssetSource for ApkAssets {
    fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>> {
        let manager = Arc::clone(&self.manager);
        let key = key.clone();
        // No `.await` inside: the `AAsset` lives within the one poll.
        Box::pin(async move { read(&manager, &key, limit) })
    }
}

fn read(manager: &AssetManager, key: &AssetKey, limit: u64) -> Result<Vec<u8>, AssetLoadError> {
    let name = CString::new(format!("{PACKAGE_DIR}/{key}"))
        .expect("BUG: an AssetKey holds no NUL, so neither does its APK path");
    // `AAssetManager_open` answers NULL for a missing entry and for a
    // directory alike: neither is an asset.
    let mut asset = manager
        .open(&name)
        .ok_or_else(|| AssetLoadError::NotFound { key: key.clone() })?;
    let too_large = || AssetLoadError::TooLarge {
        key: key.clone(),
        limit,
    };
    let length = u64::try_from(asset.length()).unwrap_or(u64::MAX);
    if length > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(usize::try_from(length).unwrap_or(usize::MAX))
        .map_err(|_| AssetLoadError::from_io(key, std::io::ErrorKind::OutOfMemory.into()))?;
    // One byte past the limit tells an asset that grew from one that fits.
    let mut bounded = (&mut asset).take(limit.saturating_add(1));
    bounded
        .read_to_end(&mut bytes)
        .map_err(|error| AssetLoadError::from_io(key, error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

impl std::fmt::Debug for ApkAssets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApkAssets").finish_non_exhaustive()
    }
}
