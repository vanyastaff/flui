//! # FLUI Macros
//!
//! Procedural macros for the FLUI framework.
//!
//! ## Status
//!
//! Phase 1 ships the **crate skeleton only** (plan §U5). The real
//! `#[derive(StatelessView)]` / `#[derive(StatefulView)]` derives land
//! in Phase 3 §U23 alongside the `impl IntoView` authoring-surface
//! switch (FR-007, FR-008, FR-009). The crate exists now so the
//! workspace dependency graph and `flui-view`'s downstream wiring are
//! in place before Phase 3 starts.
//!
//! ## Why a separate crate?
//!
//! `proc-macro` crates must be `[lib] proc-macro = true`, which makes
//! them leaf crates — they cannot depend on or be depended on by
//! ordinary library crates in the usual sense (only their generated
//! tokens reach the consuming crate). Splitting the derives into
//! `flui-macros` keeps `flui-view` itself free of the `proc-macro`
//! constraint while letting widget authors `#[derive(StatelessView)]`
//! after a single `use flui_view::prelude::*;` import (re-export wiring
//! is set up in Phase 3 §U23).
//!
//! ## Placeholder
//!
//! The placeholder derive below is intentionally a no-op. It exists so:
//!
//! 1. The crate compiles under `[lib] proc-macro = true` (a proc-macro
//!    crate with no `#[proc_macro_*]` exports emits a hard rustc
//!    error: `expected at least one proc-macro to be defined`).
//! 2. Downstream linkage (`flui-view` → `flui-macros`) can be smoke-
//!    tested in U5 without waiting for U23 to ship the real derives.
//!
//! The placeholder is removed in Phase 3 §U23 when the real derives
//! are added.

#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
#![allow(
    // Placeholder will be replaced in §U23; allowing unused for the
    // skeleton commit keeps the smoke build green without polluting
    // workspace-wide lint config.
    dead_code,
    clippy::missing_const_for_fn
)]

use proc_macro::TokenStream;

/// Placeholder derive macro.
///
/// Phase 1 skeleton only — emits an empty token stream so the
/// `proc-macro = true` crate has at least one `#[proc_macro_*]` export
/// (required by rustc). Replaced by the real `#[derive(StatelessView)]`
/// and `#[derive(StatefulView)]` derives in Phase 3 §U23.
///
/// Widget authors must not use this derive; it is wired only so the
/// downstream `flui-view` crate can prove the dependency edge at
/// compile time.
#[proc_macro_derive(FluiMacrosPlaceholder)]
pub fn flui_macros_placeholder(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
