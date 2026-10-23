//! The self-test's crate: source files in memory, with one planted violation
//! per rule and kind and one silent case per shape the rule admits.

use super::Problem::{self, Argument, Identity, Unguarded, Unread};
pub(super) use crate::source_tree::Memory;

/// The crate root.
pub(super) const ROOT: &str = "fixture/src/lib.rs";

/// `(file, entry or item, kind)` of every planted finding.
pub(super) const EXPECTED: [(&str, &str, Problem); 30] = [
    (WINDOWS, "planted", Unguarded),
    (WINDOWS, "first_then", Unguarded),
    (WINDOWS, "eager", Argument),
    (WINDOWS, "answer_runs", Argument),
    (WINDOWS, "misadapted", Unguarded),
    (WINDOWS, "shadowed", Unguarded),
    (WINDOWS, "procedure_method", Unguarded),
    (WINDOWS, "default_callback", Unguarded),
    (WINDOWS, "ITextStore_Impl::Planted", Unguarded),
    (WINDOWS, "ITextStore_Impl::ByMethodCall", Unguarded),
    (WINDOWS, "ITextStore_Impl::OtherType", Unguarded),
    (WINDOWS, "TypedEventHandler::new", Unguarded),
    (WINDOWS, "ActionHandler::do_action", Unguarded),
    (
        WINDOWS,
        "NSOperationQueue::addOperationWithBlock",
        Unguarded,
    ),
    (APPLE, "will_terminate", Unguarded),
    (APPLE, "inactive", Unguarded),
    (APPLE, "key_down", Unguarded),
    (APPLE, "RcBlock::new", Unguarded),
    (APPLE, "StackBlock::new", Unguarded),
    (APPLE, ".exec_async", Unguarded),
    (APPLE, ".exec_after", Argument),
    (APPLE, "fn hidden", Unread),
    (APPLE, "RcBlock::new", Unread),
    (APPLE, ".exec_sync", Unread),
    (APPLE, "define_class!", Unread),
    (ANDROID, "Java_dev_flui_Planted_run", Unguarded),
    (IDENTITY_FILE, "fn os_entry", Identity),
    (IDENTITY_FILE, "use other::os_entry", Identity),
    (IDENTITY_FILE, "mod panic_boundary", Identity),
    (SECOND_MODULE, "mod panic_boundary", Identity),
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
pub(crate) enum AfterPanic<A, R> {
    Answer(fn(A) -> R),
    Abort,
}

pub(crate) fn os_entry<A: Copy, R>(
    entry: &'static str,
    args: A,
    after_panic: AfterPanic<A, R>,
    body: impl FnOnce() -> R,
) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(answer) => answer,
        Err(_) => match after_panic {
            AfterPanic::Answer(answer) => answer(args),
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
use crate::other::send;
use objc2_foundation::NSOperationQueue;
use windows::Foundation::TypedEventHandler;

// silent: the body is the boundary's call, its arguments a tuple of paths
unsafe extern "system" fn guarded(hwnd: isize, message: u32) -> isize {
    os_entry("guarded", (message, hwnd), AfterPanic::Answer(|(_, _)| 0), || hwnd)
}

// silent: a unit entry, the call with its semicolon
extern "system" fn unit(value: u32) {
    os_entry("unit", (), AfterPanic::Abort, move || drop(value));
}

// silent: a constant answer runs no code
extern "system" fn constant(_value: u32) -> u32 {
    WM_NULL
}

// planted: no boundary
unsafe extern "C" fn planted(value: u32) -> u32 {
    value + 1
}

// planted: a statement before the boundary's call
extern "system" fn first_then(value: u32) -> u32 {
    let doubled = value * 2;
    os_entry("first_then", (), AfterPanic::Answer(|()| 0), move || doubled)
}

// planted: an argument that runs before the boundary
extern "system" fn eager(value: u32) -> u32 {
    os_entry(name(), (), AfterPanic::Answer(|()| 0), move || value)
}

// planted: an answer made before the boundary
extern "system" fn answer_runs(value: u32) -> u32 {
    os_entry("answer_runs", (), AfterPanic::Answer(make_answer()), move || value)
}

fn name() -> &'static str {
    "eager"
}

// silent: through a free adapter of this file
extern "system" fn adapted(value: u32) -> u32 {
    com_method(|| value)
}

fn com_method(body: impl FnOnce() -> u32) -> u32 {
    os_entry("com", (), AfterPanic::Answer(|()| 0), body)
}

// planted: a function that is not an adapter
extern "system" fn misadapted(value: u32) -> u32 {
    helper(|| value)
}

fn helper(body: impl FnOnce() -> u32) -> u32 {
    body()
}

// planted: the bare name here is the import's, not the adapter of the same
// name in `helpers`
extern "system" fn shadowed(value: u32) -> u32 {
    send(|| value)
}

mod helpers {
    pub(super) fn send(body: impl FnOnce() -> u32) -> u32 {
        crate::owner::panic_boundary::os_entry("send", (), super::AfterPanic::Answer(|()| 0), body)
    }
}

// silent: declarations and pointer types are not entries
extern "system" {
    fn GetTickCount() -> u32;
}
type Procedure = unsafe extern "system" fn(isize, u32) -> isize;

struct Store;

impl Store {
    fn com_entry(&self, method: &'static str, body: impl FnOnce() -> u32) -> u32 {
        crate::owner::panic_boundary::os_entry(method, (), AfterPanic::Abort, body)
    }

    // planted: a foreign-ABI method without a boundary
    extern "system" fn procedure_method(value: u32) -> u32 {
        let doubled = value * 2;
        doubled
    }
}

struct Other;

impl Other {
    fn com_entry(&self, method: &'static str, body: impl FnOnce() -> u32) -> u32 {
        body()
    }
}

trait Callbacks {
    // planted: a trait's foreign-ABI default without a boundary
    extern "C" fn default_callback() -> u32 {
        compute()
    }
}

impl ITextStore_Impl for Store_Impl {
    // silent: through a method adapter of a named type
    fn Guarded(&self) -> u32 {
        Store::com_entry(self, "Guarded", || 1)
    }

    // planted: a COM method without a boundary
    fn Planted(&self) -> u32 {
        compute()
    }

    // planted: a method call resolves by its receiver, not by this file
    fn ByMethodCall(&self) -> u32 {
        self.com_entry("ByMethodCall", || 3)
    }

    // planted: the named type's method is not the one that passes
    fn OtherType(&self) -> u32 {
        Other::com_entry(self, "OtherType", || 4)
    }
}

fn subscribe(settings: &UISettings) {
    // planted: a WinRT delegate without a boundary
    let _ = settings.Changed(&TypedEventHandler::new(move |_, _| Ok(())));
    // silent: a delegate through the boundary
    let _ = settings.Changed(&TypedEventHandler::new(move |_, _| {
        os_entry("delegate", (), AfterPanic::Answer(|()| Ok(())), || Ok(()))
    }));
    // planted: a closure handed to an OS crate's function, found by its import
    NSOperationQueue::addOperationWithBlock(|| work());
    // silent: an OS function given no closure
    NSOperationQueue::mainQueue();
    // silent: an OS function that runs its closure on the caller's stack
    objc2::rc::autoreleasepool(|_| work());
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
            panic_boundary::os_entry("did_finish", self, AfterPanic::Answer(|_| ()), || ())
        }

        // planted: a method without a boundary
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            tracing::info!("terminating");
        }
    }

    impl Delegate {
        // silent: through a `Self::` adapter
        #[unsafe(method(sceneDidDisconnect:))]
        fn disconnect(&self, scene: &UIScene) {
            Self::scene_event(self, scene)
        }

        // planted: `Self` defines no such method
        #[unsafe(method(sceneWillResignActive:))]
        fn inactive(&self, scene: &UIScene) {
            Self::missing(self, scene)
        }
    }
);

impl Delegate {
    fn scene_event(&self, scene: &UIScene) {
        panic_boundary::os_entry("a scene event", (), AfterPanic::Answer(|()| ()), || {
            let _ = scene;
        })
    }
}

// planted: an unparsable body
define_class!(
    struct Broken;
    impl Broken { fn }
);

// planted: a ClassBuilder method without a boundary
extern "C-unwind" fn key_down(_this: &AnyObject, _sel: Sel) {
    let _ = _sel;
}

// silent: a ClassBuilder method with a constant answer
extern "C-unwind" fn is_opaque(_this: &AnyObject, _sel: Sel) -> Bool {
    Bool::YES
}

fn observe(handler: impl Fn() + 'static) {
    // planted: a block without a boundary
    let _ = RcBlock::new(|| work());
    // planted: a block whose closure the gate cannot see
    let _ = StackBlock::new(handler);
}

fn dispatch(queue: &DispatchQueue, pending: Pending) {
    // planted: a GCD work item without a boundary
    queue.exec_async(|| work());
    // silent: through the boundary, its captures moved in
    queue.exec_async(move || {
        panic_boundary::os_entry("work", (), AfterPanic::Abort, move || drop(pending))
    });
    // planted: a body that borrows its captures, which drop outside
    queue.exec_after(delay, move || {
        panic_boundary::os_entry("late", (), AfterPanic::Answer(|()| ()), || drop(pending))
    });
}

// planted: an extern fn the gate cannot read
some_macro! {
    extern "C" fn hidden() {}
}

// planted: callbacks made in a macro's tokens
other_macro!(center, &*RcBlock::new(|| ()), queue.exec_sync(|| ()));

// silent: a pointer type in a macro's tokens
ndk_function!(fn_type, unsafe extern "C" fn(i32) -> i32);
"#;

const ANDROID_FILE: &str = r#"
use crate::owner::panic_boundary::{AfterPanic, os_entry};

fn jni_native<T: Default>(method: &'static str, body: impl FnOnce() -> T) -> T {
    os_entry(method, (), AfterPanic::Answer(|()| T::default()), body)
}

// silent: a JNI export through an adapter
#[unsafe(no_mangle)]
extern "system" fn Java_dev_flui_Guarded_run(_env: JNIEnv, _class: JClass, handle: i64) -> i64 {
    jni_native("run", || handle)
}

// planted: a JNI export without a boundary
#[unsafe(no_mangle)]
extern "system" fn Java_dev_flui_Planted_run(_env: JNIEnv, _class: JClass, handle: i64) -> i64 {
    handle + 1
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
