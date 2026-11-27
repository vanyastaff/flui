//! What a test expects of the elements a [`Find`] names: a [`Matcher`] is data that lowers to
//! exactly one protocol [`Condition`] (ADR-0400 §2), which the application's runtime evaluates at
//! frame boundaries and after input. The test process has no second evaluator, so
//! `t.expect(&find, matcher)` is the same wait on every connection.
//!
//! An expectation holds once its condition is first true. A claim about the final state is
//! written `settle` first, then the expectation or a read. The wire carries no negation besides
//! [`gone`] and [`disabled`] and no rectangle: such a claim is read and asserted in Rust, which
//! does not wait.

use flui_protocol::locator::{Locator, StateFilter, TextMatch};
use flui_protocol::wait::{Condition, Expectation};
use flui_protocol::wire::Checked;

use super::find::{Find, Quantity};

/// What a test expects of a find's elements: see the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matcher(Kind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Exists,
    Gone,
    Count(u32),
    Has(Expectation),
    All(Vec<Matcher>),
    Any(Vec<Matcher>),
}

impl Matcher {
    /// The condition this matcher asks of `find`'s elements.
    ///
    /// A find that requires exactly one element ([`Find<One>`](super::find::One) with its strict
    /// pick) holds only while exactly one element matches: the wire's `Matches` alone holds for
    /// every match meeting the expectation, so the count is asked beside it.
    pub(crate) fn condition<Q: Quantity>(&self, find: &Find<Q>) -> Condition {
        self.lower(find.locator(), find.requires_one())
    }

    fn lower(&self, locator: &Locator, strict: bool) -> Condition {
        let count = |count: u32| Condition::Matches {
            locator: locator.clone(),
            expect: Expectation::Count(count),
        };
        match &self.0 {
            Kind::Exists if strict => count(1),
            Kind::Exists => Condition::Appears(locator.clone()),
            Kind::Gone => Condition::Gone(locator.clone()),
            Kind::Count(n) => count(*n),
            Kind::Has(expect) => {
                let matches = Condition::Matches {
                    locator: locator.clone(),
                    expect: expect.clone(),
                };
                if strict {
                    Condition::All(vec![count(1), matches])
                } else {
                    matches
                }
            }
            Kind::All(parts) => Condition::All(
                parts
                    .iter()
                    .map(|part| part.lower(locator, strict))
                    .collect::<Vec<Condition>>(),
            ),
            Kind::Any(parts) => Condition::Any(
                parts
                    .iter()
                    .map(|part| part.lower(locator, strict))
                    .collect::<Vec<Condition>>(),
            ),
        }
    }
}

/// The find names an element: exactly one for a strict `Find<One>`, at least one otherwise.
#[must_use]
pub fn exists() -> Matcher {
    Matcher(Kind::Exists)
}

/// No element matches. A node whose identifier the read lifts away matches nothing, so this holds
/// for it: give such a node a role to find it (ADR-0240 §3).
#[must_use]
pub fn gone() -> Matcher {
    Matcher(Kind::Gone)
}

/// Exactly `n` elements match.
#[must_use]
pub fn count(n: u32) -> Matcher {
    Matcher(Kind::Count(n))
}

/// The accessible name is exactly `name`.
#[must_use]
pub fn name(name: impl Into<String>) -> Matcher {
    Matcher(Kind::Has(Expectation::Name(TextMatch::exact(name))))
}

/// The value is exactly `value`.
#[must_use]
pub fn value(value: impl Into<String>) -> Matcher {
    Matcher(Kind::Has(Expectation::Value(TextMatch::exact(value))))
}

/// The value contains `text`.
#[must_use]
pub fn value_contains(text: impl Into<String>) -> Matcher {
    Matcher(Kind::Has(Expectation::Value(TextMatch::contains(text))))
}

/// The description is exactly `description`.
#[must_use]
pub fn description(description: impl Into<String>) -> Matcher {
    Matcher(Kind::Has(Expectation::Description(TextMatch::exact(
        description,
    ))))
}

/// The states in `state` hold.
#[must_use]
pub fn state(state: StateFilter) -> Matcher {
    Matcher(Kind::Has(Expectation::State(state)))
}

/// Checked.
#[must_use]
pub fn checked() -> Matcher {
    let mut filter = StateFilter::default();
    filter.checked = Some(Checked::True);
    state(filter)
}

/// Not disabled, itself or through an ancestor.
#[must_use]
pub fn enabled() -> Matcher {
    let mut filter = StateFilter::default();
    filter.disabled = Some(false);
    state(filter)
}

/// Disabled, itself or through an ancestor.
#[must_use]
pub fn disabled() -> Matcher {
    let mut filter = StateFilter::default();
    filter.disabled = Some(true);
    state(filter)
}

/// Holding keyboard focus.
#[must_use]
pub fn focused() -> Matcher {
    Matcher(Kind::Has(Expectation::Focused))
}

/// Shown: its bounds keep a non-empty part inside the window and every clipping ancestor, the
/// `visible` check an action makes.
#[must_use]
pub fn visible() -> Matcher {
    Matcher(Kind::Has(Expectation::Visible))
}

/// Every one of `matchers` holds at once.
#[must_use]
pub fn all(matchers: impl IntoIterator<Item = Matcher>) -> Matcher {
    Matcher(Kind::All(matchers.into_iter().collect::<Vec<Matcher>>()))
}

/// At least one of `matchers` holds.
#[must_use]
pub fn any(matchers: impl IntoIterator<Item = Matcher>) -> Matcher {
    Matcher(Kind::Any(matchers.into_iter().collect::<Vec<Matcher>>()))
}
