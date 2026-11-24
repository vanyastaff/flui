//! Cross-platform dynamic library loading.
//!
//! Provides a safe wrapper around platform-specific dynamic library APIs:
//! - Unix: `dlopen` / `dlsym` / `dlclose` (via `libc`)
//! - Windows: `LoadLibraryW` / `GetProcAddress` / `FreeLibrary` (via `windows`)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_hot_reload::dynlib::DynLib;
//! use std::path::Path;
//!
//! let lib = DynLib::open(Path::new("libplugin.so")).expect("failed to load");
//! let build_fn: extern "C" fn(f64, f64) -> *mut std::ffi::c_void = unsafe {
//!     let ptr = lib.symbol("flui_scene_build").expect("symbol not found");
//!     std::mem::transmute(ptr)
//! };
//! ```

use std::{
    ffi::c_void,
    path::{Path, PathBuf},
};

use crate::LoadError;

/// A loaded dynamic library handle with automatic cleanup on drop.
///
/// Wraps platform-specific `dlopen`/`LoadLibraryW` and provides
/// symbol resolution via `dlsym`/`GetProcAddress`.
#[expect(
    missing_debug_implementations,
    reason = "`DynLib` holds a raw library handle with nothing meaningful to print"
)]
pub struct DynLib {
    handle: *mut c_void,
    path: PathBuf,
}

// SAFETY: `handle` is an opaque loader handle. It is never dereferenced — it is
// only handed back to `dlsym`/`dlclose` (or `GetProcAddress`/`FreeLibrary`),
// which are documented thread-safe on both backends this type covers, so the
// value itself races with nothing.
//
// NOT claimed: that the handle is uniquely owned. `dlopen` on an
// already-loaded path returns the SAME handle with an incremented reference
// count, so two `DynLib`s can hold equal handle values. That is sound because
// each `Drop` decrements exactly once, balancing N opens against N closes —
// not because `DynLib` is not `Clone`.
//
// What this impl does NOT establish, and what callers must not assume: that
// code or data resolved out of the library outlives `Drop`. See the module
// note on the plugin boundary.
#[expect(unsafe_code, reason = "`DynLib`'s raw handle is not auto-`Send`")]
unsafe impl Send for DynLib {}

impl DynLib {
    /// Load a dynamic library from the given path.
    ///
    /// # Errors
    ///
    /// [`LoadError::Missing`] when no file is at `path`, and
    /// [`LoadError::Open`] with the platform loader's text when the loader
    /// refuses the file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        match sys::load_library(path) {
            Ok(handle) => Ok(DynLib {
                handle,
                path: path.to_path_buf(),
            }),
            Err(_) if !path.exists() => Err(LoadError::Missing {
                path: path.to_path_buf(),
            }),
            Err(message) => Err(LoadError::Open {
                path: path.to_path_buf(),
                message,
            }),
        }
    }

    /// Resolve a symbol by name from the loaded library.
    ///
    /// # Safety
    ///
    /// The caller must ensure the returned pointer is transmuted to the
    /// correct function signature. Calling with a wrong signature is UB.
    #[expect(
        unsafe_code,
        reason = "an `unsafe fn`: the caller must transmute the returned pointer to the right signature, stated under `# Safety`"
    )]
    pub unsafe fn symbol(&self, name: &str) -> Option<*mut c_void> {
        sys::get_symbol(self.handle, name)
    }

    /// The file path this library was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DynLib {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is the non-null value `load_library` returned
        // (it answers `Err` for a null handle), `DynLib` is not `Clone`, and
        // `Drop` runs once — so this is neither a null nor a double close.
        #[expect(unsafe_code, reason = "`dlclose` of the handle this `DynLib` owns")]
        unsafe {
            sys::close_library(self.handle);
        }
        tracing::trace!("DynLib closed: {}", self.path.display());
    }
}

/// The native revision used by reload drivers, preserving subsecond precision
/// and distinguishing unavailable metadata from an epoch timestamp.
pub(crate) fn file_revision(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// The artifact a reload driver last failed to load, by path and
/// [`file_revision`], so a driver polling every frame does not reopen a
/// refused file on every poll. The same file is tried again after
/// [`LoadRefusal::RETRY`]: a load can fail for a reason outside its bytes
/// (a scanner or the linker still holding the file, a descriptor limit), and
/// a rebuild with the same bytes lands under the same name and revision.
#[derive(Debug)]
pub(crate) struct LoadRefusal {
    refused: Option<Refused>,
    retry: std::time::Duration,
}

#[derive(Debug)]
struct Refused {
    path: PathBuf,
    revision: Option<std::time::SystemTime>,
    at: std::time::Instant,
}

impl Default for LoadRefusal {
    fn default() -> Self {
        Self::with_retry(Self::RETRY)
    }
}

impl LoadRefusal {
    /// How long an unchanged refused artifact waits before it is opened again.
    pub(crate) const RETRY: std::time::Duration = std::time::Duration::from_secs(1);

    /// A refusal that tries an unchanged refused artifact again after `retry`.
    pub(crate) fn with_retry(retry: std::time::Duration) -> Self {
        Self {
            refused: None,
            retry,
        }
    }

    /// Load `path` with `load`, unless it is the artifact that was refused,
    /// unchanged since and refused less than the retry interval ago. A
    /// failure is remembered by the revision read before the load, so a
    /// build that lands while `load` runs is not taken for the refused file.
    ///
    /// A refusal is logged with its reason once per artifact revision: a file
    /// not built yet at `debug`, any other at `warn`. Retries of the same
    /// revision stay quiet.
    pub(crate) fn load<T>(
        &mut self,
        path: &Path,
        load: impl FnOnce(&Path) -> Result<T, LoadError>,
    ) -> Option<T> {
        let revision = file_revision(path);
        let same_artifact = self
            .refused
            .as_ref()
            .is_some_and(|refused| refused.path == path && refused.revision == revision);
        if same_artifact
            && self
                .refused
                .as_ref()
                .is_some_and(|refused| refused.at.elapsed() < self.retry)
        {
            return None;
        }
        let at = std::time::Instant::now();
        match load(path) {
            Ok(loaded) => {
                self.refused = None;
                Some(loaded)
            }
            Err(error) => {
                // The refusal is recorded before the subscriber runs, so a
                // subscriber that panics leaves this revision refused.
                self.refused = Some(Refused {
                    path: path.to_path_buf(),
                    revision,
                    at,
                });
                if !same_artifact {
                    if matches!(error, LoadError::Missing { .. }) {
                        tracing::debug!(%error, "hot reload: plugin not loaded");
                    } else {
                        tracing::warn!(%error, "hot reload: plugin refused");
                    }
                }
                None
            }
        }
    }
}

// ── Platform-specific implementations ──────────────────────────────────

#[cfg(unix)]
mod sys {
    use std::{
        ffi::{CString, c_void},
        path::Path,
    };

    /// The handle, or the loader's text on failure.
    pub(super) fn load_library(path: &Path) -> Result<*mut c_void, String> {
        let c_path = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| "the path contains a NUL byte".to_owned())?;

        // SAFETY: `c_path` is a NUL-terminated `CString` that outlives the
        // `dlopen` call, and the returned handle is null-checked before it
        // escapes this block. `dlerror` is read on the thread whose `dlopen`
        // failed, before any other loader call, and its text is copied before
        // the next one can overwrite it.
        #[expect(unsafe_code, reason = "`dlopen` of a NUL-terminated path")]
        unsafe {
            // RTLD_LOCAL prevents the plugin's symbols from polluting the global
            // symbol table. Without it, duplicate symbols between the host and
            // plugin (e.g., from shared crate dependencies like flui-foundation) cause
            // SIGBUS/SIGSEGV crashes during hot-reload when the old .so is
            // unloaded and a new one is loaded.
            let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                let error = libc::dlerror();
                return Err(if error.is_null() {
                    "dlopen failed and set no error text".to_owned()
                } else {
                    std::ffi::CStr::from_ptr(error)
                        .to_string_lossy()
                        .into_owned()
                });
            }
            Ok(handle)
        }
    }

    pub(super) fn get_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c_name = CString::new(name).ok()?;

        // SAFETY: `handle` comes from `load_library` and is kept alive by the
        // owning `DynLib` for the duration of this call; `c_name` is a
        // NUL-terminated `CString` that outlives it. The result is only
        // null-checked here — turning it into a callable is the caller's
        // obligation, documented on `DynLib::symbol`.
        #[expect(unsafe_code, reason = "`dlsym` on a live handle")]
        unsafe {
            let ptr = libc::dlsym(handle, c_name.as_ptr());
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    /// # Safety
    ///
    /// `handle` must be a valid library handle returned by `load_library`.
    #[expect(
        unsafe_code,
        reason = "an `unsafe fn`: `handle` must be a live handle from `load_library`, stated under `# Safety`"
    )]
    pub(super) unsafe fn close_library(handle: *mut c_void) {
        // SAFETY: edition 2024 makes unsafe-fn bodies safe by default, so the
        // call still needs its own block. `handle` is the value `dlopen`
        // returned in `load_library`, so it is non-null. Closing is balanced
        // rather than unique: `dlopen` refcounts, and each `DynLib::drop`
        // decrements exactly once (see the note on `unsafe impl Send`). The
        // caller-side contract is the `# Safety` doc above.
        unsafe {
            libc::dlclose(handle);
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::{
        ffi::{CString, c_void},
        os::windows::ffi::OsStrExt,
        path::Path,
    };

    use windows::Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    };

    /// The handle, or the loader's text on failure.
    pub(super) fn load_library(path: &Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the
        // `LoadLibraryW` call; a failed load is an `Err` carrying the
        // system's message, and no handle escapes this block with it.
        #[expect(unsafe_code, reason = "`LoadLibraryW` of a NUL-terminated path")]
        unsafe {
            LoadLibraryW(windows::core::PCWSTR(wide.as_ptr()))
                .map(|handle| handle.0)
                .map_err(|error| error.to_string())
        }
    }

    pub(super) fn get_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c_name = CString::new(name).ok()?;
        let module = HMODULE(handle.cast());

        // SAFETY: `handle` comes from `load_library` and is kept alive by the
        // owning `DynLib` for the duration of this call; `c_name` is a
        // NUL-terminated `CString` that outlives it. The result is only an
        // address — turning it into a callable is the caller's obligation,
        // documented on `DynLib::symbol`.
        #[expect(unsafe_code, reason = "`GetProcAddress` on a live module")]
        unsafe {
            let addr = GetProcAddress(module, windows::core::PCSTR(c_name.as_ptr().cast()));
            addr.map(|f| f as *mut c_void)
        }
    }

    /// # Safety
    ///
    /// `handle` must be a valid library handle returned by `load_library`.
    #[expect(
        unsafe_code,
        reason = "an `unsafe fn`: `handle` must be a live handle from `load_library`, stated under `# Safety`"
    )]
    pub(super) unsafe fn close_library(handle: *mut c_void) {
        // SAFETY: `handle` is the module `LoadLibraryW` returned in
        // `load_library` (the caller-side contract is the `# Safety` doc
        // above). Like `dlclose`, `FreeLibrary` is balanced rather than
        // unique: the loader refcounts, and each `DynLib::drop` decrements
        // exactly once.
        unsafe {
            let module = HMODULE(handle.cast());
            let _ = FreeLibrary(module);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::LoadRefusal;

    /// A refused artifact is opened again only once it changes, and a
    /// successful load forgets the refusal.
    #[test]
    fn a_refused_artifact_is_reopened_once_it_changes_or_after_a_wait() {
        let dir =
            std::env::temp_dir().join(format!("flui-hot-reload-refusal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let artifact = dir.join("libworker.so");
        std::fs::write(&artifact, b"not a library").expect("a broken build");
        let opened = Cell::new(0_u32);
        let mut refusal = LoadRefusal::default();
        let refuse = |_: &std::path::Path| {
            opened.set(opened.get() + 1);
            Err::<(), _>(crate::LoadError::NotAPlugin {
                path: std::path::PathBuf::from("libworker.so"),
            })
        };

        assert_eq!(refusal.load(&artifact, refuse), None);
        assert_eq!(refusal.load(&artifact, refuse), None);
        assert_eq!(
            opened.get(),
            1,
            "the unchanged refused file is not reopened"
        );

        // A later revision of the same path, as the next build writes it.
        let later = std::fs::metadata(&artifact)
            .and_then(|metadata| metadata.modified())
            .expect("a revision")
            + std::time::Duration::from_secs(1);
        std::fs::File::options()
            .write(true)
            .open(&artifact)
            .and_then(|file| file.set_modified(later))
            .expect("a new revision");
        assert_eq!(
            refusal.load(&artifact, |_| {
                opened.set(opened.get() + 1);
                Ok(())
            }),
            Some(())
        );
        assert_eq!(opened.get(), 2, "the rebuilt file is opened");
        assert_eq!(refusal.load(&artifact, refuse), None);
        assert_eq!(opened.get(), 3, "a loaded file leaves no refusal behind");

        // A failure outside the bytes clears without a new revision.
        let mut refusal = LoadRefusal::with_retry(std::time::Duration::from_millis(50));
        assert_eq!(refusal.load(&artifact, refuse), None);
        assert_eq!(refusal.load(&artifact, refuse), None);
        assert_eq!(opened.get(), 4, "not reopened inside the retry interval");
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert_eq!(refusal.load(&artifact, refuse), None);
        assert_eq!(
            opened.get(),
            5,
            "the unchanged file is tried again after it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
