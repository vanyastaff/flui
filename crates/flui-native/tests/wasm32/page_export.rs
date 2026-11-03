//! The web's development-agent endpoint (ADR-0288 §6, §7) against Node's global object: the
//! page export a script connects through, the page's timer, and the launch values the web
//! platform takes off the page as it starts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_native::page_export::{
    PAGE_EXPORT_VERSION, PageConnection, PageExport, PageExportHandler, PageLine, PageLineClosed,
    PageTimer,
};
use flui_native::{Platform, PlatformError, WebPlatform};
use flui_platform::LaunchKey;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::display::{Probe, install_probe};

#[wasm_bindgen(inline_js = r#"
export function hasGlobal(name) {
    return Object.prototype.hasOwnProperty.call(globalThis, name);
}
export function versionOf(name) { return globalThis[name].version; }
// A script's connection: what it holds, and every line its `onLine` received (`null` for
// the application's end). `onLine` throws when `failing`, and answers a `ping` with an `ack`
// sent from inside it.
export function openConnection(name, failing) {
    const client = { connection: null, lines: [] };
    client.connection = globalThis[name].connect(line => {
        client.lines.push(line);
        if (failing) throw new Error('the script failed');
        if (line === 'ping') client.connection.send('ack');
    });
    return client;
}
// The message `connect(onLine)` throws, or null.
export function connectFails(name, onLine) {
    try { globalThis[name].connect(onLine); return null; } catch (error) { return error.message; }
}
export function connectionVersion(client) { return client.connection.version; }
// The message `send(line)` throws, or null.
export function sendFrom(client, line) {
    try { client.connection.send(line); return null; } catch (error) { return error.message; }
}
export function closeFrom(client) { client.connection.close(); }
export function received(client) { return client.lines.slice(); }
export function sleep(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }
export function installLaunch(value) { globalThis.__FLUI_LAUNCH__ = value; }
// A launch object a classic script declared with `var`: a global that cannot be deleted.
export function declareLaunch(value) {
    Object.defineProperty(globalThis, '__FLUI_LAUNCH__', {
        value, writable: true, configurable: false, enumerable: true,
    });
}
export function launchLeft() { return globalThis.__FLUI_LAUNCH__ !== undefined; }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = hasGlobal)]
    fn has_global(name: &str) -> bool;
    #[wasm_bindgen(js_name = versionOf)]
    fn version_of(name: &str) -> u32;
    #[wasm_bindgen(js_name = openConnection)]
    fn open_connection(name: &str, failing: bool) -> JsValue;
    #[wasm_bindgen(js_name = connectFails)]
    fn connect_fails(name: &str, on_line: JsValue) -> Option<String>;
    #[wasm_bindgen(js_name = connectionVersion)]
    fn connection_version(client: &JsValue) -> u32;
    #[wasm_bindgen(js_name = sendFrom)]
    fn send_from(client: &JsValue, line: JsValue) -> Option<String>;
    #[wasm_bindgen(js_name = closeFrom)]
    fn close_from(client: &JsValue);
    #[wasm_bindgen(js_name = received)]
    fn received_by(client: &JsValue) -> Vec<JsValue>;
    #[wasm_bindgen(js_name = sleep)]
    fn sleep_ms(ms: u32) -> js_sys::Promise;
    #[wasm_bindgen(js_name = installLaunch)]
    fn install_launch(value: JsValue);
    #[wasm_bindgen(js_name = declareLaunch)]
    fn declare_launch(value: JsValue);
    #[wasm_bindgen(js_name = launchLeft)]
    fn launch_left() -> bool;
}

/// What a script's `onLine` received: each line, `None` for the application's end.
fn lines(client: &JsValue) -> Vec<Option<String>> {
    received_by(client)
        .into_iter()
        .map(|line| line.as_string())
        .collect()
}

async fn sleep(ms: u32) {
    wasm_bindgen_futures::JsFuture::from(sleep_ms(ms))
        .await
        .expect("a timeout resolves");
}

/// One connection as the application heard it.
struct Heard {
    line: PageLine,
    lines: RefCell<Vec<String>>,
    closes: Cell<usize>,
}

impl PageConnection for Heard {
    fn receive(&self, line: String) {
        let ping = line == "ping";
        self.lines.borrow_mut().push(line);
        // Written with nothing borrowed: the script sends its `ack` from inside `onLine`.
        if ping {
            self.line.send("ping").expect("the script listens");
        }
    }

    fn closed(&self) {
        self.closes.set(self.closes.get() + 1);
    }
}

/// The application's side of a test export: every connection it took, and whether it takes
/// the next.
#[derive(Clone, Default)]
struct Served {
    connections: Rc<RefCell<Vec<Rc<Heard>>>>,
    refuse: Rc<Cell<bool>>,
}

impl Served {
    fn connection(&self, index: usize) -> Rc<Heard> {
        Rc::clone(&self.connections.borrow()[index])
    }
}

impl PageExportHandler for Served {
    fn connect(&self, line: PageLine) -> Result<Rc<dyn PageConnection>, String> {
        if self.refuse.get() {
            return Err("the application is full".to_owned());
        }
        let heard = Rc::new(Heard {
            line,
            lines: RefCell::new(Vec::new()),
            closes: Cell::new(0),
        });
        self.connections.borrow_mut().push(Rc::clone(&heard));
        Ok(heard)
    }
}

#[wasm_bindgen_test]
fn a_page_export_carries_lines_both_ways_and_its_drop_ends_every_connection() {
    const NAME: &str = "__flui_test_export";
    let served = Served::default();
    let export = PageExport::try_new(NAME, 8, served.clone()).expect("the page takes the export");
    assert_eq!(version_of(NAME), PAGE_EXPORT_VERSION);
    assert!(
        matches!(
            PageExport::try_new(NAME, 8, Served::default()),
            Err(PlatformError::Init { .. })
        ),
        "a name the page already defines is refused"
    );

    let client = open_connection(NAME, false);
    assert_eq!(connection_version(&client), PAGE_EXPORT_VERSION);
    let heard = served.connection(0);
    heard.line.send("one").expect("the script listens");
    heard.line.send("two").expect("the script listens");
    assert_eq!(lines(&client), [Some("one".into()), Some("two".into())]);

    assert_eq!(send_from(&client, "a".into()), None);
    assert_eq!(send_from(&client, "b".into()), None);
    // Cut after one unit past the bound: still too long for the application, copied no further.
    assert_eq!(send_from(&client, "0123456789abcdef".into()), None);
    assert_eq!(
        *heard.lines.borrow(),
        ["a", "b", "012345678"],
        "lines arrive in order, a long one cut past the bound"
    );
    assert_eq!(
        send_from(&client, JsValue::from(7)).as_deref(),
        Some("send takes a string: one line")
    );

    // The application answers a `ping` from inside `receive`, and the script its `ack` from
    // inside `onLine`.
    assert_eq!(send_from(&client, "ping".into()), None);
    assert_eq!(heard.lines.borrow().last().map(String::as_str), Some("ack"));
    assert_eq!(lines(&client).last(), Some(&Some("ping".into())));

    let other = open_connection(NAME, false);
    drop(export);
    assert!(!has_global(NAME), "the global goes with the export");
    for (index, client) in [&client, &other].into_iter().enumerate() {
        assert_eq!(
            lines(client).last(),
            Some(&None),
            "the script hears the end"
        );
        assert_eq!(
            served.connection(index).closes.get(),
            1,
            "the application hears it"
        );
        assert_eq!(
            send_from(client, "late".into()).as_deref(),
            Some("the page export is gone")
        );
        assert_eq!(
            served.connection(index).line.send("late"),
            Err(PageLineClosed)
        );
    }
}

#[wasm_bindgen_test]
fn a_page_connection_ends_alone_when_its_script_or_the_application_closes_it() {
    const NAME: &str = "__flui_test_export_close";
    let served = Served::default();
    let export = PageExport::try_new(NAME, 64, served.clone()).expect("the page takes the export");

    let closed_by_script = open_connection(NAME, false);
    let closed_by_application = open_connection(NAME, false);
    let failing = open_connection(NAME, true);
    let kept = open_connection(NAME, false);

    close_from(&closed_by_script);
    assert_eq!(served.connection(0).closes.get(), 1);
    assert_eq!(
        send_from(&closed_by_script, "late".into()).as_deref(),
        Some("the connection is closed")
    );
    assert_eq!(served.connection(0).line.send("late"), Err(PageLineClosed));
    close_from(&closed_by_script);
    assert_eq!(
        served.connection(0).closes.get(),
        1,
        "a close is heard once"
    );

    served.connection(1).line.close();
    assert_eq!(lines(&closed_by_application), [None]);
    served.connection(1).line.close();
    assert_eq!(
        lines(&closed_by_application),
        [None],
        "the end is told once"
    );
    assert_eq!(
        send_from(&closed_by_application, "late".into()).as_deref(),
        Some("the connection is closed")
    );

    assert_eq!(
        served.connection(2).line.send("lost"),
        Err(PageLineClosed),
        "an onLine that throws ends its connection"
    );
    assert_eq!(
        send_from(&failing, "late".into()).as_deref(),
        Some("the connection is closed")
    );

    served.refuse.set(true);
    assert_eq!(
        connect_fails(NAME, js_sys::Function::new_no_args("").into()).as_deref(),
        Some("the application is full"),
        "a refused connection throws the application's reason"
    );
    assert_eq!(
        connect_fails(NAME, JsValue::from("not a function")).as_deref(),
        Some("connect takes a function: onLine")
    );

    assert_eq!(send_from(&kept, "still".into()), None);
    assert_eq!(*served.connection(3).lines.borrow(), ["still"]);
    drop(export);
    assert_eq!(
        [0, 1, 2, 3].map(|index| served.connection(index).closes.get()),
        [1, 0, 0, 1],
        "the drop tells only the connections still open"
    );
    assert_eq!(lines(&closed_by_application), [None]);
}

#[wasm_bindgen_test]
async fn a_page_timer_runs_once_per_arm_and_never_after_its_cancel_or_drop() {
    let runs = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&runs);
    let timer = PageTimer::try_new(move || counted.set(counted.get() + 1)).expect("a timer");

    timer.arm(Duration::from_micros(1)).expect("armed");
    sleep(30).await;
    assert_eq!(runs.get(), 1);

    timer.arm(Duration::from_millis(5)).expect("armed");
    timer.arm(Duration::from_millis(10)).expect("armed again");
    sleep(40).await;
    assert_eq!(runs.get(), 2, "arming again replaces the pending run");

    timer.arm(Duration::from_millis(5)).expect("armed");
    timer.cancel();
    sleep(30).await;
    assert_eq!(runs.get(), 2, "a cancelled run does not happen");

    timer
        .arm(Duration::MAX)
        .expect("the longest delay is held at the page's");
    timer.arm(Duration::from_millis(5)).expect("armed");
    drop(timer);
    sleep(30).await;
    assert_eq!(runs.get(), 2, "a dropped timer's run does not happen");
}

fn key(key: &'static str) -> LaunchKey {
    LaunchKey::try_new(key).expect("a launch key")
}

fn launch(entries: &[(&str, JsValue)]) -> JsValue {
    let object = js_sys::Object::new();
    for (key, value) in entries {
        js_sys::Reflect::set(&object, &JsValue::from_str(key), value).expect("a property");
    }
    object.into()
}

#[wasm_bindgen_test]
fn the_web_platform_takes_its_launch_values_off_the_page_as_it_starts() {
    install_probe("none");
    let _probe = Probe;
    let token = "t".repeat(40);
    install_launch(launch(&[
        ("flui.agent.endpoint", "__flui_agent".into()),
        ("flui.agent.token", token.as_str().into()),
        ("flui.agent.exit_on_disconnect", "".into()),
    ]));
    let platform = WebPlatform::try_new().expect("the launch is well formed");
    assert!(!launch_left(), "no later script finds the token");
    assert_eq!(
        platform.launch_value(key("flui.agent.endpoint")).as_deref(),
        Some("__flui_agent")
    );
    assert_eq!(platform.launch_value(key("flui.agent.token")), Some(token));
    assert_eq!(
        platform.launch_value(key("flui.agent.exit_on_disconnect")),
        None,
        "an empty value is no value"
    );
    drop(platform);
    let later = WebPlatform::try_new().expect("a page with no launch starts");
    assert_eq!(later.launch_value(key("flui.agent.endpoint")), None);
}

#[wasm_bindgen_test]
fn a_malformed_page_launch_fails_the_start_and_leaves_the_page_all_the_same() {
    install_probe("none");
    let _probe = Probe;
    let malformed: [JsValue; 6] = [
        JsValue::NULL,
        "flui.agent.token=t".into(),
        js_sys::Array::of1(&"t".into()).into(),
        launch(&[("flui.agent.token", JsValue::from(7))]),
        launch(&[("FLUI_AGENT_TOKEN", "t".into())]),
        launch(&[("flui.agent.token", "t".repeat(4097).into())]),
    ];
    for value in malformed {
        let shown = format!("{value:?}");
        install_launch(value);
        match WebPlatform::try_new() {
            Err(PlatformError::Init { message }) => assert!(
                message.contains("__FLUI_LAUNCH__"),
                "the error names the launch object: {message}"
            ),
            Err(other) => panic!("{shown}: refused for another reason: {other}"),
            Ok(_) => panic!("{shown}: a malformed launch starts the platform"),
        }
        assert!(!launch_left(), "{shown}: removed though it was refused");
    }
}

#[wasm_bindgen_test]
fn a_launch_object_a_script_declared_is_cleared_when_it_cannot_be_deleted() {
    install_probe("none");
    let _probe = Probe;
    declare_launch(launch(&[("flui.agent.endpoint", "__flui_agent".into())]));
    let platform = WebPlatform::try_new().expect("the launch is well formed");
    assert!(!launch_left(), "the global that stays holds nothing");
    assert_eq!(
        platform.launch_value(key("flui.agent.endpoint")).as_deref(),
        Some("__flui_agent")
    );
}
