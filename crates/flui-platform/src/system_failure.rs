//! Where a system service's unnamed failure came from.

/// Where a failure the service's own error does not name came from, so a
/// diagnostic says what the system said instead of a number a backend made
/// up. Read it for logs and reports; no answer to the user branches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SystemFailure {
    /// An operating-system call failed with this code: an `errno` on Unix
    /// (on macOS also the POSIX code an `NSError` carries), a Win32 error
    /// code on Windows.
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
    /// The browser rejected the call with this exception.
    #[error("the browser rejected the call with {0}")]
    Browser(BrowserException),
    /// A macOS framework failed with this code outside the POSIX domain:
    /// the code of an `NSError` that carries no POSIX code (a Cocoa or
    /// ServiceManagement error), or an `OSStatus` (Carbon's event manager).
    #[error("the system framework failed with code {0}")]
    Framework(i64),
}

/// The exception a browser rejects a call with: the name of its
/// `DOMException`, or a JavaScript `TypeError`, which the browser throws for
/// an argument it cannot take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum BrowserException {
    /// `AbortError`: the operation was ended before it finished.
    #[error("AbortError")]
    Abort,
    /// `DataError`: the data given is not one the call takes.
    #[error("DataError")]
    Data,
    /// `InvalidStateError`: the object is not in a state for the call (a
    /// file input taken out of the document).
    #[error("InvalidStateError")]
    InvalidState,
    /// `NotAllowedError`: the user, a permission or a permissions policy
    /// refused.
    #[error("NotAllowedError")]
    NotAllowed,
    /// `NotFoundError`: what the call names is gone.
    #[error("NotFoundError")]
    NotFound,
    /// `NotReadableError`: a file could not be read.
    #[error("NotReadableError")]
    NotReadable,
    /// `NotSupportedError`: the browser does not support the call's
    /// arguments.
    #[error("NotSupportedError")]
    NotSupported,
    /// `QuotaExceededError`: storage is full.
    #[error("QuotaExceededError")]
    QuotaExceeded,
    /// `SecurityError`: the document may not make the call (a sandboxed or
    /// cross-origin frame, no user activation).
    #[error("SecurityError")]
    Security,
    /// `SyntaxError`: a string argument does not parse (a URL).
    #[error("SyntaxError")]
    Syntax,
    /// A `TypeError`: an argument of the wrong kind.
    #[error("TypeError")]
    Type,
    /// An exception of another name, or a rejection with no name.
    #[error("an unnamed exception")]
    Other,
}

impl BrowserException {
    /// The exception a browser names `name`: a `DOMException`'s `name`, or
    /// `TypeError`; [`Other`](Self::Other) for any other name.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "AbortError" => Self::Abort,
            "DataError" => Self::Data,
            "InvalidStateError" => Self::InvalidState,
            "NotAllowedError" => Self::NotAllowed,
            "NotFoundError" => Self::NotFound,
            "NotReadableError" => Self::NotReadable,
            "NotSupportedError" => Self::NotSupported,
            "QuotaExceededError" => Self::QuotaExceeded,
            "SecurityError" => Self::Security,
            "SyntaxError" => Self::Syntax,
            "TypeError" => Self::Type,
            _ => Self::Other,
        }
    }
}

impl From<BrowserException> for SystemFailure {
    fn from(exception: BrowserException) -> Self {
        Self::Browser(exception)
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserException;

    #[test]
    fn a_browser_exception_is_read_from_its_name_and_shown_by_it() {
        for exception in [
            BrowserException::Abort,
            BrowserException::Data,
            BrowserException::InvalidState,
            BrowserException::NotAllowed,
            BrowserException::NotFound,
            BrowserException::NotReadable,
            BrowserException::NotSupported,
            BrowserException::QuotaExceeded,
            BrowserException::Security,
            BrowserException::Syntax,
            BrowserException::Type,
        ] {
            assert_eq!(
                BrowserException::from_name(&exception.to_string()),
                exception
            );
        }
        assert_eq!(
            BrowserException::from_name("OperationError"),
            BrowserException::Other
        );
        assert_eq!(BrowserException::from_name(""), BrowserException::Other);
    }
}
