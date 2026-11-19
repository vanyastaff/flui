//! The input area: what a host dispatches to a window
//! ([`PlatformInput`](crate::PlatformInput)), the [`pointer`](mod@pointer) and
//! [`keyboard`] vocabularies it carries (ADR-0143), the handle-less
//! [`input_capabilities`] (ADR-0213), the [`input_regions`] the application
//! claims for its own input (ADR-0217 §5), and the
//! [`EventTime`](crate::EventTime) every event carries.
//!
//! `PlatformInput`, the values dispatch answers with and `EventTime` are at
//! the crate root.

pub(crate) mod event;
pub(crate) mod event_time;
pub mod input_capabilities;
pub mod input_regions;
pub mod keyboard;
pub mod pointer;
