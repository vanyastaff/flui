//! Which changed files can move a count CI's `perf-gates` job compares.
//!
//! The job runs the perf scenarios of flui-widgets and compares each frame's
//! counters and allocations with the checked-in baseline
//! (`cargo xtask perf --check`). Main runs it on every push. A pull request
//! runs that comparison only when its diff reaches the code that runs once
//! per node of every frame ([`PER_NODE`]), where one more
//! allocation is a rise in every scenario, or the comparison's own inputs
//! ([`THE_JOB`]); a rise from anywhere else is left to main (ADR-0303 §9).
//! The callgrind half of the job stays on main.

/// The source roots whose code runs for every node of a frame: the element
/// owners' build walk, the render tree's storage, the pipeline owner's
/// layout, paint and semantics passes, and the
/// semantics model those passes merge.
pub(super) const PER_NODE: [&str; 4] = [
    "crates/flui-view/src/owner/",
    "crates/flui-rendering/src/storage/",
    "crates/flui-rendering/src/pipeline/",
    "crates/flui-semantics/src/",
];

/// What the comparison reads besides the code: the baseline, the scenarios,
/// the allocator and the report that count for them,
/// the task that compares them and this rule.
const THE_JOB: [&str; 6] = [
    "crates/flui-testing/src/heap.rs",
    "crates/flui-testing/src/control_cost.rs",
    "crates/flui-widgets/perf/baseline/",
    "crates/flui-widgets/tests/perf.rs",
    "tools/xtask/src/perf.rs",
    "tools/xtask/src/change_scope/perf_scope.rs",
];

/// The first of `files` under a per-node source root or among the
/// comparison's inputs, or `None`.
pub(super) fn changed_input(files: &[String]) -> Option<&str> {
    files.iter().map(String::as_str).find(|f| {
        PER_NODE
            .iter()
            .chain(&THE_JOB)
            .any(|input| f.starts_with(input))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(files: &[&str]) -> Option<String> {
        let files: Vec<String> = files.iter().map(|&f| f.to_owned()).collect();
        changed_input(&files).map(str::to_owned)
    }

    fn a_per_node_source_or_an_input_asks_for_the_job() {
        for f in [
            "crates/flui-view/src/owner/build_owner.rs",
            "crates/flui-rendering/src/storage/tree.rs",
            "crates/flui-rendering/src/pipeline/owner/semantics.rs",
            "crates/flui-testing/src/heap.rs",
            "crates/flui-semantics/src/configuration.rs",
            "crates/flui-widgets/perf/baseline/full_reassemble.toml",
            "crates/flui-widgets/tests/perf.rs",
            "tools/xtask/src/perf.rs",
        ] {
            assert_eq!(input(&["README.md", f]).as_deref(), Some(f), "{f}");
        }
    }

    fn other_sources_leave_it_to_main() {
        assert_eq!(
            input(&[
                "crates/flui-rendering/src/traits/render_object.rs",
                "crates/flui-view/src/element/mod.rs",
                "crates/flui-testing/src/host.rs",
                "crates/flui-rendering/tests/semantics_assembly.rs",
                "crates/flui-semantics/tests/identifier.rs",
                "crates/flui-semantics/ARCHITECTURE.md",
                "crates/flui-widgets/src/container.rs",
                "crates/flui-widgets/tests/semantics.rs",
                "packages/flui-material/src/ink_well.rs",
                "tools/xtask/src/perf/size.rs",
                "docs/testing.md",
            ]),
            None
        );
    }

    fn each_root_and_input_exists() {
        let root = crate::util::repo_root();
        for path in PER_NODE.iter().chain(&THE_JOB) {
            let path_on_disk = root.join(path);
            if path.ends_with('/') {
                assert!(path_on_disk.is_dir(), "{path}");
            } else {
                assert!(path_on_disk.is_file(), "{path}");
            }
        }
    }

    #[test]
    fn perf_inputs() {
        crate::table_test::run_table(
            "perf_inputs",
            &[
                (
                    "a_per_node_source_or_an_input_asks_for_the_job",
                    a_per_node_source_or_an_input_asks_for_the_job as fn(),
                ),
                (
                    "other_sources_leave_it_to_main",
                    other_sources_leave_it_to_main as fn(),
                ),
                (
                    "each_root_and_input_exists",
                    each_root_and_input_exists as fn(),
                ),
            ],
        );
    }
}
