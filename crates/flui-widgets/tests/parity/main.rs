//! Flutter parity tests — geometry assertions ported from the Flutter widget
//! test suite, run against FLUI's headless layout harness.
//!
//! Each sub-module cites the Flutter source file and line number it mirrors,
//! documents the widget → render-object type mapping, and records any
//! intentional divergences from Flutter behaviour.
//!
//! Phase covered: C1.13 (Core.1 exit gate) — geometry assertions only.
//! Paint, semantics, and the wider ~150-test corpus are Phase 3 (deferred).

#[path = "../common/mod.rs"]
mod common;

mod harness;

// ── Phase-2 ports (no new finders needed) ────────────────────────────────────
mod column_no_overflow_fp_test;
mod container_test;
mod list_view_test;
mod stateful_test;

// ── Phase-2 ports (use find_by_render_type / pump_widget) ────────────────────
mod center_test;
mod flex_test;
mod harness_self_test;
mod sized_box_test;
mod text_test;

// ── Business.1 slice — widget-catalog first five ──────────────────────────────
mod grid_view_test;
mod safe_area_test;
mod sliver_grid_test;
mod spacer_test;
mod visibility_test;

// ── Business.1 slice — CustomPaint ───────────────────────────────────────────
mod custom_paint_test;

// ── Business.1 slice 2 — CustomScrollView + eager sliver-fill wrappers ───────
mod custom_scroll_view_test;
mod sliver_fill_remaining_test;
mod sliver_fill_viewport_test;
mod sliver_ignore_pointer_test;
mod sliver_offstage_test;

// ── Core.2 — RenderFlow / Flow ───────────────────────────────────────────────
mod flow_test;
