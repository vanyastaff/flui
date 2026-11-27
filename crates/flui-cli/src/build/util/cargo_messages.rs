//! What a `cargo build` run with `--message-format=json…` reports on its stdout, read while
//! each message is written to the terminal as cargo would have written it: the compiler's
//! errors, the executable it built, and the library directories its build scripts link from.
//!
//! `flui run` reads them so the running app can show where its next build failed
//! (`build.report`, ADR-0395), and so it starts the executable the build wrote, as `cargo run`
//! would, without asking cargo to check the sources again. Only the compiler's errors are kept,
//! at most [`MAX_DIAGNOSTICS`](flui_protocol::build::MAX_DIAGNOSTICS) of them, and the rest
//! counted ([`Diagnostics`]).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use cargo_metadata::Message;
use cargo_metadata::diagnostic::DiagnosticLevel;
use flui_protocol::build::{Diagnostic, Diagnostics, SourceLocation};

/// What one build reported.
#[derive(Debug, Default)]
pub(crate) struct BuildMessages {
    /// The compiler's errors.
    pub(crate) errors: Diagnostics,
    /// The executable the build wrote, when it selected one (`--bin`, `--example`).
    pub(crate) executable: Option<Executable>,
    /// The directories the build scripts link native libraries from, as cargo states them
    /// (`native=…`, `dependency=…` or a bare path).
    pub(crate) linked_paths: Vec<PathBuf>,
}

/// An executable a build wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Executable {
    pub(crate) path: PathBuf,
    /// Cargo did not build it again: it is the executable the previous build wrote.
    pub(crate) fresh: bool,
}

/// Reads cargo's messages from `messages` to their end, writing each compiler message's rendered
/// text, and each line that is not a message, to `terminal`, and answers what they reported.
///
/// # Errors
///
/// Reading `messages` or writing `terminal` failed.
pub(crate) fn read(
    messages: impl BufRead,
    terminal: &mut impl Write,
) -> io::Result<BuildMessages> {
    let mut read = BuildMessages::default();
    for message in Message::parse_stream(messages) {
        match message? {
            Message::CompilerMessage(message) => {
                let diagnostic = message.message;
                if let Some(rendered) = &diagnostic.rendered {
                    terminal.write_all(rendered.as_bytes())?;
                }
                if let Some(error) = compile_error(&diagnostic) {
                    read.errors.push(error);
                }
            }
            Message::CompilerArtifact(artifact) => {
                if let Some(path) = artifact.executable {
                    read.executable = Some(Executable {
                        path: path.into_std_path_buf(),
                        fresh: artifact.fresh,
                    });
                }
            }
            Message::BuildScriptExecuted(script) => read.linked_paths.extend(
                script
                    .linked_paths
                    .into_iter()
                    .map(cargo_metadata::camino::Utf8PathBuf::into_std_path_buf),
            ),
            Message::TextLine(line) => writeln!(terminal, "{line}")?,
            _ => {}
        }
    }
    Ok(read)
}

/// `diagnostic` as the app shows it, when it is an error the author can fix: not the compiler's
/// closing count ("aborting due to 2 previous errors"), which has no place in the source.
fn compile_error(diagnostic: &cargo_metadata::diagnostic::Diagnostic) -> Option<Diagnostic> {
    if !matches!(
        diagnostic.level,
        DiagnosticLevel::Error | DiagnosticLevel::Ice
    ) {
        return None;
    }
    let primary = diagnostic.spans.iter().find(|span| span.is_primary);
    if primary.is_none() && diagnostic.message.starts_with("aborting due to") {
        return None;
    }
    let message = diagnostic
        .message
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let location = primary.map(|span| {
        SourceLocation::new(
            span.file_name.clone(),
            u32::try_from(span.line_start).unwrap_or(u32::MAX),
            u32::try_from(span.column_start).unwrap_or(u32::MAX),
        )
    });
    Some(Diagnostic::new(message, location))
}

#[cfg(test)]
mod tests {
    use flui_protocol::build::MAX_DIAGNOSTICS;

    use super::*;

    /// A compiler message line as cargo writes it with `--message-format=json`.
    fn message(level: &str, text: &str, span: Option<(&str, u32, u32)>) -> String {
        let spans: Vec<serde_json::Value> = span
            .map(|(file, line, column)| {
                serde_json::json!({
                    "file_name": file, "byte_start": 0, "byte_end": 1,
                    "line_start": line, "line_end": line,
                    "column_start": column, "column_end": column + 1,
                    "is_primary": true, "text": [], "label": null,
                    "suggested_replacement": null, "suggestion_applicability": null,
                    "expansion": null
                })
            })
            .into_iter()
            .collect();
        serde_json::json!({
            "reason": "compiler-message",
            "package_id": "path+file:///app#0.1.0",
            "manifest_path": "/app/Cargo.toml",
            "target": {
                "kind": ["lib"], "crate_types": ["lib"], "name": "app",
                "src_path": "/app/src/lib.rs", "edition": "2024", "doc": true,
                "doctest": true, "test": true
            },
            "message": {
                "message": text, "code": null, "level": level, "spans": spans,
                "children": [], "rendered": format!("{level}: {text}\n")
            }
        })
        .to_string()
    }

    #[test]
    fn the_errors_are_kept_and_every_message_still_reaches_the_terminal() {
        let mut lines = vec![
            message(
                "warning",
                "unused variable: `x`",
                Some(("src/lib.rs", 3, 9)),
            ),
            message(
                "error",
                "mismatched types\nexpected `u32`",
                Some(("src/lib.rs", 12, 5)),
            ),
            message("error", "cannot find value `y`", None),
            message("error", "aborting due to 2 previous errors", None),
            "   Compiling app v0.1.0".to_owned(),
        ];
        for line in 0..MAX_DIAGNOSTICS {
            lines.push(message(
                "error",
                &format!("error {line}"),
                Some(("src/main.rs", 1, 1)),
            ));
        }
        let mut terminal = Vec::new();
        let errors = read(lines.join("\n").as_bytes(), &mut terminal)
            .expect("read")
            .errors;

        assert_eq!(
            errors.kept()[..2],
            [
                Diagnostic::new(
                    "mismatched types".into(),
                    Some(SourceLocation::new("src/lib.rs".into(), 12, 5))
                ),
                Diagnostic::new("cannot find value `y`".into(), None),
            ]
        );
        assert_eq!(errors.kept().len(), MAX_DIAGNOSTICS);
        assert_eq!(
            errors.omitted(),
            2,
            "the errors past the first {MAX_DIAGNOSTICS}"
        );
        let terminal = String::from_utf8(terminal).expect("UTF-8");
        assert!(terminal.contains("warning: unused variable"), "{terminal}");
        assert!(
            terminal.contains("aborting due to 2 previous errors"),
            "{terminal}"
        );
        assert!(terminal.contains("   Compiling app v0.1.0\n"), "{terminal}");
    }

    /// The executable is the one the build selected, with whether cargo built it again, and a
    /// build script's link directories are kept as cargo states them.
    #[test]
    fn the_built_executable_and_the_link_directories_are_read() {
        let target = serde_json::json!({
            "kind": ["bin"], "crate_types": ["bin"], "name": "app",
            "src_path": "/app/src/main.rs", "edition": "2024", "doc": true,
            "doctest": false, "test": true
        });
        let artifact = |fresh: bool| {
            serde_json::json!({
                "reason": "compiler-artifact",
                "package_id": "path+file:///app#0.1.0",
                "manifest_path": "/app/Cargo.toml",
                "target": target,
                "profile": { "opt_level": "0", "debuginfo": 2, "debug_assertions": true,
                             "overflow_checks": true, "test": false },
                "features": [],
                "filenames": ["/app/target/debug/app"],
                "executable": "/app/target/debug/app",
                "fresh": fresh
            })
            .to_string()
        };
        let script = serde_json::json!({
            "reason": "build-script-executed",
            "package_id": "path+file:///sys#0.1.0",
            "linked_libs": ["sys"],
            "linked_paths": ["native=/app/target/debug/build/sys-1/out"],
            "cfgs": [], "env": [],
            "out_dir": "/app/target/debug/build/sys-1/out"
        })
        .to_string();
        for fresh in [false, true] {
            let lines = [script.clone(), artifact(fresh)].join("\n");
            let read = read(lines.as_bytes(), &mut Vec::new()).expect("read");
            assert_eq!(
                read.executable,
                Some(Executable {
                    path: PathBuf::from("/app/target/debug/app"),
                    fresh
                })
            );
            assert_eq!(
                read.linked_paths,
                [PathBuf::from("native=/app/target/debug/build/sys-1/out")]
            );
        }
        let none = read(script.as_bytes(), &mut Vec::new()).expect("read");
        assert_eq!(none.executable, None, "a build that selects no executable");
    }
}
