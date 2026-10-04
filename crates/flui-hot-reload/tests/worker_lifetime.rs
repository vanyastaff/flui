//! Real dynamic-image lifetime through the consumer worker API.
#![expect(
    unsafe_code,
    reason = "same-toolchain fixture implements the documented worker Rust ABI"
)]

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn bounded_output(mut command: Command, timeout: Duration) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fixture process");
    let deadline = Instant::now() + timeout;
    while child.try_wait().expect("fixture status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill timed-out fixture");
            let output = child.wait_with_output().expect("reap fixture");
            panic!("worker fixture exceeded deadline: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().expect("fixture output")
}

fn build_fixture(directory: &Path, value: u32) -> std::path::PathBuf {
    let source = directory.join(format!("worker_{value}.rs"));
    let library = directory.join(format!(
        "worker_{value}.{}",
        std::env::consts::DLL_EXTENSION
    ));
    let fixture = r#"
use std::sync::atomic::{AtomicUsize, Ordering};
struct Callback { value: u32, drops: *const AtomicUsize }
impl Callback { fn value(&self) -> u32 { self.value } }
impl Drop for Callback {
    fn drop(&mut self) { unsafe { (*self.drops).fetch_add(1, Ordering::SeqCst); } }
}
fn build(drops: *const AtomicUsize) -> Box<dyn Fn() -> u32> {
    let callback = Callback { value: VALUE, drops };
    Box::new(move || callback.value())
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_init(register: extern "C" fn(u64, *const ())) {
    register(0xC007_EA01, build as *const ());
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_abi_token() -> u64 { TOKEN }
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_version() -> u32 { VALUE }
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_fingerprint() -> u64 { 0xC007_EA01 }
"#;
    std::fs::write(
        &source,
        fixture
            .replace("VALUE", &value.to_string())
            .replace("TOKEN", &flui_hot_reload::abi_token().to_string()),
    )
    .expect("write actual dynamic fixture");
    let mut command = Command::new("rustc");
    command
        .args(["--edition=2024", "--crate-type=cdylib"])
        .arg(&source)
        .arg("-o")
        .arg(&library);
    let output = bounded_output(command, Duration::from_secs(60));
    assert!(
        output.status.success(),
        "worker fixture compilation failed: {output:?}"
    );
    library
}

fn worker_lifetime_child(directory: &Path) {
    const FINGERPRINT: u64 = 0xC007_EA01;
    type Build = fn(*const AtomicUsize) -> Box<dyn Fn() -> u32>;
    let first_path = build_fixture(directory, 1);
    let next_path = build_fixture(directory, 2);
    let drops = AtomicUsize::new(0);
    let first =
        flui_hot_reload::WorkerPlugin::load(&first_path).expect("first actual worker loads");
    let pointer =
        flui_hot_reload::get_worker_build_ptr(FINGERPRINT).expect("first build registered");
    // SAFETY: the fixture registers Build's exact signature; both artifacts use
    // the pinned compiler, and drops remains alive through every callback Drop.
    let first_build: Build = unsafe { std::mem::transmute(pointer) };
    let old_callback = first_build(&raw const drops);
    assert_eq!(old_callback(), 1);
    first.unload();
    assert!(
        flui_hot_reload::get_worker_build_ptr(FINGERPRINT).is_none(),
        "retirement prunes dispatch"
    );
    let next = flui_hot_reload::WorkerPlugin::load(&next_path).expect("replacement worker loads");
    let pointer =
        flui_hot_reload::get_worker_build_ptr(FINGERPRINT).expect("replacement registered");
    // SAFETY: the replacement fixture uses the same Build contract and compiler.
    let next_build: Build = unsafe { std::mem::transmute(pointer) };
    let next_callback = next_build(&raw const drops);
    assert_eq!(next_callback(), 2, "fresh dispatch follows replacement");
    assert_eq!(
        old_callback(),
        1,
        "escaped callback still runs old image code"
    );
    drop(old_callback);
    drop(next_callback);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        2,
        "both plugin-defined destructors ran"
    );
    let later_callback = next_build(&raw const drops);
    next.unload();
    assert!(flui_hot_reload::get_worker_build_ptr(FINGERPRINT).is_none());
    assert_eq!(
        later_callback(),
        2,
        "driver retirement does not invalidate escaped values"
    );
    drop(later_callback);
    assert_eq!(drops.load(Ordering::SeqCst), 3);
}

#[test]
fn admitted_worker_callbacks_survive_retirement_and_replacement() {
    if let Some(directory) = std::env::var_os("FLUI_WORKER_LIFETIME_CHILD") {
        worker_lifetime_child(Path::new(&directory));
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "flui-worker-lifetime-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock follows epoch")
            .as_nanos()
    ));
    std::fs::create_dir(&directory).expect("create fixture directory");
    let mut command = Command::new(std::env::current_exe().expect("consumer executable"));
    command
        .args([
            "--exact",
            "worker_lifetime::admitted_worker_callbacks_survive_retirement_and_replacement",
            "--nocapture",
        ])
        .env("FLUI_WORKER_LIFETIME_CHILD", &directory);
    let output = bounded_output(command, Duration::from_secs(150));
    // The child has exited, so Windows loader locks are gone before cleanup.
    std::fs::remove_dir_all(&directory).expect("remove isolated dynamic fixtures");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "worker descendant lifetime failed: {output:?}"
    );
}
