//! [`EditGeneration`]: the count of application edits a platform session is
//! checked against (ADR-0142 item 3).

/// How many application edits a store's document has taken. A platform
/// session records it when its lock opens and writes its edits back only
/// while it still admits that record: an application edit made since drops
/// the session, and the application edit wins (ADR-0142 item 3).
///
/// The count never wraps. Exhausting it is permanent, and an exhausted
/// generation admits no session, so a count that came back round can never
/// make a session that opened before an application edit look current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditGeneration(Option<u64>);

impl EditGeneration {
    /// A document no application edit has touched.
    pub const FIRST: Self = Self(Some(0));

    /// The last count before exhaustion, where a test starts a store that
    /// makes no 2^64 edits.
    #[cfg(test)]
    pub(super) const LAST: Self = Self(Some(u64::MAX));

    /// Count one more application edit; past the last count, exhausted for
    /// good.
    pub fn advance(&mut self) {
        self.0 = self.0.and_then(|count| count.checked_add(1));
    }

    /// Whether a session that opened at `opened` may write back now: no
    /// application edit came since, and the count is not exhausted.
    #[must_use]
    pub fn admits(self, opened: Self) -> bool {
        self.0.is_some() && self == opened
    }
}

impl Default for EditGeneration {
    fn default() -> Self {
        Self::FIRST
    }
}

#[cfg(test)]
mod tests {
    use super::EditGeneration;

    /// A session that opened before the count ran out stays stale, and an
    /// exhausted count admits no session again.
    #[test]
    fn an_exhausted_generation_admits_no_session() {
        let mut now = EditGeneration::FIRST;
        let opened = now;
        assert!(now.admits(opened), "no edit since: the session is current");
        now = EditGeneration::LAST;
        let last = now;
        assert!(now.admits(last), "the last count still admits its session");
        now.advance();
        assert!(
            !now.admits(last),
            "a session that opened before the last edit stays stale"
        );
        let reopened = now;
        assert!(
            !now.admits(reopened),
            "an exhausted count admits no later session"
        );
        now.advance();
        assert!(
            !now.admits(opened) && !now.admits(reopened),
            "exhaustion is permanent"
        );
    }
}
