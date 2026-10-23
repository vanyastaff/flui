//! `cargo xtask panic-boundary`: a panic in FLUI's code never leaves through
//! the operating system (ADR-0151 §16).
//!
//! Out of an `extern "C"` or `extern "system"` frame a panic aborts the
//! process (since Rust 1.81; before, it was undefined behaviour), with
//! nothing in the framework's diagnostics; out of an `extern "C-unwind"`
//! frame, which objc2's methods and blocks use, it unwinds through AppKit's
//! or libdispatch's frames, which were not written to be unwound. Containment
//! buys a logged failure and a sane answer to the OS instead. A missed
//! boundary is invisible in review, so this gate finds every entry the OS has
//! into `flui-native` and checks that each goes through the one boundary,
//! `owner::panic_boundary::os_entry`.
//!
//! **Entries.** In the crate's non-test code ([`crate::source_tree`] follows
//! its module tree the way rustc does; a module or item under a cfg that is
//! false in every build, `test`, is left out), an entry is:
//!
//! - a function with a foreign ABI (`extern "C"`, `"system"`, `"C-unwind"`,
//!   …), free, a method or a trait's default: a Win32 window procedure or
//!   enumeration callback, a JNI native or `Java_*` export, an Objective-C
//!   method a `ClassBuilder` adds, an NDK callback;
//! - a method of `define_class!` marked `#[unsafe(method(…))]` or
//!   `#[unsafe(method_id(…))]`, which objc2 turns into an `extern "C-unwind"`
//!   function;
//! - a method of an `impl I…_Impl for …`, windows-rs's interface traits: a COM
//!   vtable entry;
//! - a method of an AccessKit handler (`ActionHandler`, `ActivationHandler`,
//!   `DeactivationHandler`), which the adapters call from inside their own UIA,
//!   NSAccessibility and AT-SPI entries;
//! - a closure handed to the OS: any closure literal given to a function or
//!   constructor of an OS binding crate (`windows`, `objc2*`, `block2`,
//!   `dispatch*`, `jni`, `ndk`, `android_activity`, …, reached by its path or
//!   through a name the file imports from it), such as an Objective-C block or
//!   a WinRT delegate; a block or delegate constructor matched by name where a
//!   glob import hides it; and a GCD work item (`exec_async`, `exec_sync`,
//!   `exec_after`, `after`, `barrier_async`, `barrier_sync`), whose body must
//!   move its captures into the boundary, so they drop inside it. A named
//!   constructor or method must be given the closure literal, so its body is
//!   in view.
//!
//! **The rule.** An entry goes through the boundary when its body is one
//! expression: a constant (a literal or a path, which runs no code), a call of
//! `os_entry` (written `…panic_boundary::os_entry`, or `os_entry` imported
//! from it), or a call of an *adapter* whose own body is that call: a free
//! function of the same file, defined once at module level and not shadowed by
//! an import, called as `name(…)`, or a method of the same file defined once
//! on `Self` or a named type, called as `Self::name(…)` or `Type::name(…)`. A
//! method call (`self.name(…)`) resolves by its receiver's type and is no
//! adapter. Nothing runs before the call or after it, and its arguments, which
//! are evaluated outside the boundary, are each a literal, a path, a closure,
//! a tuple, reference, field, cast or parenthesis of those, or
//! `AfterPanic::Answer(…)` of one. `os_entry` is defined once, in
//! `owner::panic_boundary`; any other item, module or import under the name
//! `os_entry` or `panic_boundary` is a finding, so the name the rule reads is
//! the function. There is no allowlist: an entry passes or the gate fails.
//!
//! **Limits.** Functions and constructors of OS crates are caught by their
//! path; methods are not, since a receiver's type is out of the gate's
//! reach, so a new OS method that takes a closure must be added to the GCD
//! list, and a constructor reached through a glob import to the list of
//! callback types. A block's or delegate's captures live as long as the OS
//! keeps the block and drop when it releases it, outside any call: they are
//! kept to values whose destructor runs no user code.
//!
//! **Out of scope, by kind.**
//!
//! - `wasm_bindgen` closures: every wasm32 build aborts on panic, so nothing
//!   unwinds into the page; a panic traps the instance, and `catch_unwind` has
//!   nothing to catch there.
//! - winit's `ApplicationHandler` and android-activity's glue: those crates
//!   hold the boundary of their own procedures (winit resumes the panic in
//!   `run_app`, on the Rust side; android-activity aborts with a log).
//! - `flui-hot-reload`'s plugin ABI (`flui_scene_build`, `flui_app_build`):
//!   entries a host process calls in a plugin, outside `flui-native`.
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

/// What is wrong with an entry or an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Problem {
    /// An entry whose body is not a constant or one call of the boundary or
    /// of an adapter.
    Unguarded,
    /// An argument of that call that runs before the boundary does, or a
    /// work item's body that does not take its captures in.
    Argument,
    /// A second `os_entry` or `panic_boundary`, an import of either from
    /// elsewhere, or a missing boundary.
    Identity,
    /// An entry the gate cannot read: in macro tokens, or an unparsable
    /// `define_class!`.
    Unread,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unguarded => "unguarded",
            Self::Argument => "argument",
            Self::Identity => "identity",
            Self::Unread => "unread",
        })
    }
}

/// One problem the gate reports.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    file: String,
    line: usize,
    /// The entry, or the item an identity rule is about.
    what: String,
    kind: Problem,
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
        let through = |what: &str| {
            self.entries
                .iter()
                .filter(|entry| {
                    entry
                        .through
                        .as_deref()
                        .is_some_and(|through| through.starts_with(what))
                })
                .count()
        };
        format!(
            "{} entries ({}): {} through `{}::os_entry`, {} of them by {} adapters, {} constant; \
             {} files parsed",
            self.entries.len(),
            kinds.join(", "),
            self.entries.len() - through("constant"),
            BOUNDARY_MODULE.join("::"),
            through("adapter"),
            self.adapters.len(),
            through("constant"),
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
                kind: Problem::Identity,
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
                kind: Problem::Identity,
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
                Verdict::Constant => Some("constant".to_owned()),
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
                Verdict::Unguarded(why) => report.findings.push(finding(Problem::Unguarded, why)),
                Verdict::Argument(why) => report.findings.push(finding(Problem::Argument, why)),
                Verdict::Boundary | Verdict::Adapter(_) | Verdict::Constant => {}
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
            kind: Problem::Identity,
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
type Identity = (String, String, Problem);

/// `(missed, false positives)` of the rule over the fixture crate.
fn self_test_diff() -> (Vec<Identity>, Vec<Identity>) {
    let report = check(&fixture::source(), fixture::ROOT)
        .expect("BUG: the self-test crate's module tree resolves and parses");
    let seen: BTreeSet<Identity> = report
        .findings
        .iter()
        .map(|finding| (finding.file.clone(), finding.what.clone(), finding.kind))
        .collect();
    let expected: BTreeSet<Identity> = fixture::EXPECTED
        .iter()
        .map(|&(file, what, kind)| (file.to_owned(), what.to_owned(), kind))
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
