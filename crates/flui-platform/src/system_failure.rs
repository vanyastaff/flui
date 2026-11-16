//! Where a system service's unnamed failure came from.

/// Where a failure the service's own error does not name came from, so a
/// diagnostic says what the system said instead of a number a backend made
/// up. Read it for logs and reports; no answer to the user branches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SystemFailure {
    /// An operating-system call failed with this code: an `errno` on Unix,
    /// a Win32 error code on Windows.
    #[error("the operating system failed with code {0}")]
    Os(i32),
    /// The desktop portal ended the request with this response: neither an
    /// answer (`0`) nor the user's cancel (`1`). `2` is the portal's "ended
    /// some other way".
    #[error("the desktop portal ended the request with response {0}")]
    PortalResponse(u32),
    /// The message bus, or the service on it, answered with an error the
    /// backend does not name, or the connection failed mid-request.
    #[error("the message bus failed the request")]
    Bus,
    /// The system answered in a shape its contract does not allow: a
    /// success that carries no result, several results where one was asked
    /// for, a value of the wrong type.
    #[error("the system answered outside its contract")]
    MalformedAnswer,
}
