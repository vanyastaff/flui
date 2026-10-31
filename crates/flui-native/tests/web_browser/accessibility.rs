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
export function focusOutside() {
    const outside = document.createElement('button');
    outside.id = 'outside';
    document.body.append(outside);
    outside.focus();
}
export function removeOutside() { document.getElementById('outside')?.remove(); }
export function focusCanvas() { document.getElementById('flui-canvas').focus(); }
// What holds the page's focus: a node's identifier, the canvas, or the tag.
export function focused() {
    const active = document.activeElement;
    if (!active) return 'nothing';
    if (active.id === 'flui-canvas') return 'canvas';
    if (active.id === 'outside') return 'outside';
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
// The host's box, the canvas's client box and the increment button's in the viewport, each [left, top,
// width, height].
export function hostAndCanvas() {
    const box = element => {
        const rect = element.getBoundingClientRect();
        return [rect.left, rect.top, rect.width, rect.height];
    };
    // The canvas's client box, inside its border, where it draws.
    const canvas = document.getElementById('flui-canvas');
    const rect = canvas.getBoundingClientRect();
    const [x, y] = [rect.width / canvas.offsetWidth, rect.height / canvas.offsetHeight];
    return [
        ...box(host()),
        rect.left + canvas.clientLeft * x,
        rect.top + canvas.clientTop * y,
        canvas.clientWidth * x,
        canvas.clientHeight * y,
        ...box(document.querySelector('[data-flui-id="increment"]')),
    ];
}
// Whether the host, or the placeholder before it, is shown.
export function overlayShown() {
    const overlay = host() ?? document.querySelector('flui-a11y-placeholder');
    return getComputedStyle(overlay).display !== 'none';
}
export function styleCanvas(style) { document.getElementById('flui-canvas').style.cssText += style; }
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
function byId(id) { return host().querySelector(`[data-flui-id="${id}"]`); }
// The element's tag and its `href`, or null.
export function elementOf(id) {
    const element = byId(id);
    return element ? [element.tagName.toLowerCase(), element.getAttribute('href') ?? ''] : null;
}
export function locationHash() { return location.hash; }
// Move a range input as the browser does for a screen reader: a new value, then `input`.
export function moveSlider(id, value) {
    const input = byId(id);
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
}
export function sliderValue(id) { return byId(id).value; }
// Scroll an element as the browser does: its `scroll` event follows at the next frame.
export function scrollNode(id, top) { byId(id).scrollTop = top; }
export function scrollOf(id) { return byId(id).scrollTop; }
export function topOf(id) { return byId(id).offsetTop; }
export function scrollLeftOf(id) { return byId(id).scrollLeft; }
export function styleScroller(style) { document.getElementById("scroller").style.cssText += style; }
// A field's tag, value, selection, focus and the hints it gives the browser.
export function fieldState(id) {
    const field = byId(id);
    return JSON.stringify(field ? {
        tag: field.tagName.toLowerCase(), value: field.value, start: field.selectionStart,
        end: field.selectionEnd, direction: field.selectionDirection,
        focused: document.activeElement === field,
        autocomplete: field.getAttribute("autocomplete"), inputmode: field.getAttribute("inputmode"),
    } : null);
}
let held;
// Keep a field's element, to type into it once the host has let it go.
export function holdField(id) { held = byId(id); }
// The input method types into a field (or the held one): its value becomes `value`.
export function typeIntoField(id, value, caret, composing) {
    const field = id == null ? held : byId(id);
    field.value = value;
    field.setSelectionRange(caret, caret);
    field.dispatchEvent(new InputEvent("input", { bubbles: true, isComposing: composing,
        inputType: composing ? "insertCompositionText" : "insertText" }));
}
// The user selects in a field, from `start` to `end` in `direction`.
export function selectField(id, start, end, direction) {
    const field = byId(id);
    field.setSelectionRange(start, end, direction);
    field.dispatchEvent(new Event("select", { bubbles: true }));
}
export function fieldComposition(id, type) { byId(id).dispatchEvent(new CompositionEvent(type, { bubbles: true })); }
// Select as a screen reader does: the page's selection between two offsets of the text leading two
// mirrored elements.
export function selectText(anchor, anchorOffset, focus, focusOffset) {
    getSelection().setBaseAndExtent(byId(anchor).firstChild, anchorOffset, byId(focus).firstChild,
        focusOffset);
}
// The page's selection as [element, offset] ends, an element named by its identifier or "page".
export function pageSelection() {
    const selection = getSelection();
    if (!selection.rangeCount) return JSON.stringify(null);
    const end = (node, offset) => [node.parentElement.getAttribute('data-flui-id') ?? 'page', offset];
    return JSON.stringify([end(selection.anchorNode, selection.anchorOffset),
        end(selection.focusNode, selection.focusOffset)]);
}
// Select some of the page's own text, outside the host.
export function selectPageText() {
    const text = document.createElement('p');
    text.id = 'page-text';
    text.textContent = 'Page text';
    document.body.append(text);
    getSelection().setBaseAndExtent(text.firstChild, 0, text.firstChild, 4);
}
export function removePageText() { document.getElementById('page-text')?.remove(); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = placeholderLabel)]
    fn placeholder_label() -> Option<String>;
    #[wasm_bindgen(js_name = activatePlaceholder)]
    fn activate_placeholder();
    #[wasm_bindgen(js_name = focusPlaceholder)]
    fn focus_placeholder();
    #[wasm_bindgen(js_name = focusOutside)]
    fn focus_outside();
    #[wasm_bindgen(js_name = removeOutside)]
    fn remove_outside();
    #[wasm_bindgen(js_name = focusCanvas)]
    fn focus_canvas();
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
    #[wasm_bindgen(js_name = overlayShown)]
    fn overlay_shown() -> bool;
    #[wasm_bindgen(js_name = styleCanvas)]
    fn style_canvas(style: &str);
    #[wasm_bindgen(js_name = scrolledCanvas)]
    fn scrolled_canvas(style: &str, canvas_style: &str);
    #[wasm_bindgen(js_name = scrollContainer)]
    fn scroll_container(top: f64);
    #[wasm_bindgen(js_name = removeScroller)]
    fn remove_scroller();
    #[wasm_bindgen(js_name = elementOf)]
    fn element_of(id: &str) -> Option<Vec<String>>;
    #[wasm_bindgen(js_name = locationHash)]
    fn location_hash() -> String;
    #[wasm_bindgen(js_name = moveSlider)]
    fn move_slider(id: &str, value: &str);
    #[wasm_bindgen(js_name = sliderValue)]
    fn slider_value(id: &str) -> String;
    #[wasm_bindgen(js_name = scrollNode)]
    fn scroll_node(id: &str, top: f64);
    #[wasm_bindgen(js_name = scrollOf)]
    fn scroll_of(id: &str) -> f64;
    #[wasm_bindgen(js_name = topOf)]
    fn top_of(id: &str) -> f64;
    #[wasm_bindgen(js_name = scrollLeftOf)]
    fn scroll_left_of(id: &str) -> f64;
    #[wasm_bindgen(js_name = styleScroller)]
    fn style_scroller(style: &str);
    #[wasm_bindgen(js_name = fieldState)]
    fn field_state(id: &str) -> String;
    #[wasm_bindgen(js_name = holdField)]
    fn hold_field(id: &str);
    #[wasm_bindgen(js_name = typeIntoField)]
    fn type_into_field(id: Option<&str>, value: &str, caret: u32, composing: bool);
    #[wasm_bindgen(js_name = fieldComposition)]
    fn field_composition(id: &str, kind: &str);
    #[wasm_bindgen(js_name = selectField)]
    fn select_field(id: &str, start: u32, end: u32, direction: &str);
    #[wasm_bindgen(js_name = selectText)]
    fn select_text(anchor: &str, anchor_offset: u32, focus: &str, focus_offset: u32);
    #[wasm_bindgen(js_name = pageSelection)]
    fn page_selection() -> String;
    #[wasm_bindgen(js_name = selectPageText)]
    fn select_page_text();
    #[wasm_bindgen(js_name = removePageText)]
    fn remove_page_text();
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
    let platform = WebPlatform::try_new().expect("a browser platform");
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
/// the window, and the window's focus follows the tree's while it is there:
/// on the canvas while the published focus is a node that takes no DOM
/// focus, on that node's element once one does, and on the canvas again,
/// off the element, when the focus returns to the root. Once the page's
/// focus is elsewhere, the tree's focus never takes it back.
#[wasm_bindgen_test]
fn the_page_s_focus_follows_the_tree_s_while_it_is_in_the_window() {
    let (_platform, window, accessibility) = open();
    accessibility.publish(counter(0));
    focus_placeholder();
    assert_eq!(focused(), "flui-a11y-placeholder");

    activate_placeholder();
    assert_eq!(focused(), "canvas", "the root takes no DOM focus");

    let focused_on = |focus, count| {
        accessibility.publish(TreeUpdate {
            focus,
            ..counter(count)
        });
        focused()
    };
    // Focus that leaves before any node can take it is not taken back.
    focus_outside();
    assert_eq!(
        focused_on(BUTTON, 1),
        "outside",
        "the page's focus is elsewhere"
    );
    remove_outside();
    focus_canvas();
    assert_eq!(
        focused_on(BUTTON, 2),
        "increment",
        "a focusable node takes it"
    );
    assert_eq!(focused_on(ROOT, 3), "canvas", "back on the root");
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

/// Two nodes that swap places, the outer one moving under its old child,
/// end up nested as published.
#[wasm_bindgen_test]
fn nodes_that_swap_places_are_nested_as_published() {
    let (_platform, window, accessibility) = open();
    let node = |label: &str, children: &[NodeId]| {
        let mut node = Node::new(Role::Group);
        node.set_label(label);
        node.set_children(children.to_vec());
        node
    };
    let (outer, inner) = (NodeId(3), NodeId(2));
    let mut root = Node::new(Role::Window);
    root.set_children(vec![NodeId(4)]);
    accessibility.publish(TreeUpdate {
        nodes: vec![
            (ROOT, root),
            (NodeId(4), node("wrapper", &[outer])),
            (outer, node("outer", &[inner])),
            (inner, node("inner", &[])),
        ],
        tree: Some(TreeInfo::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    activate_placeholder();
    accessibility.publish(TreeUpdate {
        nodes: vec![
            (NodeId(4), node("wrapper", &[inner])),
            (inner, node("inner", &[outer])),
            (outer, node("outer", &[])),
        ],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    let group = |label: &str, children: Vec<serde_json::Value>| {
        let mut group = serde_json::json!({ "role": "group", "label": label });
        if !children.is_empty() {
            group["children"] = serde_json::Value::from(children);
        }
        group
    };
    assert_eq!(
        tree()["children"][0],
        group(
            "wrapper",
            vec![group("inner", vec![group("outer", vec![])])]
        )
    );
    window.close_now();
}

/// What stands beside the canvas is hidden with it, before activation and
/// after, as soon as the canvas's own style changes: a screen reader never
/// reaches the controls of a UI the page hid.
#[wasm_bindgen_test]
async fn the_placeholder_and_the_host_are_hidden_with_the_canvas() {
    let (_platform, window, accessibility) = open();
    accessibility.publish(counter(0));
    for (style, shown) in [
        ("display: none", false),
        ("display: block", true),
        ("visibility: hidden", false),
        ("visibility: visible", true),
    ] {
        style_canvas(&format!("; {style}"));
        super::wait(0).await;
        assert_eq!(
            overlay_shown(),
            shown,
            "the placeholder, the canvas at {style}"
        );
    }
    activate_placeholder();
    for (style, shown) in [("display: none", false), ("display: block", true)] {
        style_canvas(&format!("; {style}"));
        super::wait(0).await;
        assert_eq!(overlay_shown(), shown, "the host, the canvas at {style}");
    }
    window.close_now();
}

/// A focused node that takes text with no field of its own (a date) leaves
/// the page's focus on the canvas, where the window's text input serves it.
#[wasm_bindgen_test]
fn a_focused_node_taking_text_without_a_field_keeps_focus_on_the_canvas() {
    const FIELD: NodeId = NodeId(4);
    let with_field = |focus| {
        let mut tree = counter(0);
        tree.nodes[0].1.push_child(FIELD);
        let mut field = Node::new(Role::DateInput);
        field.set_label("Date");
        field.add_action(Action::Focus);
        field.set_bounds(Rect::new(20.0, 120.0, 220.0, 150.0));
        tree.nodes.push((FIELD, field));
        TreeUpdate { focus, ..tree }
    };
    let (_platform, window, accessibility) = open();
    accessibility.publish(with_field(ROOT));
    focus_placeholder();
    activate_placeholder();
    accessibility.publish(with_field(BUTTON));
    assert_eq!(focused(), "increment");
    accessibility.publish(with_field(FIELD));
    assert_eq!(focused(), "canvas", "the field is served on the canvas");
    window.close_now();
}

/// The host covers the canvas's drawable box in the viewport, inside its
/// border, and follows it when a container the canvas is in scrolls, also
/// when that container is transformed, which makes it the host's containing
/// block, and when the canvas alone is scaled: a screen reader's highlight
/// is drawn where the element is.
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
        (
            "a transformed scrolling container, the canvas bordered and scaled",
            "transform: scale(1.5)",
            "border: 10px solid black; padding: 0; transform: scale(2)",
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

/// A window's tree of `nodes` under a root of the canvas's size, whose
/// one child is the first of them.
fn under_root(children: Vec<(NodeId, Node)>) -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_children(
        children
            .iter()
            .take(1)
            .map(|(id, _)| *id)
            .collect::<Vec<NodeId>>(),
    );
    root.set_bounds(Rect::new(0.0, 0.0, 300.0, 200.0));
    let mut nodes = vec![(ROOT, root)];
    nodes.extend(children);
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: ROOT,
    }
}

/// The action listener's record.
fn record_actions(
    accessibility: &Arc<dyn PlatformAccessibility>,
) -> Arc<Mutex<Vec<ActionRequest>>> {
    let actions: Arc<Mutex<Vec<ActionRequest>>> = Arc::default();
    let heard = Arc::clone(&actions);
    accessibility.set_action_listener(Arc::new(move |request| {
        heard.lock().expect("the record").push(request);
    }));
    actions
}

/// A link with a URL is an anchor a screen reader reads the address of,
/// and a click on it is the node's action, never a navigation. A link given
/// its address later gets an anchor in place of its element.
#[wasm_bindgen_test]
fn a_link_is_an_anchor_whose_click_is_the_node_s_action() {
    const LINK: NodeId = NodeId(2);
    let (_platform, window, accessibility) = open();
    let actions = record_actions(&accessibility);
    let link = |url: Option<&str>| {
        let mut node = Node::new(Role::Link);
        node.set_label("Docs");
        node.set_author_id("docs");
        if let Some(url) = url {
            node.set_url(url);
        }
        node.add_action(Action::Click);
        node.set_bounds(Rect::new(20.0, 20.0, 120.0, 40.0));
        node
    };
    accessibility.publish(under_root(vec![(LINK, link(None))]));
    activate_placeholder();
    assert_eq!(
        element_of("docs"),
        Some(vec!["flui-node".to_owned(), String::new()]),
        "a link with no address is an ARIA one"
    );

    // Given an address, its element is replaced by an anchor.
    accessibility.publish(TreeUpdate {
        nodes: vec![(LINK, link(Some("#flui-navigated")))],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });

    assert_eq!(
        element_of("docs"),
        Some(vec!["a".to_owned(), "#flui-navigated".to_owned()])
    );
    let before = location_hash();
    click_node("docs");
    assert_eq!(location_hash(), before, "the click followed no address");
    let asked: Vec<_> = actions
        .lock()
        .expect("the record")
        .iter()
        .map(|request| (request.action, request.target_node))
        .collect();
    assert_eq!(asked, [(Action::Click, LINK)]);
    window.close_now();
}

/// A slider a user can move is a range input; the browser moving it asks
/// the node for the value and shows the node's own until the tree
/// publishes the moved one.
#[wasm_bindgen_test]
fn moving_a_slider_s_input_asks_the_node_for_its_value() {
    const SLIDER: NodeId = NodeId(2);
    let slider = |value: f64| {
        let mut node = Node::new(Role::Slider);
        node.set_label("Volume");
        node.set_author_id("volume");
        node.set_numeric_value(value);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
        node.set_numeric_value_step(5.0);
        node.add_action(Action::SetValue);
        node.set_bounds(Rect::new(20.0, 20.0, 220.0, 40.0));
        node
    };
    let (_platform, window, accessibility) = open();
    let actions = record_actions(&accessibility);
    accessibility.publish(under_root(vec![(SLIDER, slider(30.0))]));
    activate_placeholder();
    assert_eq!(
        element_of("volume").map(|tag| tag[0].clone()).as_deref(),
        Some("input")
    );
    assert_eq!(slider_value("volume"), "30");

    move_slider("volume", "55");
    let asked: Vec<_> = actions
        .lock()
        .expect("the record")
        .iter()
        .map(|request| (request.action, request.target_node, request.data.clone()))
        .collect();
    assert_eq!(
        asked,
        [(
            Action::SetValue,
            SLIDER,
            Some(accesskit::ActionData::NumericValue(55.0))
        )]
    );
    assert_eq!(
        slider_value("volume"),
        "30",
        "the node's value until it moves"
    );
    accessibility.publish(TreeUpdate {
        nodes: vec![(SLIDER, slider(55.0))],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    assert_eq!(slider_value("volume"), "55");
    // A new minimum the old step would snap the value to.
    let mut moved_range = slider(55.0);
    moved_range.set_min_numeric_value(2.0);
    accessibility.publish(TreeUpdate {
        nodes: vec![(SLIDER, moved_range)],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    assert_eq!(slider_value("volume"), "55", "the published value stays");
    window.close_now();
}

/// A node a platform scrolls to an offset is a scroller over content as
/// long as its extent, scrolled as far as the node is, its children where
/// they are seen, its offsets from the left edge on a right-to-left page.
/// The browser scrolling it asks the node for the offset and goes back to
/// where the node is until the node publishes the new one; the host's own
/// scroll after a publish asks for nothing, and a scroller published while
/// the canvas, or an ancestor of it, was hidden is scrolled once it is
/// shown.
#[wasm_bindgen_test]
async fn scrolling_a_scroller_asks_the_node_for_its_offset() {
    const LIST: NodeId = NodeId(2);
    const ITEM: NodeId = NodeId(3);
    let list = |position: f64| {
        let mut node = Node::new(Role::ScrollView);
        node.set_author_id("list");
        node.add_action(Action::SetScrollOffset);
        node.set_scroll_x(100.0);
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(200.0);
        node.set_scroll_y(position);
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(400.0);
        node.set_bounds(Rect::new(0.0, 0.0, 300.0, 200.0));
        node.set_children(vec![ITEM]);
        node
    };
    let item = |seen_at: f64| {
        let mut node = Node::new(Role::Button);
        node.set_label("Item");
        node.set_author_id("item");
        node.set_bounds(Rect::new(20.0, seen_at, 120.0, seen_at + 40.0));
        node
    };
    let partial = |position: f64| TreeUpdate {
        nodes: vec![(LIST, list(position)), (ITEM, item(220.0 - position))],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    };
    // A page written right to left, which the host inherits.
    scrolled_canvas("direction: rtl; height: auto; overflow: visible", "");
    let (_platform, window, accessibility) = open_on_page(RecordingWindowHandler::new());
    let actions = record_actions(&accessibility);
    accessibility.publish(under_root(vec![(LIST, list(100.0)), (ITEM, item(120.0))]));
    activate_placeholder();
    assert_eq!(scroll_of("list"), 100.0);
    assert_eq!(scroll_left_of("list"), 100.0, "from the left edge");
    assert_eq!(top_of("item"), 220.0, "the item is where it is seen");

    scroll_node("list", 250.0);
    super::wait(50).await;
    let asked: Vec<_> = actions
        .lock()
        .expect("the record")
        .drain(..)
        .map(|request| (request.action, request.target_node, request.data))
        .collect();
    assert_eq!(
        asked,
        [(
            Action::SetScrollOffset,
            LIST,
            Some(accesskit::ActionData::SetScrollOffset(
                accesskit::Point::new(100.0, 250.0)
            ))
        )]
    );
    assert_eq!(
        scroll_of("list"),
        100.0,
        "where the node is, until it moves"
    );

    accessibility.publish(partial(300.0));
    super::wait(50).await;
    assert_eq!(
        scroll_of("list"),
        300.0,
        "scrolled to the published position"
    );
    assert_eq!(top_of("item"), 220.0);
    assert!(
        actions.lock().expect("the record").is_empty(),
        "the host's own scroll asks for nothing"
    );

    style_canvas("; display: none");
    super::wait(0).await;
    // A hidden scroller has no box to scroll to what is published.
    accessibility.publish(partial(350.0));
    style_canvas("; display: block");
    super::wait(50).await;
    assert_eq!(scroll_of("list"), 350.0, "scrolled once shown again");

    style_scroller("; display: none");
    super::wait(50).await;
    accessibility.publish(partial(380.0));
    style_scroller("; display: block");
    super::wait(50).await;
    assert_eq!(scroll_of("list"), 380.0, "scrolled once its ancestor shows");
    assert!(actions.lock().expect("the record").is_empty());
    window.close_now();
    remove_scroller();
}

/// What a mirrored text field's element holds.
#[derive(Debug, serde::Deserialize)]
struct FieldState {
    tag: String,
    value: String,
    start: u32,
    end: u32,
    direction: String,
    focused: bool,
    autocomplete: Option<String>,
    inputmode: Option<String>,
}

fn field(id: &str) -> Option<FieldState> {
    serde_json::from_str(&field_state(id)).expect("the page's answer")
}

const FIELD: NodeId = NodeId(2);
const OTHER: NodeId = NodeId(3);

/// A text field named "name" holding `value`, beside a button.
fn form(value: &str, focus: NodeId) -> TreeUpdate {
    let mut name = Node::new(Role::EmailInput);
    name.set_label("Email");
    name.set_author_id("name");
    name.set_value(value);
    name.add_action(Action::Focus);
    name.set_bounds(Rect::new(20.0, 20.0, 220.0, 50.0));
    let mut button = Node::new(Role::Button);
    button.set_label("Send");
    button.set_author_id("send");
    button.add_action(Action::Focus);
    button.set_bounds(Rect::new(20.0, 60.0, 120.0, 90.0));
    let mut update = under_root(vec![(FIELD, name), (OTHER, button)]);
    update.nodes[0].1.set_children(vec![FIELD, OTHER]);
    update.focus = focus;
    update
}

/// A window whose tree, published with the field focused, is mirrored, and
/// the field's store, served.
fn served_form(
    store: &Rc<flui_testing::text_store_kit::InMemoryTextStore>,
) -> (super::HostedCanvas, Arc<dyn PlatformAccessibility>) {
    let canvas = super::HostedCanvas::open();
    let accessibility = canvas
        .window
        .accessibility()
        .expect("a canvas window has accessibility");
    focus_canvas();
    activate_placeholder();
    accessibility.publish(form(&store.text(), FIELD));
    canvas.host.focus_store(Some(super::as_store(store)));
    (canvas, accessibility)
}

/// While the tree's focus is a text field, its store is served through the
/// field's own element: the page's focus is on it, the canvas has no
/// editor, the store's text, selection and hints are the element's, the
/// input method's edits there reach the store first (composing until
/// `compositionend`), the application's edit made while it composes reaches
/// the element, and a published value that lags the store is not written
/// over it. Its keys reach the window once.
#[wasm_bindgen_test]
fn a_focused_text_field_is_edited_through_its_element() {
    let store = flui_testing::text_store_kit::InMemoryTextStore::new("ab");
    store.set_purpose(flui_platform::text_store::TextInputPurpose::Email);
    let (canvas, accessibility) = served_form(&store);
    let state = field("name").expect("the field's element");
    assert_eq!(state.tag, "input");
    assert_eq!((state.value.as_str(), state.focused), ("ab", true));
    assert_eq!(
        (state.autocomplete.as_deref(), state.inputmode.as_deref()),
        (Some("email"), Some("email")),
        "filled and typed as an address"
    );
    assert_eq!(super::edit_context_text(), None, "no editor on the canvas");
    assert!(super::editor().is_none(), "and none beside it");

    field_composition("name", "compositionstart");
    type_into_field(Some("name"), "abに", 3, true);
    assert_eq!(store.text(), "abに");
    assert!(store.composition().is_some(), "the inserted text composes");
    // The tree publishes a value that lags the store.
    accessibility.publish(form("a", FIELD));
    assert_eq!(
        field("name").expect("the field").value,
        "abに",
        "the store's"
    );
    store.app_replace(
        flui_platform::text_store::Utf16Range::try_new(
            flui_platform::text_store::Utf16Offset::ZERO,
            flui_platform::text_store::Utf16Offset::new(2),
        )
        .expect("a range"),
        "AB",
    );
    let state = field("name").expect("the field");
    let caret = u32::try_from(store.selection().range().start().get()).expect("a short text");
    assert_eq!(
        (state.value.as_str(), state.start),
        ("ABに", caret),
        "the store's"
    );
    field_composition("name", "compositionend");
    assert_eq!(store.composition(), None);
    type_into_field(Some("name"), "ABにc", 4, false);
    assert_eq!(store.text(), "ABにc");
    let state = field("name").expect("the field");
    assert_eq!((state.start, state.end), (4, 4));

    let keys = canvas.keys.borrow().len();
    super::key_at_focus("Enter", None);
    assert_eq!(canvas.keys.borrow().len(), keys + 1, "delivered once");
    assert_eq!(
        canvas.last_key().key,
        flui_platform::keyboard::Key::Named(flui_platform::keyboard::NamedKey::Enter)
    );
}

/// Where the browser has no `EditContext`, the store the field served moves
/// to a hidden editor when the tree's focus moves to another control, and
/// that control keeps the page's focus: the editor takes the canvas's focus
/// only.
#[wasm_bindgen_test]
async fn a_control_the_focus_moved_to_keeps_it_from_the_fallback_editor() {
    let _hidden = super::EditContextHidden::hide();
    let store = flui_testing::text_store_kit::InMemoryTextStore::new("ab");
    let (canvas, accessibility) = served_form(&store);
    assert!(field("name").expect("the field").focused);
    accessibility.publish(form("ab", OTHER));
    super::wait(0).await;
    assert!(
        super::editor().is_some(),
        "the store is served by the hidden editor"
    );
    assert_eq!(focused(), "send", "the button keeps the page's focus");
    canvas.window.close_now();
}

/// The tree's focus leaving a served field for a node served on the canvas
/// puts the page's focus on the canvas, and the field the host no longer
/// names does not take it back: the store is served on the canvas, by its
/// `EditContext` or, without one, by the hidden editor, which has the focus.
#[wasm_bindgen_test]
async fn the_canvas_keeps_the_focus_a_field_left() {
    for edit_context in [true, false] {
        let hidden = (!edit_context).then(super::EditContextHidden::hide);
        let store = flui_testing::text_store_kit::InMemoryTextStore::new("ab");
        let (canvas, accessibility) = served_form(&store);
        assert!(field("name").expect("the field").focused);
        accessibility.publish(form("ab", ROOT));
        super::wait(0).await;
        assert!(
            !field("name").expect("the field stays").focused,
            "the field left, with EditContext {edit_context}"
        );
        if edit_context {
            assert_eq!(focused(), "canvas");
            assert_eq!(super::edit_context_text().as_deref(), Some("ab"));
        } else {
            assert!(
                super::editor().is_some_and(|editor| editor.focused),
                "the hidden editor has the focus"
            );
        }
        canvas.window.close_now();
        drop(hidden);
    }
}

/// Where the browser has no `EditContext`, the tree's focus moving from a
/// node served on the canvas to a text field moves the page's focus from the
/// hidden editor to the field, and what is typed there edits the store.
#[wasm_bindgen_test]
async fn a_field_the_focus_moved_to_takes_the_hidden_editor_s_focus() {
    let _hidden = super::EditContextHidden::hide();
    let store = flui_testing::text_store_kit::InMemoryTextStore::new("ab");
    let canvas = super::HostedCanvas::open();
    let accessibility = canvas
        .window
        .accessibility()
        .expect("a canvas window has accessibility");
    focus_canvas();
    activate_placeholder();
    accessibility.publish(form("ab", ROOT));
    canvas.host.focus_store(Some(super::as_store(&store)));
    assert!(
        super::editor().is_some_and(|editor| editor.focused),
        "served by the focused hidden editor"
    );
    accessibility.publish(form("ab", FIELD));
    super::wait(0).await;
    assert!(super::editor().is_none(), "the hidden editor is gone");
    assert!(
        field("name").expect("the field").focused,
        "the field has the focus"
    );
    type_into_field(Some("name"), "abc", 3, false);
    assert_eq!(store.text(), "abc");
    canvas.window.close_now();
}

/// A single-line field shows the store's text without the line breaks the
/// browser drops from its value, and still serves the store: a selection or
/// an edit there keeps them, whether the store held them from the start or
/// the application put one in while the field served it.
#[wasm_bindgen_test]
async fn a_field_keeps_the_line_breaks_it_cannot_show() {
    for start in ["a\nb", "ab"] {
        let store = flui_testing::text_store_kit::InMemoryTextStore::new(start);
        let (canvas, _accessibility) = served_form(&store);
        if start == "ab" {
            store.app_replace(
                flui_platform::text_store::Utf16Range::try_new(
                    flui_platform::text_store::Utf16Offset::new(1),
                    flui_platform::text_store::Utf16Offset::new(1),
                )
                .expect("a range"),
                "\n",
            );
        }
        let state = field("name").expect("the field");
        assert_eq!(
            (state.value.as_str(), state.focused),
            ("ab", true),
            "{start:?}"
        );
        assert_eq!(super::edit_context_text(), None, "served by the field");
        select_field("name", 1, 1, "none");
        assert_eq!(store.text(), "a\nb", "a selection keeps it, from {start:?}");
        assert_eq!(store.selection().range().start().get(), 2, "past it");
        type_into_field(Some("name"), "abc", 3, false);
        assert_eq!(store.text(), "a\nbc", "an edit keeps it, from {start:?}");
        canvas.window.close_now();
        super::wait(0).await;
    }
}

/// A textarea shows a CRLF of the store as one line feed, and deleting it
/// deletes both units from the store.
#[wasm_bindgen_test]
async fn a_textarea_s_line_feed_is_the_store_s_crlf() {
    let store = flui_testing::text_store_kit::InMemoryTextStore::new("a\r\nb");
    let canvas = super::HostedCanvas::open();
    let accessibility = canvas
        .window
        .accessibility()
        .expect("a canvas window has accessibility");
    focus_canvas();
    activate_placeholder();
    let mut update = form("", FIELD);
    let mut notes = Node::new(Role::MultilineTextInput);
    notes.set_label("Notes");
    notes.set_author_id("name");
    notes.set_value(store.text());
    notes.add_action(Action::Focus);
    notes.set_bounds(Rect::new(20.0, 20.0, 220.0, 50.0));
    update.nodes[1].1 = notes;
    accessibility.publish(update);
    canvas.host.focus_store(Some(super::as_store(&store)));
    let state = field("name").expect("the field");
    assert_eq!(
        (state.tag.as_str(), state.value.as_str()),
        ("textarea", "a\nb")
    );
    type_into_field(Some("name"), "ab", 1, false);
    assert_eq!(store.text(), "ab", "the CRLF is gone");
    canvas.window.close_now();
    super::wait(0).await;
}

/// A backward selection in a field is the store's backward selection, its
/// active end at the start, and the field shows the store's selection with
/// its direction.
#[wasm_bindgen_test]
async fn a_field_s_selection_keeps_its_direction() {
    let store = flui_testing::text_store_kit::InMemoryTextStore::new("abcd");
    let (canvas, _accessibility) = served_form(&store);
    select_field("name", 1, 3, "backward");
    let selection = store.selection();
    assert_eq!(
        (selection.anchor.get(), selection.active.get()),
        (3, 1),
        "the active end is the start"
    );
    store.app_select(flui_platform::text_store::Selection {
        anchor: flui_platform::text_store::Utf16Offset::new(4),
        active: flui_platform::text_store::Utf16Offset::new(2),
    });
    let state = field("name").expect("the field");
    assert_eq!(
        (state.start, state.end, state.direction.as_str()),
        (2, 4, "backward"),
        "the application's selection"
    );
    // The browser fires the field's `select` event later.
    super::wait(0).await;
    let selection = store.selection();
    assert_eq!(
        (selection.anchor.get(), selection.active.get()),
        (4, 2),
        "the field's select event reads it back as it is"
    );
    canvas.window.close_now();
}

/// The store leaves the field's element when the tree's focus leaves the
/// field, the field is removed or the window closes: it is served on the
/// canvas again, the element loses its hints, and what is typed into the
/// element after that never reaches the store; the host shows a field no
/// store is served through as published, the value published while it was
/// served included.
#[wasm_bindgen_test]
async fn the_store_goes_back_to_the_canvas_when_the_field_goes() {
    let store = flui_testing::text_store_kit::InMemoryTextStore::new("ab");
    let (canvas, accessibility) = served_form(&store);
    assert!(field("name").expect("the field").focused);

    // A value published while the store is served is not written over it.
    accessibility.publish(form("a", FIELD));
    assert_eq!(field("name").expect("the field").value, "ab", "the store's");
    // The tree's focus moves on while the store is still the focused one.
    accessibility.publish(form("a", OTHER));
    super::wait(0).await;
    assert_eq!(
        super::edit_context_text().as_deref(),
        Some("ab"),
        "served on the canvas"
    );
    let state = field("name").expect("the field stays");
    assert_eq!(state.value, "a", "the value published while it was served");
    assert_eq!(state.autocomplete, None, "its hints are gone");
    type_into_field(Some("name"), "abz", 3, false);
    assert_eq!(store.text(), "ab", "the element no longer edits the store");
    assert_eq!(field("name").expect("the field").value, "a", "as published");

    accessibility.publish(form("ab", FIELD));
    super::wait(0).await;
    assert_eq!(super::edit_context_text(), None, "back on the field");
    hold_field("name");
    let mut root = Node::new(Role::Window);
    root.set_bounds(Rect::new(0.0, 0.0, 300.0, 200.0));
    accessibility.publish(TreeUpdate {
        nodes: vec![(ROOT, root)],
        tree: None,
        tree_id: TreeId::ROOT,
        focus: ROOT,
    });
    super::wait(0).await;
    assert!(field("name").is_none(), "the field is removed");
    assert_eq!(
        super::edit_context_text().as_deref(),
        Some("ab"),
        "served on the canvas again"
    );
    type_into_field(None, "abz", 3, false);
    assert_eq!(store.text(), "ab", "the removed element keeps no listener");

    accessibility.publish(form("ab", FIELD));
    super::wait(0).await;
    hold_field("name");
    canvas.window.close_now();
    super::wait(0).await;
    type_into_field(None, "abz", 3, false);
    assert_eq!(store.text(), "ab", "nothing listens once the window closed");
}

const AREA: NodeId = NodeId(20);
const FIRST: NodeId = NodeId(21);
const SECOND: NodeId = NodeId(22);
const FIRST_RUN: NodeId = NodeId(31);
const SECOND_RUN: NodeId = NodeId(32);
const THIRD_RUN: NodeId = NodeId(33);

fn text_run(text: &str) -> Node {
    let mut node = Node::new(Role::TextRun);
    node.set_value(text);
    node.set_character_lengths(
        text.chars()
            .map(|character| u8::try_from(character.len_utf8()).expect("one character"))
            .collect::<Vec<u8>>(),
    );
    node
}

fn at(node: NodeId, character_index: usize) -> accesskit::TextPosition {
    accesskit::TextPosition {
        node,
        character_index,
    }
}

fn between(
    anchor: accesskit::TextPosition,
    focus: accesskit::TextPosition,
) -> accesskit::TextSelection {
    accesskit::TextSelection { anchor, focus }
}

/// A selection area of two paragraphs publishing runs, "Hi 😀" and
/// " there", then "Bye", holding `selection`.
fn selectable(selection: Option<accesskit::TextSelection>) -> TreeUpdate {
    let mut area = Node::new(Role::Document);
    area.set_children(vec![FIRST, SECOND]);
    area.add_action(Action::SetTextSelection);
    area.set_bounds(Rect::new(0.0, 0.0, 300.0, 200.0));
    if let Some(selection) = selection {
        area.set_text_selection(selection);
    }
    let paragraph = |author: &str, runs: Vec<NodeId>, top: f64| {
        let mut node = Node::new(Role::Label);
        node.set_author_id(author);
        node.set_children(runs);
        node.add_action(Action::SetTextSelection);
        node.set_bounds(Rect::new(20.0, top, 220.0, top + 20.0));
        node
    };
    under_root(vec![
        (AREA, area),
        (FIRST, paragraph("first", vec![FIRST_RUN, SECOND_RUN], 20.0)),
        (SECOND, paragraph("second", vec![THIRD_RUN], 60.0)),
        (FIRST_RUN, text_run("Hi 😀")),
        (SECOND_RUN, text_run(" there")),
        (THIRD_RUN, text_run("Bye")),
    ])
}

fn page_selection_ends() -> serde_json::Value {
    serde_json::from_str(&page_selection()).expect("the page's answer")
}

/// Static text that publishes runs is selected as the page's selection
/// both ways. The tree's selection is shown in the page. A screen reader's
/// selection is asked, in run positions, of the paragraph or of the area
/// over both ends, an end inside an emoji moving back to its start, and the
/// page shows the published selection until the node publishes the new
/// one. The host's own selection asks for nothing, and the page's own
/// selection outside the host is left alone.
#[wasm_bindgen_test]
async fn the_page_s_selection_is_the_selection_of_static_text() {
    let (_platform, window, accessibility) = open();
    let actions = record_actions(&accessibility);
    accessibility.publish(selectable(Some(between(
        at(FIRST_RUN, 1),
        at(THIRD_RUN, 2),
    ))));
    activate_placeholder();
    super::wait(50).await;
    assert_eq!(
        page_selection_ends(),
        serde_json::json!([["first", 1], ["second", 2]]),
        "the tree's selection, across both paragraphs"
    );
    assert!(
        actions.lock().expect("the record").is_empty(),
        "shown, not asked"
    );
    let asked = || -> Vec<_> {
        actions
            .lock()
            .expect("the record")
            .drain(..)
            .map(|request| (request.action, request.target_node, request.data))
            .collect()
    };

    // From "Hi " to the end of the first run, which is where the second
    // starts.
    select_text("first", 3, "first", 5);
    super::wait(50).await;
    assert_eq!(
        asked(),
        [(
            Action::SetTextSelection,
            FIRST,
            Some(accesskit::ActionData::SetTextSelection(between(
                at(FIRST_RUN, 3),
                at(SECOND_RUN, 0)
            )))
        )],
        "asked of the paragraph"
    );
    assert_eq!(
        page_selection_ends(),
        serde_json::json!([["first", 1], ["second", 2]]),
        "the published selection, until the node publishes another"
    );
    super::wait(50).await;
    assert_eq!(asked(), [], "showing it again asks for nothing");

    // Between the emoji's halves, then into the second paragraph.
    select_text("first", 4, "second", 1);
    super::wait(50).await;
    assert_eq!(
        asked(),
        [(
            Action::SetTextSelection,
            AREA,
            Some(accesskit::ActionData::SetTextSelection(between(
                at(FIRST_RUN, 3),
                at(THIRD_RUN, 1)
            )))
        )],
        "asked of the area, from the emoji's start"
    );

    accessibility.publish(selectable(Some(between(
        at(FIRST_RUN, 4),
        at(SECOND_RUN, 3),
    ))));
    super::wait(50).await;
    assert_eq!(
        page_selection_ends(),
        serde_json::json!([["first", 5], ["first", 8]]),
        "the emoji is two offsets"
    );
    assert_eq!(asked(), [], "the host's own selection asks for nothing");

    select_page_text();
    super::wait(50).await;
    accessibility.publish(selectable(Some(between(
        at(THIRD_RUN, 0),
        at(THIRD_RUN, 3),
    ))));
    super::wait(50).await;
    assert_eq!(
        page_selection_ends(),
        serde_json::json!([["page", 0], ["page", 4]]),
        "the page's own selection stays"
    );
    assert_eq!(asked(), []);
    remove_page_text();
    window.close_now();
}
