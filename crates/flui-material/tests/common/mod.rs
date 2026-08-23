//! Thin re-export of the canonical headless harness shared by every
//! widget-layer test suite.
//!
//! Historically this file was a hand-maintained copy of `flui-widgets`'
//! harness, and the copies drifted: this crate's pointer dispatch kept
//! reusing `PointerId::PRIMARY` for every contact after the canonical
//! harness moved to a fresh pointer id per Down (production-faithful — a
//! platform never recycles an id into a still-tracked gesture). The harness
//! now lives once in [`flui_widgets::testing`] (compiled via the
//! `testing` feature this crate's dev-dependency enables), so mount
//! ordering, contact identity, and virtual-clock policy are shared instead
//! of forked. Note the dispatch consequences: a contact Move/Up requires a
//! preceding Down, and a contactless hover is `dispatch_pointer_hover`, not
//! `dispatch_pointer_move`.

pub use flui_widgets::testing::*;
