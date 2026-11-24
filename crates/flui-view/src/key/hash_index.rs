//! Positions in a list of children, indexed by the hash of their view key.
//!
//! A reconcile looks a new child's key up among the old children, and two
//! distinct keys may share a `u64` hash, so a hash names a bucket of
//! candidates that `ViewKey::key_eq` decides between. The buckets are chains
//! threaded through one array indexed by position, so an index over `n`
//! positions makes two allocations (the table and the chain array) however
//! many keys it holds, where a `Vec` per bucket made one per key.
//!
//! A bucket keeps its positions in the order they were inserted: of two
//! children with equal keys, the first claims first.

/// The first and last position of one hash's chain.
#[derive(Clone, Copy, Debug)]
struct Chain {
    first: usize,
    last: usize,
}

/// Where a position stands in the chains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    /// Not in the index.
    Out,
    /// The last position of its hash's chain.
    Last,
    /// Followed in its hash's chain by this position.
    Next(usize),
}

impl Link {
    fn next(self) -> Option<usize> {
        match self {
            Self::Next(position) => Some(position),
            Self::Out | Self::Last => None,
        }
    }
}

/// Positions `0..len` bucketed by key hash, each bucket in insertion order.
#[derive(Debug)]
#[expect(
    clippy::disallowed_types,
    reason = "indexed by view keys, whose hashes an application derives from its own data, so the table keeps SipHash's random seed against chosen collisions"
)]
pub(crate) struct KeyHashIndex {
    /// Each hash's chain.
    chains: std::collections::HashMap<u64, Chain>,
    /// `links[position]`: where `position` stands in its hash's chain. Empty
    /// until the first insert, so a list without keys allocates nothing.
    links: Vec<Link>,
    /// The number of positions the index covers.
    len: usize,
}

impl KeyHashIndex {
    /// An empty index over positions `0..len`.
    #[expect(
        clippy::disallowed_types,
        reason = "the table `KeyHashIndex` documents: view-key hashes keep SipHash's seed"
    )]
    pub(crate) fn over(len: usize) -> Self {
        Self {
            chains: std::collections::HashMap::new(),
            links: Vec::new(),
            len,
        }
    }

    /// Append `position` to the bucket of `hash`.
    ///
    /// # Panics
    ///
    /// When `position` is outside the index or already in it.
    pub(crate) fn insert(&mut self, hash: u64, position: usize) {
        if self.links.is_empty() {
            // A list that holds one key usually keys every child: room for
            // all of them up front, rather than a rehash at each doubling.
            self.links = vec![Link::Out; self.len];
            self.chains.reserve(self.len);
        }
        let link = self
            .links
            .get_mut(position)
            .expect("BUG: a key index position lies inside the list it indexes");
        assert_eq!(*link, Link::Out, "BUG: a position enters a key index once");
        *link = Link::Last;
        match self.chains.get_mut(&hash) {
            Some(chain) => {
                self.links[chain.last] = Link::Next(position);
                chain.last = position;
            }
            None => {
                self.chains.insert(
                    hash,
                    Chain {
                        first: position,
                        last: position,
                    },
                );
            }
        }
    }

    /// The positions in the bucket of `hash`, in insertion order.
    pub(crate) fn candidates(&self, hash: u64) -> impl Iterator<Item = usize> + '_ {
        let first = self.chains.get(&hash).map(|chain| chain.first);
        std::iter::successors(first, |&position| self.links[position].next())
    }

    /// Take the first position in the bucket of `hash` that `accepts`, and
    /// remove it from the index so no later lookup finds it.
    ///
    /// `accepts` may run user code (a key's `key_eq`); the index changes only
    /// after it returns `true`, so a panic in it leaves the index whole.
    pub(crate) fn claim(
        &mut self,
        hash: u64,
        mut accepts: impl FnMut(usize) -> bool,
    ) -> Option<usize> {
        let chain = *self.chains.get(&hash)?;
        let mut before = None;
        let mut current = Some(chain.first);
        while let Some(position) = current {
            let after = self.links[position];
            if accepts(position) {
                self.links[position] = Link::Out;
                match (before, after.next()) {
                    (None, None) => {
                        self.chains.remove(&hash);
                    }
                    (None, Some(next)) => {
                        self.chains.insert(
                            hash,
                            Chain {
                                first: next,
                                last: chain.last,
                            },
                        );
                    }
                    (Some(before), next) => {
                        self.links[before] = after;
                        if next.is_none() {
                            self.chains.insert(
                                hash,
                                Chain {
                                    first: chain.first,
                                    last: before,
                                },
                            );
                        }
                    }
                }
                return Some(position);
            }
            before = Some(position);
            current = after.next();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::KeyHashIndex;

    fn bucket(index: &KeyHashIndex, hash: u64) -> Vec<usize> {
        index.candidates(hash).collect::<Vec<usize>>()
    }

    /// A bucket answers in insertion order, a claim takes the first accepted
    /// position from the head, the middle or the tail, and a claimed position
    /// is never found again but may enter again; a bucket emptied by claims
    /// is gone.
    #[test]
    fn a_bucket_keeps_insertion_order_through_claims() {
        let mut index = KeyHashIndex::over(8);
        for position in [5, 1, 7, 3] {
            index.insert(9, position);
        }
        index.insert(4, 2);
        assert_eq!(bucket(&index, 9), [5, 1, 7, 3]);
        assert_eq!(bucket(&index, 4), [2]);
        assert_eq!(bucket(&index, 6), Vec::<usize>::new());

        assert_eq!(index.claim(9, |position| position == 7), Some(7));
        assert_eq!(bucket(&index, 9), [5, 1, 3]);
        assert_eq!(index.claim(9, |_| true), Some(5));
        assert_eq!(bucket(&index, 9), [1, 3]);
        assert_eq!(index.claim(9, |position| position == 3), Some(3));
        assert_eq!(bucket(&index, 9), [1]);
        index.insert(9, 3);
        assert_eq!(bucket(&index, 9), [1, 3], "the tail moved back to 1");
        assert_eq!(index.claim(9, |_| false), None);
        assert_eq!(index.claim(9, |_| true), Some(1));
        assert_eq!(index.claim(9, |_| true), Some(3));
        assert_eq!(bucket(&index, 9), Vec::<usize>::new());
        assert_eq!(index.claim(9, |_| true), None);
        assert_eq!(index.claim(4, |_| true), Some(2));
    }

    /// A position inserted twice would close its chain into a cycle: refused.
    #[test]
    #[should_panic(expected = "BUG: a position enters a key index once")]
    fn a_position_enters_once() {
        let mut index = KeyHashIndex::over(2);
        index.insert(1, 0);
        index.insert(2, 0);
    }
}
