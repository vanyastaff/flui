//! Opt-in probe of the text services against the real TSF and, when it is
//! installed, Microsoft IME ja-JP. It types into a real window, so it runs
//! only on request:
//!
//! `cargo test -p flui-platform --lib text_services -- --ignored --nocapture`
//!
//! Only this thread's input profile changes (`ActivateProfile` with
//! `TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE` and no process or session flag;
//! the process-wide form is a fallback the host has not needed), and the
//! previous profile is restored. The IME is switched on with the IME-on key
//! (`VK_IME_ON`) sent to the probe window. Keys are sent only while the probe
//! window is the foreground window. Throwaway: the window wiring replaces it.

use std::cell::Cell;
use std::io::Write as _;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Bounds, DevicePixelRatio, DevicePoint, Point, Size};
use flui_platform_api::text_store::{
    CommitGate, Composition, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, PointMode,
    RangeRect, Selection, TextChange, TextStore, TextStoreEdit, TextStoreError, TextStoreObserver,
    TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::Gdi::ClientToScreen,
    System::{
        Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    },
    UI::{
        Input::KeyboardAndMouse::{
            HKL, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, SetFocus,
            VIRTUAL_KEY, VK_ESCAPE, VK_PROCESSKEY, VK_RETURN, VK_SPACE,
        },
        TextServices::{
            CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD, ITfInputProcessorProfileMgr,
            TF_INPUTPROCESSORPROFILE, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_IPPMF_FORPROCESS,
            TF_PROFILETYPE_INPUTPROCESSOR, TF_PROFILETYPE_KEYBOARDLAYOUT,
        },
        WindowsAndMessaging::{
            DispatchMessageW, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect,
            GetWindowThreadProcessId, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW,
            SetForegroundWindow, TranslateMessage, WM_KEYDOWN,
        },
    },
};
use windows_core::{BOOL, GUID, PWSTR};

use super::TextServices;
use crate::shared::text_geometry::range_rect_to_screen;
use crate::traits::{Platform, WindowOptions};
use flui_platform_api::text_store::TextStoreHost as _;

/// Microsoft IME ja-JP: its TIP class and profile.
const MS_IME_JA: (GUID, GUID) = (
    GUID::from_u128(0x03b5835f_f03c_411b_9ce2_aa23e1171e36),
    GUID::from_u128(0xa76c93d9_5523_4e90_aafa_4db112f9ac76),
);
/// Where the field starts in the client area, logical pixels.
const FIELD: (f64, f64) = (40.0, 60.0);

/// An [`InMemoryTextStore`] whose layout lags: text a session inserted has
/// no layout (`NoLayout`) until the probe's next tick lays it out, as a
/// widget's does until its next frame. Its rects sit at [`FIELD`] plus
/// `shift`.
struct LateLayoutStore {
    inner: Rc<InMemoryTextStore>,
    laid_out: Rc<Cell<usize>>,
    shift: Rc<Cell<f64>>,
    observer: std::cell::RefCell<Option<Rc<dyn TextStoreObserver>>>,
}

struct LateLayoutSession<S> {
    session: S,
    laid_out: usize,
    shift: f64,
}

impl<S: Deref<Target = T>, T: TextStoreRead + ?Sized> TextStoreRead for LateLayoutSession<S> {
    fn document_len(&self) -> Utf16Offset {
        self.session.document_len()
    }
    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.session.text(range)
    }
    fn selection(&self) -> Selection {
        self.session.selection()
    }
    fn composition(&self) -> Option<Composition> {
        self.session.composition()
    }
    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        if range.end().get() > self.laid_out {
            return Err(TextStoreError::NoLayout);
        }
        let mut rect = self.session.rect_for_range(range)?;
        rect.bounds.origin.x += FIELD.0;
        rect.bounds.origin.y += FIELD.1 + self.shift;
        Ok(rect)
    }
    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        Ok(Bounds::new(
            Point::new(FIELD.0, FIELD.1 + self.shift),
            Size::new(400.0, 20.0),
        ))
    }
    fn index_at_point(
        &self,
        at: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        let at = Point::new(at.x - FIELD.0, at.y - FIELD.1 - self.shift);
        self.session.index_at_point(at, mode)
    }
}

impl<S: DerefMut<Target = T>, T: TextStoreEdit + ?Sized> TextStoreEdit for LateLayoutSession<S> {
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        self.session.replace(range, text)
    }
    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError> {
        self.session.insert_at_selection(text)
    }
    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError> {
        self.session.set_selection(selection)
    }
    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError> {
        self.session.set_composition(composition)
    }
}

impl TextStore for LateLayoutStore {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let (laid_out, shift) = (Rc::clone(&self.laid_out), Rc::clone(&self.shift));
        let grant = match grant {
            LockGrant::Read(body) => LockGrant::read(move |session| {
                body(&LateLayoutSession {
                    session,
                    laid_out: laid_out.get(),
                    shift: shift.get(),
                });
            }),
            LockGrant::ReadWrite(body) => LockGrant::read_write(move |session| {
                body(&mut LateLayoutSession {
                    session,
                    laid_out: laid_out.get(),
                    shift: shift.get(),
                });
            }),
        };
        self.inner.request_lock(grant, timing)
    }
    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }
    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.observer.replace(observer.clone());
        self.inner.set_observer(observer);
    }
}

impl LateLayoutStore {
    /// The probe's "frame": lay out what the last sessions inserted and say so.
    fn tick(&self) {
        let len = flui_platform_api::text_store::utf16::utf16_len(&self.inner.text()).get();
        if self.laid_out.replace(len) != len
            && let Some(observer) = self.observer.borrow().clone()
        {
            observer.layout_changed();
        }
    }
}

/// A visible top-level window: (handle, class, process image, rect, cloaked).
type TopLevel = (isize, String, String, RECT, bool);

fn top_level_windows() -> Vec<TopLevel> {
    unsafe extern "system" fn collect(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the `&mut Vec` `top_level_windows` passed below,
        // live for the whole `EnumWindows` call.
        let out = unsafe { &mut *(out.0 as *mut Vec<TopLevel>) };
        // SAFETY: plain queries on a handle EnumWindows just produced;
        // every buffer is a live local of the size passed.
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let mut class = [0u16; 128];
                let class_len = usize::try_from(GetClassNameW(hwnd, &mut class)).unwrap_or(0);
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &raw mut rect);
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
                let mut image = [0u16; 260];
                let mut image_len = 260u32;
                if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let _ = QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_WIN32,
                        PWSTR(image.as_mut_ptr()),
                        &raw mut image_len,
                    );
                    let _ = windows::Win32::Foundation::CloseHandle(process);
                } else {
                    image_len = 0;
                }
                let image = String::from_utf16_lossy(&image[..image_len as usize]);
                let image = image.rsplit('\\').next().unwrap_or_default().to_owned();
                let mut cloaked = 0u32;
                let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                    hwnd,
                    windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
                    (&raw mut cloaked).cast(),
                    4,
                );
                out.push((
                    hwnd.0 as isize,
                    String::from_utf16_lossy(&class[..class_len]),
                    image,
                    rect,
                    cloaked != 0,
                ));
            }
        }
        true.into()
    }
    let mut out: Vec<TopLevel> = Vec::new();
    // SAFETY: the callback only touches `out` through the pointer, during the call.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&raw mut out as isize)) };
    out
}

/// Pump this thread's messages for `ms`, ticking the store's layout and,
/// while the gate is open, running deferred grants (the commit anchor).
fn pump(ms: u64, probe: &Probe) {
    let (store, gate, processkeys) = (&probe.late, &probe.gate, &probe.processkeys);
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        let mut msg = MSG::default();
        // SAFETY: `msg` is a live local; standard message pumping on the owner thread.
        while unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_KEYDOWN && msg.wParam.0 == usize::from(VK_PROCESSKEY.0) {
                processkeys.set(processkeys.get() + 1);
            }
            if probe.offer_keystroke(&msg) {
                continue;
            }
            // SAFETY: as above.
            unsafe {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
        if gate.is_open() {
            store.run_deferred_grants();
            store.tick();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The probe's field, gate and keystroke routing.
struct Probe {
    late: Rc<LateLayoutStore>,
    gate: CommitGate,
    processkeys: Cell<usize>,
    /// When set, key messages are offered to TSF's keystroke manager first.
    keystrokes: std::cell::RefCell<Option<windows::Win32::UI::TextServices::ITfKeystrokeMgr>>,
    eaten: Cell<usize>,
}

impl Probe {
    /// Offer a key message to the keystroke manager; whether a TIP ate it.
    fn offer_keystroke(&self, msg: &MSG) -> bool {
        let Some(keys) = self.keystrokes.borrow().clone() else {
            return false;
        };
        // SAFETY: COM calls with the message's own parameters.
        let eaten = unsafe {
            match msg.message {
                0x100 => {
                    keys.TestKeyDown(msg.wParam, msg.lParam)
                        .is_ok_and(windows_core::BOOL::as_bool)
                        && keys
                            .KeyDown(msg.wParam, msg.lParam)
                            .is_ok_and(windows_core::BOOL::as_bool)
                }
                0x101 => {
                    keys.TestKeyUp(msg.wParam, msg.lParam)
                        .is_ok_and(windows_core::BOOL::as_bool)
                        && keys
                            .KeyUp(msg.wParam, msg.lParam)
                            .is_ok_and(windows_core::BOOL::as_bool)
                }
                _ => false,
            }
        };
        if eaten {
            self.eaten.set(self.eaten.get() + 1);
        }
        eaten
    }
}

/// Make `hwnd` the foreground window, attaching to the current foreground
/// thread's input for the call (the foreground lock refuses a plain
/// `SetForegroundWindow` from a background process).
fn bring_to_front(hwnd: HWND) -> bool {
    for _ in 0..20 {
        // SAFETY: plain calls on window and thread handles; the input
        // attachment is undone before the block ends.
        unsafe {
            let me = windows::Win32::System::Threading::GetCurrentThreadId();
            let other = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let attached = other != 0
                && other != me
                && windows::Win32::System::Threading::AttachThreadInput(me, other, true).as_bool();
            let _ = windows::Win32::UI::WindowsAndMessaging::BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            if attached {
                let _ = windows::Win32::System::Threading::AttachThreadInput(me, other, false);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
        // SAFETY: no arguments.
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
    }
    false
}

/// Press and release `key`, only while `hwnd` is the foreground window.
fn press(hwnd: HWND, key: VIRTUAL_KEY) {
    // SAFETY: no arguments.
    if unsafe { GetForegroundWindow() } != hwnd {
        bring_to_front(hwnd);
    }
    // SAFETY: no arguments.
    assert_eq!(
        unsafe { GetForegroundWindow() },
        hwnd,
        "the probe window lost the foreground; no key sent"
    );
    let input = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                // SAFETY: a pure table lookup.
                wScan: u16::try_from(unsafe {
                    windows::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(
                        u32::from(key.0),
                        windows::Win32::UI::Input::KeyboardAndMouse::MAPVK_VK_TO_VSC,
                    )
                })
                .unwrap_or(0),
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [
        input(windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS::default()),
        input(KEYEVENTF_KEYUP),
    ];
    // SAFETY: a live array of initialized INPUT values and the element size.
    let sent = unsafe {
        SendInput(
            &inputs,
            i32::try_from(size_of::<INPUT>()).expect("INPUT size"),
        )
    };
    assert_eq!(sent, 2, "SendInput");
}

fn type_keys(hwnd: HWND, keys: &str, probe: &Probe) {
    for key in keys.bytes() {
        press(hwnd, VIRTUAL_KEY(u16::from(key.to_ascii_uppercase())));
        pump(120, probe);
    }
}

#[test]
#[ignore = "drives the real TSF and the keyboard; run on request with --ignored --nocapture"]
fn text_services_probe() {
    let journal = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = Arc::clone(&journal);
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("flui_platform::tsf=debug")
        .with_ansi(false)
        .without_time()
        .with_writer(move || JournalWriter(Arc::clone(&sink)))
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);

    let platform = super::super::WindowsPlatform::new().expect("platform");
    let options = WindowOptions {
        title: "flui tsf probe".into(),
        size: Size::new(640.0, 200.0),
        ..Default::default()
    };
    let window = platform.open_window(options).expect("window");
    let hwnd = window
        .as_any()
        .downcast_ref::<super::super::WindowsWindow>()
        .expect("Win32 window")
        .hwnd();
    println!("PROBE foreground: {}", bring_to_front(hwnd));
    let probe = Probe {
        late: Rc::new(LateLayoutStore {
            inner: InMemoryTextStore::new(""),
            laid_out: Rc::new(Cell::new(0)),
            shift: Rc::new(Cell::new(0.0)),
            observer: std::cell::RefCell::default(),
        }),
        gate: CommitGate::new(),
        processkeys: Cell::new(0),
        keystrokes: std::cell::RefCell::new(None),
        eaten: Cell::new(0),
    };
    let (late, gate, pk) = (&probe.late, &probe.gate, &probe.processkeys);
    late.set_commit_gate(gate.clone());
    pump(300, &probe);

    // (1) activation and focus.
    let services = TextServices::activate(hwnd).expect("(1) ITfThreadMgr::Activate");
    println!(
        "PROBE (1) Activate: S_OK, TfClientId = {:#x}",
        services.client_id()
    );
    services.focus_store(Some(Rc::clone(late) as Rc<dyn TextStore>));
    println!(
        "PROBE (1) after focus_store: ITfThreadMgr::GetFocus is the field document = {}",
        services.document_has_focus()
    );

    // SAFETY: COM calls on this STA thread with live locals.
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }
            .expect("profile manager");
    let mut previous = TF_INPUTPROCESSORPROFILE::default();
    // SAFETY: as above.
    let _ = unsafe { profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &raw mut previous) };
    println!(
        "PROBE previous thread profile: type {} langid {:#06x} hkl {:?}",
        previous.dwProfileType, previous.langid, previous.hkl
    );
    let (scale, origin) = frame(hwnd);
    let field = range_rect_to_screen(
        Bounds::new(Point::new(FIELD.0, FIELD.1), Size::new(400.0, 20.0)),
        origin,
        scale,
    )
    .expect("field rect");
    // The candidate gate: the bounding box of the pixels an input method
    // changed near the field must start within one line of the field's
    // bottom edge, horizontally over the field.
    let line = (20.0 * scale.get()).ceil() as i32;
    let gate_check = |label: &str, pre: &ScreenCapture, post: &ScreenCapture| {
        let changed = pre.changed(post);
        let pass = changed.is_some_and(|r| {
            (field.bottom - line..=field.bottom + line).contains(&r.top)
                && r.left <= field.right
                && r.right >= field.left - 100
        });
        println!(
            "PROBE {label}: changed pixels {:?}; candidate gate {} (field {field:?}, line {line} px, scale {})",
            changed.map(|r| (r.left, r.top, r.right, r.bottom)),
            if pass { "PASS" } else { "FAIL" },
            scale.get()
        );
        pass
    };
    // Which top-level windows an input method showed or moved.
    let windows_report = |label: &str, before: &[TopLevel]| {
        for w in top_level_windows().iter().filter(|w| {
            w.0 != hwnd.0 as isize
                && !before
                    .iter()
                    .any(|b| b.0 == w.0 && b.3 == w.3 && b.4 == w.4)
        }) {
            println!(
                "PROBE {label}: shown or moved window {:#x} class {:?} image {:?} rect ({},{})-({},{}) cloaked {}",
                w.0, w.1, w.2, w.3.left, w.3.top, w.3.right, w.3.bottom, w.4
            );
        }
        for w in top_level_windows()
            .iter()
            .filter(|w| w.2.eq_ignore_ascii_case("TextInputHost.exe"))
        {
            println!(
                "PROBE {label}: TextInputHost window {:#x} class {:?} rect ({},{})-({},{}) cloaked {}",
                w.0, w.1, w.3.left, w.3.top, w.3.right, w.3.bottom, w.4
            );
        }
    };

    // Production routing: key messages go to TSF's keystroke manager first.
    if std::env::var_os("FLUI_TSF_PROBE_NO_KEYSTROKE_MGR").is_none() {
        *probe.keystrokes.borrow_mut() =
            windows_core::Interface::cast(services.thread_manager()).ok();
    }
    println!(
        "PROBE keystroke manager installed: {}",
        probe.keystrokes.borrow().is_some()
    );

    // Negative control: US layout, no IME, the same keys.
    let us = activate_profile(
        &profiles,
        TF_PROFILETYPE_KEYBOARDLAYOUT,
        0x0409,
        (GUID::zeroed(), GUID::zeroed()),
        HKL(0x0409_0409 as _),
    );
    println!("PROBE control: en-US layout for this thread: {us:?}");
    let pre = ScreenCapture::grab(field);
    type_keys(hwnd, "toukyou", &probe);
    press(hwnd, VK_SPACE);
    press(hwnd, VK_SPACE);
    pump(800, &probe);
    let post = ScreenCapture::grab(field);
    post.save("control-ime-off");
    let control = gate_check("control (IME off)", &pre, &post);
    println!(
        "PROBE control: store text {:?}, VK_PROCESSKEY so far {}",
        late.inner.text(),
        pk.get()
    );

    let ja = activate_profile(
        &profiles,
        TF_PROFILETYPE_INPUTPROCESSOR,
        0x0411,
        MS_IME_JA,
        HKL::default(),
    );
    println!("PROBE ja-JP profile for this thread: {ja:?}");
    if ja.is_err() {
        println!("PROBE SKIP: Microsoft IME ja-JP is not installed; points (2)-(7) need it");
        services.shutdown();
        return;
    }
    let mut active = TF_INPUTPROCESSORPROFILE::default();
    // SAFETY: a COM call with a live out-pointer.
    let _ = unsafe { profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &raw mut active) };
    println!(
        "PROBE active profile now: type {} langid {:#06x} clsid {:?} profile {:?}",
        active.dwProfileType, active.langid, active.clsid, active.guidProfile
    );
    pump(1000, &probe);
    // The IME-on key, as a user would press it, into this window only.
    press(hwnd, VIRTUAL_KEY(0x16));
    pump(500, &probe);

    // (2), (3), (4), (5), (6): toukyou with the gate shut for part of it.
    let before = top_level_windows();
    let pre = ScreenCapture::grab(field);
    type_keys(hwnd, "t", &probe);
    pump(400, &probe);
    let post = ScreenCapture::grab(field);
    post.save("first-keystroke");
    let first_key = gate_check("(4) first keystroke", &pre, &post);
    windows_report("(4) first keystroke", &before);
    println!(
        "PROBE (4) last GetTextExt answer {:?}",
        services.focused_state().and_then(|s| s.last_text_ext.get())
    );
    type_keys(hwnd, "ou", &probe);
    gate.set_open(false);
    println!("PROBE (3) gate shut");
    type_keys(hwnd, "ky", &probe);
    println!(
        "PROBE (3) while shut: store {:?}, composition {:?}",
        late.inner.text(),
        late.inner.composition()
    );
    gate.set_open(true);
    println!(
        "PROBE (3) gate open, anchor ran {} deferred grants",
        late.run_deferred_grants()
    );
    type_keys(hwnd, "ou", &probe);
    pump(300, &probe);
    println!(
        "PROBE (2) composing: store {:?}, composition {:?}",
        late.inner.text(),
        late.inner.composition()
    );
    let pre = ScreenCapture::grab(field);
    press(hwnd, VK_SPACE);
    pump(600, &probe);
    println!(
        "PROBE (2) after the first Space: store {:?}",
        late.inner.text()
    );
    press(hwnd, VK_SPACE);
    pump(800, &probe);
    let post = ScreenCapture::grab(field);
    post.save("after-space");
    let after_space = gate_check("(4) after Space (candidate list)", &pre, &post);
    windows_report("(4) after Space", &before);
    // Back to the first candidate before committing.
    press(hwnd, windows::Win32::UI::Input::KeyboardAndMouse::VK_UP);
    pump(300, &probe);
    press(hwnd, VK_RETURN);
    pump(400, &probe);
    println!(
        "PROBE (2) after Enter: store {:?}, composition {:?}",
        late.inner.text(),
        late.inner.composition()
    );

    // (7) TerminateComposition with the gate open, then shut.
    let late_store: Rc<dyn TextStore> = Rc::clone(late) as Rc<dyn TextStore>;
    type_keys(hwnd, "toukyou", &probe);
    println!(
        "PROBE (7) open gate: {:?}; store {:?}, composition {:?}",
        services.complete_composition(&late_store),
        late.inner.text(),
        late.inner.composition()
    );
    type_keys(hwnd, "kyou", &probe);
    gate.set_open(false);
    let shut = services.complete_composition(&late_store);
    println!(
        "PROBE (7) shut gate: {shut:?}; store {:?}, composition {:?}",
        late.inner.text(),
        late.inner.composition()
    );
    if shut == Ok(flui_platform_api::text_store::CompositionEnd::Abandoned) {
        flui_platform_api::text_store::commit_composition_in_place(&*late_store);
        println!("PROBE (7) owner commits the composition in place");
    }
    gate.set_open(true);
    pump(300, &probe);
    println!(
        "PROBE (7) after the anchor: store {:?}, composition {:?}",
        late.inner.text(),
        late.inner.composition()
    );

    // Negative control: the store reports its rects 150 px lower.
    late.shift.set(150.0);
    let pre = ScreenCapture::grab(field);
    type_keys(hwnd, "toukyou", &probe);
    press(hwnd, VK_SPACE);
    pump(400, &probe);
    press(hwnd, VK_SPACE);
    pump(800, &probe);
    let post = ScreenCapture::grab(field);
    post.save("shifted");
    let shifted = gate_check("control (rect shifted)", &pre, &post);
    for _ in 0..3 {
        press(hwnd, VK_ESCAPE);
        pump(150, &probe);
    }
    println!(
        "PROBE after Esc: store {:?}, composition {:?}",
        late.inner.text(),
        late.inner.composition()
    );
    pump(300, &probe);
    println!(
        "PROBE (6) VK_PROCESSKEY keydowns seen by the window: {}; key messages eaten by ITfKeystrokeMgr: {}",
        pk.get(),
        probe.eaten.get()
    );
    let log = String::from_utf8_lossy(&journal.lock().expect("journal")).into_owned();
    let nolayout = log
        .lines()
        .filter(|l| l.contains("GetTextExt") && l.contains("0x80040206"))
        .count();
    let ext_ok = log
        .lines()
        .filter(|l| l.contains("GetTextExt") && l.contains("screen="))
        .count();
    let layout = log.lines().filter(|l| l.contains("OnLayoutChange")).count();
    let async_locks = log
        .lines()
        .filter(|l| l.contains("RequestLock") && l.contains("0x00040300"))
        .count();
    println!(
        "PROBE (5) GetTextExt TS_E_NOLAYOUT {nolayout}, answered {ext_ok}, OnLayoutChange sent {layout}; (3) TS_S_ASYNC answers {async_locks}"
    );

    let restored = activate_profile(
        &profiles,
        previous.dwProfileType,
        previous.langid,
        (previous.clsid, previous.guidProfile),
        previous.hkl,
    );
    println!("PROBE previous profile restored: {restored:?}");
    services.shutdown();
    window.close();
    pump(100, &probe);
    println!(
        "PROBE (8) no abort. summary: control {control} (want false), first keystroke {first_key}, after Space {after_space}, shifted {shifted} (want false)"
    );
}

/// Activate an input profile for this thread, falling back to this process
/// if the thread-only form is refused; returns the flags that worked.
fn activate_profile(
    profiles: &ITfInputProcessorProfileMgr,
    kind: u32,
    langid: u16,
    (clsid, profile): (GUID, GUID),
    hkl: HKL,
) -> windows_core::Result<u32> {
    let mut last = Ok(0);
    for flags in [
        0,
        TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
        TF_IPPMF_FORPROCESS | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
    ] {
        // SAFETY: a COM call with live locals for the two GUIDs.
        last = unsafe {
            profiles.ActivateProfile(
                kind,
                langid,
                &raw const clsid,
                &raw const profile,
                hkl,
                flags,
            )
        }
        .map(|()| flags);
        println!(
            "PROBE ActivateProfile(type {kind}, langid {langid:#06x}, flags {flags:#x}): {last:?}"
        );
        if last.is_ok() {
            return last;
        }
    }
    last
}

/// The window's scale and client origin.
fn frame(hwnd: HWND) -> (DevicePixelRatio, DevicePoint) {
    let scale =
        super::super::platform::with_window_context(hwnd, "probe", |c| c.scale_factor.get())
            .and_then(DevicePixelRatio::new)
            .unwrap_or_default();
    let mut origin = POINT::default();
    // SAFETY: a live local.
    let _ = unsafe { ClientToScreen(hwnd, &raw mut origin) };
    (scale, DevicePoint::new(origin.x, origin.y))
}

/// A screenshot of the screen around the field: 32-bit BGRA, top-down.
struct ScreenCapture {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pixels: Vec<u8>,
}

impl ScreenCapture {
    fn grab(field: crate::shared::text_geometry::ScreenRect) -> Self {
        use windows::Win32::Graphics::Gdi::{
            BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY,
            SelectObject,
        };
        // Narrow enough to leave out windows behind the probe that repaint
        // on their own; wide enough for a candidate list under the caret.
        let (x, y, w, h) = (field.left - 150, field.top - 100, 450, 600);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: 40,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        // SAFETY: GDI calls on handles created and released here; `pixels`
        // holds w × h 32-bit pixels, the format `info` describes.
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(dc, bitmap.into());
            let _ = BitBlt(dc, 0, 0, w, h, Some(screen), x, y, SRCCOPY);
            SelectObject(dc, old);
            GetDIBits(
                dc,
                bitmap,
                0,
                h as u32,
                Some(pixels.as_mut_ptr().cast()),
                &raw mut info,
                DIB_RGB_COLORS,
            );
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
        }
        Self { x, y, w, h, pixels }
    }

    /// The screen rect bounding every pixel that differs from `other`.
    fn changed(&self, other: &Self) -> Option<RECT> {
        let mut bounds: Option<RECT> = None;
        for (i, (a, b)) in self
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(other.pixels.as_chunks::<4>().0)
            .enumerate()
        {
            if a.iter().zip(b).any(|(a, b)| a.abs_diff(*b) > 24) {
                let (px, py) = ((i as i32) % self.w + self.x, (i as i32) / self.w + self.y);
                let r = bounds.get_or_insert(RECT {
                    left: px,
                    top: py,
                    right: px + 1,
                    bottom: py + 1,
                });
                r.left = r.left.min(px);
                r.top = r.top.min(py);
                r.right = r.right.max(px + 1);
                r.bottom = r.bottom.max(py + 1);
            }
        }
        bounds
    }

    /// Save as a BMP under the temp directory.
    fn save(&self, name: &str) {
        let path = std::env::temp_dir().join(format!("flui-tsf-probe-{name}.bmp"));
        let mut header = Vec::with_capacity(54);
        header.extend_from_slice(b"BM");
        header.extend_from_slice(&(54 + self.pixels.len() as u32).to_le_bytes());
        header.extend_from_slice(&[0, 0, 0, 0, 54, 0, 0, 0]);
        header.extend_from_slice(&40u32.to_le_bytes());
        header.extend_from_slice(&self.w.to_le_bytes());
        header.extend_from_slice(&(-self.h).to_le_bytes());
        header.extend_from_slice(&[1, 0, 32, 0]);
        header.extend_from_slice(&[0; 24]);
        std::fs::File::create(&path)
            .and_then(|mut file| {
                file.write_all(&header)
                    .and_then(|()| file.write_all(&self.pixels))
            })
            .expect("bmp write");
        println!(
            "PROBE screenshot {} (screen origin {},{})",
            path.display(),
            self.x,
            self.y
        );
    }
}
/// Journal writer: every line to stdout and to the shared buffer.
struct JournalWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for JournalWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        print!("{}", String::from_utf8_lossy(buf));
        self.0.lock().expect("journal").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
