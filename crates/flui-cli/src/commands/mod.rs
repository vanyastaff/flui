//! CLI command implementations.
//!
//! Each submodule corresponds to a top-level `flui` subcommand (e.g. `create`,
//! `build`, `run`, `doctor`). Commands are registered in the clap `Cli` enum
//! and dispatched from `main.rs`.

pub(crate) mod analyze;
pub(crate) mod build;
pub(crate) mod clean;
pub(crate) mod completions;
pub(crate) mod create;
pub(crate) mod create_interactive;
pub(crate) mod devices;
pub(crate) mod devtools;
pub(crate) mod doctor;
pub(crate) mod emulators;
pub(crate) mod format;
pub(crate) mod platform;
pub(crate) mod run;
pub(crate) mod test;
pub(crate) mod upgrade;
