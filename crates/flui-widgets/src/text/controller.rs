//! [`TextEditingController`] — owns the text buffer and caret position for an
//! [`EditableText`](super::editable_text::EditableText) field.

use std::sync::{Arc, Mutex, PoisonError};

use flui_foundation::ListenerId;
use flui_foundation::notifier::{ChangeNotifier, Listenable, ListenerCallback};

// ============================================================================
// ControllerInner
// ============================================================================

/// Mutable interior of a [`TextEditingController`].
///
/// Guarded by a `Mutex` inside `Arc` so any clone of the controller refers to
/// the same live text and caret state.
struct ControllerInner {
    text: String,
    /// Byte offset of the caret into `text`.  Always a valid UTF-8 char boundary.
    caret_byte_offset: usize,
    /// The focus node of the [`EditableText`](super::EditableText) currently
    /// driven by this controller — published in its `init_state`, cleared in
    /// `dispose`. This is how a tap on the enclosing `TextField` focuses *its
    /// own* field rather than a scope-walk guess (ADR-0022 U4).
    focus_node_id: Option<flui_interaction::FocusNodeId>,
}

// ============================================================================
// TextEditingController
// ============================================================================

/// Owns the text buffer and caret position for a text input field.
///
/// Flutter parity: `widgets/editable_text.dart` `TextEditingController`.
///
/// # Sharing
///
/// `TextEditingController` is `Clone`: every clone shares the same underlying
/// buffer and listener list (both are `Arc`-backed internally).  The owning
/// widget state typically holds one clone; the key-event handler closure
/// captures a second.  `notify_listeners` propagates through every clone so
/// a listener added to any one clone fires regardless of which clone mutates
/// the buffer.
///
/// # Listening
///
/// Implement a reactive rebuild by registering via [`Listenable::add_listener`]
/// (returns a [`ListenerId`] for later removal in [`Listenable::remove_listener`])
/// or by passing [`TextEditingController::listenable`] to
/// [`AnimatedBuilder`](crate::AnimatedBuilder).
///
/// # DEFERRED (v1)
///
/// The following behaviors are absent in v1 and must not be faked:
/// - **Text selection**: only a collapsed caret (anchor == focus) is tracked.
///   Drag-to-select and selection rendering are not implemented.
/// - **Clipboard**: copy/paste/cut are not wired.
/// - **Input formatters**: no validation or transformation pipeline.
/// - **IME / composing region**: CJK and dead-key composition are not handled.
#[derive(Clone)]
pub struct TextEditingController {
    /// Shared text buffer + caret state.
    inner: Arc<Mutex<ControllerInner>>,
    /// Listener list — `ChangeNotifier` is itself `Arc`-backed so clones share
    /// the same list.
    notifier: ChangeNotifier,
}

impl std::fmt::Debug for TextEditingController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("TextEditingController")
            .field("text", &guard.text)
            .field("caret_byte_offset", &guard.caret_byte_offset)
            // `notifier` is intentionally omitted: its Arc-backed listener list
            // is noise in debug output and has no stable representation.
            .finish_non_exhaustive()
    }
}

impl Default for TextEditingController {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEditingController {
    /// Create a controller with an empty text buffer and caret at position 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ControllerInner {
                text: String::new(),
                caret_byte_offset: 0,
                focus_node_id: None,
            })),
            notifier: ChangeNotifier::new(),
        }
    }

    /// Create a controller pre-populated with `initial_text`, caret at the end.
    #[must_use]
    pub fn with_text(initial_text: impl Into<String>) -> Self {
        let text = initial_text.into();
        let caret_byte_offset = text.len();
        Self {
            inner: Arc::new(Mutex::new(ControllerInner {
                text,
                caret_byte_offset,
                focus_node_id: None,
            })),
            notifier: ChangeNotifier::new(),
        }
    }

    // =========================================================================
    // Read accessors
    // =========================================================================

    /// A snapshot of the current text buffer.
    pub fn text(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .text
            .clone()
    }

    /// The current caret position as a byte offset into [`Self::text`].
    ///
    /// Always points to a valid UTF-8 char boundary (including one past the
    /// last byte when the caret is at the end).
    pub fn caret_byte_offset(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .caret_byte_offset
    }

    // =========================================================================
    // Mutation — each method notifies listeners after the change
    // =========================================================================

    /// Insert `text` at the current caret position and advance the caret past it.
    ///
    /// Notifies listeners after the insertion.
    pub fn insert_str(&self, text: &str) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let caret = guard.caret_byte_offset;
            guard.text.insert_str(caret, text);
            guard.caret_byte_offset = caret + text.len();
        }
        self.notifier.notify_listeners();
    }

    /// Delete the character immediately to the **left** of the caret (Backspace).
    ///
    /// No-op when the caret is at the beginning of the buffer.
    pub fn backspace(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let caret = guard.caret_byte_offset;
            if caret == 0 {
                false
            } else {
                // Walk back to the previous char boundary.
                let prev_boundary = guard.text[..caret]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(idx, _)| idx);
                guard.text.drain(prev_boundary..caret);
                guard.caret_byte_offset = prev_boundary;
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Delete the character immediately to the **right** of the caret (Delete key).
    ///
    /// No-op when the caret is at the end of the buffer.
    pub fn delete_forward(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let caret = guard.caret_byte_offset;
            if caret == guard.text.len() {
                false
            } else {
                // Width of the char starting at `caret`.
                let char_width = guard.text[caret..].chars().next().map_or(0, char::len_utf8);
                guard.text.drain(caret..caret + char_width);
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret one character to the left.
    ///
    /// No-op when the caret is at the beginning.
    pub fn move_caret_left(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let caret = guard.caret_byte_offset;
            if caret == 0 {
                false
            } else {
                let prev_boundary = guard.text[..caret]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(idx, _)| idx);
                guard.caret_byte_offset = prev_boundary;
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret one character to the right.
    ///
    /// No-op when the caret is at the end.
    pub fn move_caret_right(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let caret = guard.caret_byte_offset;
            if caret == guard.text.len() {
                false
            } else {
                let char_width = guard.text[caret..].chars().next().map_or(0, char::len_utf8);
                guard.caret_byte_offset = caret + char_width;
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret to the beginning of the buffer (Home).
    ///
    /// No-op when the caret is already at position 0.
    pub fn move_caret_home(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.caret_byte_offset == 0 {
                false
            } else {
                guard.caret_byte_offset = 0;
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    /// Move the caret to the end of the buffer (End).
    ///
    /// No-op when the caret is already at the end.
    pub fn move_caret_end(&self) {
        let changed = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let end = guard.text.len();
            if guard.caret_byte_offset == end {
                false
            } else {
                guard.caret_byte_offset = end;
                true
            }
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }

    // =========================================================================
    // Reactive integration
    // =========================================================================

    /// The focus node of the `EditableText` this controller currently drives,
    /// or `None` between mounts. Published by `EditableTextState::init_state`
    /// and cleared by its `dispose` (ADR-0022 U4).
    pub(crate) fn focus_node_id(&self) -> Option<flui_interaction::FocusNodeId> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .focus_node_id
    }

    /// Publish (or clear) the driving field's focus node.
    pub(crate) fn set_focus_node_id(&self, id: Option<flui_interaction::FocusNodeId>) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .focus_node_id = id;
    }

    /// Return a listenable that fires whenever the controller's text or caret
    /// changes.  Pass it to [`AnimatedBuilder`](crate::AnimatedBuilder) to
    /// rebuild a widget subtree on every edit.
    ///
    /// The returned `Arc` wraps a clone of the internal `ChangeNotifier`, which
    /// is itself `Arc`-backed — both the widget build and the key handler share
    /// the same live listener list through their respective clones.
    pub fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::new(self.notifier.clone())
    }
}

// Delegate `Listenable` to the shared notifier so external code can subscribe
// directly on the controller rather than going through `controller.listenable()`.
impl Listenable for TextEditingController {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Basic buffer operations
    // ------------------------------------------------------------------

    #[test]
    fn empty_controller_starts_with_empty_text_and_caret_at_zero() {
        let controller = TextEditingController::new();
        assert_eq!(controller.text(), "");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn with_text_positions_caret_at_end() {
        let controller = TextEditingController::with_text("hello");
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.caret_byte_offset(), 5);
    }

    #[test]
    fn insert_str_appends_when_caret_at_end() {
        let controller = TextEditingController::new();
        controller.insert_str("hello");
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.caret_byte_offset(), 5);
    }

    #[test]
    fn insert_str_inserts_in_the_middle() {
        let controller = TextEditingController::with_text("helo");
        // Manually place caret before 'o'.
        controller.inner.lock().unwrap().caret_byte_offset = 3;
        controller.insert_str("l");
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.caret_byte_offset(), 4);
    }

    #[test]
    fn backspace_removes_char_left_of_caret() {
        let controller = TextEditingController::with_text("hello");
        controller.backspace();
        assert_eq!(controller.text(), "hell");
        assert_eq!(controller.caret_byte_offset(), 4);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let controller = TextEditingController::new();
        controller.backspace(); // Must not panic.
        assert_eq!(controller.text(), "");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn delete_forward_removes_char_right_of_caret() {
        let controller = TextEditingController::with_text("hello");
        controller.move_caret_home();
        controller.delete_forward();
        assert_eq!(controller.text(), "ello");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn delete_forward_at_end_is_noop() {
        let controller = TextEditingController::with_text("hi");
        controller.delete_forward(); // Caret already at end.
        assert_eq!(controller.text(), "hi");
        assert_eq!(controller.caret_byte_offset(), 2);
    }

    // ------------------------------------------------------------------
    // Caret navigation
    // ------------------------------------------------------------------

    #[test]
    fn move_caret_left_moves_one_char() {
        let controller = TextEditingController::with_text("abc");
        controller.move_caret_left();
        assert_eq!(controller.caret_byte_offset(), 2);
        controller.move_caret_left();
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn move_caret_left_at_start_is_noop() {
        let controller = TextEditingController::with_text("a");
        controller.move_caret_home();
        controller.move_caret_left(); // Must not underflow.
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn move_caret_right_moves_one_char() {
        let controller = TextEditingController::with_text("abc");
        controller.move_caret_home();
        controller.move_caret_right();
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn move_caret_right_at_end_is_noop() {
        let controller = TextEditingController::with_text("a");
        controller.move_caret_right(); // Already at end.
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn move_caret_home_resets_to_zero() {
        let controller = TextEditingController::with_text("hello");
        controller.move_caret_home();
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    #[test]
    fn move_caret_end_moves_to_last_byte() {
        let controller = TextEditingController::new();
        controller.insert_str("hello");
        controller.move_caret_home();
        controller.move_caret_end();
        assert_eq!(controller.caret_byte_offset(), 5);
    }

    // ------------------------------------------------------------------
    // Multi-byte (UTF-8) correctness
    // ------------------------------------------------------------------

    #[test]
    fn backspace_removes_full_multibyte_char() {
        // '€' is 3 bytes in UTF-8.
        let controller = TextEditingController::with_text("a€b");
        // Caret at end (5 bytes: 'a'=1 + '€'=3 + 'b'=1)
        controller.backspace(); // Should remove 'b' (1 byte).
        assert_eq!(controller.text(), "a€");
        assert_eq!(controller.caret_byte_offset(), 4);
        controller.backspace(); // Should remove '€' (3 bytes).
        assert_eq!(controller.text(), "a");
        assert_eq!(controller.caret_byte_offset(), 1);
    }

    #[test]
    fn delete_forward_removes_full_multibyte_char() {
        let controller = TextEditingController::with_text("€b");
        controller.move_caret_home();
        controller.delete_forward(); // Should remove '€' (3 bytes).
        assert_eq!(controller.text(), "b");
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    // ------------------------------------------------------------------
    // Change notification
    // ------------------------------------------------------------------

    #[test]
    fn listeners_fire_on_insert() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        controller.insert_str("a");
        assert_eq!(
            call_count.load(Ordering::Relaxed),
            1,
            "listener must fire on insert"
        );
    }

    #[test]
    fn listeners_do_not_fire_when_backspace_at_start() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        controller.backspace(); // No-op — must not notify.
        assert_eq!(call_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn remove_listener_stops_notifications() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let controller = TextEditingController::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        let id = controller.add_listener(Arc::new(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));
        controller.remove_listener(id);
        controller.insert_str("x");
        assert_eq!(call_count.load(Ordering::Relaxed), 0);
    }
}
