//! Runs a family of named scenarios inside one `#[test]`, so a failure names its case.

/// Runs every case even after one fails, then panics listing the failing case names.
pub(crate) fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}
