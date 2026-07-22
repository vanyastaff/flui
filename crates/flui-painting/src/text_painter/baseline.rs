//! `TextBaseline` -- the baseline to use for aligning text.
//!
//! Re-exported from [`flui_types`] — the single canonical definition for the
//! workspace. A local enum lived here while `flui-types`' definition lacked the
//! `Copy + Eq + Hash` the painting/typography hot paths need (by-value passing
//! and cache map keys); once those derives were widened on the canonical type,
//! this consolidated to a re-export (2026-06), per the resolution this module
//! previously documented.

pub use flui_types::layout::TextBaseline;
