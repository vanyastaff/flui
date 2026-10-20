//! The real `cargo xtask` binary prints the plan a task would run.
//!
//! A separate target: it spawns the built binary, which only an integration
//! test can name (`CARGO_BIN_EXE_xtask`).

use std::process::Command;

/// Dependencies can compile a library without checking its target-specific
/// tests, examples and benches. The public plan must select those packages
/// directly, including packages outside the platform backend list.
#[test]
fn native_typecheck_selects_target_gated_workspace_targets() {
    for (all_features, only) in [
        (false, None),
        (true, Some("aarch64-apple-darwin")),
        (true, Some("aarch64-linux-android")),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.args(["cross-typecheck", "--dry-run"]);
        if all_features {
            command.arg("--all-features");
        }
        if let Some(target) = only {
            command.args(["--target", target]);
        }
        let output = command
            .output()
            .expect("run the public cross-typecheck plan");
        assert!(output.status.success(), "{output:?}");
        let plan = String::from_utf8(output.stdout).expect("UTF-8 command plan");
        if only == Some("aarch64-apple-darwin") && !cfg!(target_os = "macos") {
            assert!(
                plan.contains("Skipping all-features macOS") && plan.contains("Apple SDK"),
                "{plan}"
            );
            assert!(
                !plan.lines().any(|line| line.starts_with("$ ")),
                "an SDK-only leg must not run: {plan}"
            );
            continue;
        }
        for (package, target) in [
            ("flui-engine", "aarch64-apple-darwin"),
            ("flui-widgets", "aarch64-apple-darwin"),
            ("flui-hot-reload", "x86_64-pc-windows-msvc"),
            ("flui-platform", "aarch64-linux-android"),
        ] {
            if only.is_some_and(|only| only != target) {
                continue;
            }
            assert!(
                plan.lines().any(|line| {
                    let args: Vec<_> = line.split_whitespace().collect();
                    args.windows(2).any(|pair| pair == ["-p", package])
                        && args.windows(2).any(|pair| pair == ["--target", target])
                        && args.contains(&"--all-targets")
                }),
                "missing direct all-targets coverage for {package} on {target}:\n{plan}"
            );
        }
        if all_features {
            let commands: Vec<_> = plan.lines().filter(|line| line.starts_with("$ ")).collect();
            assert_eq!(
                commands.len(),
                1,
                "--target must select only the requested native leg"
            );
            assert!(
                commands[0]
                    .split_whitespace()
                    .any(|arg| arg == "--all-features"),
                "optional native features must reach Cargo: {plan}"
            );
        }
    }
}
