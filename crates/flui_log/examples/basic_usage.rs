//! Basic usage example for flui_log
//!
//! Demonstrates how to initialize and use cross-platform logging.

use flui_log::{debug, error, info, trace, warn, Logger};

fn main() {
    // Initialize logging with default configuration
    Logger::default().init();

    info!("Application started");
    debug!("Debug information");
    trace!("Trace-level details");

    // Demonstrate structured logging
    info!(count = 42, name = "example", "Processing item");

    // Demonstrate different log levels
    warn!("This is a warning message");
    error!("This is an error message");

    info!("Application finished");
}
