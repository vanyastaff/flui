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
//! [`CANDIDATES`] of them: a least-recently-drawn table. A new key displaces
//! the least recently drawn candidate when the table is full and that one
//! was last drawn on an earlier frame; when it was drawn on this one, the
//! new key is not tracked. So a frame that draws more keys than the table
//! holds tracks the first ones it meets and admits them on their next draw,
//! instead of pushing each out before it comes round again; keys drawn on
//! earlier frames, dead ones among them, give way to the ones drawn now; and
//! a frame's end takes out up to [`AGED_PER_FRAME`] candidates not drawn on
//! it, so the table empties once nothing new is drawn. Every step costs a
//! bounded number of entries, whatever the table holds. One limit stays: a
//! frame that draws more than [`CANDIDATES`] new keys before a repeated one
//! fills the table with keys of its own, and that one is not tracked.

use std::hash::Hash;

use crate::recency::{Place, RecencyList};
use crate::tallied_map::TalliedMap;

/// The frames a chunk is drawn on under one key before its recording is
/// captured: it records on each, and the last one captures.
pub(crate) const DRAWS_TO_ADMIT: u8 = 2;

/// The candidates the table holds, at most.
pub(crate) const CANDIDATES: usize = 4096;

/// The candidates not drawn on a frame that its end takes out, at most.
const AGED_PER_FRAME: usize = 2;

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
            self.release_when_empty();
            return Admission::Capture;
        }
        if self.recency.len() >= CANDIDATES {
            self.displace_drawn_before(frame, 1);
        }
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

    /// Ends `frame`: takes out up to [`AGED_PER_FRAME`] of the least
    /// recently drawn candidates that it did not draw.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        self.displace_drawn_before(frame, AGED_PER_FRAME);
        self.release_when_empty();
    }

    /// Takes out up to `count` of the least recently drawn candidates, each
    /// only when it was last drawn before `frame`.
    fn displace_drawn_before(&mut self, frame: u64, count: usize) {
        for _ in 0..count {
            let Some(oldest) = self.recency.oldest() else {
                return;
            };
            let candidate = self.recency.get(oldest);
            if candidate.last_drawn >= frame {
                return;
            }
            let key = candidate.key;
            self.places.remove(&key);
            self.recency.remove(oldest);
        }
    }

    /// Lets the room of the list and the map go once no candidate is left.
    fn release_when_empty(&mut self) {
        if self.recency.len() == 0 {
            self.release();
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

    /// The room of the recency list and of the map, in nodes and entries.
    #[cfg(test)]
    pub(crate) fn room(&self) -> [usize; 2] {
        [self.recency.room(), self.places.room()]
    }

    /// The entries every step touched since this was last asked.
    #[cfg(test)]
    pub(crate) fn take_visits(&mut self) -> u64 {
        self.places.take_visits() + self.recency.take_visits()
    }
}

#[cfg(test)]
mod tests {
    use super::{Admission, CANDIDATES, Candidates, DRAWS_TO_ADMIT};

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
    /// tracked are admitted on their next draw; a key drawn on a later frame
    /// displaces the least recently drawn; and frames that draw nothing new
    /// empty the table, two candidates each, and let its room go.
    #[test]
    fn the_candidate_table_stays_bounded() {
        let mut candidates = Candidates::<u32>::new();
        let many = 3 * u32::try_from(CANDIDATES).expect("the bound fits a u32");
        let bound = u32::try_from(CANDIDATES).expect("the bound fits a u32");
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
        // Keys `bound..2 * bound` are the candidates now, drawn on frame 2.
        assert_eq!(candidates.len(), CANDIDATES);
        let _ = candidates.take_visits();
        for key in 0..8 {
            let _ = candidates.draw(many + key, 3);
            assert!(candidates.take_visits() < 16, "a draw is a few steps");
        }
        assert_eq!(candidates.len(), CANDIDATES, "each new key displaced one");
        assert_eq!(
            candidates.draw(bound + 8, 3),
            Admission::Capture,
            "the one after them is still tracked"
        );
        assert_eq!(
            candidates.draw(bound, 3),
            Admission::RecordOnly,
            "the least recently drawn was displaced"
        );
        let _ = candidates.take_visits();
        let mut frame = 4;
        while candidates.len() > 0 {
            let before = candidates.len();
            candidates.end_frame(frame);
            assert!(before - candidates.len() <= 2, "a frame ages two");
            // The last one lets the room go, which counts every node it held.
            let visits = candidates.take_visits();
            assert!(
                visits < 16 || candidates.len() == 0,
                "a frame's end is a few steps: {visits}"
            );
            frame += 1;
        }
        assert_eq!(candidates.room(), [0, 0], "an empty table lets its room go");
    }

    /// Keys that change every frame do not keep a stable key out: drawn
    /// after a frame's churn, on a table full of the churn of earlier
    /// frames, each stable key displaces one of those and is admitted on
    /// the next frame, while the churn goes on.
    #[test]
    fn stable_keys_are_admitted_under_churn() {
        let mut candidates = Candidates::<u32>::new();
        let churn = u32::try_from(CANDIDATES / 2).expect("the bound fits a u32");
        let stable = 1_000_u32;
        // Fresh keys on every frame, above every stable key.
        let fresh = |frame: u32, at: u32| 1_000_000 + frame * churn + at;
        for frame in 0..3_u32 {
            for at in 0..churn {
                let _ = candidates.draw(fresh(frame, at), u64::from(frame));
            }
        }
        assert_eq!(candidates.len(), CANDIDATES, "the churn fills the table");
        for frame in 3..5_u32 {
            for at in 0..churn {
                let _ = candidates.draw(fresh(frame, at), u64::from(frame));
            }
            let admitted = (0..stable)
                .filter(|&key| candidates.draw(key, u64::from(frame)) == Admission::Capture)
                .count();
            let expected = if frame == 3 { 0 } else { stable as usize };
            assert_eq!(admitted, expected, "frame {frame}");
        }
    }
}
