//! Sliver infrastructure: data structures and algorithms for lazy
//! scrollable content.
//!
//! The windowing math (offset↔index mapping, estimate-then-correct, anchor
//! correction) for lazy lists/grids lives in the protocol-agnostic
//! [`crate::virtualization`] module, backed by a focused augmented B+-tree. The
//! former flat-array `FenwickExtents` BIT that lived here was deleted: a
//! Fenwick/BIT pays `O(n)` for a mid-list insert/delete (every later index
//! shifts), which is the wrong structure for a dynamic list — see
//! `ADR-0003` (docs/adr/ADR-0003-virtualization-core-and-reentrant-build.md).
