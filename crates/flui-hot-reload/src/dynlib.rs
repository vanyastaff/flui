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

// The handle is a raw pointer but we only use it from a single thread
// (the main/render thread). This matches the existing ScenePlugin pattern.
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

        #[allow(unsafe_code)]
        unsafe {
            let ptr = libc::dlsym(handle, c_name.as_ptr());
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    /// # Safety
    ///
    /// `handle` must be a valid library handle returned by `load_library`.
    pub(super) unsafe fn close_library(handle: *mut c_void) {
        libc::dlclose(handle);
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
