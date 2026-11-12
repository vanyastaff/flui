//! `cargo xtask checks`: the source checks that compile nothing.
//!
//! Every check runs even when an earlier one failed, so one run reports every
//! problem; the checks this crate implements run in this process.

use std::process::ExitCode;

use anyhow::bail;

use super::exec::{Cmd, Runner, installed, parsed};
use crate::{
    android_dex, change_scope, changelog, docs_links, docs_paths, file_length, fonts, globals,
    icon_import, key_vocabulary, markers, module_dag, panic_boundary, panic_messages,
    platform_floor, recording_vocabulary, send_impls, testing, toolchain, try_new, wgsl, workspace,
};

/// A formatter or linter that is a binary of its own, not a cargo step.
#[derive(Debug, Clone, Copy)]
struct Tool {
    /// The program.
    program: &'static str,
    /// Its arguments for the check.
    args: &'static [&'static str],
    /// The crate that installs it.
    install: &'static str,
    /// Environment for the run.
    env: &'static [(&'static str, &'static str)],
}

/// Spelling (typos) and TOML formatting (taplo). A contributor without them
/// can still run the gate: the check is skipped with a message unless
/// `--strict` (CI installs both, so a skip is a slower loop, never a hole).
const TOOLS: [Tool; 2] = [
    Tool {
        program: "typos",
        args: &[],
        install: "typos-cli",
        env: &[],
    },
    Tool {
        program: "taplo",
        args: &["fmt", "--check"],
        install: "taplo-cli",
        // taplo logs every file it formats at INFO; only problems matter here.
        env: &[("RUST_LOG", "warn")],
    },
];

/// Runs every check; an error lists the ones that failed.
pub(super) fn run(runner: Runner, strict: bool) -> anyhow::Result<()> {
    let mut failed: Vec<String> = Vec::new();
    let mut record = |name: &str, result: anyhow::Result<()>| {
        if let Err(error) = result {
            eprintln!("checks: {name}: {error:#}");
            failed.push(name.to_owned());
        }
    };

    record(
        "fmt",
        runner.run(&Cmd::cargo(["fmt", "--all", "--", "--check"])),
    );
    for tool in TOOLS {
        if runner.dry_run || installed(tool.program, &["--version"]) {
            record(
                tool.program,
                runner.run(&tool.env.iter().fold(
                    Cmd::new(tool.program).args(tool.args),
                    |cmd, (key, value)| cmd.env(*key, *value),
                )),
            );
        } else if strict {
            record(
                tool.program,
                Err(anyhow::anyhow!(
                    "not installed (cargo install {}), and --strict makes that a failure",
                    tool.install
                )),
            );
        } else {
            println!(
                "{}: not installed, skipped (cargo install {}; CI runs it)",
                tool.program, tool.install
            );
        }
    }
    for (command, check) in in_process(strict) {
        let args: Vec<&str> = command.split(' ').skip(1).collect();
        record(command, runner.in_process(command, || check(&args)));
    }

    if failed.is_empty() {
        return Ok(());
    }
    bail!("checks: {} failed: {}", failed.len(), failed.join(", "))
}

/// One of this crate's commands, handed the arguments of its command line.
type InProcess = fn(&[&str]) -> anyhow::Result<ExitCode>;

/// This crate's own checks, in the order they run: each `cargo xtask`
/// command line and the command it names. lychee is skippable like
/// [`TOOLS`], so `--strict` reaches `docs-links` too. A gate's
/// `--self-test` is not here: it is a test of this crate
/// (`every_self_test_reports_exactly_its_planted_findings`), so
/// `cargo test -p xtask` runs it (ADR-0303 §4).
fn in_process(strict: bool) -> [(&'static str, InProcess); 23] {
    [
        (
            if strict {
                "docs-links --strict"
            } else {
                "docs-links"
            },
            |args| docs_links::docs_links(&parsed(args)?),
        ),
        ("docs-paths", |args| docs_paths::docs_paths(&parsed(args)?)),
        ("workspace", |args| workspace::workspace(&parsed(args)?)),
        ("reach", |args| workspace::reach(&parsed(args)?)),
        ("module-dag", |args| module_dag::module_dag(&parsed(args)?)),
        ("toolchain", |args| toolchain::toolchain(&parsed(args)?)),
        ("wgsl", |args| wgsl::wgsl(&parsed(args)?)),
        ("globals", |args| globals::globals(&parsed(args)?)),
        ("panic-boundary", |args| {
            panic_boundary::panic_boundary(&parsed(args)?)
        }),
        ("send-impls", |args| send_impls::send_impls(&parsed(args)?)),
        ("try-new", |args| try_new::try_new(&parsed(args)?)),
        ("panic-messages", |args| {
            panic_messages::panic_messages(&parsed(args)?)
        }),
        ("paths-filter", |args| {
            change_scope::paths_filter(&parsed(args)?)
        }),
        // `--package-list`: Cargo's file selection for flui-painting, listed
        // offline, no archive built
        ("font-assets --package-list", |args| {
            fonts::font_assets(&parsed(args)?)
        }),
        // the Material animated icon constants against their committed key frames
        ("icon-import material", |args| {
            icon_import::icon_import(&parsed(args)?)
        }),
        ("file-length", |args| {
            file_length::file_length(&parsed(args)?)
        }),
        ("markers", |args| markers::markers(&parsed(args)?)),
        ("testing", |args| testing::testing(&parsed(args)?)),
        ("key-vocabulary", |args| {
            key_vocabulary::key_vocabulary(&parsed(args)?)
        }),
        ("recording-vocabulary", |args| {
            recording_vocabulary::recording_vocabulary(&parsed(args)?)
        }),
        ("android-dex", |args| {
            android_dex::android_dex(&parsed(args)?)
        }),
        ("platform-floor", |args| {
            platform_floor::platform_floor(&parsed(args)?)
        }),
        ("changelog --check", |args| {
            changelog::changelog(&parsed(args)?)
        }),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::{CommandFactory as _, Parser as _};

    use super::*;
    use crate::{perf, public_api, send_positions};

    /// Every gate's self-test: the gate run over its planted violations,
    /// failing unless exactly the planted findings come back (ADR-0078 §4).
    const SELF_TESTS: [(&str, InProcess); 17] = [
        ("workspace", |args| workspace::workspace(&parsed(args)?)),
        ("reach", |args| workspace::reach(&parsed(args)?)),
        ("module-dag", |args| module_dag::module_dag(&parsed(args)?)),
        ("wgsl", |args| wgsl::wgsl(&parsed(args)?)),
        ("globals", |args| globals::globals(&parsed(args)?)),
        ("panic-boundary", |args| {
            panic_boundary::panic_boundary(&parsed(args)?)
        }),
        ("send-impls", |args| send_impls::send_impls(&parsed(args)?)),
        ("send-positions", |args| {
            send_positions::send_positions(&parsed(args)?)
        }),
        ("public-api", |args| public_api::public_api(&parsed(args)?)),
        ("try-new", |args| try_new::try_new(&parsed(args)?)),
        ("panic-messages", |args| {
            panic_messages::panic_messages(&parsed(args)?)
        }),
        ("file-length", |args| {
            file_length::file_length(&parsed(args)?)
        }),
        ("markers", |args| markers::markers(&parsed(args)?)),
        ("testing", |args| testing::testing(&parsed(args)?)),
        ("key-vocabulary", |args| {
            key_vocabulary::key_vocabulary(&parsed(args)?)
        }),
        ("perf", |args| perf::perf(&parsed(args)?)),
        ("changelog", |args| changelog::changelog(&parsed(args)?)),
    ];

    /// The self-tests that need more than this crate: `api-closure` reads the
    /// nightly rustdoc JSON of the workspace, so CI's `api-contracts` job
    /// runs it beside the gate.
    const SELF_TESTS_ELSEWHERE: [&str; 1] = ["api-closure"];

    fn every_self_test_reports_exactly_its_planted_findings() {
        for (command, self_test) in SELF_TESTS {
            match self_test(&["--self-test"]) {
                Ok(code) => assert_eq!(
                    code,
                    ExitCode::SUCCESS,
                    "`cargo xtask {command} --self-test` failed"
                ),
                Err(error) => panic!("`cargo xtask {command} --self-test`: {error:#}"),
            }
        }
    }

    fn every_command_with_a_self_test_is_listed() {
        let with_flag: BTreeSet<String> = crate::Cli::command()
            .get_subcommands()
            .filter(|command| {
                command
                    .get_arguments()
                    .any(|argument| argument.get_long() == Some("self-test"))
            })
            .map(|command| command.get_name().to_owned())
            .collect();
        let listed: BTreeSet<String> = SELF_TESTS
            .iter()
            .map(|(command, _)| *command)
            .chain(SELF_TESTS_ELSEWHERE)
            .map(str::to_owned)
            .collect();
        assert_eq!(with_flag, listed);
    }

    fn in_process_arguments_parse_like_the_command_line() {
        let args: wgsl::WgslArgs = parsed(["--self-test"]).expect("parses");
        assert_eq!(wgsl::wgsl(&args).expect("runs"), ExitCode::SUCCESS);
        let error = parsed::<wgsl::WgslArgs>(["--no-such-flag"]).expect_err("rejected");
        assert!(error.to_string().contains("--no-such-flag"), "{error}");
    }

    fn the_in_process_checks_include_the_link_check_under_strict() {
        let lines = |strict| in_process(strict).map(|(line, _)| line);
        assert_eq!(
            lines(false),
            [
                "docs-links",
                "docs-paths",
                "workspace",
                "reach",
                "module-dag",
                "toolchain",
                "wgsl",
                "globals",
                "panic-boundary",
                "send-impls",
                "try-new",
                "panic-messages",
                "paths-filter",
                "font-assets --package-list",
                "icon-import material",
                "file-length",
                "markers",
                "testing",
                "key-vocabulary",
                "recording-vocabulary",
                "android-dex",
                "platform-floor",
                "changelog --check",
            ]
        );
        assert_eq!(lines(true)[0], "docs-links --strict");
        assert_eq!(lines(true)[1..], lines(false)[1..]);
        // each is the command line it is printed as
        for line in lines(false).into_iter().chain(lines(true)) {
            let argv = std::iter::once("cargo xtask").chain(line.split(' '));
            if let Err(error) = crate::Cli::try_parse_from(argv) {
                panic!("`cargo xtask {line}`: {error}");
            }
        }
    }

    #[test]
    fn checks_contract() {
        crate::table_test::run_table(
            "checks_contract",
            &[
                (
                    "every_self_test_reports_exactly_its_planted_findings",
                    every_self_test_reports_exactly_its_planted_findings as fn(),
                ),
                (
                    "every_command_with_a_self_test_is_listed",
                    every_command_with_a_self_test_is_listed as fn(),
                ),
                (
                    "in_process_arguments_parse_like_the_command_line",
                    in_process_arguments_parse_like_the_command_line as fn(),
                ),
                (
                    "the_in_process_checks_include_the_link_check_under_strict",
                    the_in_process_checks_include_the_link_check_under_strict as fn(),
                ),
            ],
        );
    }
}
