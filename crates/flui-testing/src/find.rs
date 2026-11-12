//! Where a [`Tester`](crate::Tester) looks: the [`Locator`]s a test finds
//! elements with, as an assistive technology, the driver and an agent find
//! them.
//!
//! Each function returns the protocol's own [`Locator`], so a test can refine
//! one (`find::button("Save").within(find::id("dialog"))`) or write one in
//! full where these helpers stop. A locator is resolved over the window's
//! published semantics tree at every use; a widget that publishes no
//! semantics is found by the identifier its author gives it
//! (`Semantics::identifier`, ADR-0240) and [`id`].

use flui_protocol::{Locator, Role, StateFilter, TextMatch};

/// Elements whose accessible name is exactly `text`: a `Text` reads as its
/// text, a button as the text it holds.
#[must_use]
pub fn text(text: impl Into<String>) -> Locator {
    Locator::named(text)
}

/// Elements whose accessible name contains `text`.
#[must_use]
pub fn text_containing(text: impl Into<String>) -> Locator {
    Locator::default().name_matching(TextMatch::contains(text))
}

/// The button named exactly `name`.
#[must_use]
pub fn button(name: impl Into<String>) -> Locator {
    Locator::role(Role::Button).name(name)
}

/// Elements of `role`.
#[must_use]
pub fn role(role: Role) -> Locator {
    Locator::role(role)
}

/// The element whose author-given identifier is `identifier` (ADR-0240).
#[must_use]
pub fn id(identifier: impl Into<String>) -> Locator {
    Locator::identifier(identifier)
}

/// Elements whose states hold `state`.
#[must_use]
pub fn with_state(state: StateFilter) -> Locator {
    Locator::default().state(state)
}
