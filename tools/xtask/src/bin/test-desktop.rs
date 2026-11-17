//! Runs one test process on a desktop of its own, where the person at the machine neither sees
//! its windows nor reaches them with the mouse and keyboard (ADR-0355).
//!
//! nextest starts every test on Windows through this program (`.config/nextest.toml`, wrapper
//! script `test-desktop`): `test-desktop <test binary> <arguments>…`. It opens the desktop
//! `flui-test-<run id>` in the current window station, creating it if no other test of the run
//! holds it, starts the test there, waits for it and exits with its exit code. A process started
//! on a desktop has every thread and every child process on it, so a test needs no code for
//! this. The desktop is a kernel object: it goes once the last test of the run that holds it
//! exits, crashed or not, and the run's tests share one desktop heap.
//!
//! `FLUI_TEST_VISIBLE_DESKTOP=1` starts the test on the caller's desktop instead, to watch it.

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows_desktop::main()
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("test-desktop: a test desktop exists only on Windows");
    std::process::ExitCode::FAILURE
}

#[cfg(windows)]
mod windows_desktop {
    #![expect(
        unsafe_code,
        reason = "Win32 FFI; each call takes plain values or pointers to locals"
    )]

    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::process::{Command, ExitCode};

    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        SetConsoleCtrlHandler,
    };
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, CreateDesktopW, DESKTOP_CONTROL_FLAGS, GetProcessWindowStation,
        GetUserObjectInformationW, HDESK, UOI_NAME,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, INFINITE, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION,
        STARTF_USESTDHANDLES, STARTUPINFOW, WaitForSingleObject,
    };
    use windows::core::{PCWSTR, PWSTR};

    /// The variable that starts the test on the caller's desktop.
    const VISIBLE: &str = "FLUI_TEST_VISIBLE_DESKTOP";

    /// `GENERIC_ALL`: the creator may do anything with the desktop it made.
    const GENERIC_ALL: u32 = 0x1000_0000;

    pub(super) fn main() -> ExitCode {
        let mut args = std::env::args_os().skip(1);
        let Some(program) = args.next() else {
            eprintln!("usage: test-desktop <program> [arguments]…");
            return ExitCode::FAILURE;
        };
        let args: Vec<OsString> = args.collect();
        if std::env::var_os(VISIBLE).is_some_and(|value| value == "1") {
            return on_this_desktop(&program, &args);
        }
        match on_the_test_desktop(&program, &args) {
            Ok(code) => {
                // A process's exit code is a `u32` (an NTSTATUS for a crash), and
                // `process::exit` takes the same bits as an `i32`.
                #[expect(
                    clippy::cast_possible_wrap,
                    reason = "the exit code's bits are passed on unchanged"
                )]
                std::process::exit(code as i32)
            }
            Err(error) => {
                eprintln!(
                    "test-desktop: could not start {} on the test desktop: {error}\n\
                     test-desktop: {VISIBLE}=1 runs it on this desktop instead",
                    program.display()
                );
                ExitCode::FAILURE
            }
        }
    }

    /// Starts the program here, as if no wrapper were in between.
    fn on_this_desktop(program: &OsStr, args: &[OsString]) -> ExitCode {
        match Command::new(program).args(args).status() {
            Ok(status) => status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .map_or(ExitCode::FAILURE, ExitCode::from),
            Err(error) => {
                eprintln!(
                    "test-desktop: could not start {}: {error}",
                    program.display()
                );
                ExitCode::FAILURE
            }
        }
    }

    /// The name of the run's desktop: one per nextest run, so two runs at once (two agents,
    /// two worktrees) never share a foreground.
    fn desktop_name() -> String {
        let run = std::env::var("NEXTEST_RUN_ID").unwrap_or_else(|_| "manual".to_owned());
        format!("flui-test-{run}")
    }

    /// A desktop handle closed when it goes out of scope.
    struct Desktop(HDESK);

    impl Drop for Desktop {
        fn drop(&mut self) {
            // SAFETY: the handle came from `CreateDesktopW` and is closed once, here.
            let _closed = unsafe { CloseDesktop(self.0) };
        }
    }

    /// A process or thread handle closed when it goes out of scope.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle came from `CreateProcessW` and is closed once, here.
            let _closed = unsafe { CloseHandle(self.0) };
        }
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// The name of this process's window station (`WinSta0` in an interactive session).
    fn window_station() -> windows::core::Result<OsString> {
        // SAFETY: the station handle is the process's own and is not closed.
        let station = unsafe { GetProcessWindowStation() }?;
        let mut name = [0u16; 256];
        let mut needed = 0u32;
        let bytes = u32::try_from(std::mem::size_of_val(&name)).expect("BUG: 512 fits a u32");
        // SAFETY: `name` holds `bytes` bytes, the length passed.
        unsafe {
            GetUserObjectInformationW(
                HANDLE(station.0),
                UOI_NAME,
                Some(name.as_mut_ptr().cast()),
                bytes,
                Some(&raw mut needed),
            )
        }?;
        let len = name
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(name.len());
        Ok(OsString::from_wide(&name[..len]))
    }

    /// Opens the run's desktop, creating it when no process holds it.
    fn open_desktop(name: &str) -> windows::core::Result<Desktop> {
        let name = wide(OsStr::new(name));
        // SAFETY: `name` is NUL-terminated; no device, mode or security attributes.
        let desktop = unsafe {
            CreateDesktopW(
                PCWSTR(name.as_ptr()),
                PCWSTR::null(),
                None,
                DESKTOP_CONTROL_FLAGS(0),
                GENERIC_ALL,
                None,
            )
        }?;
        Ok(Desktop(desktop))
    }

    /// Marks a standard handle inheritable, so the child receives it.
    fn inheritable(which: STD_HANDLE) -> HANDLE {
        // SAFETY: reads this process's standard handle.
        let handle = unsafe { GetStdHandle(which) }.unwrap_or_default();
        if !handle.is_invalid() && !handle.0.is_null() {
            // A console handle of an older Windows refuses the flag and is inherited anyway.
            // SAFETY: the handle is this process's own standard handle.
            let _marked =
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) };
        }
        handle
    }

    fn on_the_test_desktop(program: &OsStr, args: &[OsString]) -> windows::core::Result<u32> {
        let name = desktop_name();
        let desktop = open_desktop(&name)?;
        let mut station_and_desktop = window_station()?;
        station_and_desktop.push("\\");
        station_and_desktop.push(&name);
        let mut station_and_desktop = wide(&station_and_desktop);
        let mut command_line = command_line(program, args);

        let startup = STARTUPINFOW {
            cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>())
                .expect("BUG: STARTUPINFOW's size fits a u32"),
            lpDesktop: PWSTR(station_and_desktop.as_mut_ptr()),
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: inheritable(STD_INPUT_HANDLE),
            hStdOutput: inheritable(STD_OUTPUT_HANDLE),
            hStdError: inheritable(STD_ERROR_HANDLE),
            ..STARTUPINFOW::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        // The child shares this console; Ctrl+C reaches it, and this process waits for it to
        // end instead of leaving it behind.
        // SAFETY: no handler routine; `true` ignores Ctrl+C in this process.
        let _ignored = unsafe { SetConsoleCtrlHandler(None, true) };
        // SAFETY: the command line and the desktop name are NUL-terminated buffers that outlive
        // the call; the environment and the working directory are inherited.
        unsafe {
            CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                true,
                PROCESS_CREATION_FLAGS(0),
                None,
                PCWSTR::null(),
                &raw const startup,
                &raw mut process,
            )
        }?;
        let child = Owned(process.hProcess);
        drop(Owned(process.hThread));
        // SAFETY: `child` is a live process handle.
        let waited = unsafe { WaitForSingleObject(child.0, INFINITE) };
        if waited != WAIT_OBJECT_0 {
            return Err(windows::core::Error::from_thread());
        }
        let mut code = 0u32;
        // SAFETY: `child` has exited; `code` receives its exit code.
        unsafe { GetExitCodeProcess(child.0, &raw mut code) }?;
        drop(child);
        drop(desktop);
        Ok(code)
    }

    /// The command line `CreateProcessW` parses back into `program` and `args`, quoted by the
    /// rules of the Microsoft C runtime: a backslash is literal unless a run of them precedes a
    /// quote.
    fn command_line(program: &OsStr, args: &[OsString]) -> Vec<u16> {
        let mut line = Vec::new();
        push_quoted(&mut line, program);
        for arg in args {
            line.push(u16::from(b' '));
            push_quoted(&mut line, arg);
        }
        line.push(0);
        line
    }

    fn push_quoted(line: &mut Vec<u16>, arg: &OsStr) {
        let units: Vec<u16> = arg.encode_wide().collect();
        let plain = !units.is_empty()
            && !units
                .iter()
                .any(|&unit| matches!(unit, 0x20 | 0x09 | 0x0A | 0x0B | 0x22));
        if plain {
            line.extend_from_slice(&units);
            return;
        }
        let quote = u16::from(b'"');
        let backslash = u16::from(b'\\');
        line.push(quote);
        let mut backslashes = 0usize;
        for &unit in &units {
            if unit == backslash {
                backslashes += 1;
                continue;
            }
            if unit == quote {
                line.extend(std::iter::repeat_n(backslash, backslashes * 2 + 1));
            } else {
                line.extend(std::iter::repeat_n(backslash, backslashes));
            }
            backslashes = 0;
            line.push(unit);
        }
        line.extend(std::iter::repeat_n(backslash, backslashes * 2));
        line.push(quote);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_command_line_parses_back_into_its_arguments() {
            for (args, expected) in [
                (&["a.exe", "--exact", "x::y"][..], r"a.exe --exact x::y"),
                (&[r"C:\a b\t.exe", ""][..], r#""C:\a b\t.exe" """#),
                (&["t", r#"say "hi""#][..], r#"t "say \"hi\"""#),
                (&["t", r"dir\ "][..], r#"t "dir\ ""#),
                (&["t", r"end\"][..], r"t end\"),
                (&["t", r"x y\"][..], r#"t "x y\\""#),
            ] {
                let args: Vec<OsString> = args.iter().map(OsString::from).collect();
                let line = command_line(&args[0], &args[1..]);
                assert_eq!(
                    String::from_utf16(&line[..line.len() - 1]).expect("BUG: UTF-16 line"),
                    expected
                );
            }
        }
    }
}
