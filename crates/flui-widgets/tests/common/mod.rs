//! Thin re-export of the canonical headless harness.
//!
//! The harness itself lives in [`flui_testing::widgets`], so that
//! `flui-material` and `flui-cupertino` share the exact same
//! mount/pointer/clock machinery instead of drifted copies. Test files keep
//! importing through `crate::common::…`; only the definition moved.

pub use flui_testing::widgets::*;

#[allow(dead_code)]
pub mod cases;
#[allow(dead_code)]
pub mod child_process;
