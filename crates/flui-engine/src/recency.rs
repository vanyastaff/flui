//! Values in the order they were last used, each reached by the place it was
//! given: moving one to the newest end, taking the oldest and removing one
//! by its place are each O(1), so a cache that evicts least recently used
//! values first spends what it evicts and never walks what it keeps. In a
//! test build every step counts the nodes it touches, so a test can bound
//! the work of the code above it.

/// Where a value sits in a [`RecencyList`], from the push that placed it
/// until it is taken out; the place is then given to a later push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Place(usize);

#[derive(Debug)]
enum Node<T> {
    Held {
        value: T,
        /// The value used just before this one.
        older: Option<usize>,
        /// The value used just after it.
        newer: Option<usize>,
    },
    /// Free, with the next free node.
    Vacant(Option<usize>),
}

/// Values ordered from the least to the most recently used, in one
/// allocation whose free nodes later pushes reuse until
/// [`Self::release`] lets it go.
#[derive(Debug)]
pub(crate) struct RecencyList<T> {
    nodes: Vec<Node<T>>,
    vacant: Option<usize>,
    oldest: Option<usize>,
    newest: Option<usize>,
    len: usize,
    /// The nodes reached since [`Self::take_visits`] last answered.
    #[cfg(test)]
    visits: u64,
}

impl<T> RecencyList<T> {
    /// The bytes one node of the list holds, held or free.
    pub(crate) const NODE_BYTES: usize = std::mem::size_of::<Node<T>>();

    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            vacant: None,
            oldest: None,
            newest: None,
            len: 0,
            #[cfg(test)]
            visits: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[cfg_attr(
        not(test),
        expect(clippy::unused_self, reason = "only a test build counts the visit")
    )]
    fn visit(&mut self) {
        #[cfg(test)]
        {
            self.visits += 1;
        }
    }

    /// Places `value` as the most recently used.
    pub(crate) fn push_newest(&mut self, value: T) -> Place {
        let node = Node::Held {
            value,
            older: self.newest,
            newer: None,
        };
        self.visit();
        let index = if let Some(index) = self.vacant {
            let Node::Vacant(next) = std::mem::replace(&mut self.nodes[index], node) else {
                unreachable!("BUG: the vacant chain names only vacant nodes")
            };
            self.vacant = next;
            index
        } else {
            self.nodes.push(node);
            self.nodes.len() - 1
        };
        self.link_newest(index);
        self.len += 1;
        Place(index)
    }

    /// The value at `place`.
    pub(crate) fn get(&mut self, place: Place) -> &mut T {
        self.visit();
        match &mut self.nodes[place.0] {
            Node::Held { value, .. } => value,
            Node::Vacant(_) => unreachable!("BUG: a place names a held value until it is taken"),
        }
    }

    /// The value at `place`, made the most recently used.
    pub(crate) fn touch(&mut self, place: Place) -> &mut T {
        if self.newest != Some(place.0) {
            self.unlink(place.0);
            self.link_newest(place.0);
        }
        self.get(place)
    }

    /// Takes the value at `place` out.
    pub(crate) fn remove(&mut self, place: Place) -> T {
        self.unlink(place.0);
        self.visit();
        let node = std::mem::replace(&mut self.nodes[place.0], Node::Vacant(self.vacant));
        self.vacant = Some(place.0);
        self.len -= 1;
        match node {
            Node::Held { value, .. } => value,
            Node::Vacant(_) => unreachable!("BUG: a place names a held value until it is taken"),
        }
    }

    /// The least recently used value's place.
    pub(crate) fn oldest(&self) -> Option<Place> {
        self.oldest.map(Place)
    }

    /// Lets every value go, and the room of every node with them.
    pub(crate) fn release(&mut self) {
        #[cfg(test)]
        {
            self.visits += self.nodes.len() as u64;
        }
        self.nodes = Vec::new();
        self.vacant = None;
        self.oldest = None;
        self.newest = None;
        self.len = 0;
    }

    /// The nodes reached since this last answered, which starts the count
    /// again.
    #[cfg(test)]
    pub(crate) fn take_visits(&mut self) -> u64 {
        std::mem::take(&mut self.visits)
    }

    /// The room the list holds, in nodes, held or free.
    #[cfg(test)]
    pub(crate) fn room(&self) -> usize {
        self.nodes.capacity()
    }

    fn links(&mut self, index: usize) -> (&mut Option<usize>, &mut Option<usize>) {
        self.visit();
        match &mut self.nodes[index] {
            Node::Held { older, newer, .. } => (older, newer),
            Node::Vacant(_) => unreachable!("BUG: only a held node is linked"),
        }
    }

    /// Joins the neighbours of the held node at `index`, which keeps its
    /// own links until it is linked again or freed.
    fn unlink(&mut self, index: usize) {
        let (older, newer) = {
            let (older, newer) = self.links(index);
            (*older, *newer)
        };
        match older {
            Some(older) => *self.links(older).1 = newer,
            None => self.oldest = newer,
        }
        match newer {
            Some(newer) => *self.links(newer).0 = older,
            None => self.newest = older,
        }
    }

    /// Links the held node at `index`, unlinked, after the newest.
    fn link_newest(&mut self, index: usize) {
        let previous = self.newest;
        {
            let (older, newer) = self.links(index);
            *older = previous;
            *newer = None;
        }
        match previous {
            Some(previous) => *self.links(previous).1 = Some(index),
            None => self.oldest = Some(index),
        }
        self.newest = Some(index);
    }
}

#[cfg(test)]
mod tests {
    use super::RecencyList;

    fn drain_oldest(list: &mut RecencyList<char>) -> String {
        let mut out = String::new();
        while let Some(place) = list.oldest() {
            out.push(list.remove(place));
        }
        out
    }

    /// Values leave oldest first, a touch makes one the newest, a removed
    /// value's place is given to the next push, and a release lets the room
    /// go.
    #[test]
    fn a_recency_list_orders_values_by_their_last_use() {
        let mut list = RecencyList::new();
        let [first, second, third, fourth] =
            ['a', 'b', 'c', 'd'].map(|value| list.push_newest(value));
        list.touch(second);
        list.touch(first);
        list.touch(first);
        assert_eq!(list.remove(third), 'c');
        let fifth = list.push_newest('e');
        assert_eq!(fifth, third, "the freed place is reused");
        assert_eq!((*list.get(fourth), list.len()), ('d', 4));
        assert_eq!(drain_oldest(&mut list), "dbae");
        assert_eq!(list.len(), 0);
        list.push_newest('f');
        assert!(list.room() > 0);
        list.release();
        assert_eq!((list.oldest(), list.room()), (None, 0));
        assert_eq!(drain_oldest(&mut list), "");
    }
}
