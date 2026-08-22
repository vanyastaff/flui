//! Thin re-export of the canonical headless harness.
//!
//! The harness itself lives in [`flui_widgets::testing`] (feature-gated to
//! `testing`, enabled for these tests by this crate's self
//! dev-dependency) so that `flui-material` and `flui-cupertino` share the
//! exact same mount/pointer/clock machinery instead of drifted copies. Test
//! files keep importing through `crate::common::…`; only the definition
//! moved.

pub use flui_widgets::testing::*;
