//! `flui mcp`: the MCP server an AI agent uses to drive a running app.
//!
//! See [`crate::agent::mcp`]. stdout carries the protocol and nothing else,
//! so this command prints no banner and no `--json` events; a failure ends
//! the process through the usual error path on stderr.

use std::io;
use std::path::PathBuf;

use crate::agent::mcp::{Live, serve};
use crate::error::{CliResult, ResultExt};

/// Serve MCP on stdio until the client closes stdin.
pub(crate) fn execute(session: Option<PathBuf>) -> CliResult<()> {
    let mut agent = Live::new(session);
    serve(io::stdin().lock(), io::stdout().lock(), &mut agent)
        .context("the MCP session ended with an I/O error")
}
