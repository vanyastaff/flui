//! Which changed files can change what CI's `test-without-agent` job finds.
//!
//! The job runs flui-app's agent tests in a build without debug assertions,
//! the only build that leaves the driver agent out (`cfg(driver_agent)` is a
//! debug build or the `driver` feature): such a build reads no agent launch
//! value (ADR-0288 §7). No other job compiles those tests. A pull request
//! runs the job only when its diff reaches the agent's module, the build
//! script that names the cfg, or the job's own rule; main runs it on every
//! push (ADR-0303 §9).

/// The sources the job compiles the agent's launch from: the module, and the
/// build script that decides whether the agent is built.
pub(super) const COMPILED: [&str; 2] =
    ["crates/flui-app/src/app/agent/", "crates/flui-app/build.rs"];

/// The file that states what the job reads: this rule.
const THE_JOB: [&str; 1] = ["tools/xtask/src/change_scope/without_agent_scope.rs"];

/// The first of `files` the job compiles its tests from, or the file that
/// states what it reads, or `None`.
pub(super) fn changed_input(files: &[String]) -> Option<&str> {
    files
        .iter()
        .map(String::as_str)
        .find(|f| COMPILED.iter().any(|root| f.starts_with(root)) || THE_JOB.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(files: &[&str]) -> Option<String> {
        let files: Vec<String> = files.iter().map(|&f| f.to_owned()).collect();
        changed_input(&files).map(str::to_owned)
    }

    fn the_agent_or_its_build_script_asks_for_the_job() {
        for f in [
            "crates/flui-app/src/app/agent/mod.rs",
            "crates/flui-app/src/app/agent/served/launch.rs",
            "crates/flui-app/build.rs",
            "tools/xtask/src/change_scope/without_agent_scope.rs",
        ] {
            assert_eq!(input(&["README.md", f]).as_deref(), Some(f), "{f}");
        }
    }

    fn other_sources_leave_it_to_main() {
        assert_eq!(
            input(&[
                "crates/flui-app/src/app/runner/mod.rs",
                "crates/flui-app/tests/main.rs",
                "crates/flui-runtime/src/agent/mod.rs",
                "docs/testing.md",
            ]),
            None
        );
    }

    fn each_source_and_job_file_exists() {
        let root = crate::util::repo_root();
        for path in COMPILED.iter().chain(&THE_JOB) {
            assert!(root.join(path).exists(), "{path}");
        }
    }

    #[test]
    fn without_agent_inputs() {
        crate::table_test::run_table(
            "without_agent_inputs",
            &[
                (
                    "the_agent_or_its_build_script_asks_for_the_job",
                    the_agent_or_its_build_script_asks_for_the_job as fn(),
                ),
                (
                    "other_sources_leave_it_to_main",
                    other_sources_leave_it_to_main as fn(),
                ),
                (
                    "each_source_and_job_file_exists",
                    each_source_and_job_file_exists as fn(),
                ),
            ],
        );
    }
}
