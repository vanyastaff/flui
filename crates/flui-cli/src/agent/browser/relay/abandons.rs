//! The abandons the relay owes the page ([`super::super::ABANDON_FUNCTION`]): the IDs of connect
//! calls the page did not answer in time. An unanswered call is not one that opened nothing: the
//! page may run it late and open a connection nobody holds. The abandon is owed to the page, not
//! to the client, which is closed at once.
//!
//! Each ID keeps its own count of tries and is given up after [`ABANDON_TRIES`]; at most
//! [`MAX_OWED`] are owed at once, the oldest given up for a newer one. A newly owed ID is tried
//! at once, with every ID owed, so a try naming it is on the link before the next delivery of
//! the tab's events; an unanswered try is made again after a pause that doubles with the tries of
//! the newest ID.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How many times the relay tries to abandon one ID before it gives the ID up.
pub(in super::super) const ABANDON_TRIES: u32 = 5;

/// The most IDs owed at once: a newer one gives up the oldest.
pub(in super::super) const MAX_OWED: usize = 64;

/// The abandons owed, each ID with the tries made for it.
pub(super) struct Abandons {
    owed: BTreeMap<u64, u32>,
    /// The pause after the first unanswered try of an ID.
    backoff: Duration,
    /// When the next try is due.
    next_try: Instant,
}

impl Abandons {
    pub(super) fn new(backoff: Duration) -> Self {
        Self {
            owed: BTreeMap::new(),
            backoff,
            next_try: Instant::now(),
        }
    }

    /// Owes the abandon of `id`, tried at once with every ID owed. IDs are minted in order, so
    /// the oldest owed is the first.
    pub(super) fn owe(&mut self, id: u64, now: Instant) {
        if self.owed.len() >= MAX_OWED && self.owed.pop_first().is_some() {
            crate::ui::debug("the relay owes the page too many abandons; gave up the oldest");
        }
        self.owed.insert(id, 0);
        self.next_try = now;
    }

    /// The argument of the try to make now, if one is due.
    pub(super) fn due(&self, now: Instant) -> Option<String> {
        (!self.owed.is_empty() && now >= self.next_try).then(|| self.argument())
    }

    /// The argument of a try of every ID owed, if any is: a JSON array of the IDs as strings.
    pub(super) fn argument(&self) -> String {
        serde_json::Value::from(
            self.owed
                .keys()
                .map(u64::to_string)
                .collect::<Vec<String>>(),
        )
        .to_string()
    }

    /// Whether any abandon is owed.
    pub(super) fn is_empty(&self) -> bool {
        self.owed.is_empty()
    }

    /// The page did not answer the try that ended at `now`: each ID counts the try, those at
    /// [`ABANDON_TRIES`] are given up, and the rest are tried again after a pause twice the
    /// last of the newest ID's.
    pub(super) fn unanswered(&mut self, now: Instant) {
        self.owed.retain(|_, tries| {
            *tries = tries.saturating_add(1);
            *tries < ABANDON_TRIES
        });
        let Some(fewest) = self.owed.values().copied().min() else {
            crate::ui::debug("the page answered none of the relay's abandons; given up");
            return;
        };
        let doublings = fewest.saturating_sub(1).min(16);
        self.next_try = now + self.backoff.saturating_mul(1_u32 << doublings);
    }

    /// Nothing is owed any more: the page answered, or the document went.
    pub(super) fn clear(&mut self) {
        self.owed.clear();
    }

    /// When the next try is due, while anything is owed.
    #[cfg(test)]
    fn next_try(&self) -> Option<Instant> {
        (!self.owed.is_empty()).then_some(self.next_try)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{ABANDON_TRIES, Abandons, MAX_OWED};

    const BACKOFF: Duration = Duration::from_millis(100);

    /// The pause after each unanswered try is twice the last, until the ID is given up. Fails if
    /// the pause stays the same, which keeps a page that does not answer busy at a fixed rate.
    #[test]
    fn the_pause_doubles_with_each_unanswered_try() {
        let mut abandons = Abandons::new(BACKOFF);
        let now = Instant::now();
        abandons.owe(7, now);
        assert_eq!(
            abandons.due(now).as_deref(),
            Some(r#"["7"]"#),
            "tried at once"
        );
        let mut pauses = Vec::new();
        for _ in 1..ABANDON_TRIES {
            abandons.unanswered(now);
            let next = abandons.next_try().expect("still owed");
            pauses.push(next - now);
        }
        assert_eq!(
            pauses,
            [BACKOFF, BACKOFF * 2, BACKOFF * 4, BACKOFF * 8],
            "doubling"
        );
        abandons.unanswered(now);
        assert!(abandons.is_empty(), "given up after its last try");
    }

    /// An ID owed while another is being tried is tried at once, with its own count: the older
    /// one is given up after its own tries. Fails if the count is shared, so that a newly owed
    /// ID restarts the older one's tries.
    #[test]
    fn each_id_keeps_its_own_count_of_tries() {
        let mut abandons = Abandons::new(BACKOFF);
        let now = Instant::now();
        abandons.owe(1, now);
        abandons.unanswered(now);
        abandons.unanswered(now);
        abandons.owe(2, now);
        assert_eq!(
            abandons.due(now).as_deref(),
            Some(r#"["1","2"]"#),
            "tried at once"
        );
        for _ in 2..ABANDON_TRIES {
            abandons.unanswered(now);
        }
        assert_eq!(abandons.argument(), r#"["2"]"#, "the older one is given up");
        assert_eq!(
            abandons.next_try().map(|next| next - now),
            Some(BACKOFF * 4),
            "the pause follows the newest ID's tries"
        );
    }

    /// The IDs owed are bounded: a newer one gives up the oldest. Fails if the set grows with
    /// every connect call a hung page does not answer.
    #[test]
    fn the_ids_owed_are_bounded() {
        let mut abandons = Abandons::new(BACKOFF);
        let now = Instant::now();
        let past = u64::try_from(MAX_OWED).expect("BUG: a small bound");
        for id in 0..=past {
            abandons.owe(id, now);
        }
        let owed = serde_json::from_str::<Vec<String>>(&abandons.argument()).expect("JSON");
        assert_eq!(owed.len(), MAX_OWED);
        assert_eq!(
            owed.first().map(String::as_str),
            Some("1"),
            "the oldest went"
        );
    }
}
