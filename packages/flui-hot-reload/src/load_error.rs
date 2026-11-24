//! Why a plugin library was not loaded.

use std::path::PathBuf;

/// Why [`ScenePlugin::load`](crate::ScenePlugin::load),
/// [`WorkerPlugin::load`](crate::WorkerPlugin::load) or
/// [`DynLib::open`](crate::DynLib::open) refused a library. Each
/// variant names the file and says what the developer does about it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// No file is at the path yet: the first build has not landed.
    #[error("no plugin library at {path} yet")]
    Missing {
        /// The library's path.
        path: PathBuf,
    },
    /// The platform loader refused the file: an unresolved symbol, another
    /// architecture, a missing dependency, a file still being written.
    #[error("the platform loader could not open {path}: {message}")]
    Open {
        /// The library's path.
        path: PathBuf,
        /// The loader's own text (`dlerror`, or the Windows error message).
        message: String,
    },
    /// The library exports neither plugin family's build entry.
    #[error(
        "{path} is not a FLUI plugin: it exports neither `flui_app_build` nor `flui_scene_build`; \
         expand `app_plugin!` or `scene_plugin!` in it"
    )]
    NotAPlugin {
        /// The library's path.
        path: PathBuf,
    },
    /// The library lacks a symbol its plugin family requires.
    #[error("{path} exports no `{symbol}`; rebuild it against this tree")]
    MissingSymbol {
        /// The library's path.
        path: PathBuf,
        /// The symbol it lacks.
        symbol: String,
    },
    /// Host and library were built by different compilers or source
    /// revisions, so their layouts are not known to agree.
    #[error(
        "{path} was built by another compiler or source revision (ABI token {plugin:#018x}, \
         this host's is {host:#018x}); rebuild both from one worktree with one toolchain"
    )]
    AbiMismatch {
        /// The library's path.
        path: PathBuf,
        /// This host's [`abi_token`](crate::abi_token).
        host: u64,
        /// The library's token.
        plugin: u64,
    },
    /// The worker's `flui_worker_init` panicked; the worker's stderr (logcat
    /// on Android) has the panic.
    #[error("the worker's init in {path} panicked; fix it and save again")]
    InitFailed {
        /// The library's path.
        path: PathBuf,
    },
}
