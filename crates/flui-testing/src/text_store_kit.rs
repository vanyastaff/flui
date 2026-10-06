//! The text-store conformance kit (ADR-0090 §4).
//!
//! Any crate with a text field that implements
//! [`TextStore`] runs this kit against it. The
//! kit behaves like a TSF-style input method: it asks for locks, reads and
//! edits from the platform's side, and checks what it reads against what it
//! wrote. It covers UTF-16 offsets across surrogate pairs and grapheme
//! clusters, exact platform selection, composition, the lock rules
//! (synchronous refusals, asynchronous grants after a session or a frame
//! transaction, request order, panics), what reaches the observer, what
//! reaches the field's owner and when (only committed-text changes, after
//! the session's lock is released), and — when
//! the fixture has a layout — rect and point queries.
//!
//! A field supplies a [`TextStoreFixture`] and calls [`assert_conforms`]:
//!
//! ```
//! use flui_testing::text_store_kit::{self, InMemoryFixture, KIT_VERSION};
//!
//! text_store_kit::assert_conforms(&mut InMemoryFixture::new(), KIT_VERSION);
//! ```
//!
//! The kit is versioned. [`KIT_VERSION`] is the newest; each [`Case`] names
//! the version that added it, and [`run`] with an older version runs only
//! the cases a field certified against that version already knew, so a new
//! case never breaks a pinned downstream suite by surprise.
//!
//! [`InMemoryFixture`] runs the kit against
//! [`InMemoryTextStore`],
//! the reference store, and is a worked example of a fixture.

mod cases;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_platform_api::TextStore;
use flui_platform_api::text_store::InMemoryTextStore;

/// The newest kit version.
///
/// Version 2 adds the owner-notification contract (ADR-0090 §1 as amended):
/// the owner hears only of committed-text changes, after the session's lock
/// is released, a composition may start over a selection, and a grant that
/// panics leaves no composition without what it stands for.
pub const KIT_VERSION: u32 = 2;

/// A text field under test, as the kit drives it.
pub trait TextStoreFixture {
    /// The field's store. The kit calls this again after every
    /// [`Self::reset`], so a fixture may hand out a new store per reset.
    fn store(&mut self) -> Rc<dyn TextStore>;

    /// Put the field in a known state: `text`, the caret at its end, no
    /// composition, no observer, nothing queued, commits allowed, and laid
    /// out when the fixture has geometry.
    fn reset(&mut self, text: &str);

    /// Replace the whole text the way the application would, not the
    /// platform: the store reports it to its observer.
    fn app_replace_all(&mut self, text: &str);

    /// Reach the next commit anchor, where deferred grants run. A widget
    /// fixture drives a frame here.
    ///
    /// The kit holds frame transactions itself, through the gate it
    /// installs with [`TextStore::set_commit_gate`], so a fixture has no
    /// way to stand in for a store that ignores the gate.
    fn pump(&mut self);

    /// How many change notifications the field has sent its owner (a
    /// widget's `on_changed`, say). A platform session that changed the
    /// committed text — the text without its composition — is one; a
    /// session that only composed is none.
    fn owner_notifications(&self) -> usize;

    /// Run `hook` inside each later owner notification, after the field's
    /// own handling of it; `None` removes it.
    ///
    /// The kit's hook asks the store for a synchronous lock, which a store
    /// that notifies its owner from inside the session refuses. A fixture
    /// that keeps this default never runs the hook and fails the cases that
    /// need it (kit version 2 on).
    fn set_owner_hook(&mut self, hook: Option<Rc<dyn Fn()>>) {
        let _ = hook;
    }

    /// Which optional groups of cases apply.
    fn capabilities(&self) -> FixtureCapabilities;
}

/// Which optional groups of cases a fixture takes part in.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct FixtureCapabilities {
    /// The store answers rect and point queries from a layout.
    pub geometry: bool,
    /// The store is protected (a password field): text reads must be
    /// refused, and the other cases check the refusal instead of the text.
    pub protected: bool,
}

impl FixtureCapabilities {
    /// No optional group.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            geometry: false,
            protected: false,
        }
    }

    /// With or without the geometry cases.
    #[must_use]
    pub const fn with_geometry(self, geometry: bool) -> Self {
        Self { geometry, ..self }
    }

    /// With or without the protected-store cases.
    #[must_use]
    pub const fn with_protected(self, protected: bool) -> Self {
        Self { protected, ..self }
    }
}

/// One conformance check.
pub struct Case {
    /// The case's name, which a failure reports.
    pub name: &'static str,
    /// The kit version that added the case.
    pub since: u32,
    check: fn(&mut dyn TextStoreFixture) -> Result<(), String>,
}

impl std::fmt::Debug for Case {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Case")
            .field("name", &self.name)
            .field("since", &self.since)
            .finish_non_exhaustive()
    }
}

impl Case {
    /// Run this case against `fixture`. A panic inside it — the store's or
    /// the kit's — is a failure, not an unwind.
    ///
    /// # Errors
    ///
    /// The [`CaseFailure`] saying what did not hold.
    pub fn run(&self, fixture: &mut dyn TextStoreFixture) -> Result<(), CaseFailure> {
        let outcome = catch_unwind(AssertUnwindSafe(|| (self.check)(fixture)));
        let message = match outcome {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(message)) => message,
            Err(payload) => {
                let text = payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "a non-string panic payload".to_owned());
                // The kit borrows the fixture, but owns the caught opaque payload.
                // An aggregate's Drop may panic twice before any catch can regain control.
                flui_foundation::panic::retain_opaque_payload(payload);
                format!("panicked: {text}")
            }
        };
        Err(CaseFailure {
            case: self.name,
            message,
        })
    }
}

/// A case that did not hold, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseFailure {
    /// The failing case's name.
    pub case: &'static str,
    /// What did not hold.
    pub message: String,
}

impl std::fmt::Display for CaseFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.case, self.message)
    }
}

/// Every case of the newest kit version, in running order.
#[must_use]
pub fn cases() -> &'static [Case] {
    cases::CASES
}

/// Run every case added at or before `version` against `fixture`, and
/// return the failures.
pub fn run(fixture: &mut dyn TextStoreFixture, version: u32) -> Vec<CaseFailure> {
    cases()
        .iter()
        .filter(|case| case.since <= version)
        .filter_map(|case| case.run(fixture).err())
        .collect()
}

/// [`run`], panicking with every failure when any case fails.
///
/// # Panics
///
/// When a case fails.
#[track_caller]
pub fn assert_conforms(fixture: &mut dyn TextStoreFixture, version: u32) {
    let failures = run(fixture, version);
    assert!(
        failures.is_empty(),
        "the text store does not conform to kit v{version}:\n{}",
        failures
            .iter()
            .map(|failure| format!("  - {failure}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The kit's fixture over
/// [`InMemoryTextStore`].
#[derive(Debug)]
pub struct InMemoryFixture {
    store: Rc<InMemoryTextStore>,
    protected: bool,
}

impl Default for InMemoryFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryFixture {
    /// An unprotected store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: InMemoryTextStore::new(""),
            protected: false,
        }
    }

    /// A protected store, as a password field is.
    #[must_use]
    pub fn protected() -> Self {
        let fixture = Self {
            protected: true,
            ..Self::new()
        };
        fixture.store.set_protected(true);
        fixture
    }

    /// The concrete store behind [`TextStoreFixture::store`].
    #[must_use]
    pub fn in_memory(&self) -> &Rc<InMemoryTextStore> {
        &self.store
    }
}

impl TextStoreFixture for InMemoryFixture {
    fn store(&mut self) -> Rc<dyn TextStore> {
        let store: Rc<dyn TextStore> = self.store.clone(); // the kit's erased view of the concrete reference store.
        store
    }

    fn reset(&mut self, text: &str) {
        self.store = InMemoryTextStore::new(text);
        self.store.set_protected(self.protected);
    }

    fn app_replace_all(&mut self, text: &str) {
        let whole = flui_platform_api::text_store::Utf16Range::new(
            flui_platform_api::text_store::Utf16Offset::ZERO,
            flui_platform_api::text_store::utf16::utf16_len(&self.store.text()),
        )
        .expect("BUG: zero precedes every length");
        self.store.app_replace(whole, text);
    }

    fn pump(&mut self) {
        let _ = self.store.run_deferred_grants();
    }

    fn owner_notifications(&self) -> usize {
        self.store.owner_notifications()
    }

    fn set_owner_hook(&mut self, hook: Option<Rc<dyn Fn()>>) {
        self.store.set_owner_listener(hook);
    }

    fn capabilities(&self) -> FixtureCapabilities {
        FixtureCapabilities::new()
            .with_geometry(true)
            .with_protected(self.protected)
    }
}
