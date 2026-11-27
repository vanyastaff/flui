/// Cargo package selection and artifact protocol.
pub(crate) mod cargo;
/// The cargo configuration files a cargo command reads
pub(crate) mod cargo_config;
/// The compiler errors of a `cargo build`.
pub(crate) mod cargo_messages;
/// Environment variable resolution and validation
pub(crate) mod environment;
/// Process execution utilities
pub(crate) mod process;
/// Escaping values interpolated into XML files
pub(crate) mod xml;

pub(crate) use environment::check_command_exists;
