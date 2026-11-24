//! The errors a `cargo build` run with `--message-format=json…` reports, read from its stdout
//! while each message is written to the terminal as cargo would have written it.
//!
//! `flui run` reads them so the running app can show where its next build failed
//! (`build.report`, ADR-0395). Only the compiler's errors are kept, at most
//! [`MAX_DIAGNOSTICS`] of them; the rest is counted.

use std::io::{self, BufRead, Write};

use cargo_metadata::Message;
use cargo_metadata::diagnostic::DiagnosticLevel;
use flui_protocol::build::{Diagnostic, MAX_DIAGNOSTICS, SourceLocation};

/// The compiler errors of one build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompileErrors {
    /// The first [`MAX_DIAGNOSTICS`], in the order the compiler reported them.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// How many more it reported.
    pub(crate) omitted: u32,
}

impl CompileErrors {
    fn push(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics.push(diagnostic);
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}

/// Reads cargo's messages from `messages` to their end, writing each compiler message's rendered
/// text, and each line that is not a message, to `terminal`, and answers the errors among them.
///
/// # Errors
///
/// Reading `messages` or writing `terminal` failed.
pub(crate) fn read(messages: impl BufRead, terminal: &mut impl Write) -> io::Result<CompileErrors> {
    let mut errors = CompileErrors::default();
    for message in Message::parse_stream(messages) {
        match message? {
            Message::CompilerMessage(message) => {
                let diagnostic = message.message;
                if let Some(rendered) = &diagnostic.rendered {
                    terminal.write_all(rendered.as_bytes())?;
                }
                if let Some(error) = compile_error(&diagnostic) {
                    errors.push(error);
                }
            }
            Message::TextLine(line) => writeln!(terminal, "{line}")?,
            _ => {}
        }
    }
    Ok(errors)
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
        let errors = read(lines.join("\n").as_bytes(), &mut terminal).expect("read");

        assert_eq!(
            errors.diagnostics[..2],
            [
                Diagnostic::new(
                    "mismatched types".into(),
                    Some(SourceLocation::new("src/lib.rs".into(), 12, 5))
                ),
                Diagnostic::new("cannot find value `y`".into(), None),
            ]
        );
        assert_eq!(errors.diagnostics.len(), MAX_DIAGNOSTICS);
        assert_eq!(
            errors.omitted, 2,
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
}
