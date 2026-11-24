/// Cargo package selection and artifact protocol.
pub(crate) mod cargo;
/// The compiler errors of a `cargo build`.
pub(crate) mod compile_errors;
/// Environment variable resolution and validation
pub(crate) mod environment;
/// Process execution utilities
pub(crate) mod process;
/// Escaping values interpolated into XML files
pub(crate) mod xml;

pub(crate) use environment::check_command_exists;
