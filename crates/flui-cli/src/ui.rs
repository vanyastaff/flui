//! Output policy for the CLI: where text goes, in which shape, and how much.
//!
//! Every command talks to the terminal through this module rather than
//! through `cliclack` directly, so one global switch changes the whole tool:
//!
//! - **Human mode** (default): `cliclack`-styled text on **stderr**. Nothing
//!   is written to stdout except payloads a user would pipe (completion
//!   scripts). That is what makes `flui devices > list.txt` sane.
//! - **JSON mode** (`--json`): one JSON object per line on **stdout**
//!   (NDJSON) — every object carries an `event` field — and *no* human
//!   decoration anywhere. Human warnings and errors still reach stderr, so
//!   a pipeline that only reads stdout never sees a spinner glyph.
//! - **Quiet** (`--quiet`): progress and informational lines are dropped;
//!   warnings, errors and command output survive.
//!
//! Interactivity is a separate axis: prompts and hot-keys need a terminal
//! on both ends *and* no `CI`/`--non-interactive` opt-out. Anything that
//! would block on a prompt asks [`is_interactive`] first and fails with an
//! actionable error instead of hanging a CI job.
//!
//! The policy is process-global (`OnceLock`) because a CLI has exactly one
//! terminal; commands are free functions and threading a context through
//! every helper only for this would be noise.

use serde::Serialize;
use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::sync::OnceLock;

/// Shape of the CLI's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Styled text for a person, on stderr.
    #[default]
    Human,
    /// NDJSON on stdout, one `{"event": …}` object per line.
    Json,
}

/// How much human-mode output to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Verbosity {
    /// Warnings, errors and command output only.
    Quiet,
    /// The usual progress narration.
    #[default]
    Normal,
    /// Also debug logs from FLUI's own crates (`-v`).
    Verbose,
}

/// Whether to emit ANSI colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorChoice {
    /// Colour when stderr is a terminal and `NO_COLOR` is unset.
    #[default]
    Auto,
    /// Always colour, even into a pipe.
    Always,
    /// Never colour.
    Never,
}

/// The resolved output policy for this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Human or JSON.
    pub mode: OutputMode,
    /// Quiet / normal / verbose.
    pub verbosity: Verbosity,
    /// Whether prompts and hot-keys may be used.
    pub interactive: bool,
}

static POLICY: OnceLock<Policy> = OnceLock::new();

/// Install the process-wide output policy. Later calls are ignored: the
/// first caller (the entry point) owns the decision.
///
/// `non_interactive` is the explicit opt-out; the environment can also
/// force it (see [`is_interactive`] for the rule).
pub fn install(mode: OutputMode, verbosity: Verbosity, color: ColorChoice, non_interactive: bool) {
    apply_color(color);
    let interactive = !non_interactive && environment_allows_interaction();
    let _ = POLICY.set(Policy {
        mode,
        verbosity,
        interactive,
    });
}

/// The active policy (defaults when [`install`] was never called, e.g. in
/// unit tests).
pub fn policy() -> Policy {
    POLICY.get().copied().unwrap_or(Policy {
        mode: OutputMode::Human,
        verbosity: Verbosity::Normal,
        interactive: environment_allows_interaction(),
    })
}

/// `true` when output is NDJSON.
pub fn is_json() -> bool {
    policy().mode == OutputMode::Json
}

/// `true` when progress narration is suppressed (JSON mode counts: no
/// human text may leak into a machine stream).
pub fn is_quiet() -> bool {
    is_json() || policy().verbosity == Verbosity::Quiet
}

/// `true` when the CLI may prompt or read hot-keys.
///
/// Requires a terminal on stdin and stderr, no `CI` variable (any value,
/// the convention every CI vendor follows), no `FLUI_NON_INTERACTIVE`, no
/// `--non-interactive`, and human mode — a JSON consumer is a program.
pub fn is_interactive() -> bool {
    let policy = policy();
    policy.interactive && policy.mode == OutputMode::Human
}

fn environment_allows_interaction() -> bool {
    if std::env::var_os("CI").is_some() || std::env::var_os("FLUI_NON_INTERACTIVE").is_some() {
        return false;
    }
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Resolve `--color` together with the conventional environment switches.
///
/// Precedence: the flag, then `NO_COLOR` (any value disables), then
/// `CLICOLOR_FORCE` (non-`0` enables), then `TERM=dumb` disables, then the
/// terminal check `console` performs itself.
fn apply_color(choice: ColorChoice) {
    let enabled = match choice {
        ColorChoice::Always => Some(true),
        ColorChoice::Never => Some(false),
        ColorChoice::Auto => {
            if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
                Some(false)
            } else if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| !v.is_empty() && v != "0")
            {
                Some(true)
            } else if std::env::var_os("TERM").is_some_and(|v| v == "dumb") {
                Some(false)
            } else {
                None
            }
        }
    };
    if let Some(enabled) = enabled {
        console::set_colors_enabled(enabled);
        console::set_colors_enabled_stderr(enabled);
    }
}

// ============================================================================
// JSON events
// ============================================================================

/// Emit one NDJSON event on stdout. A no-op in human mode.
///
/// `event` is the discriminator (`"doctor.check"`, `"run.app.start"`, …);
/// `payload` is merged in beside it. Write failures (a closed pipe) are
/// ignored: the consumer went away, and a CLI must not crash because `head`
/// stopped reading.
pub fn emit<T: Serialize>(event: &str, payload: &T) {
    if !is_json() {
        return;
    }
    let mut value = match serde_json::to_value(payload) {
        Ok(serde_json::Value::Object(map)) => map,
        Ok(other) => {
            let mut map = serde_json::Map::new();
            map.insert("data".into(), other);
            map
        }
        Err(error) => {
            tracing::error!(%error, event, "BUG: event payload is not serializable");
            return;
        }
    };
    value.insert("event".into(), serde_json::Value::String(event.into()));
    let mut stdout = std::io::stdout().lock();
    if serde_json::to_writer(&mut stdout, &value).is_ok() {
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
}

// ============================================================================
// Human-mode narration (stderr)
// ============================================================================

/// Command banner, e.g. `flui doctor`.
pub fn intro(title: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::intro(title)
}

/// Closing line after success.
pub fn outro(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::outro(message)
}

/// Closing line after failure. Shown even when quiet — a failure is never
/// noise — but never in JSON mode, where the `error` event carries it.
pub fn outro_cancel(message: impl Display) -> std::io::Result<()> {
    if is_json() {
        return Ok(());
    }
    cliclack::outro_cancel(message)
}

/// Informational line.
pub fn info(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::log::info(message)
}

/// Success line.
pub fn success(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::log::success(message)
}

/// Progress step line.
pub fn step(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::log::step(message)
}

/// Low-emphasis remark.
pub fn remark(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::log::remark(message)
}

/// Warning line. Survives `--quiet`; in JSON mode it goes to stderr as
/// plain text so the stdout stream stays pure.
pub fn warning(message: impl Display) -> std::io::Result<()> {
    if is_json() {
        return writeln!(std::io::stderr(), "warning: {message}");
    }
    cliclack::log::warning(message)
}

/// Error line. Always shown; plain text in JSON mode.
pub fn error(message: impl Display) -> std::io::Result<()> {
    if is_json() {
        return writeln!(std::io::stderr(), "error: {message}");
    }
    cliclack::log::error(message)
}

/// Boxed note with a title.
pub fn note(title: impl Display, body: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    cliclack::note(title, body)
}

/// A spinner that is silent when narration is suppressed.
#[must_use]
pub fn spinner() -> Spinner {
    if is_quiet() {
        Spinner(None)
    } else {
        Spinner(Some(cliclack::spinner()))
    }
}

/// Progress spinner handle; see [`spinner`].
pub struct Spinner(Option<cliclack::ProgressBar>);

impl std::fmt::Debug for Spinner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spinner")
            .field("active", &self.0.is_some())
            .finish()
    }
}

impl Spinner {
    /// Start spinning with a message.
    pub fn start(&self, message: impl Display) {
        if let Some(bar) = &self.0 {
            bar.start(message);
        }
    }

    /// Stop with a final message.
    pub fn stop(&self, message: impl Display) {
        if let Some(bar) = &self.0 {
            bar.stop(message);
        }
    }

    /// Stop with a failure message.
    pub fn error(&self, message: impl Display) {
        if let Some(bar) = &self.0 {
            bar.error(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_human_normal() {
        let policy = policy();
        assert_eq!(policy.mode, OutputMode::Human);
        assert_eq!(policy.verbosity, Verbosity::Normal);
    }

    #[test]
    fn verbosity_orders_quiet_below_verbose() {
        assert!(Verbosity::Quiet < Verbosity::Normal);
        assert!(Verbosity::Normal < Verbosity::Verbose);
    }
}
