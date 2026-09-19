//! macOS clipboard implementation using NSPasteboard
//!
//! Provides clipboard access using the macOS Cocoa NSPasteboard API.
//!
//! # Thread model
//!
//! Every NSPasteboard operation is routed through an **owner lane**: the
//! AppKit main thread's dispatch queue for production instances, and a
//! process-wide serial dispatch queue for test instances. Apple documents
//! NSPasteboard as main-thread-only (the AppKit Thread Safety Summary lists
//! no exception), so the main lane is the one lane all other process
//! pasteboard traffic already serializes on — menu commands, other
//! frameworks' controls, and pasteboard services. A per-instance lock cannot
//! coordinate with traffic that does not take it; routing onto the main lane
//! puts FLUI on the same serialization as the rest of the process.
//!
//! The struct stores no raw Objective-C object: the pasteboard is resolved
//! to a live `id` on the owner lane for each operation, so no `id` is ever
//! exchanged across threads. Cross-thread calls block on the lane until the
//! operation completes.
//!
//! The lane machinery itself lives in the shared [`super::owner_lane`]
//! module — this file only decides what runs on the lane.

use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardType};
use objc2_foundation::NSString;

use crate::traits::Clipboard;

/// macOS NSPasteboard-based clipboard implementation
///
/// Thread-safe proxy around NSPasteboard. Every operation routes to the owner
/// lane and resolves the pasteboard `id` there, so no raw Objective-C object
/// is stored in the struct or shared across threads.
///
/// Liveness: a cross-thread call **blocks** on the owner lane until the
/// operation completes. Do not call clipboard from a background thread before
/// `Platform::run` has started the event loop, and never wait from the main
/// thread on a thread that is blocked inside a clipboard call — the lane
/// would stall.
pub struct MacOSClipboard {
    /// `None` = general pasteboard; `Some(name)` = named pasteboard (tests).
    pasteboard: Option<String>,
    /// Owner lane every operation is routed through.
    owner: &'static dispatch::Queue,
    /// True when `owner` is the main queue, so "on the main thread" is
    /// equivalent to "on the owner lane" for the direct-path probe.
    owner_is_main: bool,
}

impl std::fmt::Debug for MacOSClipboard {
    // Hand-written: `dispatch::Queue` has no `Debug` representation, and the
    // pasteboard is identified only by an optional board name.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOSClipboard").finish_non_exhaustive()
    }
}

/// The plain-text pasteboard UTI (`NSPasteboardTypeString`).
///
/// `public.utf8-plain-text` is the UTI AppKit's own `NSPasteboardTypeString`
/// constant holds; a `NSPasteboardType` is an `NSString`, so this wraps the
/// literal.
fn string_type() -> Retained<NSPasteboardType> {
    NSPasteboardType::from_str("public.utf8-plain-text")
}

/// Resolve the pasteboard object for a board name on the owner lane.
///
/// Returns `generalPasteboard` when `name` is `None`, otherwise the named
/// pasteboard created on demand. Named boards are resolved per operation and
/// never stored; both constructors return a retained object, so there is no
/// nil case for a caller to check.
fn resolve(name: Option<String>) -> Retained<NSPasteboard> {
    match name {
        None => NSPasteboard::generalPasteboard(),
        Some(name) => {
            let ns_name = NSString::from_str(&name);
            NSPasteboard::pasteboardWithName(&ns_name)
        }
    }
}

impl MacOSClipboard {
    /// Create a new clipboard instance backed by the general pasteboard.
    ///
    /// The pasteboard id is not resolved here; it is resolved on the owner
    /// lane inside each operation, so construction never touches AppKit.
    pub fn new() -> Self {
        tracing::debug!("Created macOS clipboard (NSPasteboard)");
        Self {
            pasteboard: None,
            owner: super::owner_lane::owner_queue(),
            owner_is_main: true,
        }
    }

    /// Create a test instance on a caller-supplied lane for a named board.
    ///
    /// Test lanes pass the shared `test_owner_queue()`; `owner_is_main` is
    /// false so even a call made on the OS main thread routes through the
    /// test lane rather than touching AppKit pasteboard traffic off-lane.
    #[cfg(test)]
    fn for_test(owner: &'static dispatch::Queue, name: impl Into<String>) -> Self {
        Self {
            pasteboard: Some(name.into()),
            owner,
            owner_is_main: false,
        }
    }

    /// Get the current change count
    ///
    /// Change count increments each time the pasteboard contents change.
    /// Use this to detect if clipboard has changed without reading contents.
    #[cfg_attr(not(test), expect(dead_code))]
    fn change_count(&self) -> i64 {
        self.with_pasteboard_on_owner(|pasteboard| pasteboard.changeCount() as i64)
    }

    /// Run `f` with the pasteboard id resolved on the owner lane.
    ///
    /// The id is produced and consumed within this call: it never outlives
    /// the enclosing autoreleasepool and never crosses threads. On the owner
    /// lane (or, for a main-lane instance, on the main thread) `f` runs
    /// directly with no dispatch; from any other thread the operation is
    /// dispatched synchronously to the owner queue and the caller blocks
    /// until it completes.
    fn with_pasteboard_on_owner<R: Send>(&self, f: impl FnOnce(&NSPasteboard) -> R + Send) -> R {
        let name = self.pasteboard.clone();
        // The pasteboard is re-resolved on the lane inside the closure — never
        // captured from the calling thread (a `Retained<NSPasteboard>` is not
        // `Send`, which is what keeps the routing honest). Routing itself (the
        // direct in-lane path, the `isMainThread` shortcut for main-lane
        // instances, and the catch_unwind-shielded `exec_sync` for everything
        // else) is the shared `owner_lane` machinery's job.
        super::owner_lane::exec_on_owner(self.owner, self.owner_is_main, || {
            objc2::rc::autoreleasepool(|_pool| f(&resolve(name)))
        })
    }
}

impl Default for MacOSClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for MacOSClipboard {
    fn read_text(&self) -> Option<String> {
        self.with_pasteboard_on_owner(|pasteboard| {
            let string_type = string_type();

            // `types()` is `None` only when the pasteboard has no types at
            // all; a missing text type is the ordinary "no text" case.
            let types = pasteboard.types()?;
            if !types.iter().any(|t| *t == *string_type) {
                tracing::debug!("Pasteboard does not contain text");
                return None;
            }

            let ns_string = pasteboard.stringForType(&string_type)?;
            let rust_string = ns_string.to_string();
            tracing::debug!(len = rust_string.len(), "Read text from clipboard");
            Some(rust_string)
        })
    }

    fn write_text(&self, text: String) {
        self.with_pasteboard_on_owner(|pasteboard| {
            // Clear existing contents; the return value is the new change
            // count, which this backend does not consume.
            pasteboard.clearContents();

            let ns_string = NSString::from_str(&text);
            let string_type = string_type();
            // The pasteboard copies the string's contents during the set.
            let success = pasteboard.setString_forType(&ns_string, &string_type);

            if success {
                tracing::debug!(len = text.len(), "Wrote text to clipboard");
            } else {
                tracing::error!(len = text.len(), "Failed to write text to clipboard");
            }
        });
    }

    fn has_text(&self) -> bool {
        self.with_pasteboard_on_owner(|pasteboard| {
            let string_type = string_type();
            pasteboard
                .types()
                .is_some_and(|types| types.iter().any(|t| *t == *string_type))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::owner_lane::test_owner_queue;
    use super::*;

    /// Unique board name per test name and per test process.
    fn test_board(name: &str) -> String {
        format!("flui.clipboard.{name}.{}", std::process::id())
    }

    /// A test instance on the shared test lane for a uniquely named board.
    fn test_clipboard(name: &str) -> MacOSClipboard {
        MacOSClipboard::for_test(test_owner_queue(), test_board(name))
    }

    #[test]
    fn test_clipboard_creation() {
        let clipboard = test_clipboard("creation");
        // `resolve` travels through the shared lane like every other
        // pasteboard operation, never as a raw message to AppKit from a test
        // worker thread. A named board always resolves to a live object, so
        // the assertion is simply that the lane ran the body at all.
        let ran = clipboard.with_pasteboard_on_owner(|_pasteboard| true);
        assert!(ran, "Named pasteboard operation must run on the owner lane");
    }

    #[test]
    fn test_clipboard_roundtrip() {
        let clipboard = test_clipboard("roundtrip");

        let test_text = "Hello from FLUI macOS!";
        clipboard.write_text(test_text.to_string());

        let read_back = clipboard.read_text();
        assert_eq!(read_back.as_deref(), Some(test_text));
    }

    #[test]
    fn test_has_text() {
        let clipboard = test_clipboard("has_text");

        // Write text
        clipboard.write_text("Test".to_string());

        // Check if text is available
        assert!(
            clipboard.has_text(),
            "Clipboard should have text after write"
        );
    }

    #[test]
    fn test_change_count() {
        let clipboard = test_clipboard("change_count");

        let count1 = clipboard.change_count();
        clipboard.write_text("Test 1".to_string());
        let count2 = clipboard.change_count();

        assert!(count2 > count1, "Change count should increment after write");
    }

    #[test]
    fn test_concurrent_access_is_crash_free_and_serialized() {
        // ONE shared board for the whole hammer. The test lane serializes
        // every operation, so every write is atomic and every read sees a
        // complete known payload — never None, never a torn string. The
        // primary gate is crash-free completion of the whole hammer.
        let board = test_board("hammer");
        let sentinel = "stable-sentinel";

        let sentinel_guard = MacOSClipboard::for_test(test_owner_queue(), board.clone());
        sentinel_guard.write_text(sentinel.to_string());

        let known_payloads: Vec<String> = std::iter::once(sentinel.to_string())
            .chain((0..8).map(|i| format!("payload-{i}")))
            .collect();

        let mut threads = Vec::new();
        for i in 0..8 {
            // Each thread runs its own instance; all instances share the
            // lane and the shared board.
            let clipboard = MacOSClipboard::for_test(test_owner_queue(), board.clone());
            let known_payloads = known_payloads.clone();
            threads.push(std::thread::spawn(move || {
                clipboard.write_text(format!("payload-{i}"));
                let read = clipboard.read_text();
                assert!(
                    read.is_some(),
                    "Read must never be None on the serialized lane"
                );
                assert!(
                    known_payloads.iter().any(|p| p == read.as_deref().unwrap()),
                    "Read must be one of the known complete payloads, got {read:?}"
                );
            }));
        }
        for thread in threads {
            thread
                .join()
                .expect("hammer thread must finish without panicking");
        }

        let final_guard = MacOSClipboard::for_test(test_owner_queue(), board);
        final_guard.write_text("final".to_string());
        assert_eq!(final_guard.read_text().as_deref(), Some("final"));
    }
}
