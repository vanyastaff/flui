//! Live smoke on a real Windows desktop: drives the repository's
//! `a11y_probe` counter through the MCP tools the way an agent would, and
//! checks the count the accessibility tree reports.
//!
//! Build the probe, then run this test by name:
//!
//! ```text
//! cargo build --release --example a11y_probe --features material,a11y
//! cargo nextest run -p flui-desktop-mcp --test live_windows --run-ignored only --no-capture
//! ```
//!
//! `FLUI_A11Y_PROBE` overrides the probe path. The test moves the real
//! pointer, and clicks, types and drags inside the probe window, each only
//! after the probe is confirmed in front.
#![cfg(target_os = "windows")]

mod support;

use std::path::PathBuf;

use serde_json::{Value, json};
use support::Client;

fn probe() -> PathBuf {
    std::env::var_os("FLUI_A11Y_PROBE").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/release/examples/a11y_probe.exe")
        },
        PathBuf::from,
    )
}

/// The structured JSON of a successful tool reply.
fn ok(tool: &str, reply: &Value) -> Value {
    assert_ne!(reply["isError"], true, "{tool} failed: {reply}");
    reply["structuredContent"].clone()
}

fn show(step: &str, value: &Value) {
    let text = value.to_string();
    let short: String = text.chars().take(600).collect();
    println!(
        "-- {step}: {short}{}",
        if text.len() > 600 { " …" } else { "" }
    );
}

/// The RGBA pixels of a screen rect, from a capture of the probe window
/// (whatever monitor it is on), mapped through the reply's `source` and
/// scale.
fn region(client: &mut Client, window_id: u64, rect: &Value) -> Vec<u8> {
    use base64::Engine as _;
    let shot = client.call("screenshot", json!({ "window_id": window_id }));
    assert_ne!(shot["isError"], true, "screenshot failed: {shot}");
    let meta: Value = serde_json::from_str(shot["content"][1]["text"].as_str().unwrap_or("{}"))
        .expect("BUG: screenshot metadata is JSON");
    let png = base64::engine::general_purpose::STANDARD
        .decode(shot["content"][0]["data"].as_str().unwrap_or_default())
        .expect("BUG: image data is base64");
    let image = image::load_from_memory(&png)
        .expect("BUG: the screenshot is a PNG")
        .to_rgba8();
    let at = |v: &Value, k: &str| v[k].as_i64().expect("BUG: rect fields are integers");
    let scale = |k: &str| meta[k].as_f64().expect("BUG: the scale is a number");
    let px = |screen: i64, origin: i64, s: f64| ((screen - origin) as f64 * s).round() as i64;
    let x = px(at(rect, "x"), at(&meta["source"], "x"), scale("scale_x"));
    let y = px(at(rect, "y"), at(&meta["source"], "y"), scale("scale_y"));
    let w = (at(rect, "width") as f64 * scale("scale_x")).round() as i64;
    let h = (at(rect, "height") as f64 * scale("scale_y")).round() as i64;
    assert!(
        x >= 0 && y >= 0 && x + w <= i64::from(image.width()) && y + h <= i64::from(image.height()),
        "the count lies inside the probe window's capture"
    );
    image::imageops::crop_imm(&image, x as u32, y as u32, w as u32, h as u32)
        .to_image()
        .into_raw()
}

#[test]
#[ignore = "needs an interactive Windows desktop"]
fn a11y_probe_counter_through_mcp() {
    let probe = probe();
    assert!(
        probe.exists(),
        "{} is missing; build it with `cargo build --release --example a11y_probe --features material,a11y`",
        probe.display()
    );
    let (mut client, init) = Client::start();
    show("initialize", &init["serverInfo"]);

    let launched = ok(
        "launch",
        &client.call(
            "launch",
            json!({ "program": probe.to_string_lossy(), "wait_for_window_ms": 20_000 }),
        ),
    );
    show("launch", &launched);
    let pid = launched["pid"].as_u64().expect("BUG: launch returns a pid");
    let window_id = launched["window"]["id"]
        .as_u64()
        .expect("BUG: the probe opens a window within 20 s");

    let listed = ok(
        "list_windows",
        &client.call("list_windows", json!({ "pid": pid })),
    );
    show("list_windows", &listed);
    assert_eq!(listed["windows"][0]["title"], "FLUI Accessibility Probe");

    // Real input is what this test is for: without the foreground it
    // proves nothing, so it retries and then fails rather than skipping.
    let mut activated = Value::Null;
    for _ in 0..5 {
        activated = ok(
            "activate_window",
            &client.call("activate_window", json!({ "window_id": window_id })),
        );
        if activated["became_foreground"] == true {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    show("activate_window", &activated);
    assert_eq!(
        activated["became_foreground"], true,
        "inconclusive: Windows kept another window in front of the probe, so no real input \
         could be tested; run it with the desktop unlocked and idle"
    );

    let shot = client.call("screenshot", json!({ "window_id": window_id }));
    assert_ne!(shot["isError"], true, "screenshot failed: {shot}");
    assert_eq!(shot["content"][0]["type"], "image");
    assert_eq!(shot["content"][0]["mimeType"], "image/png");
    let meta: Value = serde_json::from_str(shot["content"][1]["text"].as_str().unwrap_or("{}"))
        .expect("BUG: screenshot metadata is JSON");
    show("screenshot", &meta);
    assert!(meta["width"].as_u64().unwrap_or(0) > 100);

    // AccessKit builds its UIA tree on first client contact; wait for it.
    let button = ok(
        "wait_for",
        &client.call(
            "wait_for",
            json!({ "pid": pid, "name": "Increment", "role": "Button", "timeout_ms": 15_000 }),
        ),
    );
    show("wait_for Increment", &button);

    let tree = ok(
        "accessibility_tree",
        &client.call("accessibility_tree", json!({ "pid": pid })),
    );
    show("accessibility_tree", &tree);

    // The count is the last Text before the button.
    let texts = ok(
        "find",
        &client.call("find", json!({ "pid": pid, "role": "Text" })),
    );
    show("find Text", &texts);
    let texts = texts["matches"].as_array().cloned().unwrap_or_default();
    assert_eq!(texts.len(), 2, "prompt and count texts");
    let count = texts[1].clone();
    let count_rect = count["rect"].clone();
    let named = count.get("name").is_some();
    if named {
        assert_eq!(count["name"], "0", "count starts at 0");
    }
    let pixels_before = region(&mut client, window_id, &count_rect);

    let found = ok(
        "find",
        &client.call("find", json!({ "pid": pid, "name": "Increment" })),
    );
    show("find Increment", &found);
    let element = found["matches"][0]["id"]
        .as_str()
        .expect("BUG: the Increment button is found")
        .to_owned();
    assert_eq!(
        element,
        button["element"]["id"].as_str().unwrap_or_default(),
        "the same element keeps its handle across reads"
    );

    let invoked = ok(
        "invoke",
        &client.call("invoke", json!({ "element": element })),
    );
    show("invoke", &invoked);
    if named {
        let after = ok(
            "wait_for",
            &client.call(
                "wait_for",
                json!({ "pid": pid, "role": "Text", "name": "1", "timeout_ms": 5_000 }),
            ),
        );
        show("wait_for count text 1", &after);
    } else {
        println!(
            "-- the count Text has no UIA Name (FLUI publishes Role::Label text as `label`, \
             and AccessKit's UIA adapter names a Label from its `value`); checking pixels"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let pixels_after = region(&mut client, window_id, &count_rect);
    assert_ne!(
        pixels_before, pixels_after,
        "the count's pixels change after invoke"
    );
    println!("-- count region pixels changed after invoke");

    // Input the safety rule must refuse: the test process owns no foreground window.
    let refused = client.call("key", json!({ "combo": "a", "pid": std::process::id() }));
    show("key refused", &refused["content"][0]);
    assert_eq!(refused["isError"], true, "{refused}");
    let text = refused["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("not the foreground window"),
        "refused for that reason: {text}"
    );

    // A real pointer click on the same button.
    {
        let clicked = ok(
            "click",
            &client.call(
                "click",
                json!({ "element": element, "window_id": window_id }),
            ),
        );
        show("click", &clicked);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let pixels_clicked = region(&mut client, window_id, &count_rect);
        assert_ne!(
            pixels_after, pixels_clicked,
            "the count's pixels change after the click"
        );
        println!("-- count region pixels changed after click");
        if named {
            let after_click = ok(
                "wait_for",
                &client.call(
                    "wait_for",
                    json!({ "pid": pid, "role": "Text", "name": "2", "timeout_ms": 5_000 }),
                ),
            );
            show("wait_for count text 2", &after_click);
        }
    }

    // Pattern actions the button does not support name what it does support.
    for (tool, args) in [
        ("toggle", json!({ "element": element })),
        ("set_value", json!({ "element": element, "value": "5" })),
        ("select", json!({ "element": element })),
    ] {
        let reply = client.call(tool, args);
        show(&format!("{tool} (unsupported)"), &reply["content"][0]);
        assert_eq!(reply["isError"], true, "{reply}");
        let text = reply["content"][0]["text"].as_str().unwrap_or_default();
        assert!(text.contains("supports: [Invoke]"), "{text}");
    }
    // `focus` succeeds only when focus actually moves. The probe's button
    // does not publish itself as keyboard-focusable to UI Automation, so the
    // request is accepted and focus stays put: that must be an error, not a
    // success the agent would act on.
    let focused = client.call("focus", json!({ "element": element }));
    show("focus", &focused["content"][0]);
    if button["element"]["is_keyboard_focusable"] == true {
        let node = ok("focus", &focused);
        assert_eq!(node["element"]["has_keyboard_focus"], true, "{node}");
    } else {
        assert_eq!(focused["isError"], true, "{focused}");
        let text = focused["content"][0]["text"].as_str().unwrap_or_default();
        assert!(text.contains("did not take keyboard focus"), "{text}");
    }

    // The pointer lands where asked, in physical pixels (DPI-aware SendInput).
    let (cx, cy) = (
        button["element"]["rect"]["x"].as_i64().unwrap_or(0) + 20,
        button["element"]["rect"]["y"].as_i64().unwrap_or(0) + 10,
    );
    let moved = ok(
        "move_mouse",
        &client.call("move_mouse", json!({ "x": cx, "y": cy })),
    );
    show("move_mouse", &moved);
    assert_eq!(moved["pointer"], json!({ "x": cx, "y": cy }));

    // Harmless input inside the probe.
    {
        for (tool, args) in [
            ("key", json!({ "combo": "tab", "window_id": window_id })),
            // A character that needs Shift on the layout goes out with it.
            (
                "key",
                json!({ "combo": "ctrl+plus", "window_id": window_id }),
            ),
            ("type_text", json!({ "text": "x", "window_id": window_id })),
            (
                "scroll",
                json!({ "x": cx, "y": cy, "dy": 1, "window_id": window_id }),
            ),
            (
                "drag",
                json!({ "from": { "x": cx, "y": cy - 40 }, "to": { "x": cx + 30, "y": cy - 40 },
                        "duration_ms": 150, "window_id": window_id }),
            ),
        ] {
            let reply = ok(tool, &client.call(tool, args));
            show(tool, &reply);
        }
    }
    // A point outside the target window is refused before any input is sent.
    let outside = client.call(
        "click",
        json!({ "x": -30_000, "y": -30_000, "window_id": window_id }),
    );
    show("click outside (refused)", &outside["content"][0]);
    assert_eq!(outside["isError"], true, "{outside}");
    let text = outside["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("no input is sent there"),
        "refused for that reason: {text}"
    );

    // The shell takes the Windows key before any window sees it: refused
    // with a target, whoever is in front.
    let shell = client.call("key", json!({ "combo": "win+r", "window_id": window_id }));
    show("key win+r (refused)", &shell["content"][0]);
    let text = shell["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Windows shell") && text.contains("`meta+r`"),
        "{text}"
    );

    let killed = ok("kill", &client.call("kill", json!({ "pid": pid })));
    show("kill", &killed);
}
