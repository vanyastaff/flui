//! The journal of native input (ADR-0215 §4): what a host's pointer
//! registries were fed, what its loop and window deliveries were handed
//! (system notices, session ends, reopens, input method events, close
//! requests and closes), what a window's input method did to the text store
//! it served, and what its platform answered reads of the system with
//! (preferences, displays), before the application heard it.
//!
//! - `capture` (debug builds with `native-capture`): the hook every
//!   [`ContactRegistry`](crate::input::assembly::ContactRegistry), loop
//!   delivery, window delivery and platform created on a thread reports its
//!   calls to while a capture is installed there;
//! - `store_capture` (the same builds): the window's text store host and
//!   stores that stand between a pull-model input method and the field, and
//!   journal each lock into the window's stream;
//! - `recorder` (the same builds): the sink that folds those calls into one
//!   `codec::NativeRecording` per registry, delivery or platform;
//! - `lines` (the same builds): the sink that hands each of them on as the wire's lines, a
//!   header and then its rows, for the driver agent's recordings;
//! - `codec`, `order` and `store` (with `native-capture` or `testing`): the recording
//!   read, written and replayed, in the schema `flui-protocol` declares, so a
//!   reader such as the driver reads it without this crate; `store` holds a
//!   text store's locks and their replay on another store.
//!
//! The backends link no serializer: the codec and its dependencies come only
//! with those features.

#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod capture;
#[cfg(any(feature = "native-capture", feature = "testing"))]
pub mod codec;
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod lines;
#[cfg(any(feature = "native-capture", feature = "testing"))]
mod order;
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod recorder;
#[cfg(any(feature = "native-capture", feature = "testing"))]
pub mod store;
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub(crate) mod store_capture;
