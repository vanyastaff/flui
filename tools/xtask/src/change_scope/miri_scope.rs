//! Which changed files can change what CI's `miri` job finds.
//!
//! The job interprets every unit test of the crates the others stand on
//! (flui-foundation, flui-log) and the render-node arena of flui-rendering,
//! beside narrower filters over the rendering owner, the layout lease, the
//! GlobalKey contract and the engine's surface lease (`tasks::miri_plan`). A
//! pull request runs the job only when its diff reaches the sources those
//! whole-module runs interpret ([`changed_input`]), or the job's own plan;
//! main runs it on every push (ADR-0303 §9).

/// The source roots the job interprets whole: a change under one can make a
/// test Miri rejects (a leak, an unsupported foreign call, undefined
/// behavior) where the stable test run passes.
pub(super) const INTERPRETED: [&str; 3] = [
    "crates/flui-foundation/src/",
    "crates/flui-log/src/",
    "crates/flui-rendering/src/storage/",
];

/// The files that state what the job runs: its plan and this rule.
const THE_JOB: [&str; 2] = [
    "tools/xtask/src/tasks.rs",
    "tools/xtask/src/change_scope/miri_scope.rs",
];

/// The first of `files` under a root the job interprets whole, or one of the
/// files that state what it runs, or `None`.
pub(super) fn changed_input(files: &[String]) -> Option<&str> {
    files
        .iter()
        .map(String::as_str)
        .find(|f| INTERPRETED.iter().any(|root| f.starts_with(root)) || THE_JOB.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(files: &[&str]) -> Option<String> {
        let files: Vec<String> = files.iter().map(|&f| f.to_owned()).collect();
        changed_input(&files).map(str::to_owned)
    }

    fn an_interpreted_source_asks_for_the_job() {
        for f in [
            "crates/flui-foundation/src/notifier_generic.rs",
            "crates/flui-log/src/backend/mod.rs",
            "crates/flui-rendering/src/storage/tree.rs",
            "tools/xtask/src/tasks.rs",
        ] {
            assert_eq!(input(&["README.md", f]).as_deref(), Some(f), "{f}");
        }
    }

    fn other_sources_leave_it_to_main() {
        assert_eq!(
            input(&[
                "crates/flui-foundation/tests/commit.rs",
                "crates/flui-rendering/src/pipeline/mod.rs",
                "crates/flui-widgets/src/container.rs",
                "docs/testing.md",
            ]),
            None
        );
    }

    fn each_root_and_job_file_exists() {
        let root = crate::util::repo_root();
        for dir in INTERPRETED {
            assert!(root.join(dir).is_dir(), "{dir}");
        }
        for file in THE_JOB {
            assert!(root.join(file).is_file(), "{file}");
        }
    }

    #[test]
    fn miri_inputs() {
        crate::table_test::run_table(
            "miri_inputs",
            &[
                (
                    "an_interpreted_source_asks_for_the_job",
                    an_interpreted_source_asks_for_the_job as fn(),
                ),
                (
                    "other_sources_leave_it_to_main",
                    other_sources_leave_it_to_main as fn(),
                ),
                (
                    "each_root_and_job_file_exists",
                    each_root_and_job_file_exists as fn(),
                ),
            ],
        );
    }
}
