//! Which recordings the cache captures: a chunk drawn under one key on
//! [`DRAWS_TO_ADMIT`] frames, as Flutter's raster cache admits a picture
//! only after it has been seen on `access_threshold` frames (ADR-0304 §9).
//! Capturing a chunk's IR copies every arena it appended, so a chunk drawn
//! once, a first frame's or content that changes every frame, is recorded
//! and never captured.
//!
//! A key the cache does not hold is a candidate from its first draw until
//! it is admitted or displaced. The candidates sit in a [`RecencyList`] in
//! the order they were last drawn, found by a [`TalliedMap`], at most
//! [`CANDIDATES`] of them. A new candidate displaces at most
//! [`DISPLACED_PER_DRAW`] of the oldest, each only once it has gone
//! [`STALE_AFTER`] frames undrawn; while none is that old, a new key is not
//! tracked. So a frame that draws more keys than the table holds tracks
//! the first ones it meets and admits them on their next draw, instead of
//! pushing each out before it comes round again, and every draw costs a
//! bounded number of steps, whatever the table holds.

use std::hash::Hash;

use crate::recency::{Place, RecencyList};
use crate::tallied_map::TalliedMap;

/// The frames a chunk is drawn on under one key before its recording is
/// captured: it records on each, and the last one captures.
pub(crate) const DRAWS_TO_ADMIT: u8 = 2;

/// The candidates the table holds, at most.
pub(crate) const CANDIDATES: usize = 4096;

/// The frames a candidate may go undrawn and still not be displaced: the
/// targets that share a painter draw their frames between each other's, so
/// a key need not come back on the very next frame.
const STALE_AFTER: u64 = 4;

/// The oldest candidates a new one displaces at most.
const DISPLACED_PER_DRAW: usize = 2;

/// What a draw under a key the cache does not hold says about capturing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    /// Record it and capture the recording.
    Capture,
    /// Record it only: it has not been drawn on enough frames yet.
    RecordOnly,
}

#[derive(Debug)]
struct Candidate<K> {
    key: K,
    /// The frames it was drawn on, below [`DRAWS_TO_ADMIT`].
    frames: u8,
    last_drawn: u64,
}

/// Keys drawn on fewer than [`DRAWS_TO_ADMIT`] frames, the least recently
/// drawn first.
#[derive(Debug)]
pub(crate) struct Candidates<K> {
    places: TalliedMap<K, Place>,
    recency: RecencyList<Candidate<K>>,
}

impl<K: Hash + Eq + Copy> Candidates<K> {
    pub(crate) fn new() -> Self {
        Self {
            places: TalliedMap::new(),
            recency: RecencyList::new(),
        }
    }

    /// Notes a draw under `key`, which the cache does not hold, on `frame`,
    /// and answers whether to capture its recording. A key drawn twice on
    /// one frame counts that frame once. A key admitted leaves the table: a
    /// capture that is then refused (a clip of its own, a glyph not placed)
    /// starts it over.
    pub(crate) fn draw(&mut self, key: K, frame: u64) -> Admission {
        if let Some(&place) = self.places.get(&key) {
            let candidate = self.recency.touch(place);
            if candidate.last_drawn != frame {
                candidate.last_drawn = frame;
                candidate.frames = candidate.frames.saturating_add(1);
            }
            if candidate.frames < DRAWS_TO_ADMIT {
                return Admission::RecordOnly;
            }
            self.places.remove(&key);
            self.recency.remove(place);
            if self.recency.len() == 0 {
                self.release();
            }
            return Admission::Capture;
        }
        self.displace_stale(frame);
        if self.recency.len() < CANDIDATES {
            let place = self.recency.push_newest(Candidate {
                key,
                frames: 1,
                last_drawn: frame,
            });
            // The key was not tracked: no place is replaced.
            let _ = self.places.insert(key, place);
        }
        Admission::RecordOnly
    }

    /// Takes out up to [`DISPLACED_PER_DRAW`] of the oldest candidates that
    /// went more than [`STALE_AFTER`] frames undrawn.
    fn displace_stale(&mut self, frame: u64) {
        for _ in 0..DISPLACED_PER_DRAW {
            let Some(oldest) = self.recency.oldest() else {
                return;
            };
            let candidate = self.recency.get(oldest);
            if candidate.last_drawn.saturating_add(STALE_AFTER) >= frame {
                return;
            }
            let key = candidate.key;
            self.places.remove(&key);
            self.recency.remove(oldest);
        }
    }

    /// Drops every candidate, with the room that held them: the keys they
    /// were drawn under name a resource epoch that has passed.
    pub(crate) fn release(&mut self) {
        self.places.release();
        self.recency.release();
    }

    /// The candidates the table holds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.recency.len()
    }

    /// The entries every step touched since this was last asked.
    #[cfg(test)]
    pub(crate) fn take_visits(&mut self) -> u64 {
        self.places.take_visits() + self.recency.take_visits()
    }
}

#[cfg(test)]
mod tests {
    use super::{Admission, CANDIDATES, Candidates, DRAWS_TO_ADMIT, STALE_AFTER};

    /// A key drawn on fewer than [`DRAWS_TO_ADMIT`] frames is recorded only,
    /// however often each frame draws it; drawn on the admitting frame, it is
    /// captured and leaves the table.
    #[test]
    fn a_key_is_captured_on_the_frame_that_admits_it() {
        let mut candidates = Candidates::<u32>::new();
        for frame in 1..u64::from(DRAWS_TO_ADMIT) {
            for _ in 0..3 {
                assert_eq!(candidates.draw(7, frame), Admission::RecordOnly);
            }
        }
        assert_eq!(
            candidates.draw(7, u64::from(DRAWS_TO_ADMIT)),
            Admission::Capture
        );
        assert_eq!(candidates.len(), 0, "an admitted key leaves the table");
    }

    /// However many keys a frame draws, the table holds at most
    /// [`CANDIDATES`] and each draw touches a few entries; the keys it
    /// tracked are admitted on their next draw; and once the rest go stale,
    /// new keys displace them.
    #[test]
    fn the_candidate_table_stays_bounded() {
        let mut candidates = Candidates::<u32>::new();
        let many = 3 * u32::try_from(CANDIDATES).expect("the bound fits a u32");
        for key in 0..many {
            let _ = candidates.draw(key, 1);
            assert!(candidates.len() <= CANDIDATES, "{}", candidates.len());
            assert!(candidates.take_visits() < 16, "a draw is a few steps");
        }
        assert_eq!(candidates.len(), CANDIDATES);
        let admitted = (0..many)
            .filter(|&key| candidates.draw(key, 2) == Admission::Capture)
            .count();
        assert_eq!(
            admitted, CANDIDATES,
            "the keys tracked on the first frame are admitted on the second"
        );
        // The keys untracked on the first frame are the candidates now.
        let held = candidates.len();
        assert_eq!(held, CANDIDATES);
        let stale = 2 + STALE_AFTER + 1;
        let _ = candidates.take_visits();
        for key in 0..8 {
            let _ = candidates.draw(many + key, stale);
            assert!(candidates.take_visits() < 16, "a draw is a few steps");
        }
        assert_eq!(
            candidates.len(),
            held - 8,
            "each new key displaced two stale ones"
        );
    }
}
