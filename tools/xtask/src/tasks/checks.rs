//! `cargo xtask checks`: the generated files against their sources and the
//! changelog fragments. rustfmt, typos, taplo, lychee and the shell checks run as steps of CI's
//! `checks` job beside it.
//!
//! Every check runs even when an earlier one failed, so one run reports every
//! problem; each runs in this process.

use std::process::ExitCode;

use anyhow::bail;

use super::exec::{Runner, parsed};
use crate::{
    android_dex, changelog, fonts, icon_import, key_vocabulary, platform_floor,
    recording_vocabulary,
};

/// Runs every check; an error lists the ones that failed.
pub(super) fn run(runner: Runner) -> anyhow::Result<()> {
    let mut failed: Vec<&str> = Vec::new();
    for (command, check) in IN_PROCESS {
        let args: Vec<&str> = command.split(' ').skip(1).collect();
        if let Err(error) = runner.in_process(command, || check(&args)) {
            eprintln!("checks: {command}: {error:#}");
            failed.push(command);
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    bail!("checks: {} failed: {}", failed.len(), failed.join(", "))
}

/// One of this crate's commands, handed the arguments of its command line.
type InProcess = fn(&[&str]) -> anyhow::Result<ExitCode>;

/// This crate's own checks, in the order they run: each `cargo xtask`
/// command line and the command it names. A gate's `--self-test` is not
/// here: it is a test of this crate
/// (`every_self_test_reports_exactly_its_planted_findings`), so
/// `cargo test -p xtask` runs it (ADR-0303 §4).
const IN_PROCESS: [(&str, InProcess); 7] = [
    // `--package-list`: Cargo's file selection for flui-painting, listed
    // offline, no archive built
    ("font-assets --package-list", |args| {
        fonts::font_assets(&parsed(args)?)
    }),
    // the Material animated icon constants against their committed key frames
    ("icon-import material", |args| {
        icon_import::icon_import(&parsed(args)?)
    }),
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
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::{CommandFactory as _, Parser as _};

    use super::*;

    /// Every gate's self-test: the gate run over its planted violations,
    /// failing unless exactly the planted findings come back (ADR-0078 §4).
    const SELF_TESTS: [(&str, InProcess); 2] = [
        ("key-vocabulary", |args| {
            key_vocabulary::key_vocabulary(&parsed(args)?)
        }),
        ("changelog", |args| changelog::changelog(&parsed(args)?)),
    ];

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
            .map(str::to_owned)
            .collect();
        assert_eq!(with_flag, listed);
    }

    fn in_process_arguments_parse_like_the_command_line() {
        let args: key_vocabulary::KeyVocabularyArgs = parsed(["--self-test"]).expect("parses");
        assert_eq!(
            key_vocabulary::key_vocabulary(&args).expect("runs"),
            ExitCode::SUCCESS
        );
        let error =
            parsed::<key_vocabulary::KeyVocabularyArgs>(["--no-such-flag"]).expect_err("rejected");
        assert!(error.to_string().contains("--no-such-flag"), "{error}");
    }

    /// The checks `cargo xtask checks` runs, each printed as the command line
    /// that runs it, and none a `--self-test` (that is a test of this crate).
    fn every_in_process_check_is_its_command_line() {
        let lines = IN_PROCESS.map(|(line, _)| line);
        assert_eq!(
            lines,
            [
                "font-assets --package-list",
                "icon-import material",
                "key-vocabulary",
                "recording-vocabulary",
                "android-dex",
                "platform-floor",
                "changelog --check",
            ]
        );
        assert!(!lines.iter().any(|line| line.contains("--self-test")));
        for line in lines {
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
                    "every_in_process_check_is_its_command_line",
                    every_in_process_check_is_its_command_line as fn(),
                ),
            ],
        );
    }
}
