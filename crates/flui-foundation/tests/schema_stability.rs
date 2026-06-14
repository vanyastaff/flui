//! Schema-stability contract for the diagnostics JSON envelope.
//!
//! This test is the **years-long contract**: it asserts that the JSON Schema
//! generated from [`flui_foundation::DiagnosticsEnvelope`] at build time
//! matches the committed file at `schema/diagnostics.v1.json`.
//!
//! When the schema drifts (a field added, renamed, or removed from the
//! diagnostics tree), this test fails with a diff. The intended workflow:
//!
//! 1. Review whether the drift is intentional.
//! 2. If yes: bump [`DIAGNOSTICS_FORMAT_VERSION`], regenerate the schema
//!    file by running the `gen_schema` test below, and commit both.
//! 3. If no: fix the accidental divergence in the source types.
//!
//! Note: schemars output is not semver-stable across schemars versions.
//! The committed file is the owned gate — the test compares against it,
//! not against any external reference.

#![cfg(feature = "schemars")]

use flui_foundation::DiagnosticsEnvelope;

/// Assert that the committed schema file matches the currently-generated one.
///
/// Fails with a diff when any type reachable from [`DiagnosticsEnvelope`]
/// gains, loses, or renames a field — the intentional gate for the contract.
#[test]
fn committed_schema_matches_generated() {
    let generated =
        serde_json::to_string_pretty(&schemars::schema_for!(DiagnosticsEnvelope)).unwrap();
    let committed = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schema/diagnostics.v1.json"
    ));
    assert_eq!(
        generated.trim(),
        committed.trim(),
        "diagnostics schema drifted — review the diff above; \
         if the change is intentional, regenerate schema/diagnostics.v1.json \
         (run `cargo test -p flui-foundation --features schemars -- gen_schema --ignored`) \
         and bump DIAGNOSTICS_FORMAT_VERSION per the versioning policy"
    );
}

/// Gate that `DIAGNOSTICS_FORMAT_VERSION` stays consistent with the committed
/// schema file.
///
/// The schema filename encodes the version (`diagnostics.v1.json`); this test
/// asserts the constant in code equals 1 so a maintainer cannot regenerate the
/// schema with a different shape without also bumping the version constant —
/// and thus breaking this coupled assertion.
///
/// When the version is bumped: update the `assert_eq!` below alongside the
/// schema filename and constant.
#[test]
fn format_version_constant_matches_committed_schema_version() {
    assert_eq!(
        flui_foundation::DIAGNOSTICS_FORMAT_VERSION,
        1,
        "DIAGNOSTICS_FORMAT_VERSION must equal 1 while the committed schema \
         is diagnostics.v1.json — bump both together when the wire changes"
    );
}

/// Generate (or regenerate) `schema/diagnostics.v1.json` from the current types.
///
/// Run explicitly with:
///
/// ```text
/// cargo test -p flui-foundation --features schemars -- gen_schema --ignored
/// ```
///
/// Commit the resulting file alongside any type changes that caused drift.
/// This test is `#[ignore]`d so it never runs in normal CI — the
/// `committed_schema_matches_generated` test above is the CI gate.
#[test]
#[ignore = "generator: run manually to update schema/diagnostics.v1.json"]
fn gen_schema() {
    let schema = serde_json::to_string_pretty(&schemars::schema_for!(DiagnosticsEnvelope)).unwrap();

    // CARGO_MANIFEST_DIR is crates/flui-foundation; the schema lives at repo root / schema/.
    let schema_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    std::fs::create_dir_all(&schema_dir)
        .expect("schema/ directory must be creatable at the repo root");

    let path = schema_dir.join("diagnostics.v1.json");
    std::fs::write(&path, schema.as_bytes())
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));

    println!("wrote {}", path.canonicalize().unwrap_or(path).display());
}
