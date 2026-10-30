//! The clipboard: the system's text clipboard on each host (ADR-0151 §10).
//!
//! The model is `lock`, the process-wide lock that every Win32 clipboard
//! session runs under, which the Win32 host and winit's `arboard` clipboard on
//! Windows share. Beside it, one file per platform holds the host's clipboard
//! and fills the clipboard position of its table:
//!
//! - `windows`: `OpenClipboard` with an owner window of its own;
//! - `apple`: `NSPasteboard` on macOS, `UIPasteboard` on iOS;
//! - `android`: no file yet; the host holds a `MemoryClipboard` until the
//!   system clipboard is reached through JNI;
//! - `web`: `navigator.clipboard`, whose read answers when its promise
//!   settles;
//! - `winit`: `arboard`;
//! - `headless`: a `MemoryClipboard`, one per loop.

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
mod headless;
#[cfg(windows)]
mod lock;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

use flui_platform::capability::Resolver;

/// The host's plain-text clipboard, owner-local: what
/// [`Platform::clipboard`](crate::Platform::clipboard) hands out and the
/// application's clipboard provider builds its handles over (ADR-0152).
///
/// A read answers through its [`Resolver`]: at once where the operating
/// system reads synchronously (Win32, AppKit, UIKit, `arboard`), when the
/// browser's promise settles on the web. It never calls back into its caller.
pub trait NativeClipboard {
    /// Read the clipboard's text and answer `answer` with it, or with `None`
    /// when it holds no text.
    fn read_text(&self, answer: Resolver<Option<String>>);

    /// Replace the clipboard's text.
    fn write_text(&self, text: String);
}

/// A clipboard the operating system reads synchronously: its read answers
/// at once.
pub(crate) trait SyncClipboard {
    /// The clipboard's text now.
    fn read_now(&self) -> Option<String>;

    /// Replace the clipboard's text.
    fn write_now(&self, text: String);
}

impl<C: SyncClipboard> NativeClipboard for C {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(self.read_now());
    }

    fn write_text(&self, text: String) {
        self.write_now(text);
    }
}

/// A clipboard a loop holds for its whole life, which a `Send + Sync`
/// platform keeps in its state; each [`Platform::clipboard`](crate::Platform::clipboard)
/// hands out an owner-local `Rc` over it.
pub(crate) trait SharedClipboard: NativeClipboard + Send + Sync {}

impl<C: NativeClipboard + Send + Sync> SharedClipboard for C {}

impl<C: NativeClipboard + ?Sized> NativeClipboard for std::sync::Arc<C> {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        (**self).read_text(answer);
    }

    fn write_text(&self, text: String) {
        (**self).write_text(text);
    }
}

/// The in-memory clipboard of the headless host and of a host without a
/// system clipboard yet: it reads back what the application wrote.
#[derive(Default)]
pub struct MemoryClipboard {
    text: parking_lot::Mutex<Option<String>>,
}

impl MemoryClipboard {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MemoryClipboard {
    // Hand-written: the text may be private to the user. Whether it holds
    // any is safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryClipboard")
            .field("has_text", &self.text.lock().is_some())
            .finish()
    }
}

impl SyncClipboard for MemoryClipboard {
    fn read_now(&self) -> Option<String> {
        self.text.lock().clone()
    }

    fn write_now(&self, text: String) {
        *self.text.lock() = Some(text);
    }
}
