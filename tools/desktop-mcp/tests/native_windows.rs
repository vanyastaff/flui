//! Real Win32 controls driven only through MCP, with observable postconditions.
//! Run explicitly on an interactive Windows desktop:
//! `cargo nextest run -p flui-desktop-mcp --test native_windows --run-ignored only --test-threads 1 --no-capture`.
#![cfg(target_os = "windows")]
#![expect(
    unsafe_code,
    reason = "the isolated native test fixture owns its Win32 windows and message loop"
)]

mod support;

use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::Client;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOWFRAME, GetStockObject, GetSysColor, HBRUSH, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::SS_BLACKRECT;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput,
    SetFocus,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_AUTOCHECKBOX, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_READONLY,
    GetMessageW, GetSystemMetrics, HMENU, IsDialogMessageW, MSG, PostQuitMessage, RegisterClassW,
    SM_SWAPBUTTON, SetTimer, SetWindowTextW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_DESTROY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_TIMER, WNDCLASSW,
    WS_BORDER, WS_CHILD, WS_MINIMIZE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy, Default)]
struct Metrics {
    wheel: i32,
    down: Option<(i32, i32)>,
    drag: (i32, i32),
}

thread_local! {
    static METRICS: Cell<Metrics> = const { Cell::new(Metrics { wheel: 0, down: None, drag: (0, 0) }) };
    static EARLY_RELEASED: Cell<bool> = const { Cell::new(false) };
}

fn point(lparam: LPARAM) -> (i32, i32) {
    let bytes = lparam.0.to_le_bytes();
    (
        i32::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        i32::from(i16::from_le_bytes([bytes[2], bytes[3]])),
    )
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows invokes this procedure with the live fixture HWND and
    // message values. Text buffers remain alive until SetWindowTextW returns.
    unsafe {
        match message {
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            WM_TIMER => {
                let _ = DestroyWindow(window);
                return LRESULT(0);
            }
            WM_LBUTTONDOWN => METRICS.with(|state| {
                let _ = SetFocus(Some(window));
                let mut metrics = state.get();
                metrics.down = Some(point(lparam));
                state.set(metrics);
            }),
            WM_MOUSEMOVE
                if wparam.0 & 1 != 0
                    && std::env::var_os("FLUI_MCP_DROP_EARLY").is_some()
                    && !EARLY_RELEASED.with(Cell::get) =>
            {
                // This event belongs to our visible fixture while the MCP drag
                // holds its primary button. An injected release changes the real
                // OS button state, exactly what an intervening physical release
                // does; no press or move is injected outside the fixture.
                let flags = if GetSystemMetrics(SM_SWAPBUTTON) == 0 {
                    MOUSEEVENTF_LEFTUP
                } else {
                    MOUSEEVENTF_RIGHTUP
                };
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dwFlags: flags,
                            ..Default::default()
                        },
                    },
                };
                if SendInput(&[input], size_of::<INPUT>() as i32) == 1 {
                    EARLY_RELEASED.with(|released| released.set(true));
                }
            }
            WM_LBUTTONUP | WM_MOUSEWHEEL => METRICS.with(|state| {
                let mut metrics = state.get();
                if message == WM_MOUSEWHEEL {
                    let bytes = wparam.0.to_le_bytes();
                    metrics.wheel += i32::from(i16::from_le_bytes([bytes[2], bytes[3]]));
                } else if let Some(from) = metrics.down.take() {
                    let to = point(lparam);
                    metrics.drag = (to.0 - from.0, to.1 - from.1);
                }
                state.set(metrics);
                let title: Vec<u16> = format!(
                    "MCP Native Fixture wheel={} drag={},{}{}",
                    metrics.wheel,
                    metrics.drag.0,
                    metrics.drag.1,
                    if EARLY_RELEASED.with(Cell::get) {
                        " released_early=true"
                    } else {
                        ""
                    }
                )
                .encode_utf16()
                .chain(Some(0))
                .collect();
                let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
            }),
            _ => {}
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

/// This helper is a separate process launched and contained by the MCP server.
/// Direct ignored-test runs without its marker finish without opening a window.
#[test]
#[ignore = "helper process for native_controls_through_mcp"]
fn native_fixture_process() {
    if std::env::var_os("FLUI_MCP_NATIVE_FIXTURE").is_none() {
        return;
    }
    // SAFETY: all windows belong to this thread, the registered procedure is
    // static, and all borrowed names survive their synchronous API calls.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .expect("BUG: fixture sets DPI awareness before creating windows");
        let instance = HINSTANCE(GetModuleHandleW(None).expect("BUG: executable module").0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            lpszClassName: w!("FluiMcpNativeFixture"),
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&raw const class), 0);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("MCP Native Fixture wheel=0 drag=0,0"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_MINIMIZE,
            100,
            100,
            620,
            440,
            None,
            None,
            Some(instance),
            None,
        )
        .expect("BUG: fixture window");
        let child = |class, text, extra, y, id| {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(extra),
                25,
                y,
                240,
                30,
                Some(window),
                Some(HMENU(id as *mut core::ffi::c_void)),
                Some(instance),
                None,
            )
            .expect("BUG: fixture control")
        };
        let _ = child(w!("EDIT"), w!(""), WS_BORDER.0, 25, 1001_usize);
        let _ = child(
            w!("EDIT"),
            w!("locked"),
            WS_BORDER.0 | ES_READONLY as u32,
            70,
            1002_usize,
        );
        let _ = child(
            w!("BUTTON"),
            w!("Native checkbox"),
            BS_AUTOCHECKBOX as u32,
            115,
            1003_usize,
        );
        // A solid native marker gives capture a pixel-precise oracle whose
        // screen rectangle is independently obtained through accessibility.
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Pixel landmark"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_BLACKRECT.0),
            330,
            185,
            40,
            24,
            Some(window),
            Some(HMENU(1004_usize as *mut core::ffi::c_void)),
            Some(instance),
            None,
        )
        .expect("BUG: pixel landmark control");
        // A bounded lifetime also covers a containment regression in the server.
        assert_ne!(SetTimer(Some(window), 1, 60_000, None), 0);
        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&raw mut message, None, 0, 0).0;
            assert_ne!(status, -1, "BUG: fixture message loop");
            if status == 0 {
                break;
            }
            if !IsDialogMessageW(window, &raw const message).as_bool() {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
    }
}

fn call(client: &mut Client, tool: &str, args: Value) -> Value {
    for (attempt, args) in std::iter::repeat_n(args, 3).enumerate() {
        let reply = client.call(tool, args);
        if reply["isError"] != true {
            return reply["structuredContent"].clone();
        }
        let error = &reply["structuredContent"]["error"];
        // The real desktop is shared. A pre-event refusal can be retried;
        // a partial or uncertain action must never be repeated by this helper.
        if attempt < 2 && error["code"] == "busy" && error.get("effect").is_none() {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        panic!("{tool}: {reply}");
    }
    unreachable!("BUG: every final attempt returns or panics")
}

fn element(client: &mut Client, window: &str, id: &str) -> String {
    let result = call(
        client,
        "wait_for",
        json!({"window": window, "automation_id": id, "timeout_ms": 5000}),
    );
    result["element"]["id"]
        .as_str()
        .expect("BUG: native element handle")
        .to_owned()
}

fn state(client: &mut Client, element: &str, expected: Value) {
    call(
        client,
        "wait_for",
        json!({"root": element, "element": element, "state": expected, "timeout_ms": 5000}),
    );
}

fn title_until(client: &mut Client, pid: u64, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = call(client, "list_windows", json!({"pid": pid}));
        if result["windows"]
            .as_array()
            .is_some_and(|windows| windows.iter().any(|w| w["title"] == expected))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected:?}, got {result}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn assert_capture_landmark(client: &mut Client, window: &str) {
    use base64::Engine as _;
    let marker = call(
        client,
        "wait_for",
        json!({
            "window": window, "automation_id": "1004", "timeout_ms": 5000
        }),
    );
    let rect = &marker["element"]["rect"];
    let reply = client.call("screenshot", json!({"window": window, "max_side": 4096}));
    assert_ne!(reply["isError"], true, "screenshot: {reply}");
    let content = reply["content"]
        .as_array()
        .expect("BUG: screenshot content");
    let image = content
        .iter()
        .find(|item| item["type"] == "image")
        .expect("BUG: screenshot image");
    let meta = content
        .iter()
        .find(|item| item["type"] == "text")
        .expect("BUG: screenshot metadata");
    let meta: Value = serde_json::from_str(meta["text"].as_str().expect("BUG: text metadata"))
        .expect("BUG: JSON screenshot metadata");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image["data"].as_str().expect("BUG: base64 screenshot"))
        .expect("BUG: valid base64 screenshot");
    let image = image::load_from_memory(&bytes)
        .expect("BUG: PNG screenshot")
        .to_rgba8();
    assert_eq!(
        meta["scale_x"], 1.0,
        "native window must not be rescaled: {meta}"
    );
    assert_eq!(
        meta["scale_y"], 1.0,
        "native window must not be rescaled: {meta}"
    );
    let integer = |value: &Value, key: &str| value[key].as_i64().expect("BUG: integer geometry");
    let x = integer(rect, "x") - integer(&meta["source"], "x");
    let y = integer(rect, "y") - integer(&meta["source"], "y");
    let width = integer(rect, "width");
    let height = integer(rect, "height");
    assert_eq!((width, height), (40, 24), "{marker}");
    // SS_BLACKRECT uses COLOR_WINDOWFRAME, not literal RGB black:
    // https://learn.microsoft.com/windows/win32/controls/static-control-styles
    // SAFETY: reads one system color; no pointers or mutable OS state.
    let color = unsafe { GetSysColor(COLOR_WINDOWFRAME) }.to_le_bytes();
    let inside = [color[0], color[1], color[2]];
    let outside = [255; 3];
    assert!(
        inside.iter().zip(outside).any(|(&a, b)| a.abs_diff(b) > 32),
        "the marker must contrast beyond both color tolerances"
    );
    for (px, py, expected) in [
        (x + 2, y + height / 2, inside),
        (x + width - 3, y + height / 2, inside),
        (x + width / 2, y + 2, inside),
        (x + width / 2, y + height - 3, inside),
        (x - 2, y + height / 2, outside),
        (x + width + 1, y + height / 2, outside),
        (x + width / 2, y - 2, outside),
        (x + width / 2, y + height + 1, outside),
    ] {
        let px = u32::try_from(px).expect("BUG: marker lies inside capture");
        let py = u32::try_from(py).expect("BUG: marker lies inside capture");
        let pixel = image.get_pixel(px, py).0;
        // Compositor color conversion can shift channel values slightly.
        // Preserve the independent spatial oracle: crossing this edge swaps
        // the dark frame color and white, far beyond this tolerance.
        assert!(
            pixel[..3]
                .iter()
                .zip(expected)
                .all(|(&actual, wanted)| actual.abs_diff(wanted) <= 16),
            "capture pixel ({px},{py}) {pixel:?} must match native marker edge {expected:?}; marker={rect}, capture={meta}"
        );
    }
}

#[test]
#[ignore = "moves real pointer and keyboard on an interactive Windows desktop"]
fn native_controls_through_mcp() {
    let (mut client, _) = Client::start();
    let executable = std::env::current_exe().expect("BUG: test executable path");
    let launched = call(
        &mut client,
        "launch",
        json!({
            "program": executable.to_string_lossy(),
            "args": ["--exact", "native_fixture_process", "--ignored", "--nocapture"],
            "env": {"FLUI_MCP_NATIVE_FIXTURE": "1"}
        }),
    );
    let pid = launched["pid"].as_u64().expect("BUG: child pid");
    let waited = call(
        &mut client,
        "wait_for_window",
        json!({"pid": pid, "timeout_ms": 15000}),
    );
    let window = waited["window"]["id"]
        .as_str()
        .expect("BUG: child window")
        .to_owned();
    assert_eq!(waited["window"]["is_minimized"], true, "{waited}");
    let activated = call(&mut client, "activate_window", json!({"window": window}));
    assert_eq!(activated["became_foreground"], true, "{activated}");
    assert_eq!(activated["window"]["id"], window, "{activated}");
    assert_eq!(activated["window"]["is_minimized"], false, "{activated}");
    assert_eq!(activated["window"]["is_focused"], true, "{activated}");
    let restored = call(&mut client, "list_windows", json!({"pid": pid}));
    let current = restored["windows"]
        .as_array()
        .expect("BUG: window list")
        .iter()
        .find(|entry| entry["id"] == window)
        .expect("BUG: restored fixture keeps its handle");
    assert_eq!(activated["window"]["rect"], current["rect"], "{activated}");
    let unknown = client.call(
        "wait_for",
        json!({"window": window, "element": "e999999", "gone": true, "timeout_ms": 100}),
    );
    assert_eq!(unknown["isError"], true, "{unknown}");
    assert_eq!(
        unknown["structuredContent"]["error"]["code"], "unknown_handle",
        "{unknown}"
    );
    let edit = element(&mut client, &window, "1001");
    let readonly = element(&mut client, &window, "1002");
    let checkbox = element(&mut client, &window, "1003");
    assert_capture_landmark(&mut client, &window);

    call(&mut client, "focus", json!({"element": edit}));
    state(&mut client, &edit, json!({"focused": true}));
    let landmark = element(&mut client, &window, "1004");
    let refused = client.call("focus", json!({"element": landmark}));
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "action_unsupported",
        "{refused}"
    );
    assert!(
        refused["structuredContent"]["error"]["effect"].is_null(),
        "{refused}"
    );
    state(&mut client, &edit, json!({"focused": true}));
    call(
        &mut client,
        "type_text",
        json!({"window": window, "text": "Agent42"}),
    );
    state(&mut client, &edit, json!({"value": "Agent42"}));
    call(
        &mut client,
        "key",
        json!({"window": window, "combo": "backspace"}),
    );
    state(&mut client, &edit, json!({"value": "Agent4"}));
    call(
        &mut client,
        "key",
        json!({"window": window, "combo": "tab"}),
    );
    state(&mut client, &readonly, json!({"focused": true}));
    call(
        &mut client,
        "set_value",
        json!({"element": edit, "value": "assigned"}),
    );
    state(&mut client, &edit, json!({"value": "assigned"}));
    let refused = client.call(
        "set_value",
        json!({"element": readonly, "value": "changed"}),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "action_unsupported",
        "{refused}"
    );
    state(&mut client, &readonly, json!({"value": "locked"}));

    state(&mut client, &checkbox, json!({"checked": false}));
    call(&mut client, "toggle", json!({"element": checkbox}));
    state(&mut client, &checkbox, json!({"checked": true}));
    // A native accessibility provider may focus/activate its child HWND.
    // Establish the explicit top-level target again before physical input.
    let activated = call(&mut client, "activate_window", json!({"window": window}));
    assert_eq!(activated["became_foreground"], true, "{activated}");
    call(
        &mut client,
        "click",
        json!({"window": window, "element": checkbox}),
    );
    state(&mut client, &checkbox, json!({"checked": false}));

    let windows = call(&mut client, "list_windows", json!({"pid": pid}));
    let rect = &windows["windows"][0]["rect"];
    let x = rect["x"].as_i64().expect("BUG: window x") + 320;
    let y = rect["y"].as_i64().expect("BUG: window y") + 260;
    call(
        &mut client,
        "click",
        json!({"window": window, "x": x, "y": y}),
    );
    call(
        &mut client,
        "scroll",
        json!({"window": window, "x": x, "y": y, "dy": 1}),
    );
    title_until(&mut client, pid, "MCP Native Fixture wheel=-120 drag=0,0");
    call(
        &mut client,
        "drag",
        json!({"window": window, "from": {"x": x, "y": y}, "to": {"x": x + 100, "y": y + 40}, "duration_ms": 150}),
    );
    title_until(
        &mut client,
        pid,
        "MCP Native Fixture wheel=-120 drag=100,40",
    );
    call(&mut client, "kill", json!({"pid": pid}));
}

#[test]
#[ignore = "moves the real pointer and releases the drag button inside its owned fixture"]
fn a_native_release_interrupts_the_drag() {
    let (mut client, _) = Client::start();
    let executable = std::env::current_exe().expect("BUG: test executable path");
    let launched = call(
        &mut client,
        "launch",
        json!({
            "program": executable.to_string_lossy(),
            "args": ["--exact", "native_fixture_process", "--ignored", "--nocapture"],
            "env": {"FLUI_MCP_NATIVE_FIXTURE": "1", "FLUI_MCP_DROP_EARLY": "1"}
        }),
    );
    let pid = launched["pid"].as_u64().expect("BUG: child pid");
    let waited = call(
        &mut client,
        "wait_for_window",
        json!({"pid": pid, "timeout_ms": 15000}),
    );
    let window = waited["window"]["id"].as_str().expect("BUG: child window");
    let activated = call(&mut client, "activate_window", json!({"window": window}));
    assert_eq!(activated["became_foreground"], true, "{activated}");
    let x = activated["window"]["rect"]["x"].as_i64().expect("BUG: x") + 320;
    let y = activated["window"]["rect"]["y"].as_i64().expect("BUG: y") + 260;
    let result = client.call(
        "drag",
        json!({
            "window": window,
            "from": {"x": x, "y": y}, "to": {"x": x + 100, "y": y + 40},
            "duration_ms": 1000
        }),
    );
    assert_eq!(result["isError"], true, "{result}");
    let error = &result["structuredContent"]["error"];
    assert_eq!(error["code"], "busy", "{result}");
    assert_eq!(error["effect"]["kind"], "partial", "{result}");
    assert!(
        error["message"]
            .as_str()
            .expect("BUG: message")
            .contains("no longer down"),
        "{result}"
    );
    assert!(
        error["effect"]["sent"].as_u64() < error["effect"]["total"].as_u64(),
        "{result}"
    );
    let listed = call(&mut client, "list_windows", json!({"pid": pid}));
    let title = listed["windows"][0]["title"]
        .as_str()
        .expect("BUG: fixture title");
    assert!(title.contains("released_early=true"), "{listed}");
    assert!(!title.contains("drag=100,40"), "{listed}");
    // The actual system cursor is the independent oracle that no subsequent
    // hover movement completed the abandoned drag path.
    let mut cursor = windows::Win32::Foundation::POINT::default();
    // SAFETY: the output pointer refers to this initialized, writable POINT.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&raw mut cursor) }
        .expect("BUG: cursor position is readable");
    assert_ne!(
        (i64::from(cursor.x), i64::from(cursor.y)),
        (x + 100, y + 40)
    );
    call(&mut client, "kill", json!({"pid": pid}));
}
