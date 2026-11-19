//! The application's packaged assets, read where each platform's package
//! keeps them (ADR-0384 §3–§4).
//!
//! `flui build` copies asset `key` to `flui_assets/<key>` inside the
//! package, and this module reads it there: beside the executable on Windows
//! and Linux, under the main bundle's resources on macOS and iOS
//! (`NSBundle::resourcePath`), in the APK's `assets/` through
//! `AAssetManager` on Android, and at the page-relative URL
//! `flui_assets/<key>` on the Web. A desktop development run reads the
//! project itself when `flui run` names it in `FLUI_ASSET_ROOT`. The process's
//! current directory is never a root.

use std::sync::Arc;

use flui_platform::assets::{AssetBundle, AssetFuture, AssetKey, AssetLoadError, AssetSource};

#[cfg(target_os = "android")]
mod apk;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(any(test, target_arch = "wasm32"))]
mod page_url;
#[cfg(any(test, not(any(target_os = "android", target_arch = "wasm32"))))]
mod root;

#[cfg(target_os = "android")]
pub use apk::ApkAssets;
#[cfg(target_arch = "wasm32")]
pub use page::PageAssets;

/// The directory inside a package that holds its assets.
pub const PACKAGE_DIR: &str = "flui_assets";

/// The environment variable a desktop development run names the project
/// root in, so the run reads the project's assets as they are edited.
pub const DEVELOPMENT_ROOT_VAR: &str = "FLUI_ASSET_ROOT";

/// The bundle this process's package holds: the directory `flui_assets/`
/// beside the executable or under the main bundle's resources, or the
/// project root a desktop development run names in [`DEVELOPMENT_ROOT_VAR`].
///
/// A process whose package cannot be located (no executable path, no main
/// bundle) gets a bundle that holds nothing, never the current directory.
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
#[must_use]
pub fn package_assets() -> AssetBundle {
    // iOS has no development root: the application runs from its bundle.
    #[cfg(target_os = "ios")]
    let development = None;
    #[cfg(not(target_os = "ios"))]
    let development = std::env::var_os(DEVELOPMENT_ROOT_VAR);
    match root::package_root(development, root::package_resources()) {
        Some(root) => AssetBundle::new(Arc::new(flui_platform::assets::DirectoryAssets::new(root))),
        None => no_assets(),
    }
}

/// The bundle this page's package holds, at the page-relative URL
/// `flui_assets/<key>`; one that holds nothing where the page has no base
/// URL (a worker).
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn package_assets() -> AssetBundle {
    match PageAssets::of_this_page() {
        Some(page) => AssetBundle::new(Arc::new(page)),
        None => no_assets(),
    }
}

/// A bundle that holds no asset: every key is
/// [`NotFound`](AssetLoadError::NotFound).
#[must_use]
pub fn no_assets() -> AssetBundle {
    AssetBundle::new(Arc::new(NoAssets))
}

/// The source of a process with no package to read.
struct NoAssets;

impl AssetSource for NoAssets {
    fn load(&self, key: &AssetKey, _limit: u64) -> AssetFuture<Vec<u8>> {
        let key = key.clone();
        Box::pin(std::future::ready(Err(AssetLoadError::NotFound { key })))
    }
}
