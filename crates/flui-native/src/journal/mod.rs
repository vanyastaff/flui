//! The journal of native input (ADR-0215 §4): what a host's pointer
//! registries were fed, before FLUI translated it.
//!
//! - `capture` (debug builds with `native-capture`): the hook every
//!   [`ContactRegistry`](crate::input::assembly::ContactRegistry) created on a
//!   thread reports its calls to while a capture is installed there;
//! - `recorder` (the same builds): the sink that folds those calls into one
//!   [`NativeRecording`](codec::NativeRecording) per registry;
//! - `codec` (with `native-capture` or `testing`): the recording read,
//!   written and replayed, in the schema `flui-protocol` declares, so a reader
//!   such as the driver reads it without this crate.
//!
//! The backends link no serializer: the codec and its dependencies come only
//! with those features.

#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod capture;
#[cfg(any(feature = "native-capture", feature = "testing"))]
pub mod codec;
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod recorder;
