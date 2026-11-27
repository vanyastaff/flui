//! # `flui_driver`
//!
//! Drives a running FLUI app for tests, people and agents (ADR-0238): one API over every target,
//! the same methods the `flui drive` CLI and the `flui mcp` server reach.
//!
//! The app knows the truth: its trees, its frames, the input it received. Every debug FLUI app,
//! and a release build with the `driver` feature, serves it through its agent (ADR-0239). The
//! driver composes that truth with what only the OS can do, through a [`Lane`]: deliver real
//! input, and own the process.
//!
//! ```no_run
//! # #[cfg(feature = "headless")]
//! # fn counter() -> flui_widgets::Text { flui_widgets::Text::new("Count: 0") }
//! # #[cfg(feature = "headless")]
//! # fn main() -> flui_driver::Result<()> {
//! use flui_driver::{Driver, Launch, Locator, Role, expect};
//!
//! let mut driver = Driver::launch(Launch::headless(counter))?;
//! let mut window = driver.main_window()?;
//! window.locate(Locator::role(Role::Button).name("Increment")).tap()?;
//! expect(window.locate(Locator::identifier("count"))).to_have_value("1")?;
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "headless"))]
//! # fn main() {}
//! ```
//!
//! - **Targets.** `Launch::headless` hosts a view in process on a virtual clock (feature
//!   `headless`); [`Launch::executable`] and [`Launch::cargo_example`] start an app on this host;
//!   [`Driver::attach`] connects to one already running. Code written against one runs against
//!   every other.
//! - **Input paths.** Every action takes the semantic, synthetic or OS path and its answer names
//!   the path taken. The default is synthetic everywhere; OS input is asked for, announced,
//!   guarded against the user's own input, isolated where it can be, and one session at a time on
//!   a host ([`os_input`]).
//! - **Failures** are typed ([`Error`]), one variant per wire code, and a check that could not be
//!   made is `cannot_verify` in the [`Report`](protocol::report::Report), never a pass.

mod client;
mod compose;
mod connection;
pub mod coordinates;
mod driver;
mod error;
mod expect;
pub mod lane;
mod launch;
pub mod os_input;
pub mod pixels;
pub mod projection;
pub mod scenario;
pub mod steps;
#[cfg(all(feature = "testing", any(unix, windows)))]
pub mod testing;
pub mod trace;
mod window;

#[cfg(any(unix, windows))]
pub use connection::socket::Endpoint;
pub use driver::Driver;
pub use error::{Awaited, Error, Result};
pub use expect::{Expect, SoftExpect, expect, expect_soft};
pub use flui_protocol::input::{InputPath, InputPolicy};
pub use flui_protocol::locator::{Locator, Pick, StateFilter, TextMatch};
pub use flui_protocol::session::Isolation;
pub use flui_protocol::wire::Role;
pub use lane::Lane;
pub use launch::{Artifact, Launch};
pub use scenario::{Scenario, WindowSpec};
pub use window::{Element, ElementLocator, Window};

/// The wire vocabulary the API speaks: methods, parameters, outputs and errors.
pub use flui_protocol as protocol;
