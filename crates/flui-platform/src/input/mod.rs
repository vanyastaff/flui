//! The input area: what a host dispatches to a window
//! ([`PlatformInput`](crate::PlatformInput)), the [`pointer`](mod@pointer) and
//! [`keyboard`] vocabularies it carries (ADR-0143), the handle-less
//! [`capabilities`] (ADR-0213), the input [`regions`] the application
//! claims for its own input (ADR-0217 §5), and the
//! [`EventTime`](crate::EventTime) every event carries.
//!
//! `PlatformInput`, the values dispatch answers with and `EventTime` are at
//! the crate root.

pub mod capabilities;
pub(crate) mod event;
pub(crate) mod event_time;
pub mod keyboard;
pub mod pointer;
pub mod regions;
