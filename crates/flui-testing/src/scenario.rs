//! A test is one [`Tester`] over a [`Connection`] (ADR-0400): the connection sets the depth, and
//! the tester's verbs, finders and matchers are the agent protocol's vocabulary, evaluated in the
//! application's runtime on every target.
//!
//! - [`Tester`]: the verbs, each with its `try_` twin; reads are turns with no frame, an act ends
//!   with an owner turn that drains what its frames queued, and a wait is the connection's.
//! - [`find`]: [`Find<One>`](Find) and `Find<Many>` over the protocol's
//!   [`Locator`](flui_protocol::locator::Locator).
//! - [`matcher`]: [`Matcher`]s, data that lowers to one protocol
//!   [`Condition`](flui_protocol::wait::Condition).
//! - [`Headless`]: a view mounted in process on a [`VirtualClock`].
//! - [`TestError`] and its [`Diagnosis`]; [`TestReport`], what a test's end found.
//!
//! ```
//! use flui_testing::scenario::{Tester, find, matcher::exists};
//! use flui_widgets::{Center, Text};
//!
//! let mut t = Tester::mount(Center::new().child(Text::new("Hello")));
//! t.expect(&find::text("Hello"), exists());
//! ```
//!
//! [`Connection`]: flui_protocol::client::Connection

mod clock;
mod error;
pub mod find;
mod headless;
pub mod matcher;
mod report;
mod tester;
#[cfg(test)]
mod testing;
pub(crate) mod window;
pub(crate) use headless::Windows;

pub use clock::{Advanced, VirtualClock};
pub use error::{Diagnosis, TestError};
pub use find::Find;
pub use headless::{FRAME, Headless, Observed};
pub use matcher::Matcher;
pub use report::TestReport;
pub use tester::{Checks, EXPECT_LIMIT, Settled, SettledAt, Tester};
pub use window::TestWindow;
