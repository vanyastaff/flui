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
    use windows::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: the current-process pseudo-handle is valid for the process's
    // whole life and needs no closing.
    user_sid_of(unsafe { GetCurrentProcess() })
}

/// The SID, in string form, of the user the process `pid` runs as: what the
/// later launch of a single-instance application compares with its own
/// before it hands its launch to the process serving the application's pipe.
///
/// # Errors
///
/// The system's error when the process cannot be opened for a query, or its
/// token or user cannot be read.
#[cfg(windows)]
#[cfg(feature = "single-instance")]
pub(crate) fn process_user_sid(pid: u32) -> std::io::Result<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    /// Closes the process handle on every path out.
    struct Process(HANDLE);
    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: `self.0` is the handle `OpenProcess` opened below,
            // owned by this guard alone and closed only here.
            let _: windows::core::Result<()> = unsafe { CloseHandle(self.0) };
        }
    }

    // SAFETY: plain values; the handle returned is owned by the guard.
    let process = Process(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(std::io::Error::from)?,
    );
    user_sid_of(process.0)
}

/// The SID, in string form, of the user `process`'s token names.
#[cfg(windows)]
fn user_sid_of(process: windows::Win32::Foundation::HANDLE) -> std::io::Result<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows::Win32::System::Threading::OpenProcessToken;
    use windows::core::PWSTR;

    /// Closes the token on every path out.
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: `self.0` is the token `OpenProcessToken` opened below,
            // owned by this guard alone and closed only here.
            let _: windows::core::Result<()> = unsafe { CloseHandle(self.0) };
        }
    }

    let mut raw = HANDLE::default();
    // SAFETY: `process` is a process handle the caller keeps open for the
    // call, and `raw` is a live out-pointer for it.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut raw) }
        .map_err(std::io::Error::from)?;
    let token = Token(raw);

    let mut len = 0_u32;
    // SAFETY: a size query: no buffer, `len` a live out-pointer. It fails
    // with ERROR_INSUFFICIENT_BUFFER by design; `len` says whether it sized.
    let _: windows::core::Result<()> =
        unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &raw mut len) };
    if len == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // `u64` storage keeps the TOKEN_USER at the head of the buffer aligned.
    let mut buffer = vec![0_u64; (len as usize).div_ceil(8)];
    // SAFETY: `buffer` holds at least `len` writable bytes, as passed.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut len,
        )
    }
    .map_err(std::io::Error::from)?;
    // SAFETY: the call succeeded, so the buffer starts with an initialised
    // TOKEN_USER (aligned by the `u64` storage) whose SID points into the
    // same buffer, which outlives this borrow.
    let sid = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() }.User.Sid;
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid while `buffer` lives; `text` is a live
    // out-pointer the call fills with a LocalAlloc'd string.
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
