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
//! let build_fn: extern "C" fn(f32, f32) -> *mut std::ffi::c_void = unsafe {
//!     let ptr = lib.symbol("flui_scene_build").expect("symbol not found");
//!     std::mem::transmute(ptr)
//! };
//! ```

use std::{
    ffi::c_void,
    path::{Path, PathBuf},
};

/// A loaded dynamic library handle with automatic cleanup on drop.
///
/// Wraps platform-specific `dlopen`/`LoadLibraryW` and provides
/// symbol resolution via `dlsym`/`GetProcAddress`.
#[allow(missing_debug_implementations)]
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
#[allow(unsafe_code)]
unsafe impl Send for DynLib {}

impl DynLib {
    /// Load a dynamic library from the given path.
    ///
    /// Returns `None` if the file doesn't exist or loading fails.
    pub fn open(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        let handle = sys::load_library(path)?;
        Some(DynLib {
            handle,
            path: path.to_path_buf(),
        })
    }

    /// Resolve a symbol by name from the loaded library.
    ///
    /// # Safety
    ///
    /// The caller must ensure the returned pointer is transmuted to the
    /// correct function signature. Calling with a wrong signature is UB.
    #[allow(unsafe_code)]
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
        // (it returns `None` on a null handle), `DynLib` is not `Clone`, and
        // `Drop` runs once — so this is neither a null nor a double close.
        #[allow(unsafe_code)]
        unsafe {
            sys::close_library(self.handle);
        }
        tracing::trace!("DynLib closed: {}", self.path.display());
    }
}

/// Get the modification time of a file as seconds since the Unix epoch.
///
/// Returns 0 if the file doesn't exist or metadata can't be read.
pub fn file_mtime(path: impl AsRef<Path>) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

// ── Platform-specific implementations ──────────────────────────────────

#[cfg(unix)]
mod sys {
    use std::{
        ffi::{CStr, CString, c_void},
        path::Path,
    };

    pub(super) fn load_library(path: &Path) -> Option<*mut c_void> {
        let path_str = path.to_str()?;
        let c_path = CString::new(path_str).ok()?;

        // SAFETY: `c_path` is a NUL-terminated `CString` that outlives the
        // `dlopen` call, and the returned handle is null-checked before it
        // escapes this block.
        //
        // CAVEAT, not a justification: POSIX does NOT require `dlerror` to be
        // thread-safe (§2.9.1 lists it among the exemptions). glibc and musl
        // give it a per-thread slot, so the error read below is sound there,
        // but on an implementation with a shared slot a concurrent `dlopen`
        // could rewrite the buffer between `dlerror()` and `CStr::from_ptr`.
        // `DynLib: Send` makes that reachable; it is unaddressed.
        #[allow(unsafe_code)]
        unsafe {
            // Clear previous error
            libc::dlerror();

            // RTLD_LOCAL prevents the plugin's symbols from polluting the global
            // symbol table. Without it, duplicate symbols between the host and
            // plugin (e.g., from shared crate dependencies like flui-types) cause
            // SIGBUS/SIGSEGV crashes during hot-reload when the old .so is
            // unloaded and a new one is loaded.
            let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                let err = libc::dlerror();
                if !err.is_null() {
                    let msg = CStr::from_ptr(err).to_string_lossy();
                    tracing::trace!("dlopen failed for {}: {}", path.display(), msg);
                }
                return None;
            }
            Some(handle)
        }
    }

    pub(super) fn get_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c_name = CString::new(name).ok()?;

        // SAFETY: `handle` comes from `load_library` and is kept alive by the
        // owning `DynLib` for the duration of this call; `c_name` is a
        // NUL-terminated `CString` that outlives it. The result is only
        // null-checked here — turning it into a callable is the caller's
        // obligation, documented on `DynLib::symbol`.
        #[allow(unsafe_code)]
        unsafe {
            let ptr = libc::dlsym(handle, c_name.as_ptr());
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    /// # Safety
    ///
    /// `handle` must be a valid library handle returned by `load_library`.
    #[allow(unsafe_code)]
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

    pub(super) fn load_library(path: &Path) -> Option<*mut c_void> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

        #[allow(unsafe_code)]
        unsafe {
            let handle = LoadLibraryW(windows::core::PCWSTR(wide.as_ptr())).ok()?;
            Some(handle.0)
        }
    }

    pub(super) fn get_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c_name = CString::new(name).ok()?;
        let module = HMODULE(handle.cast());

        #[allow(unsafe_code)]
        unsafe {
            let addr = GetProcAddress(module, windows::core::PCSTR(c_name.as_ptr().cast()));
            addr.map(|f| f as *mut c_void)
        }
    }

    /// # Safety
    ///
    /// `handle` must be a valid library handle returned by `load_library`.
    #[allow(unsafe_code)]
    pub(super) unsafe fn close_library(handle: *mut c_void) {
        unsafe {
            let module = HMODULE(handle.cast());
            let _ = FreeLibrary(module);
        }
    }
}
