//! Ending the process at once, for the exit watchdog (ADR-0258 §7).
#![expect(
    unsafe_code,
    reason = "ending the process at once is `TerminateProcess` or `_exit`, both FFI"
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
