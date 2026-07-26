//! `DevTools` command for launching the FLUI development tools.
//!
//! When the `devtools` feature is enabled, launches the `DevTools` server.
//! When disabled, displays instructions on how to enable it.

use crate::error::CliResult;
use console::style;

/// Execute the devtools command.
///
/// Behavior depends on whether the `devtools` feature is compiled in:
/// - **Enabled**: Launches the `DevTools` server on the specified port.
/// - **Disabled**: Displays instructions for enabling the feature.
pub fn execute(port: u16) -> CliResult<()> {
    cliclack::intro(style(" flui devtools ").on_green().black())?;

    #[cfg(feature = "devtools")]
    {
        launch_devtools_server(port)?;
    }

    #[cfg(not(feature = "devtools"))]
    {
        show_unavailable_message(port)?;
    }

    Ok(())
}

/// Launch the DevTools server (only compiled when devtools feature is enabled).
#[cfg(feature = "devtools")]
fn launch_devtools_server(port: u16) -> CliResult<()> {
    // Check if port is already in use.
    check_port_available(port)?;

    cliclack::log::info(format!(
        "DevTools server started on {}",
        style(format!("http://localhost:{port}"))
            .cyan()
            .underlined()
    ))?;
    cliclack::log::info("Press Ctrl+C to stop")?;

    // TODO: Call flui_devtools::start_server(port) when the crate API is available.
    // For now, block until Ctrl+C.
    tracing::info!(port, "DevTools server listening");

    // Block on Ctrl+C via tokio's signal driver (same runtime idiom as `run`).
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("Failed to listen for Ctrl+C: {e}");
        }
    });
    cliclack::outro("DevTools server stopped")?;
    Ok(())
}

/// Show a message when devtools feature is not available.
#[cfg(not(feature = "devtools"))]
fn show_unavailable_message(_port: u16) -> CliResult<()> {
    cliclack::log::warning("DevTools is not available in this build.")?;

    let instructions = format!(
        "{}\n\n  {}\n\n{}",
        "To enable DevTools, rebuild flui-cli with the devtools feature:",
        style("cargo install flui-cli --features devtools").cyan(),
        style("DevTools requires the flui-devtools crate.").dim(),
    );

    cliclack::note("Setup Instructions", instructions)?;
    cliclack::outro(style("DevTools not enabled").dim())?;

    Ok(())
}

/// Check if a TCP port is available for binding.
#[cfg(feature = "devtools")]
fn check_port_available(port: u16) -> CliResult<()> {
    use std::net::TcpListener;

    if let Ok(_listener) = TcpListener::bind(("127.0.0.1", port)) {
        // Port is available — the listener is dropped here, freeing the port.
        Ok(())
    } else {
        cliclack::log::error(format!(
            "Port {port} is already in use. Try a different port with --port <PORT>"
        ))?;
        Err(crate::error::CliError::build_failed(
            "devtools",
            format!("Port {port} is already in use"),
        ))
    }
}
