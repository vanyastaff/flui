//! The self-test's crate: source files in memory, with one planted violation
//! per rule and kind and one silent case per shape the rule admits.

use std::collections::BTreeMap;

use super::{ARGUMENT, IDENTITY, UNGUARDED, UNREAD};
use crate::globals::scan::Source;

/// The crate root.
pub(super) const ROOT: &str = "fixture/src/lib.rs";

/// In-memory files, repository-relative.
pub(super) struct Memory(pub(super) BTreeMap<String, String>);

impl Source for Memory {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }
}

/// `(file, entry or item, kind)` of every planted finding.
pub(super) const EXPECTED: [(&str, &str, &str); 19] = [
    (WINDOWS, "planted", UNGUARDED),
    (WINDOWS, "first_then", UNGUARDED),
    (WINDOWS, "eager", ARGUMENT),
    (WINDOWS, "misadapted", UNGUARDED),
    (WINDOWS, "ITextStore_Impl::Planted", UNGUARDED),
    (WINDOWS, "TypedEventHandler::new", UNGUARDED),
    (WINDOWS, "ActionHandler::do_action", UNGUARDED),
    (APPLE, "will_terminate", UNGUARDED),
    (APPLE, "key_down", UNGUARDED),
    (APPLE, "RcBlock::new", UNGUARDED),
    (APPLE, "StackBlock::new", UNGUARDED),
    (APPLE, ".exec_async", UNGUARDED),
    (APPLE, "fn hidden", UNREAD),
    (APPLE, "define_class!", UNREAD),
    (ANDROID, "Java_dev_flui_Planted_run", UNGUARDED),
    (IDENTITY_FILE, "fn os_entry", IDENTITY),
    (IDENTITY_FILE, "use other::os_entry", IDENTITY),
    (IDENTITY_FILE, "mod panic_boundary", IDENTITY),
    (SECOND_MODULE, "mod panic_boundary", IDENTITY),
];

const WINDOWS: &str = "fixture/src/feature/windows.rs";
const APPLE: &str = "fixture/src/feature/apple.rs";
const ANDROID: &str = "fixture/src/feature/android.rs";
const IDENTITY_FILE: &str = "fixture/src/feature/identity.rs";
const SECOND_MODULE: &str = "fixture/src/feature/panic_boundary.rs";

/// The crate.
pub(super) fn source() -> Memory {
    Memory(
        FILES
            .iter()
            .map(|&(rel, text)| (rel.to_owned(), text.to_owned()))
            .collect(),
    )
}

const FILES: [(&str, &str); 10] = [
    (ROOT, LIB),
    ("fixture/src/owner.rs", "pub(crate) mod panic_boundary;\n"),
    ("fixture/src/owner/panic_boundary.rs", BOUNDARY),
    ("fixture/src/feature.rs", FEATURE),
    (WINDOWS, WINDOWS_FILE),
    (APPLE, APPLE_FILE),
    (ANDROID, ANDROID_FILE),
    (IDENTITY_FILE, IDENTITY_TEXT),
    (SECOND_MODULE, "pub(crate) fn nothing() {}\n"),
    // never read: mounted under `cfg(test)`
    (
        "fixture/src/tests.rs",
        "extern \"C\" fn stub() -> u32 { 7 }\n",
    ),
];

const LIB: &str = r"
mod feature;
mod owner;
#[cfg(test)]
mod tests;
";

const BOUNDARY: &str = r"
pub(crate) enum AfterPanic<R> {
    Answer(fn() -> R),
    Abort,
}

pub(crate) fn os_entry<R>(
    entry: &'static str,
    after_panic: AfterPanic<R>,
    body: impl FnOnce() -> R,
) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(answer) => answer,
        Err(_) => match after_panic {
            AfterPanic::Answer(answer) => answer(),
            AfterPanic::Abort => std::process::abort(),
        },
    }
}
";

const FEATURE: &str = r"
mod android;
mod apple;
mod identity;
mod panic_boundary;
mod windows;
";

const WINDOWS_FILE: &str = r#"
use crate::owner::panic_boundary::{AfterPanic, os_entry};

// silent: the body is the boundary's call
unsafe extern "system" fn guarded(hwnd: isize, message: u32) -> isize {
    os_entry("guarded", AfterPanic::Answer(|| 0), || hwnd + message as isize)
}

// silent: a unit entry, the call with its semicolon
extern "system" fn unit(value: u32) {
    os_entry("unit", AfterPanic::Abort, move || drop(value));
}

// planted: no boundary
unsafe extern "C" fn planted(value: u32) -> u32 {
    value + 1
}

// planted: a statement before the boundary's call
extern "system" fn first_then(value: u32) -> u32 {
    let doubled = value * 2;
    os_entry("first_then", AfterPanic::Answer(|| 0), move || doubled)
}

// planted: an argument that runs before the boundary
extern "system" fn eager(value: u32) -> u32 {
    os_entry(name(), AfterPanic::Answer(|| 0), move || value)
}

fn name() -> &'static str {
    "eager"
}

// silent: through an adapter of this file
extern "system" fn adapted(value: u32) -> u32 {
    com(|| value)
}

fn com(body: impl FnOnce() -> u32) -> u32 {
    os_entry("com", AfterPanic::Answer(|| 0), body)
}

// planted: a function that is not an adapter
extern "system" fn misadapted(value: u32) -> u32 {
    helper(|| value)
}

fn helper(body: impl FnOnce() -> u32) -> u32 {
    body()
}

// silent: declarations and pointer types are not entries
extern "system" {
    fn GetTickCount() -> u32;
}
type Procedure = unsafe extern "system" fn(isize, u32) -> isize;

struct Store;

impl Store {
    fn com_entry(&self, method: &'static str, body: impl FnOnce() -> u32) -> u32 {
        crate::owner::panic_boundary::os_entry(method, AfterPanic::Abort, body)
    }
}

impl ITextStore_Impl for Store_Impl {
    // silent: through a method adapter
    fn Guarded(&self) -> u32 {
        self.com_entry("Guarded", || 1)
    }

    // planted: a COM method without a boundary
    fn Planted(&self) -> u32 {
        2
    }
}

fn subscribe(settings: &UISettings) {
    // planted: a WinRT delegate without a boundary
    let _ = settings.Changed(&TypedEventHandler::new(move |_, _| Ok(())));
    // silent: a delegate through the boundary
    let _ = settings.Changed(&TypedEventHandler::new(move |_, _| {
        os_entry("delegate", AfterPanic::Answer(|| Ok(())), || Ok(()))
    }));
}

struct Action;

// planted: an AccessKit handler without a boundary
impl ActionHandler for Action {
    fn do_action(&mut self, request: u32) {
        let _ = request;
    }
}

// silent: test code
#[cfg(test)]
extern "C" fn stub() -> u32 {
    7
}
"#;

const APPLE_FILE: &str = r#"
use crate::owner::panic_boundary;
use crate::owner::panic_boundary::AfterPanic;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "Delegate"]
    #[ivars = Weak<Control>]
    struct Delegate;

    unsafe impl NSApplicationDelegate for Delegate {
        // silent: a method through the boundary
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish(&self, _notification: &NSNotification) {
            panic_boundary::os_entry("did_finish", AfterPanic::Answer(|| ()), || ())
        }

        // planted: a method without a boundary
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            tracing::info!("terminating");
        }
    }
);

// planted: an unparsable body
define_class!(
    struct Broken;
    impl Broken { fn }
);

// planted: a ClassBuilder method without a boundary
extern "C-unwind" fn key_down(_this: &AnyObject, _sel: Sel) {}

fn observe(handler: impl Fn() + 'static) {
    // planted: a block without a boundary
    let _ = RcBlock::new(|| ());
    // planted: a block whose closure the gate cannot see
    let _ = StackBlock::new(handler);
}

fn dispatch(queue: &DispatchQueue) {
    // planted: a GCD work item without a boundary
    queue.exec_async(|| ());
    // silent: through the boundary
    queue.exec_async(|| panic_boundary::os_entry("work", AfterPanic::Abort, || ()));
}

// planted: an extern fn the gate cannot read
some_macro! {
    extern "C" fn hidden() {}
}

// silent: a pointer type in a macro's tokens
ndk_function!(fn_type, unsafe extern "C" fn(i32) -> i32);
"#;

const ANDROID_FILE: &str = r#"
use crate::owner::panic_boundary::{AfterPanic, os_entry};

fn contained<T: Default>(method: &'static str, body: impl FnOnce() -> T) -> T {
    os_entry(method, AfterPanic::Answer(T::default), body)
}

// silent: a JNI export through an adapter
#[unsafe(no_mangle)]
extern "system" fn Java_dev_flui_Guarded_run(_env: JNIEnv, _class: JClass, handle: i64) -> i64 {
    contained("run", || handle)
}

// planted: a JNI export without a boundary
#[unsafe(no_mangle)]
extern "system" fn Java_dev_flui_Planted_run(_env: JNIEnv, _class: JClass, handle: i64) -> i64 {
    handle
}
"#;

const IDENTITY_TEXT: &str = r"
// planted: an import of another `os_entry`
use other::os_entry;

// planted: a second `os_entry`
fn os_entry() {}

// planted: a second module named `panic_boundary`
mod panic_boundary {}
";
