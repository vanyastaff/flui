//! The loop's record of what each UI runtime runs (ADR-0393 §3).
//!
//! The record decides; it calls nothing. `WorkerReload` feeds it what the hook
//! polled and asks it, at each UI runtime's frame boundary, what that UI runtime
//! owes, then does the reassemble and reports the outcome with the hook lent.

use flui_foundation::UiRuntimeId;
use flui_view::dev_reload::{CodeVersion, LayoutEpoch, ReloadEvent, Revision};
use rustc_hash::FxHashMap;

/// One event loop's versions: what the application was composed from, the
/// newest version loaded, and the revision each UI runtime runs.
#[derive(Debug, Default)]
pub(super) struct Record {
    composed: Composed,
    loaded: Option<CodeVersion>,
    runtimes: FxHashMap<UiRuntimeId, Runs>,
}

/// The layout the running application was composed from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Composed {
    /// Composed before the loop saw any version: the first version reported
    /// is taken as the one it was composed from.
    #[default]
    Unversioned,
    /// Composed from a build of this layout.
    Layout(LayoutEpoch),
    /// A restart composed nothing: no version reassembles until a later one
    /// composes an application.
    Nothing,
}

#[derive(Debug, Clone, Copy, Default)]
struct Runs {
    /// The revision the UI runtime's tree was built or reassembled from;
    /// `None` before the loop saw one.
    revision: Option<Revision>,
    /// The code already loaded asked for a rebuild this UI runtime has not
    /// done yet.
    rebuild_owed: bool,
}

/// What a UI runtime owes at its frame boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Due {
    Nothing,
    /// Reassemble, and report it as reassembled to `Some` version; a rebuild
    /// the loaded code asked for has `None` and is not reported.
    Reassemble(Option<CodeVersion>),
}

impl Record {
    /// What one poll reported: the version loaded, and whether the loaded code
    /// asked every UI runtime to rebuild. `Some` is the version a restart is
    /// due for, because its layout is not the one the application was
    /// composed from.
    pub(super) fn report(&mut self, event: ReloadEvent, rebuild: bool) -> Option<CodeVersion> {
        if rebuild {
            for runs in self.runtimes.values_mut() {
                runs.rebuild_owed = true;
            }
        }
        let ReloadEvent::Loaded(version) = event else {
            return None;
        };
        if self.loaded == Some(version) {
            return None;
        }
        self.loaded = Some(version);
        match self.composed {
            Composed::Unversioned => {
                self.composed = Composed::Layout(version.layout);
                None
            }
            Composed::Layout(layout) if layout == version.layout => None,
            Composed::Layout(_) | Composed::Nothing => Some(version),
        }
    }

    /// The restart for the version [`report`](Self::report) returned composed
    /// no application.
    pub(super) fn restart_failed(&mut self) {
        self.composed = Composed::Nothing;
    }

    /// A UI runtime whose tree was just built, from the code loaded now. A UI
    /// runtime already recorded keeps its record.
    pub(super) fn mounted(&mut self, runtime: UiRuntimeId) {
        let current = self.current();
        self.runtimes.entry(runtime).or_insert(Runs {
            revision: current,
            rebuild_owed: false,
        });
    }

    /// What `runtime` owes at this frame boundary, recorded as done: a version
    /// of the composed layout it does not run yet, else a rebuild the loaded
    /// code asked for.
    pub(super) fn due(&mut self, runtime: UiRuntimeId) -> Due {
        self.mounted(runtime);
        let reassembling = self.reassembling();
        let runs = self
            .runtimes
            .get_mut(&runtime)
            .expect("BUG: `mounted` just recorded the UI runtime");
        if let Some(version) = reassembling
            && runs.revision != Some(version.revision)
        {
            runs.revision = Some(version.revision);
            runs.rebuild_owed = false;
            return Due::Reassemble(Some(version));
        }
        if std::mem::take(&mut runs.rebuild_owed) {
            Due::Reassemble(None)
        } else {
            Due::Nothing
        }
    }

    /// `runtime` retired.
    pub(super) fn retired(&mut self, runtime: UiRuntimeId) {
        self.runtimes.remove(&runtime);
    }

    /// Whether `runtime` is in the record.
    #[cfg(test)]
    pub(super) fn holds(&self, runtime: UiRuntimeId) -> bool {
        self.runtimes.contains_key(&runtime)
    }

    /// The loop ended: the next loop composes its application anew.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The loaded version a UI runtime reassembles to: one of the layout the
    /// application was composed from.
    fn reassembling(&self) -> Option<CodeVersion> {
        let version = self.loaded?;
        (self.composed == Composed::Layout(version.layout)).then_some(version)
    }

    /// The revision a tree built now is built from.
    fn current(&self) -> Option<Revision> {
        self.reassembling().map(|version| version.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(layout: u64, revision: u64) -> CodeVersion {
        CodeVersion::new(
            LayoutEpoch::try_new(layout).expect("a test layout is not zero"),
            Revision::try_new(revision).expect("a test revision is not zero"),
        )
    }

    fn runtime(index: usize) -> UiRuntimeId {
        UiRuntimeId::new(index)
    }

    #[test]
    fn the_record_follows_each_order_of_versions() {
        let (a, b) = (runtime(1), runtime(2));
        let mut record = Record::default();
        record.mounted(a);
        assert_eq!(record.due(a), Due::Nothing, "nothing loaded yet");

        assert_eq!(
            record.report(ReloadEvent::Loaded(version(1, 1)), false),
            None,
            "the first version is the one the application was composed from"
        );
        assert_eq!(record.due(a), Due::Reassemble(Some(version(1, 1))));
        assert_eq!(record.due(a), Due::Nothing, "once");

        record.mounted(b);
        assert_eq!(
            record.due(b),
            Due::Nothing,
            "built from the code loaded now"
        );

        assert_eq!(
            record.report(ReloadEvent::Loaded(version(1, 2)), false),
            None
        );
        assert_eq!(
            record.report(ReloadEvent::Loaded(version(1, 3)), false),
            None
        );
        assert_eq!(
            record.due(b),
            Due::Reassemble(Some(version(1, 3))),
            "two versions before a boundary apply once, the latest"
        );
        assert_eq!(
            record.report(ReloadEvent::Loaded(version(1, 3)), false),
            None
        );
        assert_eq!(record.due(b), Due::Nothing, "a version reported twice");
        assert_eq!(record.due(a), Due::Reassemble(Some(version(1, 3))));

        assert_eq!(record.report(ReloadEvent::Unchanged, true), None);
        assert_eq!(
            record.due(a),
            Due::Reassemble(None),
            "the loaded code's rebuild"
        );
        assert_eq!(record.due(a), Due::Nothing);
        assert_eq!(record.due(b), Due::Reassemble(None), "each UI runtime once");

        assert_eq!(
            record.report(ReloadEvent::Loaded(version(2, 4)), false),
            Some(version(2, 4)),
            "another layout needs a restart"
        );
        assert_eq!(record.due(a), Due::Nothing, "no reassemble across layouts");
        record.restart_failed();
        assert_eq!(
            record.report(ReloadEvent::Loaded(version(1, 5)), false),
            Some(version(1, 5)),
            "after a failed restart every version restarts"
        );
        assert_eq!(record.due(b), Due::Nothing);

        record.retired(b);
        assert!(!record.holds(b), "a retired UI runtime leaves the record");
        record.clear();
        assert_eq!(
            record.report(ReloadEvent::Loaded(version(2, 6)), false),
            None
        );
    }
}
