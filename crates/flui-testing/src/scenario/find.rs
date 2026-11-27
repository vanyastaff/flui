//! Where a [`Tester`](super::Tester) looks: [`Find`], a description over the protocol's
//! [`Locator`] that the application resolves again at each use (ADR-0400 §2).
//!
//! A `Find<One>` names one element: an action takes it, so acting on a set does not compile, and
//! one that matches several elements at run time is
//! [`TestError::Ambiguous`](super::TestError::Ambiguous) with every candidate, never the first. A
//! `Find<Many>` names a set: a count, a read of every match, and an expectation over all of them.
//! A find is never a node handle.
//!
//! The functions here each start a `Find<One>`. A widget that publishes no semantics is found by
//! the identifier its author gives a group around it
//! (`Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)`, ADR-0240) and
//! [`id`].

use std::fmt;
use std::marker::PhantomData;

use flui_protocol::locator::{Locator, Pick, StateFilter, TextMatch};
use flui_protocol::wire::Role;

/// A [`Find`] that names exactly one element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "an uninhabited marker: a quantity is a type, never a value"
)]
pub enum One {}

/// A [`Find`] that names every element it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "an uninhabited marker: a quantity is a type, never a value"
)]
pub enum Many {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::One {}
    impl Sealed for super::Many {}
}

/// How many elements a [`Find`] names: [`One`] or [`Many`], and no other.
pub trait Quantity: sealed::Sealed {
    /// Whether a find of this quantity with `locator` requires exactly one match.
    #[doc(hidden)]
    fn requires_one(locator: &Locator) -> bool;
}

impl Quantity for One {
    /// The strict pick requires exactly one; a pick of one of several does not.
    fn requires_one(locator: &Locator) -> bool {
        locator.pick == Pick::Strict
    }
}

impl Quantity for Many {
    fn requires_one(_: &Locator) -> bool {
        false
    }
}

/// A description of the elements a test means, resolved by the application at each use: see the
/// [module docs](self).
#[derive(Clone, PartialEq, Eq)]
pub struct Find<Q = One> {
    locator: Locator,
    quantity: PhantomData<Q>,
}

impl<Q> fmt::Debug for Find<Q> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Find").field(&self.locator).finish()
    }
}

impl<Q> Find<Q> {
    fn of(locator: Locator) -> Self {
        Self {
            locator,
            quantity: PhantomData,
        }
    }

    /// The protocol's locator this find sends.
    #[must_use]
    pub fn locator(&self) -> &Locator {
        &self.locator
    }
}

impl Find<One> {
    /// The one element `locator` names: its [`Pick`] says which match, and the default, strict
    /// pick requires exactly one.
    #[must_use]
    pub fn from_locator(locator: Locator) -> Self {
        Self::of(locator)
    }

    /// Also: under an element `ancestor` matches.
    #[must_use]
    pub fn within<Q>(self, ancestor: Find<Q>) -> Self {
        Self::of(self.locator.within(ancestor.locator))
    }

    /// Also: with a descendant `descendant` matches.
    #[must_use]
    pub fn has<Q>(self, descendant: Find<Q>) -> Self {
        Self::of(self.locator.has(descendant.locator))
    }

    /// Every element this find matches.
    #[must_use]
    pub fn all(self) -> Find<Many> {
        Find::of(self.locator.pick(Pick::Strict))
    }
}

impl<Q: Quantity> Find<Q> {
    /// Whether the find requires exactly one match, rather than naming a set or picking one of
    /// several.
    pub(crate) fn requires_one(&self) -> bool {
        Q::requires_one(&self.locator)
    }
}

impl Find<Many> {
    /// The first match, in tree order.
    #[must_use]
    pub fn first(self) -> Find<One> {
        Find::of(self.locator.pick(Pick::First))
    }

    /// The match at `index` in tree order, from 0.
    #[must_use]
    pub fn nth(self, index: u32) -> Find<One> {
        Find::of(self.locator.pick(Pick::Nth(index)))
    }

    /// The last match, in tree order.
    #[must_use]
    pub fn last(self) -> Find<One> {
        Find::of(self.locator.pick(Pick::Last))
    }
}

/// The element whose accessible name is exactly `text`: a `Text` reads as its text, a button as
/// the text it holds.
#[must_use]
pub fn text(text: impl Into<String>) -> Find<One> {
    Find::of(Locator::named(text))
}

/// The element whose accessible name contains `text`.
#[must_use]
pub fn text_containing(text: impl Into<String>) -> Find<One> {
    Find::of(Locator::default().name_matching(TextMatch::contains(text)))
}

/// The button named exactly `name`.
#[must_use]
pub fn button(name: impl Into<String>) -> Find<One> {
    Find::of(Locator::role(Role::Button).name(name))
}

/// The element of `role`.
#[must_use]
pub fn role(role: Role) -> Find<One> {
    Find::of(Locator::role(role))
}

/// The element whose author-given identifier is `identifier` (ADR-0240).
#[must_use]
pub fn id(identifier: impl Into<String>) -> Find<One> {
    Find::of(Locator::identifier(identifier))
}

/// The element whose states hold `state`.
#[must_use]
pub fn with_state(state: StateFilter) -> Find<One> {
    Find::of(Locator::default().state(state))
}

/// The window's root element.
#[must_use]
pub fn root() -> Find<One> {
    Find::of(Locator::root())
}
