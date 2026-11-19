//! Where a host reports a text store's refusal of an edit it asked for.
//!
//! The contract hands a refusal back, at once through the result or later to
//! a reporter the caller supplies (`commit_composition_in_place`); it does not
//! log. A host commits a composition in place on every platform the same way,
//! through [`commit_in_place`], so a refusal is reported once and with one
//! message whichever host met it.

use flui_platform::text_store::{TextStore, TextStoreError, commit_composition_in_place};

/// The target of the log line [`report`] writes, which a test counts.
pub(crate) const LOG_TARGET: &str = "flui_native::text_store";

/// Commit `store`'s composition in place, as a host does when its platform
/// could not end it, or when focus leaves the field: a refusal is reported,
/// whether the store answers it at once or when the queued commit runs.
pub(crate) fn commit_in_place(store: &dyn TextStore) {
    if let Err(error) = commit_composition_in_place(store, report) {
        report(error);
    }
}

/// Log that a text store refused the commit of its composition in place; the
/// composition stays as it was.
pub(crate) fn report(error: TextStoreError) {
    tracing::warn!(
        target: LOG_TARGET,
        %error,
        "a text store refused the commit in place the native host asked for"
    );
}

/// Counts the lines [`report`] writes on this thread while `body` runs.
#[cfg(test)]
pub(crate) fn reports_in(body: impl FnOnce()) -> usize {
    crate::testing::log::lines_in(LOG_TARGET, body)
}

/// A field that takes no edits, for a host model's test: it composes over
/// its whole text, refuses every edit, and while `queues` is set queues each
/// grant for [`TextStore::run_deferred_grants`].
#[cfg(test)]
pub(crate) mod read_only {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use flui_foundation::geometry::{Bounds, Point};
    use flui_platform::text_store::{
        CommitGate, Composition, LockGrant, LockOutcome, LockTiming, PointMode, RangeRect,
        Selection, TextChange, TextStore, TextStoreEdit, TextStoreError, TextStoreObserver,
        TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range,
    };

    /// See the module.
    #[derive(Default)]
    pub(crate) struct ReadOnlyField {
        pub(crate) queues: Cell<bool>,
        queued: RefCell<Vec<LockGrant>>,
    }

    impl TextStore for ReadOnlyField {
        fn status(&self) -> TextStoreStatus {
            TextStoreStatus::EDITABLE_SINGLE_LINE
        }

        fn press_target_at(
            &self,
            _: Point<f64>,
        ) -> Result<flui_platform::text_store::PressTarget, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }

        fn request_lock(
            &self,
            grant: LockGrant,
            _: LockTiming,
        ) -> Result<LockOutcome, TextStoreError> {
            if self.queues.get() {
                self.queued.borrow_mut().push(grant);
                return Ok(LockOutcome::Deferred);
            }
            run(grant);
            Ok(LockOutcome::Granted)
        }

        fn run_deferred_grants(&self) -> usize {
            let queued = std::mem::take(&mut *self.queued.borrow_mut());
            let ran = queued.len();
            queued.into_iter().for_each(run);
            ran
        }

        fn set_commit_gate(&self, _: CommitGate) {}

        fn set_observer(&self, _: Option<Rc<dyn TextStoreObserver>>) {}
    }

    fn run(grant: LockGrant) {
        match grant {
            LockGrant::Read(body) => body(&Session),
            LockGrant::ReadWrite(body) => body(&mut Session),
        }
    }

    /// The text "ab", composing.
    struct Session;

    const LEN: Utf16Offset = Utf16Offset::new(2);

    impl TextStoreRead for Session {
        fn document_len(&self) -> Utf16Offset {
            LEN
        }
        fn text(&self, _: Utf16Range) -> Result<String, TextStoreError> {
            Ok("ab".to_owned())
        }
        fn selection(&self) -> Selection {
            Selection::collapsed(LEN)
        }
        fn composition(&self) -> Option<Composition> {
            Some(Composition {
                range: Utf16Range::try_new(Utf16Offset::new(0), LEN).expect("ordered"),
                hides_caret: false,
            })
        }
        fn rect_for_range(&self, _: Utf16Range) -> Result<RangeRect, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
        fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
        fn index_at_point(
            &self,
            _: Point<f64>,
            _: PointMode,
        ) -> Result<Utf16Offset, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
        fn visible_bounds(&self) -> Result<Option<Bounds<f64>>, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
    }

    impl TextStoreEdit for Session {
        fn replace(&mut self, _: Utf16Range, _: &str) -> Result<TextChange, TextStoreError> {
            Err(TextStoreError::Protected)
        }
        fn insert_at_selection(&mut self, _: &str) -> Result<TextChange, TextStoreError> {
            Err(TextStoreError::Protected)
        }
        fn set_selection(&mut self, _: Selection) -> Result<(), TextStoreError> {
            Err(TextStoreError::Protected)
        }
        fn set_composition(&mut self, _: Option<Composition>) -> Result<(), TextStoreError> {
            Err(TextStoreError::Protected)
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::text_store::TextStore as _;

    use super::read_only::ReadOnlyField;
    use super::{commit_in_place, reports_in};

    /// A refusal is reported once: at once when the store answers the commit
    /// at once, and when the queued commit runs when it queued it.
    #[test]
    fn a_refused_commit_in_place_is_reported_once() {
        let field = ReadOnlyField::default();
        assert_eq!(reports_in(|| commit_in_place(&field)), 1, "at once");

        field.queues.set(true);
        assert_eq!(reports_in(|| commit_in_place(&field)), 0, "not yet");
        assert_eq!(
            reports_in(|| assert_eq!(field.run_deferred_grants(), 1)),
            1,
            "when the queued commit runs"
        );
    }
}
