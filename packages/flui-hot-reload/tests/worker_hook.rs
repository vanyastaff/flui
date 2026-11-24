//! A worker hook loads its worker on the thread that attaches it, through the
//! public hook and a real dynamic image.

use flui_hot_reload::{WorkerReloadHook, get_worker_build_ptr};
use flui_sdk::view::dev_reload::{DevReloadHook, ReloadWake};

use crate::worker_lifetime::compile_fixture;

/// An application makes its hook while it builds its configuration, which
/// may be on another thread than the one that runs the event loop and looks
/// the worker's builds up. Fails when the hook loads the worker as it is made:
/// the builds then land in the registry of the thread that made it, and the
/// loop's thread finds none.
#[test]
fn a_worker_hook_made_on_another_thread_registers_on_the_loop_thread() {
    const FINGERPRINT: u64 = 0x100B_7E4D;
    let directory =
        std::env::temp_dir().join(format!("flui-worker-hook-thread-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&directory));
    std::fs::create_dir(&directory).expect("create fixture directory");
    let worker = compile_fixture(
        &directory,
        "loop_thread_worker",
        r#"
fn build() {}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_init(register: extern "C" fn(u64, *const ()), _request_rebuild: extern "C-unwind" fn()) -> bool {
    register(0x100B_7E4D, build as *const ());
    true
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_abi_token() -> u64 { TOKEN }
"#,
    );

    let mut hook = std::thread::spawn(move || WorkerReloadHook::new(worker))
        .join()
        .expect("the configuring thread runs to its end");
    hook.attach(ReloadWake::new(|| {}));
    hook.poll();
    assert!(
        get_worker_build_ptr(FINGERPRINT).is_some(),
        "the loop's thread finds the build its worker registered"
    );
    drop(hook);
    drop(std::fs::remove_dir_all(&directory));
}
