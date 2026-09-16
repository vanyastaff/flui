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

use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};

use crate::traits::Clipboard;

/// The production owner lane: the dispatch queue bound to the AppKit main
/// thread. Calls from other threads are dispatched here synchronously.
fn owner_queue() -> &'static dispatch::Queue {
    static Q: std::sync::OnceLock<dispatch::Queue> = std::sync::OnceLock::new();
    Q.get_or_init(dispatch::Queue::main)
}

/// The test-instance owner lane: one process-wide serial queue shared by
/// every test instance. `cargo test` never pumps the AppKit main thread, so
/// `owner_queue()` there would deadlock; a serial queue serializes
/// pasteboard traffic exactly as the main lane does for production. Every
/// test instance routes through this same lane so test traffic never
/// straddles lanes.
#[cfg(test)]
fn test_owner_queue() -> &'static dispatch::Queue {
    static Q: std::sync::OnceLock<dispatch::Queue> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        dispatch::Queue::create(
            "flui.clipboard.test-owner",
            dispatch::QueueAttribute::Serial,
        )
    })
}

/// RAII reentrancy marker: records the owner lane the current thread is
/// executing a block on. GCD pools worker threads across queues, so a sticky
/// flag would route a later call on a reused thread off-lane; this guard's
/// `Drop` clears the marker when the block returns.
///
/// The marker is the lane identity itself — the owner queue's address — so
/// the reentrancy probe can tell "on THIS instance's lane" from "on some
/// other lane a reused worker thread last ran". All production instances
/// share `owner_queue()` and all test instances share `test_owner_queue()`,
/// so the probe is exact for every instance of either kind.
struct OnOwnerQueueGuard {
    owner: *const dispatch::Queue,
}

impl OnOwnerQueueGuard {
    fn new(owner: &'static dispatch::Queue) -> Self {
        let owner = std::ptr::from_ref(owner);
        ON_OWNER_QUEUE.with(|on_lane| on_lane.set(Some(owner)));
        OnOwnerQueueGuard { owner }
    }
}

impl Drop for OnOwnerQueueGuard {
    fn drop(&mut self) {
        // Clear only if the marker is still ours: a nested block on a
        // different lane overwrites it, and it is that block's own guard —
        // which runs after this one — that clears it.
        ON_OWNER_QUEUE.with(|on_lane| {
            if on_lane.get() == Some(self.owner) {
                on_lane.set(None);
            }
        });
    }
}

thread_local! {
    static ON_OWNER_QUEUE: std::cell::Cell<Option<*const dispatch::Queue>> =
        const { std::cell::Cell::new(None) };
}

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

/// Resolve the pasteboard object for a board name on the owner lane.
///
/// Returns `generalPasteboard` when `name` is `None`, otherwise the named
/// pasteboard created on demand. Named boards are resolved per operation and
/// never stored past the enclosing autoreleasepool.
fn resolve(name: Option<String>) -> id {
    match name {
        None => unsafe {
            // SAFETY: `+[NSPasteboard generalPasteboard]` is a documented
            // thread-safe class method returning the process-wide singleton
            // (or nil, which every caller checks). The object is managed by
            // AppKit for the process lifetime — it is neither autoreleased nor
            // owned by the enclosing pool — and the code only ever uses it on
            // the owner lane inside a call, never storing or returning it.
            msg_send![class!(NSPasteboard), generalPasteboard]
        },
        Some(name) => unsafe {
            // SAFETY: `+[NSPasteboard pasteboardWithName:]` returns the named
            // pasteboard, creating it if it does not already exist (or nil on
            // failure). The object lives in AppKit's process-wide named-board
            // table, outliving the enclosing pool; the name NSString is owned
            // (+1) and released here, and the returned `id` is used only on
            // the owner lane inside the calling operation, never stored or
            // returned through `R`.
            let ns_name = NSString::alloc(nil);
            let ns_name = NSString::init_str(ns_name, &name);
            let pasteboard: id = msg_send![class!(NSPasteboard), pasteboardWithName: ns_name];
            let _: () = msg_send![ns_name, release];
            pasteboard
        },
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
            owner: owner_queue(),
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
        self.with_pasteboard_on_owner(|pasteboard| {
            // SAFETY: `changeCount` is a plain integer getter; the id is live
            // on the owner lane inside the enclosing autoreleasepool, and
            // nil is handled before the message is sent.
            unsafe {
                if pasteboard == nil {
                    return 0;
                }
                msg_send![pasteboard, changeCount]
            }
        })
    }

    /// Run `f` with the pasteboard id resolved on the owner lane.
    ///
    /// The id is produced and consumed within this call: it never outlives
    /// the enclosing autoreleasepool and never crosses threads. On the owner
    /// lane (or, for a main-lane instance, on the main thread) `f` runs
    /// directly with no dispatch; from any other thread the operation is
    /// dispatched synchronously to the owner queue and the caller blocks
    /// until it completes.
    fn with_pasteboard_on_owner<R: Send>(&self, f: impl FnOnce(id) -> R + Send) -> R {
        // SAFETY: `+[NSThread isMainThread]` is a documented thread-safe class
        // method with no arguments and a BOOL return; it may be called from
        // any thread at any time.
        let is_main: bool = unsafe { msg_send![class!(NSThread), isMainThread] };
        let name = self.pasteboard.clone();

        if ON_OWNER_QUEUE.with(std::cell::Cell::get) == Some(std::ptr::from_ref(self.owner))
            || (self.owner_is_main && is_main)
        {
            objc::rc::autoreleasepool(|| f(resolve(name)))
        } else {
            // dispatch invokes the closure through an `extern "C"` trampoline
            // with no panic catch, so a Rust panic unwinding through
            // libdispatch's C frames would be undefined behaviour. The
            // dispatched body is therefore catch_unwind-wrapped and the panic
            // payload is replayed on the caller once `exec_sync` returns. The
            // RAII guard and the `catch_unwind` call themselves sit in front
            // of the shield, but both are infallible (`Cell::set` and a plain
            // std call) while GCD is running the block.
            let result = self.owner.exec_sync(move || {
                let _guard = OnOwnerQueueGuard::new(self.owner);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    objc::rc::autoreleasepool(|| f(resolve(name)))
                }))
            });
            match result {
                Ok(value) => value,
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
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
            // SAFETY: `pasteboard` is a live NSPasteboard id resolved on the
            // owner lane inside the enclosing autoreleasepool. The `types`
            // array and the `stringForType:` NSString are autoreleased and
            // live through the call; the UTF8String buffer is copied into an
            // owned Rust String before the pool drains.
            unsafe {
                if pasteboard == nil {
                    tracing::warn!("Pasteboard is nil, cannot read text");
                    return None;
                }

                // NSPasteboardTypeString is the UTI for plain text
                // (kUTTypeUTF8PlainText). Created autoreleased so the
                // enclosing pool owns its lifetime regardless of early
                // returns.
                let string_type: id = msg_send![
                    class!(NSString),
                    stringWithUTF8String: c"public.utf8-plain-text".as_ptr()
                ];

                // Get available types
                let types: id = msg_send![pasteboard, types];
                if types == nil {
                    tracing::debug!("No types available on pasteboard");
                    return None;
                }

                // Check if text is available
                let has_string: bool = msg_send![types, containsObject: string_type];

                if !has_string {
                    tracing::debug!("Pasteboard does not contain text");
                    return None;
                }

                // Read string
                let ns_string: id = msg_send![pasteboard, stringForType: string_type];
                if ns_string == nil {
                    tracing::warn!("Failed to read string from pasteboard");
                    return None;
                }

                // Convert NSString to Rust String
                let c_str: *const i8 = msg_send![ns_string, UTF8String];
                if c_str.is_null() {
                    tracing::warn!("Failed to get UTF8String from NSString");
                    return None;
                }

                let rust_string = std::ffi::CStr::from_ptr(c_str)
                    .to_string_lossy()
                    .into_owned();

                tracing::debug!(len = rust_string.len(), "Read text from clipboard");
                Some(rust_string)
            }
        })
    }

    fn write_text(&self, text: String) {
        self.with_pasteboard_on_owner(|pasteboard| {
            // SAFETY: `pasteboard` is a live NSPasteboard id on the owner
            // lane inside the enclosing autoreleasepool. The content
            // NSString is created and released entirely in this scope; the
            // pasteboard copies its contents during `setString:forType:`.
            unsafe {
                if pasteboard == nil {
                    tracing::error!("Pasteboard is nil, cannot write text");
                    return;
                }

                // Clear existing contents (`-clearContents` returns BOOL)
                let _: bool = msg_send![pasteboard, clearContents];

                // Create NSString from Rust String
                let ns_string = NSString::alloc(nil);
                let ns_string = NSString::init_str(ns_string, &text);

                // NSPasteboardTypeString UTI; created autoreleased, so the
                // enclosing pool owns its lifetime.
                let string_type: id = msg_send![
                    class!(NSString),
                    stringWithUTF8String: c"public.utf8-plain-text".as_ptr()
                ];

                // Write string to pasteboard
                let success: bool = msg_send![pasteboard, setString:ns_string forType:string_type];

                // The pasteboard copied the string's contents; release the
                // owned(+1) NSString. The UTI string is autoreleased and is
                // drained with the pool.
                let _: () = msg_send![ns_string, release];

                if success {
                    tracing::debug!(len = text.len(), "Wrote text to clipboard");
                } else {
                    tracing::error!(len = text.len(), "Failed to write text to clipboard");
                }
            }
        });
    }

    fn has_text(&self) -> bool {
        self.with_pasteboard_on_owner(|pasteboard| {
            // SAFETY: `types`/`containsObject:` operate on autoreleased
            // objects consumed before the enclosing pool drains; the UTI
            // string is created autoreleased and released with the pool.
            unsafe {
                if pasteboard == nil {
                    return false;
                }

                let string_type: id = msg_send![
                    class!(NSString),
                    stringWithUTF8String: c"public.utf8-plain-text".as_ptr()
                ];

                let types: id = msg_send![pasteboard, types];
                if types == nil {
                    return false;
                }

                msg_send![types, containsObject: string_type]
            }
        })
    }
}

#[cfg(test)]
mod tests {
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
        // The nil probe travels through the shared lane like every other
        // pasteboard operation, never as a raw message to AppKit from a test
        // worker thread.
        let resolved = clipboard.with_pasteboard_on_owner(|pasteboard| pasteboard != nil);
        assert!(resolved, "Named pasteboard should resolve to a live id");
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
