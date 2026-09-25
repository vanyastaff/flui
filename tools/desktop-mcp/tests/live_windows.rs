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

/// The error code of a failed tool reply.
fn code(tool: &str, reply: &Value) -> String {
    assert_eq!(reply["isError"], true, "{tool} succeeded: {reply}");
    reply["structuredContent"]["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn show(step: &str, value: &Value) {
    let text = value.to_string();
    let short: String = text.chars().take(600).collect();
    println!(
        "-- {step}: {short}{}",
        if text.len() > 600 { " …" } else { "" }
    );
}

/// A screenshot of the probe window: its metadata (with the `s` handle) and
/// the decoded image.
fn capture(client: &mut Client, window: &str) -> (Value, image::RgbaImage) {
    use base64::Engine as _;
    let shot = client.call("screenshot", json!({ "window": window }));
    assert_ne!(shot["isError"], true, "screenshot failed: {shot}");
    assert_eq!(shot["content"][0]["type"], "image");
    assert_eq!(shot["content"][0]["mimeType"], "image/png");
    assert!(
        shot.get("structuredContent").is_none(),
        "an image reply carries no structured content: {shot}"
    );
    let meta: Value = serde_json::from_str(shot["content"][1]["text"].as_str().unwrap_or("{}"))
        .expect("BUG: screenshot metadata is JSON");
    let png = base64::engine::general_purpose::STANDARD
        .decode(shot["content"][0]["data"].as_str().unwrap_or_default())
        .expect("BUG: image data is base64");
    let image = image::load_from_memory(&png)
        .expect("BUG: the screenshot is a PNG")
        .to_rgba8();
    // The small probe is below max_side: Windows returns physical pixels,
    // with no implicit border crop misreported as a scale transformation.
    assert_eq!(meta["width"], meta["source"]["width"], "{meta}");
    assert_eq!(meta["height"], meta["source"]["height"], "{meta}");
    assert_eq!(meta["scale_x"], 1.0, "{meta}");
    assert_eq!(meta["scale_y"], 1.0, "{meta}");
    (meta, image)
}

/// The RGBA pixels of a screen rect inside a capture, mapped through the
/// reply's `source` and scale.
fn region(meta: &Value, image: &image::RgbaImage, rect: &Value) -> Vec<u8> {
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
    image::imageops::crop_imm(image, x as u32, y as u32, w as u32, h as u32)
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
        &client.call("launch", json!({ "program": probe.to_string_lossy() })),
    );
    show("launch", &launched);
    let pid = launched["pid"].as_u64().expect("BUG: launch returns a pid");
    assert_eq!(launched["bound"], true, "{launched}");

    let waited = ok(
        "wait_for_window",
        &client.call(
            "wait_for_window",
            json!({ "pid": pid, "timeout_ms": 20_000 }),
        ),
    );
    show("wait_for_window", &waited);
    let window = waited["window"]["id"]
        .as_str()
        .expect("BUG: the probe opens a window within 20 s")
        .to_owned();
    assert!(window.starts_with('w'), "a session handle: {window}");
    assert_eq!(waited["window"]["targetable"], true, "{waited}");

    let listed = ok(
        "list_windows",
        &client.call("list_windows", json!({ "pid": pid })),
    );
    show("list_windows", &listed);
    assert_eq!(listed["windows"][0]["title"], "FLUI Accessibility Probe");
    assert_eq!(
        listed["windows"][0]["id"], window,
        "the same window keeps its handle across listings"
    );

    // Real input is what this test is for: without the foreground it
    // proves nothing, so it retries and then fails rather than skipping.
    let mut activated = Value::Null;
    for _ in 0..5 {
        activated = ok(
            "activate_window",
            &client.call("activate_window", json!({ "window": window })),
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
    assert_eq!(activated["foreground"]["window"], window, "{activated}");

    let (meta, image) = capture(&mut client, &window);
    show("screenshot", &meta);
    assert!(meta["width"].as_u64().unwrap_or(0) > 100);
    let shot = meta["id"]
        .as_str()
        .expect("BUG: a screenshot handle")
        .to_owned();
    assert!(shot.starts_with('s'), "{shot}");

    // AccessKit builds its UIA tree on first client contact; wait for it.
    let button = ok(
        "wait_for",
        &client.call(
            "wait_for",
            json!({ "pid": pid, "name": "Increment", "role": "button", "timeout_ms": 15_000 }),
        ),
    );
    show("wait_for Increment", &button);
    let element = button["element"]["id"]
        .as_str()
        .expect("BUG: the Increment button is found")
        .to_owned();
    assert_eq!(button["element"]["role"], "button");
    assert_eq!(button["element"]["native_role"], "Button");
    assert!(
        button["element"]["actions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == "invoke")),
        "{button}"
    );

    let tree = ok(
        "accessibility_tree",
        &client.call("accessibility_tree", json!({ "pid": pid })),
    );
    show("accessibility_tree", &tree);
    let outline = tree["outline"].as_str().expect("BUG: the outline is text");
    assert!(
        outline.contains(&format!("[ref={element}]")) && outline.contains("[window="),
        "{outline}"
    );
    let tree = ok(
        "accessibility_tree",
        &client.call(
            "accessibility_tree",
            json!({ "window": window, "format": "json", "max_nodes": 5 }),
        ),
    );
    assert_eq!(tree["roots"][0]["window"], window, "{tree}");
    assert_eq!(
        tree["truncated"], true,
        "five nodes cannot be the whole probe"
    );

    // The count is the last label before the button.
    let texts = ok(
        "find",
        &client.call(
            "find",
            json!({ "pid": pid, "role": "label", "format": "json" }),
        ),
    );
    show("find label", &texts);
    let texts = texts["matches"].as_array().cloned().unwrap_or_default();
    assert_eq!(texts.len(), 2, "prompt and count texts");
    let count = texts[1].clone();
    let count_rect = count["rect"].clone();
    let named = count.get("name").is_some();
    if named {
        assert_eq!(count["name"], "0", "count starts at 0");
    }
    let pixels_before = region(&meta, &image, &count_rect);

    let found = ok(
        "find",
        &client.call("find", json!({ "pid": pid, "name": "Increment" })),
    );
    show("find Increment", &found);
    assert!(
        found["outline"]
            .as_str()
            .is_some_and(|o| o.contains(&format!("[ref={element}]"))),
        "the same element keeps its handle across reads: {found}"
    );

    let invoked = ok(
        "invoke",
        &client.call("invoke", json!({ "element": element })),
    );
    show("invoke", &invoked);
    assert_eq!(invoked["element"]["id"], element, "{invoked}");
    if named {
        let after = ok(
            "wait_for",
            &client.call(
                "wait_for",
                json!({ "pid": pid, "role": "label", "name": "1", "timeout_ms": 5_000 }),
            ),
        );
        show("wait_for count text 1", &after);
    } else {
        println!(
            "-- the count label has no UIA Name (FLUI publishes Role::Label text as `label`, \
             and AccessKit's UIA adapter names a Label from its `value`); checking pixels"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let (meta, image) = capture(&mut client, &window);
    let pixels_after = region(&meta, &image, &count_rect);
    assert_ne!(
        pixels_before, pixels_after,
        "the count's pixels change after invoke"
    );
    println!("-- count region pixels changed after invoke");

    // Input the safety rules must refuse. A pid this session never handed
    // out is unknown, refused before anything is looked at.
    let refused = client.call("key", json!({ "combo": "a", "pid": std::process::id() }));
    show(
        "key to an unlisted pid (refused)",
        &refused["structuredContent"],
    );
    assert_eq!(code("key", &refused), "unknown_handle");
    assert_eq!(refused["structuredContent"]["error"]["kind"], "process");
    // A listed window of another application, while the probe is in front,
    // is refused as not the foreground window, which the error names.
    let all = ok("list_windows", &client.call("list_windows", json!({})));
    if let Some(other) = all["windows"].as_array().into_iter().flatten().find(|w| {
        w["pid"].as_u64() != Some(pid) && w["is_minimized"] != true && w["targetable"] == true
    }) {
        let refused = client.call("key", json!({ "combo": "a", "window": other["id"] }));
        show(
            "key to a window behind the probe (refused)",
            &refused["structuredContent"],
        );
        assert_eq!(code("key", &refused), "not_foreground");
        assert_eq!(
            refused["structuredContent"]["error"]["foreground"]["window"], window,
            "{refused}"
        );
    } else {
        println!("-- NOT EXERCISED: no other application window to aim at");
    }

    // A real pointer click on the same button, by element.
    {
        let clicked = ok(
            "click",
            &client.call("click", json!({ "element": element, "window": window })),
        );
        show("click", &clicked);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (meta, image) = capture(&mut client, &window);
        let pixels_clicked = region(&meta, &image, &count_rect);
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
                    json!({ "pid": pid, "role": "label", "name": "2", "timeout_ms": 5_000 }),
                ),
            );
            show("wait_for count text 2", &after_click);
        }
    }

    // And by a screenshot pixel: the button's centre in image pixels.
    {
        let (meta, image) = capture(&mut client, &window);
        let rect = &button["element"]["rect"];
        let at = |k: &str, o: &str, s: &str| {
            let screen = rect[k].as_f64().unwrap_or(0.0) + rect[o].as_f64().unwrap_or(0.0) / 2.0;
            ((screen - meta["source"][k].as_f64().unwrap_or(0.0)) * meta[s].as_f64().unwrap_or(1.0))
                .round() as i64
        };
        let (px, py) = (at("x", "width", "scale_x"), at("y", "height", "scale_y"));
        let clicked = ok(
            "click",
            &client.call(
                "click",
                json!({ "screenshot": meta["id"], "x": px, "y": py, "window": window }),
            ),
        );
        show("click by screenshot pixel", &clicked);
        let pixels_before = region(&meta, &image, &count_rect);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (meta, image) = capture(&mut client, &window);
        assert_ne!(
            pixels_before,
            region(&meta, &image, &count_rect),
            "the count's pixels change after the screenshot-pixel click"
        );
    }

    // Actions the button does not offer name what it does offer.
    for (tool, args) in [
        ("toggle", json!({ "element": element })),
        ("set_value", json!({ "element": element, "value": "5" })),
        ("select", json!({ "element": element })),
        ("expand", json!({ "element": element })),
    ] {
        let reply = client.call(tool, args);
        show(
            &format!("{tool} (unsupported)"),
            &reply["structuredContent"],
        );
        assert_eq!(code(tool, &reply), "action_unsupported");
        let supported = &reply["structuredContent"]["error"]["supported"];
        assert!(
            supported
                .as_array()
                .is_some_and(|s| s.iter().any(|a| a == "invoke")),
            "{reply}"
        );
    }
    // `focus` succeeds only when focus actually moves. The probe's button
    // does not publish itself as keyboard-focusable to UI Automation, so the
    // request is accepted and focus stays put: that must be an error, not a
    // success the agent would act on.
    let focused = client.call("focus", json!({ "element": element }));
    show("focus", &focused["structuredContent"]);
    if button["element"]["focusable"] == true {
        let node = ok("focus", &focused);
        assert_eq!(node["element"]["focused"], true, "{node}");
    } else {
        assert_eq!(code("focus", &focused), "not_supported");
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
    assert_eq!(moved["at"], json!({ "x": cx, "y": cy }));

    // Harmless input inside the probe.
    for (tool, args) in [
        ("key", json!({ "combo": "tab", "window": window })),
        // A character that needs Shift on the layout goes out with it.
        ("key", json!({ "combo": "ctrl+plus", "window": window })),
        ("type_text", json!({ "text": "x", "window": window })),
        (
            "scroll",
            json!({ "x": cx, "y": cy, "dy": 1, "window": window }),
        ),
        (
            "drag",
            json!({ "from": { "x": cx, "y": cy - 40 }, "to": { "x": cx + 30, "y": cy - 40 },
                    "duration_ms": 150, "window": window }),
        ),
    ] {
        let reply = ok(tool, &client.call(tool, args));
        show(tool, &reply);
    }
    // A point outside the target window is refused before any input is sent.
    let outside = client.call(
        "click",
        json!({ "x": -30_000, "y": -30_000, "window": window }),
    );
    show("click outside (refused)", &outside["structuredContent"]);
    assert_eq!(code("click", &outside), "outside_target");

    // The shell takes the Windows key before any window sees it: refused
    // with a target, whoever is in front.
    let shell = client.call("key", json!({ "combo": "win+r", "window": window }));
    show("key win+r (refused)", &shell["structuredContent"]);
    assert_eq!(code("key", &shell), "not_supported");
    let text = shell["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Windows shell") && text.contains("`meta+r`"),
        "{text}"
    );

    // Waiting for the window to be gone ends once it is.
    let killed = ok("kill", &client.call("kill", json!({ "pid": pid })));
    show("kill", &killed);
    let gone = client.call(
        "wait_for",
        json!({ "window": window, "role": "button", "gone": true, "timeout_ms": 5_000 }),
    );
    show("wait_for gone after kill", &gone);
    assert!(
        gone["structuredContent"]["gone"] == true || code("wait_for", &gone) == "gone",
        "{gone}"
    );
    let stale = client.call("invoke", json!({ "element": element }));
    assert_eq!(code("invoke", &stale), "gone", "{stale}");
}
