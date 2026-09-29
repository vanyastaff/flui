//! Row runner for table-driven tests: every row runs, and a failure names
//! each failing row instead of stopping at the first.

pub(crate) type Case = (&'static str, fn());

pub(crate) fn run_cases(table: &str, cases: &[Case]) {
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "{table}: failing rows: {failed:?}");
}
