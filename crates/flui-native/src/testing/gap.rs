//! The entry of `docs/gaps.md` a host's scenario fails as, and the assertion
//! that it does (ADR-0243).
//!
//! An entry is named by its heading's anchor, never by its rank: the rank is
//! the order of the entries and changes whenever one closes or moves. The test
//! `every_gap_id_is_a_gaps_entry` holds each [`GapId`](crate::testing::gap::GapId)'s slug to a heading of
//! `docs/gaps.md`.

use std::fmt;

/// Declares [`GapId`], its [`GapId::ALL`] and its [`GapId::slug`] from one
/// list, so no variant can miss either.
macro_rules! gap_ids {
    ($($(#[doc = $doc:literal])* $variant:ident => $slug:literal,)+) => {
        /// An entry of `docs/gaps.md`: what FLUI has not built on a host yet,
        /// which a scenario fails as there.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum GapId {
            $($(#[doc = $doc])* $variant,)+
        }

        impl GapId {
            /// Every entry a scenario names.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The anchor of the entry's heading in `docs/gaps.md`, as GitHub
            /// renders it.
            #[must_use]
            pub const fn slug(self) -> &'static str {
                match self {
                    $(Self::$variant => $slug,)+
                }
            }
        }
    };
}

gap_ids! {
    /// "Secure text entry and concealed clipboard entries are partial": a
    /// concealed write is an ordinary one, kept by the clipboard's history.
    SecureTextEntryAndConcealedClipboard =>
        "secure-text-entry-and-concealed-clipboard-entries-are-partial",
    /// "Accessibility bridge fidelity (properties and announcements)": an
    /// adapter does not report the assistive cursor or tell a route change.
    AccessibilityBridgeFidelity =>
        "accessibility-bridge-fidelity-properties-and-announcements",
}

impl fmt::Display for GapId {
    /// The entry as a path into the repository: `docs/gaps.md#<slug>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "docs/gaps.md#{}", self.slug())
    }
}

/// Assert that a scenario's `outcome` is the known failure `gap`: the run
/// passes while the gap is open on this host, and fails once the scenario
/// passes, so the fix closes the gap's entry and this call with it.
///
/// # Panics
///
/// When `outcome` passes, or fails as another gap.
pub fn expect_fail(gap: GapId, outcome: Result<(), GapId>) {
    match outcome {
        Err(failed) if failed == gap => {
            eprintln!("expected failure: {gap} is open on this host");
        }
        Err(failed) => panic!("the scenario failed as {failed}, expected {gap}"),
        Ok(()) => {
            panic!("{gap} is closed on this host: remove its expect_fail and update the entry")
        }
    }
}
