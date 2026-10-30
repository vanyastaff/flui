//! The semantics host: the tree a window publishes, read back from the DOM
//! the browser's accessibility tree is built from, and the DOM events an
//! assistive technology sends back.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use accesskit::{Action, ActionRequest, Node, NodeId, Rect, Role, TreeId, TreeInfo, TreeUpdate};
use flui_native::{HostWindow, Platform, WebPlatform};
use flui_platform::WindowOptions;
use flui_semantics::platform::PlatformAccessibility;
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::RecordingWindowHandler;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(inline_js = r#"
function host() { return document.querySelector('flui-semantics-host'); }
export function placeholderLabel() {
    return document.querySelector('flui-a11y-placeholder')?.getAttribute('aria-label') ?? null;
}
export function activatePlaceholder() { document.querySelector('flui-a11y-placeholder').click(); }
export function focusPlaceholder() { document.querySelector('flui-a11y-placeholder').focus(); }
// What holds the page's focus: a node's identifier, the canvas, or the tag.
export function focused() {
    const active = document.activeElement;
    if (!active) return 'nothing';
    if (active.id === 'flui-canvas') return 'canvas';
    return active.getAttribute('data-flui-id') ?? active.tagName.toLowerCase();
}
// The host's elements as the browser's accessibility tree reads them.
export function semanticsTree() {
    const read = element => {
        const children = [...element.children].map(read);
        const node = { role: element.getAttribute('role') };
        for (const [key, name] of [['label', 'aria-label'], ['id', 'data-flui-id']]) {
            const value = element.getAttribute(name);
            if (value !== null) node[key] = value;
        }
        const text = [...element.childNodes].filter(child => child.nodeType === Node.TEXT_NODE)
            .map(child => child.data).join('');
        if (text) node.text = text;
        if (children.length) node.children = children;
        return node;
    };
    const root = host()?.firstElementChild;
    return JSON.stringify(root ? read(root) : null);
}
export function hostOpacity() { return getComputedStyle(host()).opacity; }
export function clickNode(id) { document.querySelector(`[data-flui-id="${id}"]`).click(); }
export function clickText(text) {
    [...host().querySelectorAll('flui-node')].find(element => element.textContent === text).click();
}
// The host's box, the canvas's and the increment button's in the viewport, each [left, top,
// width, height].
export function hostAndCanvas() {
    const box = element => {
        const rect = element.getBoundingClientRect();
        return [rect.left, rect.top, rect.width, rect.height];
    };
    return [
        ...box(host()),
        ...box(document.getElementById('flui-canvas')),
        ...box(document.querySelector('[data-flui-id="increment"]')),
    ];
}
// The canvas, styled `canvasStyle`, inside a scrolling container styled `style`, below the
// page's own content.
export function scrolledCanvas(style, canvasStyle) {
    document.getElementById('flui-canvas')?.remove();
    document.getElementById('scroller')?.remove();
    const scroller = document.createElement('div');
    scroller.id = 'scroller';
    scroller.style.cssText = `height: 120px; overflow: auto; margin-top: 40px; ${style}`;
    const above = document.createElement('div');
    above.style.height = '50px';
    const canvas = document.createElement('canvas');
    canvas.id = 'flui-canvas';
    canvas.style.cssText = `display: block; width: 300px; height: 200px; ${canvasStyle}`;
    scroller.append(above, canvas);
    document.body.append(scroller);
}
export function scrollContainer(top) {
    const scroller = document.getElementById('scroller');
    scroller.scrollTop = top;
    scroller.dispatchEvent(new Event('scroll'));
}
export function removeScroller() { document.getElementById('scroller')?.remove(); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = placeholderLabel)]
    fn placeholder_label() -> Option<String>;
    #[wasm_bindgen(js_name = activatePlaceholder)]
    fn activate_placeholder();
    #[wasm_bindgen(js_name = focusPlaceholder)]
    fn focus_placeholder();
    fn focused() -> String;
    #[wasm_bindgen(js_name = semanticsTree)]
    fn semantics_tree() -> String;
    #[wasm_bindgen(js_name = hostOpacity)]
    fn host_opacity() -> String;
    #[wasm_bindgen(js_name = clickNode)]
    fn click_node(id: &str);
    #[wasm_bindgen(js_name = clickText)]
    fn click_text(text: &str);
    #[wasm_bindgen(js_name = hostAndCanvas)]
    fn host_and_canvas() -> Vec<f64>;
    #[wasm_bindgen(js_name = scrolledCanvas)]
    fn scrolled_canvas(style: &str, canvas_style: &str);
    #[wasm_bindgen(js_name = scrollContainer)]
    fn scroll_container(top: f64);
    #[wasm_bindgen(js_name = removeScroller)]
    fn remove_scroller();
}

const ROOT: NodeId = NodeId(1);
const BUTTON: NodeId = NodeId(2);
const COUNT: NodeId = NodeId(3);

/// The counter's tree: an "Increment" button and the count beside it.
fn counter(count: u32) -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_children(vec![BUTTON, COUNT]);
    root.set_bounds(Rect::new(0.0, 0.0, 300.0, 200.0));
    let mut button = Node::new(Role::Button);
    button.set_label("Increment");
    button.set_author_id("increment");
    button.add_action(Action::Click);
    button.add_action(Action::Focus);
    button.set_bounds(Rect::new(20.0, 20.0, 120.0, 60.0));
    let mut label = Node::new(Role::Label);
    label.set_value(format!("Count {count}"));
    label.set_bounds(Rect::new(20.0, 80.0, 220.0, 100.0));
    TreeUpdate {
        nodes: vec![(ROOT, root), (BUTTON, button), (COUNT, label)],
        tree: Some(TreeInfo::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: ROOT,
    }
}

fn tree() -> serde_json::Value {
    serde_json::from_str(&semantics_tree()).expect("the page's answer")
}

fn counter_in_dom(count: u32) -> serde_json::Value {
    serde_json::json!({
        "role": null,
        "children": [
            { "role": "button", "id": "increment", "text": "Increment" },
            { "role": null, "text": format!("Count {count}") },
        ],
    })
}

/// A window on a canvas the backend creates, with its accessibility.
fn open() -> (
    WebPlatform,
    Rc<dyn HostWindow>,
    Arc<dyn PlatformAccessibility>,
) {
    super::fresh_canvas();
    open_on_page(RecordingWindowHandler::new())
}

fn open_on_page(
    handler: Rc<RecordingWindowHandler>,
) -> (
    WebPlatform,
    Rc<dyn HostWindow>,
    Arc<dyn PlatformAccessibility>,
) {
    let platform = WebPlatform::new().expect("a browser platform");
    let window = platform
        .open_window(WindowOptions::default(), handler)
        .expect("the page's canvas opens");
    let accessibility = window
        .accessibility()
        .expect("a canvas window has accessibility");
    (platform, window, accessibility)
}

/// Before a screen reader asks, the page holds only the placeholder; its
/// activation tells the composition root and mirrors the tree published
/// before it, and every later publish changes the DOM.
#[wasm_bindgen_test]
fn the_tree_reaches_the_dom_once_the_placeholder_is_activated() {
    let (_platform, window, accessibility) = open();
    let activations = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&activations);
    accessibility.set_activation_listener(Arc::new(move |active| {
        heard.lock().expect("the record").push(active);
    }));
    accessibility.publish(counter(0));
    assert!(!accessibility.is_active());
    assert_eq!(placeholder_label().as_deref(), Some("Enable accessibility"));
    assert_eq!(tree(), serde_json::Value::Null, "no tree before activation");

    activate_placeholder();
    assert!(accessibility.is_active());
    assert_eq!(*activations.lock().expect("the record"), [true]);
    assert_eq!(placeholder_label(), None, "the placeholder gave way");
    assert_eq!(
        tree(),
        counter_in_dom(0),
        "the tree published before activation"
    );
    assert_eq!(host_opacity(), "0", "the canvas alone paints");

    accessibility.publish(counter(1));
    assert_eq!(tree(), counter_in_dom(1));

    // Static text that gains a child keeps its own text before it.
    let mut count = Node::new(Role::Label);
    count.set_value("Count 1");
    count.set_children(vec![NodeId(4)]);
    count.set_bounds(Rect::new(20.0, 80.0, 220.0, 100.0));
    let mut unit = Node::new(Role::Label);
    unit.set_value("taps");
    accessibility.publish(TreeUpdate {
        nodes: vec![(COUNT, count), (NodeId(4), unit)],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    assert_eq!(
        tree()["children"][1],
        serde_json::json!({
            "role": null,
            "text": "Count 1",
            "children": [{ "role": null, "text": "taps" }],
        })
    );
    window.close_now();
}

/// A click on an element is the action its node supports, and only that.
#[wasm_bindgen_test]
fn a_click_on_an_element_reaches_the_action_listener() {
    let (_platform, window, accessibility) = open();
    let actions: Arc<Mutex<Vec<ActionRequest>>> = Arc::default();
    let heard = Arc::clone(&actions);
    accessibility.set_action_listener(Arc::new(move |request| {
        heard.lock().expect("the record").push(request);
    }));
    accessibility.publish(counter(0));
    activate_placeholder();

    click_node("increment");
    click_text("Count 0");
    let actions = actions.lock().expect("the record");
    let asked: Vec<_> = actions
        .iter()
        .map(|request| (request.action, request.target_node))
        .collect();
    assert_eq!(asked, [(Action::Click, BUTTON)], "the label has no action");
    drop(actions);
    window.close_now();
}

/// A keyboard user who activates the placeholder keeps the page's focus in
/// the window: on the canvas while the published focus is a node that takes
/// no DOM focus, on that node's element once one does.
#[wasm_bindgen_test]
fn activating_the_focused_placeholder_keeps_focus_in_the_window() {
    let (_platform, window, accessibility) = open();
    accessibility.publish(counter(0));
    focus_placeholder();
    assert_eq!(focused(), "flui-a11y-placeholder");

    activate_placeholder();
    assert_eq!(focused(), "canvas", "the root takes no DOM focus");

    accessibility.publish(TreeUpdate {
        focus: BUTTON,
        ..counter(1)
    });
    assert_eq!(
        focused(),
        "increment",
        "the claim waited for a focusable node"
    );
    window.close_now();
}

/// A publish that removes the focused element keeps the page's focus in
/// the window, and the browser's `focusout` on the removed element is
/// handled once the tree is no longer being changed: a handler that closes
/// the window when told of it finds nothing borrowed.
#[wasm_bindgen_test]
fn removing_the_focused_element_keeps_focus_in_the_window() {
    super::fresh_canvas();
    let handler = RecordingWindowHandler::new();
    let (_platform, window, accessibility) = open_on_page(Rc::clone(&handler));
    accessibility.publish(counter(0));
    focus_placeholder();
    activate_placeholder();
    accessibility.publish(TreeUpdate {
        focus: BUTTON,
        ..counter(0)
    });
    assert_eq!(focused(), "increment");

    let closing = Rc::downgrade(&window);
    handler.react_to_state_change(move || {
        if let Some(window) = closing.upgrade() {
            window.close_now();
        }
    });
    let mut root = Node::new(Role::Window);
    root.set_children(vec![COUNT]);
    let mut count = Node::new(Role::Label);
    count.set_value("Count 0");
    accessibility.publish(TreeUpdate {
        nodes: vec![(ROOT, root), (COUNT, count)],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    assert_eq!(focused(), "canvas", "the root takes no DOM focus");
    assert_eq!(tree()["children"].as_array().map(Vec::len), Some(1));
    window.close_now();
}

/// The host covers the canvas in the viewport, and follows it when a
/// container the canvas is in scrolls, also when that container is
/// transformed, which makes it the host's containing block, and when the
/// canvas alone is scaled: a screen reader's highlight is drawn where the
/// element is.
#[wasm_bindgen_test]
fn the_semantics_host_stays_over_the_canvas_when_its_container_scrolls() {
    for (container, style, canvas_style) in [
        ("a scrolling container", "", ""),
        (
            "a transformed scrolling container",
            "transform: translate(15px, 25px) scale(1.5)",
            "",
        ),
        (
            "a scrolling container, the canvas scaled",
            "",
            "transform: scale(2)",
        ),
        (
            "a transformed scrolling container, the canvas scaled",
            "transform: scale(1.5)",
            "transform: scale(0.5, 2)",
        ),
    ] {
        scrolled_canvas(style, canvas_style);
        let (_platform, window, accessibility) = open_on_page(RecordingWindowHandler::new());
        accessibility.publish(counter(0));
        activate_placeholder();

        for scrolled in [0.0, 30.0] {
            scroll_container(scrolled);
            let boxes = host_and_canvas();
            let [host, canvas, button] = [&boxes[..4], &boxes[4..8], &boxes[8..]];
            // The button's bounds are (20, 20)-(120, 60) in the canvas's 300 by 200.
            let (x, y) = (canvas[2] / 300.0, canvas[3] / 200.0);
            let expected = [
                canvas[0] + 20.0 * x,
                canvas[1] + 20.0 * y,
                100.0 * x,
                40.0 * y,
            ];
            let apart = |placed: &[f64], expected: &[f64]| {
                placed
                    .iter()
                    .zip(expected)
                    .map(|(placed, expected)| (placed - expected).abs())
                    .fold(0.0, f64::max)
            };
            assert!(
                apart(host, canvas) < 0.5 && apart(button, &expected) < 0.5,
                "in {container} scrolled by {scrolled}: the host at {host:?}, the canvas at \
                 {canvas:?}, the button at {button:?} where {expected:?} is"
            );
        }
        window.close_now();
        remove_scroller();
    }
}
