//! A stable identifier for an entry of `docs/gaps.md`, and the assertion a
//! host's scenario uses to fail as a known gap (ADR-0243).
//!
//! A gap entry renumbers as earlier entries close, so a test that names a gap
//! by its number drifts and fails for the wrong reason. [`GapId`] names the
//! entry by its heading instead; the number is a derived fact shown in
//! messages and proved against the document by
//! [`gap_slugs_biject_with_the_document`].

/// An entry of `docs/gaps.md`: what FLUI has not built on a host yet, which a
/// scenario fails as there.
///
/// The identifier is the entry's heading as a slug, stable across the
/// renumbering `docs/gaps.md` undergoes. No code constructs a [`GapId`] from a
/// bare number; [`GapId::number`] is only for messages, and its agreement with
/// the document is enforced by [`gap_slugs_biject_with_the_document`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GapId {
    /// Gap 26: secure text entry and concealed clipboard entries are partial.
    SecureTextEntryAndConcealedClipboard,
}

impl GapId {
    /// The entry's number in `docs/gaps.md`, for messages (not an identity).
    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Self::SecureTextEntryAndConcealedClipboard => 26,
        }
    }

    /// The entry's heading in `docs/gaps.md`, as it reads after `### N. `.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Self::SecureTextEntryAndConcealedClipboard => {
                "Secure text entry and concealed clipboard entries are partial"
            }
        }
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
            eprintln!(
                "expected failure: gap {} is open on this host",
                gap.number()
            );
        }
        Err(failed) => panic!(
            "the scenario failed as gap {}, expected gap {}",
            failed.number(),
            gap.number()
        ),
        Ok(()) => panic!(
            "gap {} is closed on this host: remove its expect_fail and update docs/gaps.md",
            gap.number()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`GapId`] names exactly one `### N. Title` line of
    /// `docs/gaps.md`, at the position [`GapId::number`] claims: the slug and
    /// the number both agree with the document, and neither drifts alone.
    #[test]
    fn gap_slugs_biject_with_the_document() {
        let doc = include_str!("../../../../docs/gaps.md");
        let headings: Vec<&str> = doc
            .lines()
            .filter(|line| line.starts_with("### "))
            .collect();

        check(GapId::SecureTextEntryAndConcealedClipboard, &headings);
    }

    fn check(gap: GapId, headings: &[&str]) {
        let expected = format!("### {}. {}", gap.number(), gap.heading());
        assert_eq!(
            headings.get((gap.number() - 1) as usize).copied(),
            Some(expected.as_str()),
            "{} should sit at `{expected}` in docs/gaps.md",
            gap.heading()
        );
    }
}
