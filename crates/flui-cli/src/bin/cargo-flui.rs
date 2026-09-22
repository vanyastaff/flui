//! `cargo flui …` is `flui …`.
//!
//! Cargo runs an external subcommand as `cargo-flui flui <args>`. This
//! binary drops that repeated `flui`, runs the `flui` binary installed next
//! to it (both come from one `cargo install flui-cli`), and returns its exit
//! status unchanged so the exit-code contract documented in `flui --help`
//! holds under either spelling. It never parses the arguments itself: there
//! is one CLI, not two.

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|first| first == "flui") {
        args.next();
    }
    let args: Vec<OsString> = args.collect();

    let flui = match sibling("flui") {
        Some(path) => path,
        None => PathBuf::from("flui"),
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `exec` keeps one process: signals, the terminal and the exit
        // status all belong to `flui` directly. It only returns on failure.
        let error = Command::new(&flui).args(&args).exec();
        eprintln!("cargo-flui: cannot run {}: {error}", flui.display());
        ExitCode::from(3)
    }
    #[cfg(not(unix))]
    {
        match Command::new(&flui).args(&args).status() {
            Ok(status) => match status.code() {
                Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
                None => ExitCode::FAILURE,
            },
            Err(error) => {
                eprintln!("cargo-flui: cannot run {}: {error}", flui.display());
                ExitCode::from(3)
            }
        }
    }
}

/// The `name` binary in this executable's own directory, when it is there.
fn sibling(name: &str) -> Option<PathBuf> {
    let path = env::current_exe()
        .ok()?
        .with_file_name(format!("{name}{}", env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}
