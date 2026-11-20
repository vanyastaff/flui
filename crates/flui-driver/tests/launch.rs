//! What a launched app leaves behind when its session ends, on Unix: nothing it started.

use std::process::Command;
use std::time::{Duration, Instant};

use flui_driver::{Driver, Launch};

/// Whether the process `pid` is still there, as the shell's `kill -0` answers.
fn alive(pid: &str) -> bool {
    Command::new("/bin/sh")
        .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
        .status()
        .expect("the shell runs")
        .success()
}

/// The app leads a process group of its own, which ends with the session: a grandchild the app
/// started dies with it. The app here starts one and never serves its agent, so the session ends
/// at its connect timeout.
#[test]
fn a_launched_apps_process_group_ends_with_its_session() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let pid_file = dir.path().join("grandchild.pid");
    let script = format!("sleep 600 & echo $! > '{}'; wait", pid_file.display());
    let launched = Driver::launch(
        Launch::executable("/bin/sh")
            .arg("-c")
            .arg(script)
            .connect_wait(Duration::from_secs(1)),
    );
    assert!(launched.is_err(), "the script serves no agent");
    let grandchild = std::fs::read_to_string(&pid_file).expect("the app started its grandchild");
    let grandchild = grandchild.trim();
    assert!(!grandchild.is_empty(), "the grandchild's pid was written");
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(grandchild) {
        assert!(
            Instant::now() < deadline,
            "grandchild {grandchild} outlived the session that launched its parent"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
