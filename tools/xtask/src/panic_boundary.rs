//! `cargo xtask panic-boundary`: a panic in FLUI's code never unwinds into the
//! operating system (ADR-0151 §16).
//!
//! An unwind out of a function the OS calls aborts the process with nothing of
//! the framework's diagnostics (`extern "C"` and `extern "system"`), or runs
//! through frames that were never written to be unwound, such as AppKit's
//! (`extern "C-unwind"`, which objc2's methods and blocks use). A missed
//! boundary is invisible in review, so this gate finds every entry the OS has
//! into `flui-native` and checks that each goes through the one boundary,
//! `owner::panic_boundary::os_entry`.
//!
//! **Entries.** In the crate's non-test code ([`scan`] follows its module tree
//! the way rustc does; a module or item under a cfg that is false in every
//! build, `test`, is left out), an entry is:
//!
//! - a function with a foreign ABI (`extern "C"`, `"system"`, `"C-unwind"`,
//!   …): a Win32 window procedure or enumeration callback, a JNI native or
//!   `Java_*` export, an Objective-C method a `ClassBuilder` adds, an NDK
//!   callback;
//! - a method of `define_class!` marked `#[unsafe(method(…))]` or
//!   `#[unsafe(method_id(…))]`, which objc2 turns into an `extern "C-unwind"`
//!   function;
//! - a method of an `impl I…_Impl for …`, windows-rs's interface traits: a COM
//!   vtable entry;
//! - a method of an AccessKit handler (`ActionHandler`, `ActivationHandler`,
//!   `DeactivationHandler`), which the adapters call from inside their own UIA,
//!   NSAccessibility and AT-SPI entries;
//! - a closure handed to the OS: an Objective-C block (`RcBlock::new`,
//!   `StackBlock::new`), a WinRT delegate (`TypedEventHandler::new` and the
//!   other delegate types of `windows::Foundation`), a GCD work item
//!   (`exec_async`, `exec_sync`, `exec_after`, `after`, `barrier_async`,
//!   `barrier_sync`; the crate keeps these method names for GCD). The
//!   constructor or method must be given the closure literal, so its body is
//!   in view.
//!
//! **The rule.** An entry goes through the boundary when its body is one
//! expression, a call of `os_entry` (written `…panic_boundary::os_entry`, or
//! `os_entry` imported from it), or a call of an *adapter*: a function of the
//! same file, defined there once, whose own body is that call, called as
//! `name(…)`, `Self::name(…)` or `self.name(…)`. Nothing runs before the call
//! or after it, and its arguments, which are evaluated outside the boundary,
//! are each a literal, a path, a closure, a reference, field, cast or
//! parenthesis of one, or `AfterPanic::Answer(…)` of one. `os_entry` is
//! defined once, in `owner::panic_boundary`; any other item, module or import
//! under the name `os_entry` or `panic_boundary` is a finding, so the name the
//! rule reads is the function. There is no allowlist: an entry passes or the
//! gate fails.
//!
//! **Out of scope, by kind.**
//!
//! - `wasm_bindgen` closures: every wasm32 build aborts on panic, so nothing
//!   unwinds into the page; a panic traps the instance, and `catch_unwind` has
//!   nothing to catch there.
//! - winit's `ApplicationHandler` and android-activity's glue: those crates
//!   hold the boundary of their own procedures (winit resumes the panic in
//!   `run_app`, on the Rust side; android-activity aborts with a log).
//! - Test code: a stub's panic aborts the test process, which fails the run.
//! - Code a dependency's proc-macro or macro generates: only the tokens written
//!   in this crate are read. `define_class!` is parsed; in any other macro an
//!   `extern` function definition, a block or delegate constructor or a GCD
//!   method is a finding, since its body cannot be read there.
//!
//! `--self-test` runs the rule over a built-in crate with planted violations
//! and fails unless it reports exactly those (ADR-0078 §4); `--list` prints
//! every entry and how it reaches the boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use std::process::ExitCode;

use crate::source_tree::{Disk, Source, Target, walk};
use crate::util;

mod fixture;
mod scan;
#[cfg(test)]
mod tests;

use scan::{BOUNDARY_MODULE, Kind, Verdict};

/// The crate whose entries the gate reads: its library root.
const ROOT: &str = "crates/flui-native/src/lib.rs";

/// Arguments for `cargo xtask panic-boundary`.
#[derive(Debug, clap::Args)]
pub(crate) struct PanicBoundaryArgs {
    /// Run the rule over a built-in crate with planted violations instead of
    /// the workspace; exit 1 unless it reports exactly those.
    #[arg(long)]
    self_test: bool,
    /// Print every entry, where it is, and how it reaches the boundary.
    #[arg(long, conflicts_with = "self_test")]
    list: bool,
}

/// `cargo xtask panic-boundary`. Exit code 1 lists every finding.
pub(crate) fn panic_boundary(args: &PanicBoundaryArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let report = check(&Disk(util::repo_root()), ROOT)?;
    if args.list {
        for entry in &report.entries {
            println!("{entry}");
        }
    }
    if report.findings.is_empty() {
        println!("panic-boundary: {}", report.summary());
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("panic-boundary: {} problem(s)", report.findings.len());
    for finding in &report.findings {
        eprintln!("  - {finding}");
    }
    Ok(ExitCode::FAILURE)
}

/// One problem the gate reports.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    file: String,
    line: usize,
    /// The entry, or the item an identity rule is about.
    what: String,
    kind: &'static str,
    detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} — {}: {}",
            self.file, self.line, self.what, self.kind, self.detail
        )
    }
}

/// An entry whose body is not one call of the boundary or of an adapter.
const UNGUARDED: &str = "unguarded";
/// An argument of that call that runs before the boundary does.
const ARGUMENT: &str = "argument";
/// A second `os_entry` or `panic_boundary`, an import of either from
/// elsewhere, or a missing boundary.
const IDENTITY: &str = "identity";
/// An entry the gate cannot read: in macro tokens, or an unparsable
/// `define_class!`.
const UNREAD: &str = "unread";

/// One entry and how it reaches the boundary, for `--list`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Listed {
    file: String,
    line: usize,
    kind: Kind,
    name: String,
    /// `None` when it does not reach it.
    through: Option<String>,
}

impl fmt::Display for Listed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} `{}` — {}",
            self.file,
            self.line,
            self.kind,
            self.name,
            self.through
                .as_deref()
                .unwrap_or("NOT THROUGH THE BOUNDARY")
        )
    }
}

/// What one run found.
#[derive(Debug, Default)]
struct Report {
    findings: Vec<Finding>,
    entries: Vec<Listed>,
    /// `(file, name)` of every adapter an entry calls.
    adapters: BTreeSet<(String, String)>,
    files: usize,
}

impl Report {
    fn summary(&self) -> String {
        let mut by_kind: BTreeMap<Kind, usize> = BTreeMap::new();
        for entry in &self.entries {
            *by_kind.entry(entry.kind).or_default() += 1;
        }
        let kinds: Vec<String> = Kind::ALL
            .iter()
            .map(|kind| format!("{} {kind}s", by_kind.get(kind).copied().unwrap_or(0)))
            .collect();
        let via_adapters = self
            .entries
            .iter()
            .filter(|entry| {
                entry
                    .through
                    .as_deref()
                    .is_some_and(|through| through.starts_with("adapter"))
            })
            .count();
        format!(
            "{} entries ({}) go through `{}::os_entry`, {via_adapters} of them by {} adapters; \
             {} files parsed",
            self.entries.len(),
            kinds.join(", "),
            BOUNDARY_MODULE.join("::"),
            self.adapters.len(),
            self.files
        )
    }
}

/// Reads the crate at `root` (a library root, repository-relative) and judges
/// each entry.
fn check(source: &dyn Source, root: &str) -> anyhow::Result<Report> {
    let target = Target {
        src: root.to_owned(),
        bin: None,
    };
    let files = walk(source, &target)?;
    let mut report = Report {
        files: files.len(),
        ..Report::default()
    };
    let mut boundary = None;
    for file in &files {
        let is_boundary = file.module == BOUNDARY_MODULE;
        if is_boundary {
            boundary = Some(file.rel.clone());
        } else if file.module.last().map(String::as_str) == Some(scan::MODULE) {
            report.findings.push(Finding {
                file: file.rel.clone(),
                line: 1,
                what: format!("mod {}", scan::MODULE),
                kind: IDENTITY,
                detail: format!(
                    "a second module named `{}`; the boundary is `{}`",
                    scan::MODULE,
                    BOUNDARY_MODULE.join("::")
                ),
            });
        }
        let scanned = scan::scan_file(file, is_boundary);
        report.findings.extend(scanned.findings.iter().cloned());
        if is_boundary && !scanned.defines_boundary {
            report.findings.push(Finding {
                file: file.rel.clone(),
                line: 1,
                what: format!("fn {}", scan::BOUNDARY),
                kind: IDENTITY,
                detail: "the boundary module does not define the boundary at its top level"
                    .to_owned(),
            });
        }
        for (entry, verdict) in scanned.judged() {
            let through = match &verdict {
                Verdict::Boundary => Some("os_entry".to_owned()),
                Verdict::Adapter(name) => {
                    report.adapters.insert((file.rel.clone(), name.clone()));
                    Some(format!("adapter `{name}`"))
                }
                Verdict::Unguarded(_) | Verdict::Argument(_) => None,
            };
            let finding = |kind, detail: &str| Finding {
                file: file.rel.clone(),
                line: entry.line,
                what: entry.name.clone(),
                kind,
                detail: format!("{} {}", entry.kind, detail),
            };
            match &verdict {
                Verdict::Unguarded(why) => report.findings.push(finding(UNGUARDED, why)),
                Verdict::Argument(why) => report.findings.push(finding(ARGUMENT, why)),
                Verdict::Boundary | Verdict::Adapter(_) => {}
            }
            report.entries.push(Listed {
                file: file.rel.clone(),
                line: entry.line,
                kind: entry.kind,
                name: entry.name.clone(),
                through,
            });
        }
    }
    if boundary.is_none() {
        report.findings.push(Finding {
            file: root.to_owned(),
            line: 1,
            what: format!("mod {}", BOUNDARY_MODULE.join("::")),
            kind: IDENTITY,
            detail: "the crate has no boundary module".to_owned(),
        });
    }
    report.findings.sort();
    report.entries.sort();
    Ok(report)
}

// ---------------------------------------------------------------------------
// self-test

/// `(file, what, kind)`: what the self-test compares.
type Identity = (String, String, String);

/// `(missed, false positives)` of the rule over the fixture crate.
fn self_test_diff() -> (Vec<Identity>, Vec<Identity>) {
    let report = check(&fixture::source(), fixture::ROOT)
        .expect("BUG: the self-test crate's module tree resolves and parses");
    let seen: BTreeSet<Identity> = report
        .findings
        .iter()
        .map(|finding| {
            (
                finding.file.clone(),
                finding.what.clone(),
                finding.kind.to_owned(),
            )
        })
        .collect();
    let expected: BTreeSet<Identity> = fixture::EXPECTED
        .iter()
        .map(|&(file, what, kind)| (file.to_owned(), what.to_owned(), kind.to_owned()))
        .collect();
    (
        expected.difference(&seen).cloned().collect(),
        seen.difference(&expected).cloned().collect(),
    )
}

/// `cargo xtask panic-boundary --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra) = self_test_diff();
    for (file, what, kind) in &missed {
        println!("self-test: MISSED ({file}, {what}, {kind})");
    }
    for (file, what, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({file}, {what}, {kind})");
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "panic-boundary: self-test ok ({} expected findings, no others)",
        fixture::EXPECTED.len()
    );
    ExitCode::SUCCESS
}
