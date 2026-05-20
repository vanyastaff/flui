//! Arity types re-exported from [`flui_tree`].
//!
//! This module is a transitional re-export shim. The canonical home of the
//! arity primitives is `flui_tree::arity`; this module exists only so that
//! the existing 18+ call sites inside `flui-rendering` can continue to write
//! `use crate::arity::X` without churn. New code should import directly
//! from `flui_tree`.
//!
//! Mythos Step 5a (2026-05-20): trimmed from 48 LOC of duplicated docs +
//! dead `TreeChildrenAccess` alias to the minimum re-export. The full
//! deletion of this module + rewire of internal call sites to `flui_tree`
//! is tracked in `crates/flui-rendering/ARCHITECTURE.md` under "Outstanding
//! refactors".

pub use flui_tree::{Arity, ArityStorage, ArityStorageView, Leaf, Optional, Single, Variable};
