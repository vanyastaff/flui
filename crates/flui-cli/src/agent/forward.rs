//! The port `adb forward` holds for an Android app's agent (ADR-0288 §6).
//!
//! An Android app serves its agent on an abstract socket, which only a process on the device
//! reaches. `adb forward tcp:0 localabstract:<name>` opens a loopback port on this host whose
//! every connection the device's `adbd` carries to that socket, as the shell's user, which the
//! app admits. The forward lives as long as the session that opened it.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::error::{CliError, CliResult, ResultExt};

/// How long one `adb forward` command may take.
const ADB_TIMEOUT: Duration = Duration::from_secs(10);

/// A loopback port `adb` carries to an abstract socket on one device. Dropping it removes the
/// forward.
#[derive(Debug)]
pub(crate) struct AdbForward {
    adb: PathBuf,
    serial: String,
    port: u16,
}

impl AdbForward {
    /// Forwards a free loopback port to the abstract socket `socket` on the device `serial`.
    pub(crate) fn open(adb: &Path, serial: &str, socket: &str) -> CliResult<Self> {
        let remote = format!("localabstract:{socket}");
        let context = format!("adb -s {serial} forward tcp:0 {remote}");
        let output = crate::proc::output_with_timeout(
            Command::new(adb).args(["-s", serial, "forward", "tcp:0", &remote]),
            ADB_TIMEOUT,
        )
        .with_context(|| context.clone())?;
        if !output.status.success() {
            crate::ui::debug(format!(
                "{context}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return Err(CliError::CommandFailed {
                context,
                exit_code: output.status.code(),
            });
        }
        let port = allocated_port(&output.stdout).ok_or_else(|| CliError::RunFailed {
            details: format!(
                "{context} printed {:?}, not the port it allocated",
                String::from_utf8_lossy(&output.stdout).trim()
            ),
        })?;
        Ok(Self {
            adb: adb.to_owned(),
            serial: serial.to_owned(),
            port,
        })
    }

    /// The loopback address a client connects to.
    pub(crate) fn address(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }
}

impl Drop for AdbForward {
    fn drop(&mut self) {
        // A device that went away took the forward with it: nothing to report.
        let _ = crate::proc::output_with_timeout(
            Command::new(&self.adb).args([
                "-s",
                &self.serial,
                "forward",
                "--remove",
                &format!("tcp:{}", self.port),
            ]),
            ADB_TIMEOUT,
        );
    }
}

/// The port `adb forward tcp:0` prints, alone on its line; `None` for anything else.
fn allocated_port(stdout: &[u8]) -> Option<u16> {
    std::str::from_utf8(stdout)
        .ok()?
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
}

#[cfg(test)]
mod tests {
    use super::allocated_port;

    #[test]
    fn the_allocated_port_is_the_number_adb_prints() {
        let cases: [(&[u8], Option<u16>); 6] = [
            (b"43123\n", Some(43123)),
            (b"43123\r\n", Some(43123)),
            (b"0\n", None),
            (b"70000\n", None),
            (b"error: device offline\n", None),
            (b"", None),
        ];
        for (stdout, port) in cases {
            assert_eq!(
                allocated_port(stdout),
                port,
                "{:?}",
                String::from_utf8_lossy(stdout)
            );
        }
    }
}
