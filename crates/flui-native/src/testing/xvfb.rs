//! An Xvfb of a test's own, on a display number no other X server holds:
//! the X server a harness starts for its compositor or window manager, or a
//! case that holds its server with a grab, which on the run's shared display
//! would freeze every other test's client.

use std::os::linux::net::SocketAddrExt as _;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a server just started gets to answer.
const STARTING: Duration = Duration::from_secs(10);

/// Start Xvfb on a display number no other server holds, with `-nolisten
/// tcp -noreset` and what `configure` adds (its screen, a process group of
/// its own): the process and its display's name (`:N`).
///
/// Processes start at once (nextest runs each case in its own), so each
/// tries numbers from one its process id picks, and a server that finds its
/// number taken exits at once and the next is tried. (`-displayfd` would let
/// the server pick, but it needs a writable `/tmp/.X11-unix`, which WSLg's
/// is not.) A server still running a moment after its socket answers is
/// taken to be this one. `-noreset`: a server resets as its last client
/// leaves, and refuses the connections that arrive meanwhile.
///
/// # Errors
///
/// What spawning `Xvfb` failed with: [`std::io::ErrorKind::NotFound`] when
/// it is not installed.
///
/// # Panics
///
/// When no number is free, or a server started does not answer within ten
/// seconds; that server is killed first.
pub fn start(configure: impl Fn(&mut Command)) -> std::io::Result<(Child, String)> {
    let first = 100 + std::process::id() % 800;
    for number in (first..first + 200).map(|number| 100 + number % 900) {
        let display = format!(":{number}");
        let mut command = Command::new("Xvfb");
        command
            .args([&display, "-nolisten", "tcp", "-noreset"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure(&mut command);
        let mut server = command.spawn()?;
        let deadline = Instant::now() + STARTING;
        loop {
            if server.try_wait().ok().flatten().is_some() {
                // The number was taken.
                break;
            }
            if answers(number) {
                std::thread::sleep(Duration::from_millis(200));
                if server.try_wait().ok().flatten().is_none() {
                    return Ok((server, display));
                }
                break;
            }
            if Instant::now() >= deadline {
                let _ = server.kill();
                let _ = server.wait();
                panic!("timed out waiting for Xvfb on {display}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    panic!("no free X display number");
}

/// Whether an X server listens on display `number`: on the abstract socket
/// Xvfb opens beside the path one, which a server whose `/tmp/.X11-unix` is
/// not writable listens on alone, or on the path.
fn answers(number: u32) -> bool {
    let path = format!("/tmp/.X11-unix/X{number}");
    let on_abstract = SocketAddr::from_abstract_name(path.as_bytes())
        .is_ok_and(|address| UnixStream::connect_addr(&address).is_ok());
    on_abstract || UnixStream::connect(&path).is_ok()
}
