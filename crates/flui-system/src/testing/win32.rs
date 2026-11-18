//! The Windows providers' test kit: an owner whose turns the test runs,
//! windows a provider can be acquired in, a message pump for a stand-in
//! modal loop, and the stand-ins of the OS seams (the shell, the hot keys,
//! the taskbar), each answering as the test sets it.

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS;
use windows::Win32::UI::Shell::TBPFLAG;

use crate::win32::Owner;
use crate::win32::launcher::{HeldFile, ShellSystem, Trashed};
use crate::win32::shortcuts::HotkeySystem;
use crate::win32::taskbar::{TaskbarButtons, TaskbarSystem};

use flui_platform::capability::{
    CompletionSource, HostBridge, OwnerJob, OwnerPoster, OwnerScope, PendingCompletions,
    ProviderContext, RuntimeLocals, WindowCloser, WindowRef,
};
use flui_platform::haptics::PlatformHaptics;
use flui_platform::{
    AdmittedCommand, ApplyError, Cursor, CursorError, GestureGeometry, PlatformSystemChrome,
    PlatformTextInput, PlatformWindow, PreferenceQueryError, RedrawRequester, SurfaceTarget,
    SwapChainStatistics, WindowAt, WindowExtent, WindowGeometry, WindowId, WindowIdMint,
    WindowState,
};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

type Jobs = Arc<Mutex<Vec<OwnerJob>>>;

pub(crate) struct TestOwner {
    jobs: Jobs,
    poster: OwnerPoster,
    pending: PendingCompletions,
    completions: CompletionSource,
    locals: RuntimeLocals,
    bridge: HostBridge,
}

impl TestOwner {
    pub(crate) fn new() -> Self {
        let jobs: Jobs = Arc::default();
        let queue = Arc::clone(&jobs);
        let poster = OwnerPoster::new(move |job| {
            queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            Ok(())
        });
        let pending = PendingCompletions::new();
        let completions = CompletionSource::new(poster.clone(), pending.clone());
        Self {
            jobs,
            poster,
            pending,
            completions,
            locals: RuntimeLocals::new(),
            bridge: HostBridge::new(),
        }
    }

    pub(crate) fn ctx(&self, window: Option<WindowRef>) -> ProviderContext<'_> {
        ProviderContext::new(
            &self.poster,
            &self.completions,
            &self.locals,
            &self.bridge,
            window,
        )
    }

    /// Run the jobs queued so far; returns how many ran.
    pub(crate) fn turn(&self) -> usize {
        let jobs = std::mem::take(&mut *self.jobs.lock().unwrap_or_else(PoisonError::into_inner));
        let ran = jobs.len();
        let scope = OwnerScope::new(&self.pending);
        for job in jobs {
            job.run(&scope);
        }
        ran
    }

    /// Run turns until `done`, or panic after ten seconds: what another
    /// thread answers arrives as a posted job.
    pub(crate) fn turn_until(&self, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "the owner waited ten seconds");
            if self.turn() == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// A window with no native handle: what a provider sees of a window it
/// cannot own a dialog with.
struct Handleless;

impl HasWindowHandle for Handleless {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for Handleless {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for Handleless {
    fn pre_present_notify(
        &self,
        _: Option<flui_sdk::foundation::FrameStamp>,
        _: SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}

/// A window with no native handle, open until its closer closes it.
pub(crate) fn window() -> (WindowRef, WindowCloser) {
    open(Arc::new(Handleless))
}

/// A window whose handle names the Win32 window `hwnd`, which a fake
/// system never reaches: what a provider owns its calls with.
pub(crate) fn win32_window(hwnd: isize) -> (WindowRef, WindowCloser) {
    open(Arc::new(Named(
        std::num::NonZeroIsize::new(hwnd).expect("the test names a window"),
    )))
}

fn open(target: Arc<dyn SurfaceTarget>) -> (WindowRef, WindowCloser) {
    thread_local! {
        // One mint per test thread: two windows of one test are two ids.
        static WINDOW_IDS: WindowIdMint = const { WindowIdMint::new() };
    }
    let (closer, lifetime) = WindowCloser::new();
    let id = WINDOW_IDS
        .with(WindowIdMint::next)
        .expect("BUG: a test opens far fewer windows than a mint issues");
    let window: Rc<dyn PlatformWindow> = Rc::new(TestWindow { id, target });
    (WindowRef::new(&window, lifetime), closer)
}

/// A window that is its id and its surface: what a [`WindowRef`] reads of
/// it. Every verb answers that it does nothing.
struct TestWindow {
    id: WindowId,
    target: Arc<dyn SurfaceTarget>,
}

impl PlatformWindow for TestWindow {
    flui_platform::window_takes_no_frame_verbs!();

    fn id(&self) -> WindowId {
        self.id
    }
    fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
        Ok(None)
    }
    fn request_redraw(&self) {}
    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        Arc::new(|| {})
    }
    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        Arc::clone(&self.target)
    }
    fn state(&self) -> WindowState {
        WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO)
    }
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        command.against(&self.state().capabilities)?;
        Ok(())
    }
    fn set_cursor(&self, _: &Cursor) -> Result<(), CursorError> {
        Ok(())
    }
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        None
    }
    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
    fn window_at(&self, _: flui_sdk::foundation::geometry::Point<f64>) -> WindowAt {
        WindowAt::Unknown
    }
    fn reveal_after_first_frame(&self) {}
}

/// A window known only by its handle's value.
struct Named(std::num::NonZeroIsize);

impl HasWindowHandle for Named {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = raw_window_handle::RawWindowHandle::Win32(
            raw_window_handle::Win32WindowHandle::new(self.0),
        );
        // SAFETY: the handle carries a value no code under test hands to
        // the system: the fakes it reaches only compare it.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Named {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for Named {
    fn pre_present_notify(
        &self,
        _: Option<flui_sdk::foundation::FrameStamp>,
        _: SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}

/// Dispatch this thread's waiting messages, as a modal loop does between
/// its waits.
pub(crate) fn pump() {
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};
    let mut message = MSG::default();
    // SAFETY: `message` is a live out-parameter; only this thread's own
    // messages are dispatched.
    while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        // SAFETY: as above.
        unsafe { DispatchMessageW(&raw const message) };
    }
}

/// A shell whose file system holds one file under the name it resolves to,
/// and which records what it was asked to execute.
pub(crate) struct FakeShell {
    pub(crate) final_path: PathBuf,
    pub(crate) directory: bool,
    pub(crate) dangerous: bool,
    pub(crate) hold_error: Option<i32>,
    pub(crate) executed: Mutex<Vec<String>>,
    pub(crate) trashed: Result<Trashed, i32>,
}

impl FakeShell {
    /// A shell holding a document whose final name is `final_path`.
    pub(crate) fn holding(final_path: &str) -> Self {
        Self {
            final_path: PathBuf::from(final_path),
            directory: false,
            dangerous: false,
            hold_error: None,
            executed: Mutex::new(Vec::new()),
            trashed: Ok(Trashed::Done),
        }
    }

    /// What it was asked to execute.
    pub(crate) fn executed(&self) -> Vec<String> {
        self.executed.lock().expect("BUG: the test's lock").clone()
    }
}

impl ShellSystem for FakeShell {
    fn hold(&self, _: &Path) -> Result<HeldFile, i32> {
        if let Some(code) = self.hold_error {
            return Err(code);
        }
        Ok(HeldFile {
            final_path: self.final_path.clone(),
            directory: self.directory,
            _handle: Box::new(()),
        })
    }

    fn is_dangerous(&self, _: &Path) -> bool {
        self.dangerous
    }

    fn execute(&self, target: &OsStr, _: Option<Owner>) -> Result<(), i32> {
        self.executed
            .lock()
            .expect("BUG: the test's lock")
            .push(target.to_string_lossy().into_owned());
        Ok(())
    }

    fn reveal(&self, _: &Path) -> Result<(), i32> {
        Ok(())
    }

    fn trash(&self, _: &Path, _: Option<Owner>) -> Result<Trashed, i32> {
        self.trashed
    }
}

/// The hot keys a [`FakeHotkeys`] holds.
pub(crate) type Held = Rc<RefCell<Vec<(i32, HOT_KEY_MODIFIERS, u32)>>>;

/// A hot key system that records what it holds and refuses what it was
/// told to.
#[derive(Default)]
pub(crate) struct FakeHotkeys {
    pub(crate) held: Held,
    pub(crate) refuse: Option<i32>,
}

impl HotkeySystem for FakeHotkeys {
    fn register(&self, id: i32, modifiers: HOT_KEY_MODIFIERS, virtual_key: u32) -> Result<(), i32> {
        if let Some(code) = self.refuse {
            return Err(code);
        }
        self.held.borrow_mut().push((id, modifiers, virtual_key));
        Ok(())
    }

    fn unregister(&self, id: i32) {
        self.held.borrow_mut().retain(|(held, _, _)| *held != id);
    }
}

/// One call a [`FakeTaskbar`] took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TaskbarCall {
    Progress(TBPFLAG, Option<u64>),
    Badge(Option<u64>),
    Watch,
    Unwatch,
}

/// A taskbar that records each call and refuses a change while its button
/// is not made (`E_FAIL`, as the taskbar answers).
pub(crate) struct FakeTaskbar {
    pub(crate) calls: Rc<RefCell<Vec<TaskbarCall>>>,
    pub(crate) made: Rc<Cell<bool>>,
}

impl FakeTaskbar {
    fn answer(&self) -> Result<(), i32> {
        if self.made.get() {
            Ok(())
        } else {
            Err(windows::Win32::Foundation::E_FAIL.0)
        }
    }
}

impl TaskbarSystem for FakeTaskbar {
    fn progress(&self, _: Owner, state: TBPFLAG, value: Option<u64>) -> Result<(), i32> {
        self.calls
            .borrow_mut()
            .push(TaskbarCall::Progress(state, value));
        self.answer()
    }

    fn badge(&self, _: Owner, count: Option<u64>) -> Result<(), i32> {
        self.calls.borrow_mut().push(TaskbarCall::Badge(count));
        self.answer()
    }

    fn watch(&self, _: Owner, _: &Rc<TaskbarButtons>) -> Result<(), i32> {
        self.calls.borrow_mut().push(TaskbarCall::Watch);
        Ok(())
    }

    fn unwatch(&self, _: Owner) {
        self.calls.borrow_mut().push(TaskbarCall::Unwatch);
    }
}
