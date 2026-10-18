//! The real `cargo xtask` binary takes the run lock before a heavy command.
//!
//! A separate target: it spawns the built binary, which only an integration
//! test can name (`CARGO_BIN_EXE_xtask`).

use std::any::Any;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Dependencies can compile a library without checking its target-specific
/// tests, examples and benches. The public plan must select those packages
/// directly, including packages outside the platform backend list.
#[test]
fn native_typecheck_selects_target_gated_workspace_targets() {
    for (all_features, only) in [
        (false, None),
        (true, Some("aarch64-apple-darwin")),
        (true, Some("aarch64-linux-android")),
    ] {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.args(["cross-typecheck", "--dry-run"]);
        if all_features {
            command.arg("--all-features");
        }
        if let Some(target) = only {
            command.args(["--target", target]);
        }
        let output = command
            .env("FLUI_XTASK_LOCK_FILE", dir.path().join("heavy.lock"))
            .output()
            .expect("run the public cross-typecheck plan");
        assert!(output.status.success(), "{output:?}");
        let plan = String::from_utf8(output.stdout).expect("UTF-8 command plan");
        if only == Some("aarch64-apple-darwin") && !cfg!(target_os = "macos") {
            assert!(
                plan.contains("Skipping all-features macOS") && plan.contains("Apple SDK"),
                "{plan}"
            );
            assert!(
                !plan.lines().any(|line| line.starts_with("$ ")),
                "an SDK-only leg must not run: {plan}"
            );
            continue;
        }
        for (package, target) in [
            ("flui-engine", "aarch64-apple-darwin"),
            ("flui-widgets", "aarch64-apple-darwin"),
            ("flui-hot-reload", "x86_64-pc-windows-msvc"),
            ("flui-platform", "aarch64-linux-android"),
        ] {
            if only.is_some_and(|only| only != target) {
                continue;
            }
            assert!(
                plan.lines().any(|line| {
                    let args: Vec<_> = line.split_whitespace().collect();
                    args.windows(2).any(|pair| pair == ["-p", package])
                        && args.windows(2).any(|pair| pair == ["--target", target])
                        && args.contains(&"--all-targets")
                }),
                "missing direct all-targets coverage for {package} on {target}:\n{plan}"
            );
        }
        if all_features {
            let commands: Vec<_> = plan.lines().filter(|line| line.starts_with("$ ")).collect();
            assert_eq!(
                commands.len(),
                1,
                "--target must select only the requested native leg"
            );
            assert!(
                commands[0]
                    .split_whitespace()
                    .any(|arg| arg == "--all-features"),
                "optional native features must reach Cargo: {plan}"
            );
        }
    }
}

/// Kills the child if the test fails before it exits.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// `cargo xtask clean-nested` against `lock_path`, with none of what decides
/// whom it waits for inherited from the test's own environment (an agent's
/// shell may carry a host pool).
fn clean_nested(dir: &Path, lock_path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("clean-nested")
        .env("FLUI_XTASK_LOCK_FILE", lock_path)
        .env("CARGO_TARGET_DIR", dir.join("target"));
    for key in [
        "FLUI_XTASK_NO_LOCK",
        "FLUI_XTASK_LOCK_HELD",
        "HELI_RUN",
        "CARGO_MAKEFLAGS",
        "MAKEFLAGS",
        "MFLAGS",
    ] {
        command.env_remove(key);
    }
    command
}

/// `clean-nested` is heavy (it deletes the nested-test caches) and, pointed
/// at an empty target directory, deletes nothing: held behind the lock, it
/// must say so and wait, then finish once the lock is released. A jobserver
/// it cannot reach (a stale variable, a stopped pool) does not excuse it.
#[test]
fn a_heavy_command_waits_for_the_run_lock() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lock_path = dir.path().join("heavy.lock");
    let lock = File::create(&lock_path).expect("create the lock file");
    lock.lock().expect("hold the run lock");

    let child = clean_nested(dir.path(), &lock_path)
        .env(
            "CARGO_MAKEFLAGS",
            "-j --jobserver-auth=flui-xtask-test-stopped-pool",
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cargo xtask clean-nested");
    let mut child = Reaped(child);
    let stderr = child.0.stderr.take().expect("piped stderr");
    let (lines, received) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    let waiting = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match received.recv_timeout(left) {
            Ok(line) if line.contains("xtask: waiting for") => break line,
            Ok(_) => {}
            Err(error) => panic!("the run never said it was waiting for the lock ({error})"),
        }
    };
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        matches!(child.0.try_wait(), Ok(None)),
        "the run must not finish while the lock is held ({waiting})"
    );

    lock.unlock().expect("release the run lock");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("poll the run") {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the run did not finish after the lock was released"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "the run proceeds once the lock is free");
}

/// A jobserver of the kind a host pool makes on this platform (a named
/// semaphore on Windows, a FIFO elsewhere), with its `--jobserver-auth`
/// value; it exists while the returned value lives.
#[cfg(windows)]
#[expect(unsafe_code, reason = "creating a named semaphore is a Win32 call")]
fn live_pool(_dir: &Path) -> (String, impl Sized + use<>) {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::CreateSemaphoreW;
    use windows::core::HSTRING;
    struct Semaphore(HANDLE);
    impl Drop for Semaphore {
        fn drop(&mut self) {
            // SAFETY: the handle was created below and is closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    let name = format!("flui-xtask-run-lock-pool-{}", std::process::id());
    // SAFETY: no security attributes; the name outlives the call.
    let handle = unsafe { CreateSemaphoreW(None, 4, 4, &HSTRING::from(&name)) }
        .expect("create the pool's semaphore");
    (name, Semaphore(handle))
}

#[cfg(unix)]
fn live_pool(dir: &Path) -> (String, impl Sized + use<>) {
    let fifo = dir.join("pool");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo: {status}");
    (format!("fifo:{}", fifo.display()), ())
}

/// A run something else already schedules within the host budget (heli
/// runs it, or it inherits a reachable host pool) must not queue behind an
/// unrelated heavy run in another checkout. A pool says so in one line.
#[test]
fn a_scheduled_command_proceeds_while_another_checkout_holds_the_host_lock() {
    fn heli_run(command: &mut Command, _dir: &Path) -> (Option<&'static str>, Box<dyn Any>) {
        command.env("HELI_RUN", "r17");
        (None, Box::new(()))
    }
    fn reachable_pool(command: &mut Command, dir: &Path) -> (Option<&'static str>, Box<dyn Any>) {
        let (auth, pool) = live_pool(dir);
        command.env("CARGO_MAKEFLAGS", format!("-j --jobserver-auth={auth}"));
        (Some("not taking the run lock"), Box::new(pool))
    }
    /// Sets how the run is scheduled; what its stderr must say, and what
    /// must live while it runs.
    type Schedule = fn(&mut Command, &Path) -> (Option<&'static str>, Box<dyn Any>);
    let rows: [(&str, Schedule); 2] = [("heli_run", heli_run), ("reachable_pool", reachable_pool)];
    for (row, schedule) in rows {
        let dir = tempfile::tempdir().expect("temp dir");
        let lock_path = dir.path().join("heavy.lock");
        let lock = File::create(&lock_path).expect("lock file");
        lock.lock().expect("another checkout holds the lock");
        let mut command = clean_nested(dir.path(), &lock_path);
        let (says, _pool) = schedule(&mut command, dir.path());
        let child = command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the scheduled command");
        let mut child = Reaped(child);
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = child.0.try_wait().expect("poll command") {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "{row}: the run waited behind another checkout's host lock"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut stderr = String::new();
        child
            .0
            .stderr
            .take()
            .expect("piped stderr")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        assert!(status.success(), "{row}: {status}\n{stderr}");
        if let Some(says) = says {
            assert!(
                stderr.contains(says),
                "{row}: stderr says `{says}`:\n{stderr}"
            );
        }
        drop(lock);
    }
}
