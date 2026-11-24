//! The endpoints this command serves itself, private to the current user: the relay that
//! carries a page's agent ([`super::browser::relay`]) and `flui run`'s run endpoint
//! (`run.reload`, ADR-0401).

use std::io;
#[cfg(windows)]
use std::time::Duration;

use interprocess::local_socket::{Listener, ListenerNonblockingMode, ListenerOptions};

use super::client::name;

/// The session's endpoint, private to the current user: a socket in the session's `0700`
/// directory, or a pipe whose DACL grants the current user alone.
#[cfg(unix)]
pub(crate) fn bind(endpoint: &str) -> io::Result<Listener> {
    ListenerOptions::new()
        .name(name(endpoint)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .create_sync()
}

#[cfg(windows)]
pub(crate) fn bind(endpoint: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    let sddl = widestring::U16CString::from_str(format!("D:P(A;;GA;;;{})", current_user_sid()?))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    ListenerOptions::new()
        .name(name(endpoint)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .security_descriptor(descriptor)
        .create_sync()
}

/// The current user's SID, as `whoami` gives it: the user is named by SID rather than as the
/// pipe's owner, which is the Administrators group for an elevated process.
#[cfg(windows)]
fn current_user_sid() -> io::Result<String> {
    let output = crate::proc::output_with_timeout(
        std::process::Command::new("whoami").arg("/user"),
        Duration::from_secs(10),
    )?;
    sid_in_whoami(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("`whoami /user` named no SID"))
}

/// The SID in `whoami /user`'s table, whose last row is `machine\user  S-1-5-21-…`. The headings
/// are in the system's language and a user's name may hold spaces, so the SID is the last word
/// that starts as one does.
#[cfg(any(windows, test))]
pub(crate) fn sid_in_whoami(table: &str) -> Option<&str> {
    table
        .split_whitespace()
        .rev()
        .find(|word| word.starts_with("S-1-"))
}
