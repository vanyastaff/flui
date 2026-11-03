/// Cargo package selection and artifact protocol.
pub(crate) mod cargo;
/// Environment variable resolution and validation
pub(crate) mod environment;
/// Process execution utilities
pub(crate) mod process;
/// Escaping values interpolated into XML files
pub(crate) mod xml;

pub(crate) use environment::check_command_exists;
