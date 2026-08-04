//! `DevTools` command for the FLUI development tools.
//!
//! There is no `DevTools` server to launch — this command has never started
//! one, regardless of the `devtools` feature. Both paths below say so
//! honestly and name what `flui-devtools` actually offers as a library
//! today: `InspectorCounters` (mount/rebuild/unmount tallies over the
//! ADR-0040 observation seam) plus opt-in `profiler`/`timeline`/`hot_reload`
//! modules an embedder wires up manually. See `crates/flui-devtools/FEATURES.md`.

#[cfg(feature = "devtools")]
use crate::error::CliError;
use crate::error::CliResult;
use console::style;

/// Execute the devtools command.
///
/// Neither build reaches a running server: with the `devtools` feature
/// compiled in this reports what `flui-devtools` provides as a library and
/// returns [`CliError::NotImplemented`] (nonzero exit); without it, this
/// shows the same message plus the build instruction.
pub fn execute(port: u16) -> CliResult<()> {
    cliclack::intro(style(" flui devtools ").on_green().black())?;

    #[cfg(feature = "devtools")]
    {
        report_not_implemented(port)
    }

    #[cfg(not(feature = "devtools"))]
    {
        show_unavailable_message(port)
    }
}

/// Report honestly that no DevTools server exists, and name what
/// `flui-devtools` actually provides as a library (only compiled when the
/// `devtools` feature is available).
#[cfg(feature = "devtools")]
fn report_not_implemented(port: u16) -> CliResult<()> {
    cliclack::log::warning(format!(
        "DevTools server is not implemented — no listener opens on port {port}."
    ))?;

    let available = format!(
        "{}\n\n  {}\n  {}\n  {}\n  {}\n\n{}",
        "`flui-devtools` exists as a library with feature-gated subsystems \
         the CLI does not wire up:",
        "- inspector: InspectorCounters — mount/rebuild/unmount tallies \
         over the ADR-0040 observation seam",
        "- profiling: Profiler — frame/phase timing, jank detection",
        "- timeline: Timeline — Chrome-trace event export",
        "- hot-reload: HotReloader — file-watch callbacks",
        style(
            "An embedder wires these in directly today; there is no \
             CLI-launched server. See crates/flui-devtools/FEATURES.md."
        )
        .dim(),
    );
    cliclack::note("What actually exists", available)?;
    cliclack::outro(style("DevTools server not implemented").red())?;

    tracing::info!(port, "flui devtools: no server implementation to launch");

    Err(CliError::not_implemented("flui devtools server"))
}

/// Show a message when the `devtools` feature is not compiled in.
///
/// Note: enabling the feature does not change the outcome — see
/// [`report_not_implemented`], which this build does not compile. There is
/// no server to gain access to; this only unlocks `flui-devtools` as a
/// library dependency.
#[cfg(not(feature = "devtools"))]
fn show_unavailable_message(_port: u16) -> CliResult<()> {
    cliclack::log::warning("DevTools is not available in this build.")?;

    let instructions = format!(
        "{}\n\n  {}\n\n{}",
        "To use `flui-devtools`' library subsystems (inspector counters, \
         profiler, timeline, hot-reload), rebuild flui-cli with the \
         devtools feature — note this does not add a DevTools server:",
        style("cargo install flui-cli --features devtools").cyan(),
        style("DevTools requires the flui-devtools crate.").dim(),
    );

    cliclack::note("Setup Instructions", instructions)?;
    cliclack::outro(style("DevTools not enabled").dim())?;

    Ok(())
}
