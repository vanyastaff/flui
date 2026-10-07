//! Insertion-ordered memberships for signal readers and their reverse index.
//!
//! Small sets stay inline. Larger sets index membership and leave holes on
//! removal, compacting only after as many removals as surviving members. Thus
//! a shared signal's rebuild wave does linear bookkeeping without changing
//! peer scheduling order. Keys here are internal IDs, never user values.

use std::collections::{HashMap, hash_map::Entry};
use std::hash::Hash;

use smallvec::SmallVec;

pub(super) enum Membership<T> {
    Inline(SmallVec<[T; 4]>),
    Indexed(Box<Indexed<T>>),
}

pub(super) struct Indexed<T> {
    members: Vec<Option<T>>,
    positions: HashMap<T, usize>,
}

impl<T> Default for Membership<T> {
    fn default() -> Self {
        Self::Inline(SmallVec::new())
    }
}

impl<T: Copy + Eq + Hash> Membership<T> {
    #[inline]
    pub(super) fn insert(&mut self, member: T) {
        if let Self::Inline(members) = self {
            if members.contains(&member) {
                return;
            }
            if members.len() < 4 {
                members.push(member);
                return;
            }
            let positions = members
                .iter()
                .copied()
                .enumerate()
                .map(|(index, value)| (value, index))
                .collect();
            let members = members.iter().copied().map(Some).collect();
            *self = Self::Indexed(Box::new(Indexed { members, positions }));
        }
        if let Self::Indexed(indexed) = self
            && let Entry::Vacant(position) = indexed.positions.entry(member)
        {
            position.insert(indexed.members.len());
            indexed.members.push(Some(member));
        }
    }

    #[inline]
    pub(super) fn remove(&mut self, member: &T) {
        match self {
            Self::Inline(members) => {
                if let Some(index) = members.iter().position(|value| value == member) {
                    members.remove(index);
                }
            }
            Self::Indexed(indexed) => {
                let Some(index) = indexed.positions.remove(member) else {
                    return;
                };
                indexed.members[index] = None;
                if indexed.positions.len() <= 4 {
                    *self = Self::Inline(indexed.members.iter().flatten().copied().collect());
                } else if indexed.members.len() >= indexed.positions.len() * 2 {
                    indexed.members.retain(Option::is_some);
                    for (index, member) in indexed.members.iter().flatten().enumerate() {
                        *indexed
                            .positions
                            .get_mut(member)
                            .expect("BUG: every live membership has an index") = index;
                    }
                }
            }
        }
    }

    #[inline]
    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        let mut index = 0;
        std::iter::from_fn(move || match self {
            Self::Inline(members) => {
                let member = members.get(index)?;
                index += 1;
                Some(member)
            }
            Self::Indexed(indexed) => loop {
                let member = indexed.members.get(index)?;
                index += 1;
                if let Some(member) = member {
                    return Some(member);
                }
            },
        })
    }

    pub(super) fn to_vec(&self) -> Vec<T> {
        match self {
            Self::Inline(members) => members.to_vec(),
            Self::Indexed(indexed) => {
                let mut members = Vec::with_capacity(indexed.positions.len());
                members.extend(indexed.members.iter().flatten().copied());
                members
            }
        }
    }

    #[inline]
    pub(super) fn snapshot(&self) -> SmallVec<[T; 4]> {
        match self {
            Self::Inline(members) => members.clone(),
            Self::Indexed(indexed) => {
                let mut members = SmallVec::with_capacity(indexed.positions.len());
                members.extend(indexed.members.iter().flatten().copied());
                members
            }
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}
