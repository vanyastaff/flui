//! Windows cannot replace the executable that is driving a workspace test run.

#![cfg(windows)]

use std::process::Command;

fn assert_plan(stage: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["test", "--dry-run"])
        .args(stage)
        .output()
        .expect("run the public test task");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 task plan");
    let commands: Vec<_> = stdout
        .lines()
        .filter(|line| line.starts_with("$ cargo nextest run"))
        .collect();
    let workspace = commands
        .iter()
        .position(|line| line.contains("--workspace"))
        .expect("workspace tests remain scheduled");
    assert!(
        commands[workspace].contains("--exclude xtask"),
        "the workspace can overwrite the running driver: {}",
        commands[workspace]
    );
    assert!(
        commands[..workspace].iter().any(|line| {
            line.contains("-p xtask") && line.contains("--bins") && line.contains("--tests")
        }),
        "driver unit and integration tests must run before feature unification: {commands:?}"
    );
}

#[test]
fn workspace_tests_preserve_driver_coverage_without_rebuilding_the_running_binary() {
    for stage in [
        &[][..],
        &["--fast"][..],
        &["--nested"][..],
        &["--no-trybuild"][..],
    ] {
        assert_plan(stage);
    }
}

#[test]
fn platform_compiler_checks_follow_the_selected_test_stage() {
    for (stage, compiler_runs) in [
        (&[][..], 1),
        (&["--fast"][..], 0),
        (&["--nested"][..], 1),
        (&["--nested", "--nested-group", "trybuild"][..], 1),
        (&["--nested", "--nested-group", "native"][..], 0),
        (&["--no-trybuild"][..], 0),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["test", "--dry-run"])
            .args(stage)
            .output()
            .expect("run the public test task");
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 task plan");
        let commands: Vec<_> = stdout
            .lines()
            .filter(|line| line.starts_with('$'))
            .collect();
        let compiler = commands
            .iter()
            .filter(|line| line.contains("-p flui-platform --test compiler_guards"))
            .count();
        assert_eq!(compiler, compiler_runs, "{stage:?}: {stdout}");
        for native in commands.iter().filter(|line| {
            line.contains("-p flui-platform") && !line.contains("--test compiler_guards")
        }) {
            assert!(
                native.contains("-E 'not (group(trybuild))'"),
                "compiler checks must not run in the native leg: {native}"
            );
        }
    }
}
