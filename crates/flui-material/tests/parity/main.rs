//! Flutter parity tests — value-level assertions ported from the Flutter
//! Material test suite.
//!
//! Each sub-module cites the Flutter test file and the oracle test's own
//! description (not upstream line numbers — those drift across releases;
//! the description string is the stable anchor) it mirrors, and records any
//! oracle tests intentionally **not** ported, with a reason.
//!
//! Oracle checkout tag `3.44.0`.
//!
//! ## Scope
//!
//! Only the default-M3-baseline and `copyWith`/equality assertions are
//! ported — the parts of `color_scheme_test.dart`/`theme_data_test.dart`
//! this crate actually implements. `ColorScheme.fromSeed` and its assertions
//! are deferred along with `fromSeed` itself (see `src/color_scheme.rs`
//! module docs) — not silently dropped, named here so the gap is visible.

mod color_scheme_test;
mod theme_data_test;
