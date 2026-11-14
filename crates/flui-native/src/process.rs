//! Process-wide calls: ending the process at once, for the exit watchdog
//! (ADR-0258 §7), and the current user: its SID on Windows, its ID on Linux and Android.
#![expect(
    unsafe_code,
    reason = "ending the process is `TerminateProcess` or `_exit`, the token reads are Win32 and the user ID `geteuid`, all FFI"
)]

/// End the process now with `code`, running no destructor, `atexit` handler
/// or DLL detach.
///
/// The exit watchdog's one call: the thread it ends may be blocked in
/// application code, and `std::process::exit` runs `atexit` handlers and, on
/// Windows, DLL detach under the loader lock, either of which that thread may
/// hold. Windows ends the process with `TerminateProcess`; every other system
/// with `_exit`.
pub fn end_process(code: i32) -> ! {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // The exit code is the bit pattern of `code`, as `ExitProcess` takes it.
        let exit_code = u32::from_ne_bytes(code.to_ne_bytes());
        // SAFETY: `GetCurrentProcess` returns the pseudo-handle of this
        // process, valid for its whole life and never closed.
        let _: windows::core::Result<()> =
            unsafe { TerminateProcess(GetCurrentProcess(), exit_code) };
        // `TerminateProcess` on the current process does not return when it
        // succeeds; a refusal leaves no gentler way out.
        std::process::abort()
    }
    #[cfg(unix)]
    {
        // SAFETY: `_exit` is async-signal-safe and touches no Rust state: it
        // ends the process without running anything this process registered.
        unsafe { libc::_exit(code) }
    }
}

/// The current user's SID in string form (`S-1-5-21-…`), read from the
/// process token.
///
/// The development agent's named pipe grants this SID, and nobody else,
/// access (ADR-0239). The user is named by SID rather than as the owner
/// (`OW`): an elevated process's objects are owned by the Administrators
/// group, which would shut out the same user's unelevated client and admit
/// other administrators.
///
/// # Errors
///
/// The system's error when the token or its user cannot be read.
#[cfg(windows)]
pub fn current_user_sid() -> std::io::Result<String> {
    use windows::Win32::Security::TokenUser;

    token_sid(&Token::of_current()?, TokenUser)
}

/// Who a process runs as: its user and its mandatory integrity level, each
/// named by its SID in string form (`S-1-5-21-…`, `S-1-16-<level>`).
///
/// A single-instance application's later launch hands its launch only to a
/// process of its own user at its own integrity level or above, and the
/// first instance grants its mutex and pipe to its logon and refuses writes
/// from below its level, so an unelevated process of the same user cannot
/// hand an elevated instance links and files.
#[cfg(all(windows, feature = "single-instance"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenIdentity {
    /// The user's SID.
    pub(crate) user: String,
    /// The integrity level's SID.
    pub(crate) integrity: String,
}

#[cfg(all(windows, feature = "single-instance"))]
impl TokenIdentity {
    /// This process's identity.
    ///
    /// # Errors
    ///
    /// The system's error when the token cannot be read.
    pub(crate) fn current() -> std::io::Result<Self> {
        Self::of_token(&Token::of_current()?)
    }

    /// The identity of the process `pid`.
    ///
    /// # Errors
    ///
    /// The system's error when the process cannot be opened for a query, or
    /// its token cannot be read.
    pub(crate) fn of_process(pid: u32) -> std::io::Result<Self> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

        // SAFETY: plain values; the handle returned is owned by the guard.
        let process = Handle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .map_err(std::io::Error::from)?,
        );
        Self::of_token(&Token::of_process(&process)?)
    }

    fn of_token(token: &Token) -> std::io::Result<Self> {
        use windows::Win32::Security::{TokenIntegrityLevel, TokenUser};

        Ok(Self {
            user: token_sid(token, TokenUser)?,
            integrity: token_sid(token, TokenIntegrityLevel)?,
        })
    }

    /// The integrity level: the last sub-authority of its SID, such as
    /// `0x2000` for medium and `0x3000` for high. `None` for a SID that is
    /// not a mandatory label's.
    pub(crate) fn level(&self) -> Option<u32> {
        self.integrity.strip_prefix("S-1-16-")?.parse::<u32>().ok()
    }
}

/// This process's logon SID, in string form (`logon_sid` says what it
/// names). A single-instance application grants its mutex and pipe to it:
/// another logon of the same user, over Remote Desktop or a second console
/// sign-in, holds the user's SID and not this one.
///
/// # Errors
///
/// As `logon_sid`.
#[cfg(all(windows, feature = "single-instance"))]
pub(crate) fn current_logon_sid() -> std::io::Result<String> {
    logon_sid(&Token::of_current()?)
}

/// A handle closed on every path out.
#[cfg(windows)]
struct Handle(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a handle the system opened for this guard,
        // owned by it alone and closed only here.
        let _: windows::core::Result<()> =
            unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// A process token opened for a query.
#[cfg(windows)]
struct Token(Handle);

#[cfg(windows)]
impl Token {
    /// This process's token.
    fn of_current() -> std::io::Result<Self> {
        use windows::Win32::System::Threading::GetCurrentProcess;

        // SAFETY: the current-process pseudo-handle is valid for the
        // process's whole life and needs no closing.
        Self::of_raw(unsafe { GetCurrentProcess() })
    }

    /// The token of `process`, which the guard keeps open for the call.
    #[cfg(feature = "single-instance")]
    fn of_process(process: &Handle) -> std::io::Result<Self> {
        Self::of_raw(process.0)
    }

    fn of_raw(process: windows::Win32::Foundation::HANDLE) -> std::io::Result<Self> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Security::TOKEN_QUERY;
        use windows::Win32::System::Threading::OpenProcessToken;

        let mut raw = HANDLE::default();
        // SAFETY: `process` is a process handle the caller keeps open for the
        // call, and `raw` is a live out-pointer for it.
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut raw) }
            .map_err(std::io::Error::from)?;
        Ok(Self(Handle(raw)))
    }
}

/// The SID, in string form, that `token`'s `class` names: `TokenUser` and
/// `TokenIntegrityLevel` both answer a `SID_AND_ATTRIBUTES` at the head of
/// their buffer (`TOKEN_USER`, `TOKEN_MANDATORY_LABEL`).
#[cfg(windows)]
fn token_sid(
    token: &Token,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> std::io::Result<String> {
    use windows::Win32::Security::SID_AND_ATTRIBUTES;

    let buffer = token_information(token, class)?;
    // SAFETY: the buffer holds the answer of a class that starts with a
    // `SID_AND_ATTRIBUTES` (aligned by the `u64` storage), whose SID points
    // into the same buffer, which outlives this borrow.
    let sid = unsafe { &*buffer.as_ptr().cast::<SID_AND_ATTRIBUTES>() }.Sid;
    // SAFETY: `sid` points into `buffer`, alive for the call.
    unsafe { sid_text(sid) }
}

/// The logon SID of `token`, in string form (`S-1-5-5-X-Y`): the group the
/// system gives each logon session, which tells two logons of one user apart
/// where the user's SID does not.
///
/// # Errors
///
/// The system's error when the token's groups cannot be read, and
/// [`std::io::ErrorKind::NotFound`] for a token in no logon session.
#[cfg(all(windows, feature = "single-instance"))]
fn logon_sid(token: &Token) -> std::io::Result<String> {
    use windows::Win32::Security::{SID_AND_ATTRIBUTES, TOKEN_GROUPS, TokenGroups};
    use windows::Win32::System::SystemServices::SE_GROUP_LOGON_ID;

    let buffer = token_information(token, TokenGroups)?;
    let groups = buffer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: the call answered a `TOKEN_GROUPS` (aligned by the `u64`
    // storage): its count, then that many `SID_AND_ATTRIBUTES` laid out from
    // `Groups` on, all inside `buffer`, which outlives these borrows.
    let groups: &[SID_AND_ATTRIBUTES] = unsafe {
        std::slice::from_raw_parts(
            (&raw const (*groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            (*groups).GroupCount as usize,
        )
    };
    let logon = SE_GROUP_LOGON_ID.cast_unsigned();
    let group = groups
        .iter()
        .find(|group| group.Attributes & logon == logon)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "the process runs in no logon session",
            )
        })?;
    // SAFETY: the group's SID points into `buffer`, alive for the call.
    unsafe { sid_text(group.Sid) }
}

/// What `token` answers for `class`, in `u64` storage, which keeps the
/// structure at the head of the buffer aligned.
#[cfg(windows)]
fn token_information(
    token: &Token,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> std::io::Result<Vec<u64>> {
    use windows::Win32::Security::GetTokenInformation;

    let mut len = 0_u32;
    // SAFETY: a size query: no buffer, `len` a live out-pointer. It fails
    // with ERROR_INSUFFICIENT_BUFFER by design; `len` says whether it sized.
    let _: windows::core::Result<()> =
        unsafe { GetTokenInformation(token.0.0, class, None, 0, &raw mut len) };
    if len == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut buffer = vec![0_u64; (len as usize).div_ceil(8)];
    // SAFETY: `buffer` holds at least `len` writable bytes, as passed.
    unsafe {
        GetTokenInformation(
            token.0.0,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut len,
        )
    }
    .map_err(std::io::Error::from)?;
    Ok(buffer)
}

/// `sid` in string form (`S-1-…`).
///
/// # Safety
///
/// `sid` points to a valid SID for the call.
#[cfg(windows)]
unsafe fn sid_text(sid: windows::Win32::Security::PSID) -> std::io::Result<String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::core::PWSTR;

    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid for the call, as the caller states; `text` is a
    // live out-pointer the call fills with a LocalAlloc'd string.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(std::io::Error::from)?;
    // SAFETY: the call succeeded, so `text` is a NUL-terminated UTF-16
    // string this code owns until the `LocalFree` below.
    let converted = unsafe { text.to_string() };
    // SAFETY: `text` was allocated by the system for this code, is freed
    // once, and is not read after.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    converted.map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// The process's effective user ID.
///
/// The development agent's abstract socket, which any process of the system can reach, admits
/// a peer of this user (ADR-0288 §6), as a pipe's DACL admits the current user's SID on Windows.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[must_use]
pub fn current_user_id() -> u32 {
    // SAFETY: `geteuid` takes nothing, always succeeds and touches no memory of this process.
    unsafe { libc::geteuid() }
}
