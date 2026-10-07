//! The real `cargo xtask` binary takes the run lock before a heavy command.
//!
//! A separate target: it spawns the built binary, which only an integration
//! test can name (`CARGO_BIN_EXE_xtask`).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Dependencies can compile a library without checking its target-specific
/// tests, examples and benches. The public plan must select those packages
/// directly, including packages outside the platform backend list.
#[test]
fn native_typecheck_selects_target_gated_workspace_targets() {
    let dir = tempfile::tempdir().expect("temp dir");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["cross-typecheck", "--dry-run"])
        .env("FLUI_XTASK_LOCK_FILE", dir.path().join("heavy.lock"))
        .output()
        .expect("run the public cross-typecheck plan");
    assert!(output.status.success(), "{output:?}");
    let plan = String::from_utf8(output.stdout).expect("UTF-8 command plan");
    for (package, target) in [
        ("flui-engine", "aarch64-apple-darwin"),
        ("flui-widgets", "aarch64-apple-darwin"),
        ("flui-hot-reload", "x86_64-pc-windows-msvc"),
    ] {
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

/// `clean-nested` is heavy (it deletes the nested-test caches) and, pointed
/// at an empty target directory, deletes nothing: held behind the lock, it
/// must say so and wait, then finish once the lock is released.
#[test]
fn a_heavy_command_waits_for_the_run_lock() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lock_path = dir.path().join("heavy.lock");
    let lock = File::create(&lock_path).expect("create the lock file");
    lock.lock().expect("hold the run lock");

    let child = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("clean-nested")
        .env("FLUI_XTASK_LOCK_FILE", &lock_path)
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .env_remove("FLUI_XTASK_NO_LOCK")
        .env_remove("FLUI_XTASK_LOCK_HELD")
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
