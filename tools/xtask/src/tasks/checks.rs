//! `cargo xtask checks`: the source checks that compile nothing.
//!
//! Every check runs even when an earlier one failed, so one run reports every
//! problem; the checks this crate implements run in this process.

use std::process::ExitCode;

use anyhow::bail;

use super::exec::{Cmd, Runner, installed, parsed};
use crate::{change_scope, docs_links, fonts, globals, toolchain, wgsl, workspace};

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
/// [`TOOLS`], so `--strict` reaches `docs-links` too.
fn in_process(strict: bool) -> [(&'static str, InProcess); 10] {
    [
        (
            if strict {
                "docs-links --strict"
            } else {
                "docs-links"
            },
            |args| docs_links::docs_links(&parsed(args)?),
        ),
        ("workspace --self-test", |args| {
            workspace::workspace(&parsed(args)?)
        }),
        ("workspace", |args| workspace::workspace(&parsed(args)?)),
        ("toolchain", |args| toolchain::toolchain(&parsed(args)?)),
        ("wgsl --self-test", |args| wgsl::wgsl(&parsed(args)?)),
        ("wgsl", |args| wgsl::wgsl(&parsed(args)?)),
        ("globals --self-test", |args| {
            globals::globals(&parsed(args)?)
        }),
        ("globals", |args| globals::globals(&parsed(args)?)),
        ("paths-filter", |args| {
            change_scope::paths_filter(&parsed(args)?)
        }),
        // `--package-list`: Cargo's file selection for flui-painting, listed
        // offline, no archive built
        ("font-assets --package-list", |args| {
            fonts::font_assets(&parsed(args)?)
        }),
    ]
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[test]
    fn in_process_arguments_parse_like_the_command_line() {
        let args: wgsl::WgslArgs = parsed(["--self-test"]).expect("parses");
        assert_eq!(wgsl::wgsl(&args).expect("runs"), ExitCode::SUCCESS);
        let error = parsed::<wgsl::WgslArgs>(["--no-such-flag"]).expect_err("rejected");
        assert!(error.to_string().contains("--no-such-flag"), "{error}");
    }

    #[test]
    fn the_skippable_tools_are_the_text_checks() {
        let lines: Vec<String> = TOOLS
            .iter()
            .map(|tool| Cmd::new(tool.program).args(tool.args).to_string())
            .collect();
        assert_eq!(lines, ["typos", "taplo fmt --check"]);
    }

    #[test]
    fn the_in_process_checks_include_the_link_check_under_strict() {
        let lines = |strict| in_process(strict).map(|(line, _)| line);
        assert_eq!(
            lines(false),
            [
                "docs-links",
                "workspace --self-test",
                "workspace",
                "toolchain",
                "wgsl --self-test",
                "wgsl",
                "globals --self-test",
                "globals",
                "paths-filter",
                "font-assets --package-list",
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
}
