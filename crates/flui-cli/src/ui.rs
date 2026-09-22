//! Output policy for the CLI: where text goes, in which shape, and how much.
//!
//! Every command talks to the terminal through this module, so one global
//! switch changes the whole tool:
//!
//! - **Human mode** (default): styled text on **stderr** — a bar down the
//!   left, one glyph per line kind — drawn here with `console`. Nothing is
//!   written to stdout except payloads a user would pipe (completion
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
//! actionable error instead of hanging a CI job. The prompts themselves
//! live in [`prompt`], over `dialoguer`.
//!
//! The policy is process-global (`OnceLock`) because a CLI has exactly one
//! terminal; commands are free functions and threading a context through
//! every helper only for this would be noise.
//!
//! The drawing used to be `cliclack`'s. It went because it brought 42 of the
//! CLI's 116 crates — ICU text segmentation, with its data tables and
//! proc-macros, to word-wrap prompt text — for a dozen lines of glyphs and a
//! spinner that fit in this file.

use serde::Serialize;
use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use console::{StyledObject, Term, style};

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
    /// Also [`debug`] diagnostics: commands run, probes made, paths skipped.
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
            let _ = writeln!(
                std::io::stderr(),
                "error: BUG: the `{event}` event payload is not serializable: {error}"
            );
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
//
// The shape: a bar `│` runs down the left edge from `┌  title` to
// `└  closing line`; every line in between opens with one glyph that says
// what kind of line it is, and continuation lines of a multi-line message
// hang under the bar.

const BAR: &str = "│";

fn bar() -> StyledObject<&'static str> {
    style(BAR).dim()
}

/// One glyph-led block: the first line after the glyph, the rest under the
/// bar, then an empty bar line to separate it from the next block.
fn block(glyph: StyledObject<&'static str>, message: &str) -> std::io::Result<()> {
    let mut err = std::io::stderr().lock();
    let mut lines = message.lines();
    writeln!(err, "{glyph}  {}", lines.next().unwrap_or_default())?;
    for line in lines {
        writeln!(err, "{}  {line}", bar())?;
    }
    writeln!(err, "{}", bar())
}

/// Command banner, e.g. `flui doctor`.
pub fn intro(title: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    let mut err = std::io::stderr().lock();
    writeln!(err, "{}  {title}", style("┌").dim())?;
    writeln!(err, "{}", bar())
}

/// Closing line after success.
pub fn outro(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    let mut err = std::io::stderr().lock();
    writeln!(err, "{}  {message}", style("└").dim())?;
    writeln!(err)
}

/// Closing line after failure. Shown even when quiet — a failure is never
/// noise — but never in JSON mode, where the `error` event carries it.
pub fn outro_cancel(message: impl Display) -> std::io::Result<()> {
    if is_json() {
        return Ok(());
    }
    let mut err = std::io::stderr().lock();
    writeln!(err, "{}  {}", style("└").red(), style(message).red())?;
    writeln!(err)
}

/// Informational line.
pub fn info(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    block(style("●").cyan(), &message.to_string())
}

/// Success line.
pub fn success(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    block(style("◆").green(), &message.to_string())
}

/// Progress step line.
pub fn step(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    block(style("◇").green(), &message.to_string())
}

/// Low-emphasis remark.
pub fn remark(message: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    block(bar(), &style(message).dim().to_string())
}

/// Diagnostic line for `--verbose`: which command ran, which probe failed,
/// which path was skipped and why. Plain dimmed text on stderr, shown only
/// under `-v` — in JSON mode too, since stderr is not the machine stream.
///
/// This is the CLI's whole logging story. It links no logging framework:
/// with no framework crate in its graph there is nothing for a `RUST_LOG`
/// filter to select, and a second voice on stderr beside the narration
/// above would only compete with it.
pub fn debug(message: impl Display) {
    if policy().verbosity != Verbosity::Verbose {
        return;
    }
    let _ = writeln!(
        std::io::stderr(),
        "{}",
        style(format!("debug: {message}")).dim()
    );
}

/// Warning line. Survives `--quiet`; in JSON mode it goes to stderr as
/// plain text so the stdout stream stays pure.
pub fn warning(message: impl Display) -> std::io::Result<()> {
    if is_json() {
        return writeln!(std::io::stderr(), "warning: {message}");
    }
    block(style("▲").yellow(), &message.to_string())
}

/// Error line. Always shown; plain text in JSON mode.
pub fn error(message: impl Display) -> std::io::Result<()> {
    if is_json() {
        return writeln!(std::io::stderr(), "error: {message}");
    }
    block(style("■").red(), &message.to_string())
}

/// Titled note: the title on the glyph line, the body hanging under the bar.
pub fn note(title: impl Display, body: impl Display) -> std::io::Result<()> {
    if is_quiet() {
        return Ok(());
    }
    block(
        style("◇").green(),
        &format!("{}\n{body}", style(title).bold()),
    )
}

// ============================================================================
// Spinner
// ============================================================================

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_INTERVAL: Duration = Duration::from_millis(80);

/// A spinner that is silent when narration is suppressed.
#[must_use]
pub fn spinner() -> Spinner {
    if is_quiet() {
        Spinner(None)
    } else {
        Spinner(Some(Active {
            message: Arc::new(Mutex::new(String::new())),
            stop: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }))
    }
}

/// Progress spinner handle; see [`spinner`].
///
/// On a terminal it animates in place on stderr; into a pipe it prints the
/// start message once and the final message once, so a log of the run
/// reads like the terminal did, minus the animation.
pub struct Spinner(Option<Active>);

struct Active {
    message: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

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
        let Some(active) = &self.0 else { return };
        let message = message.to_string();
        let term = Term::stderr();
        if !term.is_term() {
            let _ = writeln!(std::io::stderr(), "{}  {message}", style("◇").dim());
            return;
        }
        *active
            .message
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = message;
        active.stop.store(false, Ordering::SeqCst);
        let shared = Arc::clone(&active.message);
        let stop = Arc::clone(&active.stop);
        let handle = std::thread::Builder::new()
            .name("flui-spinner".into())
            .spawn(move || {
                let _ = term.hide_cursor();
                let mut frame = 0usize;
                while !stop.load(Ordering::SeqCst) {
                    let text = shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let _ = term.clear_line();
                    let _ = term.write_str(&format!(
                        "{}  {text}",
                        style(FRAMES[frame % FRAMES.len()]).magenta()
                    ));
                    frame += 1;
                    std::thread::sleep(FRAME_INTERVAL);
                }
                let _ = term.clear_line();
                let _ = term.show_cursor();
            })
            .ok();
        *active
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = handle;
    }

    fn finish(&self, glyph: StyledObject<&'static str>, message: impl Display) {
        let Some(active) = &self.0 else { return };
        active.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = active
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = handle.join();
        }
        let _ = block(glyph, &message.to_string());
    }

    /// Stop with a final message.
    pub fn stop(&self, message: impl Display) {
        self.finish(style("◇").green(), message);
    }

    /// Stop with a failure message.
    pub fn error(&self, message: impl Display) {
        self.finish(style("■").red(), message);
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        // A spinner dropped mid-flight (an early `?`) must not leave a thread
        // redrawing over whatever the error path prints next.
        if let Some(active) = &self.0 {
            active.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = active
                .thread
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = handle.join();
            }
        }
    }
}

// ============================================================================
// Prompts
// ============================================================================

/// Interactive prompts, on stderr, for callers that checked
/// [`is_interactive`] first.
///
/// Every function returns `Ok(None)` when the user backed out (Esc, `q` or
/// Ctrl-C) so the caller can map that to its own cancellation error, and
/// `Err` only for a real terminal failure.
pub mod prompt {
    use console::{Term, style};
    use dialoguer::theme::ColorfulTheme;
    use dialoguer::{Confirm, Input, MultiSelect, Select};

    fn theme() -> ColorfulTheme {
        ColorfulTheme::default()
    }

    /// Ctrl-C surfaces as an interrupted I/O error; that is a cancellation,
    /// not a failure.
    fn cancel_aware<T>(result: dialoguer::Result<T>) -> std::io::Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(dialoguer::Error::IO(error)) if error.kind() == std::io::ErrorKind::Interrupted => {
                Ok(None)
            }
            Err(dialoguer::Error::IO(error)) => Err(error),
        }
    }

    fn cancel_aware_opt<T>(result: dialoguer::Result<Option<T>>) -> std::io::Result<Option<T>> {
        cancel_aware(result).map(Option::flatten)
    }

    /// Label with a dimmed description, for menu items.
    fn item(name: &str, description: &str) -> String {
        format!("{name}  {}", style(description).dim())
    }

    /// Free-text input with validation; `default` is offered when set.
    pub fn input(
        label: &str,
        default: Option<&str>,
        validate: impl Fn(&str) -> Result<(), String>,
    ) -> std::io::Result<Option<String>> {
        let theme = theme();
        let mut prompt = Input::<String>::with_theme(&theme).with_prompt(label);
        if let Some(default) = default {
            prompt = prompt.default(default.to_string());
        }
        let prompt = prompt.validate_with(move |value: &String| validate(value));
        cancel_aware(prompt.interact_text_on(&Term::stderr()))
    }

    /// Single choice among `(value, name, description)` items.
    pub fn select<T: Clone>(label: &str, items: &[(T, &str, &str)]) -> std::io::Result<Option<T>> {
        let labels: Vec<String> = items
            .iter()
            .map(|(_, name, description)| item(name, description))
            .collect();
        let chosen = cancel_aware_opt(
            Select::with_theme(&theme())
                .with_prompt(label)
                .items(&labels)
                .default(0)
                .interact_on_opt(&Term::stderr()),
        )?;
        Ok(chosen.map(|index| items[index].0.clone()))
    }

    /// Any number of choices among `(value, name, description)` items; an
    /// empty selection is a valid answer.
    pub fn multiselect<T: Clone>(
        label: &str,
        items: &[(T, &str, &str)],
    ) -> std::io::Result<Option<Vec<T>>> {
        let labels: Vec<String> = items
            .iter()
            .map(|(_, name, description)| item(name, description))
            .collect();
        let chosen = cancel_aware_opt(
            MultiSelect::with_theme(&theme())
                .with_prompt(label)
                .items(&labels)
                .interact_on_opt(&Term::stderr()),
        )?;
        Ok(chosen.map(|indices| indices.into_iter().map(|i| items[i].0.clone()).collect()))
    }

    /// Yes/no question, defaulting to no.
    pub fn confirm(label: &str) -> std::io::Result<Option<bool>> {
        cancel_aware_opt(
            Confirm::with_theme(&theme())
                .with_prompt(label)
                .default(false)
                .interact_on_opt(&Term::stderr()),
        )
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
