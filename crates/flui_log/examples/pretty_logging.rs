//! Pretty logging example using tracing-forest
//!
//! Run with: cargo run --example pretty_logging --features pretty
//!
//! This demonstrates hierarchical logging with timing information.

#[cfg(feature = "pretty")]
use flui_log::{info, warn, Logger};

#[cfg(feature = "pretty")]
#[tracing::instrument]
fn init_app() {
    info!("Initializing application");
    load_config();
    setup_database();
}

#[cfg(feature = "pretty")]
#[tracing::instrument]
fn load_config() {
    info!("Loading configuration files");
    std::thread::sleep(std::time::Duration::from_millis(100));
    info!("Configuration loaded successfully");
}

#[cfg(feature = "pretty")]
#[tracing::instrument]
fn setup_database() {
    info!("Setting up database connection");
    std::thread::sleep(std::time::Duration::from_millis(200));
    connect_to_db();
    info!("Database ready");
}

#[cfg(feature = "pretty")]
#[tracing::instrument]
fn connect_to_db() {
    info!("Connecting to database...");
    std::thread::sleep(std::time::Duration::from_millis(150));
    info!("Connected successfully");
}

#[cfg(feature = "pretty")]
#[tracing::instrument]
fn main_logic() {
    info!("Running main application logic");

    for i in 0..3 {
        process_item(i);
    }

    warn!("Some items need attention");
}

#[cfg(feature = "pretty")]
#[tracing::instrument]
fn process_item(id: i32) {
    info!(item_id = id, "Processing item");
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[cfg(feature = "pretty")]
fn main() {
    // Initialize with pretty logging enabled
    Logger::new().with_filter("trace").with_pretty(true).init();

    info!("🌲 Application started with pretty logging");

    init_app();
    main_logic();

    info!("✅ Application finished");
}

#[cfg(not(feature = "pretty"))]
fn main() {
    // Use compile_error-style approach: print to stderr and exit.
    // This is an example binary, not library code — tracing is not yet
    // initialized so we must use std I/O to tell the user what to do.
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr(),
        "This example requires the 'pretty' feature!\n\
         Run with: cargo run --example pretty_logging --features pretty"
    );
    std::process::exit(1);
}
