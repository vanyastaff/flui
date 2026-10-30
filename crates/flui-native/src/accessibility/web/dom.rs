//! The semantics host in the page: the adapter a canvas window hands out.
//!
//! Before assistive technology asks, the page holds one element beside the
//! canvas, a placeholder button only a screen reader finds; the browser
//! tells a page nothing of a screen reader otherwise. Activating it puts the
//! semantics host in its place, `position: fixed` over the canvas with
//! `pointer-events: none`, so the pointer still reaches the canvas and FLUI
//! keeps hit-testing it, and tells the composition root, which publishes the
//! whole tree. Nothing deactivates it: the browser does not say when a screen
//! reader leaves.
//!
//! Every update runs through [`Mirror::apply`] and then on the DOM; the work
//! is queued, so a DOM event that a change fires synchronously (a removed
//! element's `focusout`), a listener that publishes again from inside its
//! call, or an action asked for while an update runs waits for the work
//! before it, and no listener runs while the tree is borrowed. The host
//! listens on itself for the events of every element in it, so no element
//! keeps a listener of its own.
//!
//! A panic in a DOM listener traps the instance (wasm32 aborts on panic), so
//! nothing here needs an OS-entry boundary.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use accesskit::{ActionRequest, NodeId, TreeId, TreeUpdate};
use flui_platform::keyboard::KeyState;
use flui_semantics::AssistiveFocusSupport;
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlCanvasElement, HtmlElement};

use super::aria::{self, ElementSpec, Placement};
use super::mirror::{Mirror, Op, Refused};
use crate::accessibility::bridge::BridgeShared;
use crate::owner::slot::OwnerSlot;
use crate::owner::web::listener::{CanvasListener, viewport_listeners, warn_failure};
use crate::owner::web::{Accepting, CanvasEditor, CanvasKeys, CanvasRef};

/// The attribute that names the node an element mirrors.
const NODE_ATTRIBUTE: &str = "data-flui-node";

/// The Web's accessibility for one canvas window.
pub struct WebAccessibility {
    shared: Arc<BridgeShared>,
    /// The page's side, on the page's thread; empty once the window closed.
    page: OwnerSlot<Rc<Page>>,
}

impl WebAccessibility {
    /// The browser tells a page nothing of where a screen reader's cursor is
    /// ([gap 36](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#36-accessibility-bridge-fidelity-properties-and-announcements)).
    pub(crate) const ASSISTIVE_FOCUS: AssistiveFocusSupport = AssistiveFocusSupport::Unreported;

    fn page(&self) -> Option<Rc<Page>> {
        self.page.cloned()
    }
}

/// Each canvas window gets its placeholder beside the canvas when it opens.
impl crate::host::Accessibility<CanvasRef> for WebAccessibility {
    fn for_window(window: &CanvasRef) -> Arc<dyn crate::host::WindowAccessibility> {
        let shared = Arc::new(BridgeShared::new());
        let page = OwnerSlot::new();
        match Page::try_new(window, &shared) {
            Ok(made) => {
                if page.replace(made).is_err() {
                    tracing::warn!("a canvas's accessibility was made off the page's thread");
                }
            }
            Err(error) => warn_failure("the accessibility placeholder could not be added", &error),
        }
        Arc::new(Self { shared, page })
    }
}

impl crate::host::WindowAccessibility for WebAccessibility {
    /// Remove the placeholder or the semantics host and their listeners.
    fn detach(&self) {
        if let Some(page) = self.page.take() {
            page.retire();
        }
    }

    /// The canvas's focus area includes the semantics host, whose focus the
    /// window reads itself.
    fn focus_changed(&self, _: bool) {}
}

impl PlatformAccessibility for WebAccessibility {
    fn publish(&self, update: TreeUpdate) {
        self.shared.retain_if_self_contained(&update);
        if !self.shared.is_active() {
            return;
        }
        if let Some(page) = self.page() {
            page.submit(Work::Publish(update));
        }
    }

    fn is_active(&self) -> bool {
        self.shared.is_active()
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        self.shared.set_activation_listener(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        self.shared.set_action_listener(listener);
    }

    fn assistive_focus_support(&self) -> AssistiveFocusSupport {
        Self::ASSISTIVE_FOCUS
    }
}

impl std::fmt::Debug for WebAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}

/// One piece of the page's work, run in the order it was asked for.
enum Work {
    /// The placeholder was activated.
    Activate,
    /// Mirror an update.
    Publish(TreeUpdate),
    /// Tell the composition root of an action.
    Action(ActionRequest),
    /// The canvas may have moved in the viewport.
    Place,
}

/// An element the page added, with the listeners on it.
struct Overlay {
    element: HtmlElement,
    _listeners: Vec<CanvasListener>,
}

/// What stands beside the canvas.
enum Stage {
    Placeholder(Overlay),
    Host(Overlay),
    Retired,
}

#[derive(Default)]
struct Tree {
    mirror: Mirror,
    elements: HashMap<NodeId, HtmlElement>,
}

/// The page's side of one canvas window's accessibility.
struct Page {
    this: Weak<Page>,
    shared: Arc<BridgeShared>,
    canvas: HtmlCanvasElement,
    document: Document,
    accepting: Accepting,
    keys: Rc<dyn CanvasKeys>,
    editor: Rc<dyn CanvasEditor>,
    stage: RefCell<Stage>,
    tree: RefCell<Tree>,
    work: RefCell<VecDeque<Work>>,
    running: Cell<bool>,
    /// The placeholder had the page's focus when it was activated: the
    /// tree's focus takes it once the tree is mirrored.
    claim_focus: Cell<bool>,
    /// The composition root was asked for the whole tree after a refused
    /// update and has not answered with one that stands yet.
    asked_again: Cell<bool>,
}

impl Page {
    fn try_new(window: &CanvasRef, shared: &Arc<BridgeShared>) -> Result<Rc<Self>, JsValue> {
        let document = window
            .canvas
            .owner_document()
            .ok_or_else(|| JsValue::from_str("the canvas has no document"))?;
        let page = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            shared: Arc::clone(shared),
            canvas: window.canvas.clone(),
            document,
            accepting: window.accepting.clone(),
            keys: Rc::clone(&window.keys),
            editor: Rc::clone(&window.editor),
            stage: RefCell::new(Stage::Retired),
            tree: RefCell::default(),
            work: RefCell::default(),
            running: Cell::new(false),
            claim_focus: Cell::new(false),
            asked_again: Cell::new(false),
        });
        let placeholder = page.placeholder()?;
        *page.stage.borrow_mut() = Stage::Placeholder(placeholder);
        Ok(page)
    }

    /// The button only a screen reader finds: one pixel, clipped away.
    fn placeholder(&self) -> Result<Overlay, JsValue> {
        let element = self.element("flui-a11y-placeholder")?;
        for (name, value) in [
            ("role", "button"),
            ("tabindex", "0"),
            ("aria-label", "Enable accessibility"),
        ] {
            element.set_attribute(name, value)?;
        }
        let style = element.style();
        for (property, value) in [
            ("position", "fixed"),
            ("left", "0"),
            ("top", "0"),
            ("width", "1px"),
            ("height", "1px"),
            ("overflow", "hidden"),
            ("clip-path", "inset(50%)"),
            ("white-space", "nowrap"),
        ] {
            style.set_property(property, value)?;
        }
        let target: &web_sys::EventTarget = element.as_ref();
        let this = self.this.clone();
        let click = self.listen(target, "click", move |_| {
            if let Some(this) = this.upgrade() {
                this.submit(Work::Activate);
            }
        })?;
        // A focused `role="button"` element that is not a `<button>` gets
        // no `click` from the keyboard.
        let this = self.this.clone();
        let key = self.listen(target, "keydown", move |event| {
            let key = event.unchecked_ref::<web_sys::KeyboardEvent>().key();
            if (key == "Enter" || key == " ")
                && let Some(this) = this.upgrade()
            {
                event.prevent_default();
                this.submit(Work::Activate);
            }
        })?;
        self.place_beside_canvas(&element)?;
        Ok(Overlay {
            element,
            _listeners: vec![click, key],
        })
    }

    /// The semantics host, with the listeners for every element it will
    /// hold.
    fn host(&self) -> Result<Overlay, JsValue> {
        let element = self.element("flui-semantics-host")?;
        let style = element.style();
        for (property, value) in [
            ("position", "fixed"),
            ("display", "block"),
            ("margin", "0"),
            ("padding", "0"),
            ("border", "0"),
            ("pointer-events", "none"),
        ] {
            style.set_property(property, value)?;
        }
        let target: &web_sys::EventTarget = element.as_ref();
        let mut listeners = Vec::new();
        for kind in ["click", "focusin", "contextmenu"] {
            let this = self.this.clone();
            listeners.push(self.listen(target, kind, move |event| {
                if let Some(this) = this.upgrade() {
                    this.node_event(kind, &event);
                }
            })?);
        }
        // The window reads its focus again when it leaves or enters the host
        // from outside the canvas's focus area.
        for kind in ["focusin", "focusout"] {
            let editor = Rc::clone(&self.editor);
            listeners.push(self.listen(target, kind, move |event| {
                if !editor.is_handoff(&event) {
                    editor.sync();
                }
            })?);
        }
        // A key pressed on an element of the host is the window's, as on the
        // canvas.
        for (kind, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
            let keys = Rc::clone(&self.keys);
            listeners.push(self.listen(target, kind, move |event| {
                keys.deliver(&event, state);
            })?);
        }
        let this = self.this.clone();
        listeners.extend(
            viewport_listeners(&self.accepting, move || {
                if let Some(this) = this.upgrade() {
                    this.submit(Work::Place);
                }
            })
            .map_err(|error| JsValue::from_str(&error.to_string()))?,
        );
        self.place_beside_canvas(&element)?;
        Ok(Overlay {
            element,
            _listeners: listeners,
        })
    }

    fn element(&self, tag: &str) -> Result<HtmlElement, JsValue> {
        Ok(self.document.create_element(tag)?.unchecked_into())
    }

    fn listen(
        &self,
        target: &web_sys::EventTarget,
        kind: &'static str,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<CanvasListener, JsValue> {
        CanvasListener::install(target, kind, &self.accepting, body)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    /// Insert `element` as the canvas's next sibling.
    fn place_beside_canvas(&self, element: &HtmlElement) -> Result<(), JsValue> {
        let parent = self
            .canvas
            .parent_node()
            .ok_or_else(|| JsValue::from_str("the canvas is not in a document"))?;
        parent.insert_before(element, self.canvas.next_sibling().as_ref())?;
        Ok(())
    }

    /// Queue `work` and run the queue, unless it is running already, in
    /// which case the run in progress reaches it.
    fn submit(&self, work: Work) {
        self.work.borrow_mut().push_back(work);
        if self.running.replace(true) {
            return;
        }
        loop {
            let next = self.work.borrow_mut().pop_front();
            let Some(next) = next else {
                break;
            };
            self.perform(next);
        }
        self.running.set(false);
    }

    fn perform(&self, work: Work) {
        match work {
            Work::Activate => self.activate(),
            Work::Publish(update) => self.mirror(update),
            Work::Action(request) => {
                // The tree's own focus, echoed by the element it focused.
                let echo = request.action == accesskit::Action::Focus
                    && self.tree.borrow().mirror.focus() == Some(request.target_node);
                if !echo {
                    self.shared.notify_action(request);
                }
            }
            Work::Place => self.place(),
        }
    }

    /// Put the semantics host in the placeholder's place and tell the
    /// composition root.
    fn activate(&self) {
        let placeholder_focused = match &*self.stage.borrow() {
            Stage::Placeholder(placeholder) => self.holds_focus(&placeholder.element),
            Stage::Host(_) | Stage::Retired => return,
        };
        let host = match self.host() {
            Ok(host) => host,
            Err(error) => {
                warn_failure("the semantics host could not be added", &error);
                return;
            }
        };
        self.editor.set_semantics(Some(&host.element));
        let replaced = std::mem::replace(&mut *self.stage.borrow_mut(), Stage::Host(host));
        if let Stage::Placeholder(placeholder) = replaced {
            placeholder.element.remove();
        }
        self.claim_focus.set(placeholder_focused);
        self.place();
        // The tree retained before activation goes first, then what the
        // root publishes when told.
        if let Some(retained) = self.shared.retained() {
            self.work.borrow_mut().push_back(Work::Publish(retained));
        }
        self.shared.notify_activation(true);
    }

    fn mirror(&self, update: TreeUpdate) {
        let applied = self.tree.borrow_mut().mirror.apply(update);
        let ops = match applied {
            Ok(ops) => ops,
            Err(refused) => {
                tracing::warn!(%refused, "an accessibility update was not mirrored");
                // The whole tree stands on its own; a subtree never will.
                if refused != Refused::Subtree && !self.asked_again.replace(true) {
                    self.shared.notify_activation(true);
                }
                return;
            }
        };
        self.asked_again.set(false);
        let Some(host) = self.host_element() else {
            return;
        };
        let focus_was_inside = self.holds_focus(&host);
        let mut focus_moved = false;
        {
            let mut tree = self.tree.borrow_mut();
            for op in ops {
                if matches!(op, Op::Focus(_)) {
                    focus_moved = true;
                }
                if let Err(error) = run(&self.document, &host, &mut tree.elements, op) {
                    warn_failure("the semantics host could not be changed", &error);
                }
            }
        }
        self.place();
        let follow = self.claim_focus.get() || focus_was_inside;
        if follow && (focus_moved || !self.holds_focus(&host)) {
            self.follow_focus();
        }
    }

    /// Move the page's focus to the tree's focus.
    fn follow_focus(&self) {
        let element = {
            let tree = self.tree.borrow();
            if tree.mirror.is_empty() {
                return;
            }
            tree.mirror
                .focus()
                .and_then(|focus| tree.elements.get(&focus).cloned())
        };
        self.claim_focus.set(false);
        if let Some(element) = element {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            if let Err(error) = element.focus_with_options(&options) {
                warn_failure("the focused node's element could not be focused", &error);
            }
        }
    }

    /// A `click`, `focusin` or `contextmenu` on an element of the host,
    /// turned into the action its node supports.
    fn node_event(&self, kind: &str, event: &web_sys::Event) {
        let Some(node) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .and_then(|target| target.closest("[data-flui-node]").ok().flatten())
            .and_then(|element| element.get_attribute(NODE_ATTRIBUTE))
            .and_then(|id| id.parse::<u64>().ok())
            .map(NodeId)
        else {
            return;
        };
        // An event the host's own change fired (its `focus()`) finds the
        // tree borrowed: it is the tree's, not the user's.
        let Ok(tree) = self.tree.try_borrow() else {
            return;
        };
        let action = tree
            .mirror
            .node(node)
            .and_then(|mirrored| aria::event_action(kind, mirrored));
        drop(tree);
        let Some(action) = action else {
            return;
        };
        if kind == "contextmenu" {
            event.prevent_default();
        }
        self.submit(Work::Action(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node,
            data: None,
        }));
    }

    /// Put the host over the canvas's box in the viewport, at the canvas's
    /// layout size, which the tree's CSS pixels are in.
    ///
    /// A transformed ancestor of the canvas (the host's too, as its sibling)
    /// makes the host's `fixed` box relative to that ancestor and scales it
    /// with the canvas: the host is measured where it landed and moved by
    /// the difference, in its own scale. A rotated ancestor is not followed.
    fn place(&self) {
        let Some(host) = self.host_element() else {
            return;
        };
        let canvas = self.canvas.get_bounding_client_rect();
        let width = f64::from(self.canvas.offset_width());
        let height = f64::from(self.canvas.offset_height());
        let style = host.style();
        let set = |left: f64, top: f64| {
            for (property, value) in [
                ("left", left),
                ("top", top),
                ("width", width),
                ("height", height),
            ] {
                style.set_property(property, &pixels(value))?;
            }
            Ok::<(), JsValue>(())
        };
        if let Err(error) = set(canvas.left(), canvas.top()) {
            warn_failure("the semantics host could not be placed", &error);
            return;
        }
        let placed = host.get_bounding_client_rect();
        let scale = |landed: f64, laid_out: f64| {
            let scale = landed / laid_out;
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            }
        };
        let left = canvas.left() + (canvas.left() - placed.left()) / scale(placed.width(), width);
        let top = canvas.top() + (canvas.top() - placed.top()) / scale(placed.height(), height);
        let moved = (left - canvas.left()).abs() > f64::EPSILON
            || (top - canvas.top()).abs() > f64::EPSILON;
        if moved && let Err(error) = set(left, top) {
            warn_failure("the semantics host could not be placed", &error);
        }
    }

    fn host_element(&self) -> Option<HtmlElement> {
        match &*self.stage.borrow() {
            Stage::Host(host) => Some(host.element.clone()),
            Stage::Placeholder(_) | Stage::Retired => None,
        }
    }

    /// Whether the page's focus is on `element` or inside it.
    fn holds_focus(&self, element: &HtmlElement) -> bool {
        self.document
            .active_element()
            .is_some_and(|active| element.contains(Some(&active)))
    }

    /// The window closed: remove what stands beside the canvas, for good.
    fn retire(&self) {
        let stage = std::mem::replace(&mut *self.stage.borrow_mut(), Stage::Retired);
        self.work.borrow_mut().clear();
        match stage {
            Stage::Placeholder(placeholder) => placeholder.element.remove(),
            Stage::Host(host) => {
                self.editor.set_semantics(None);
                host.element.remove();
            }
            Stage::Retired => {}
        }
        let elements = std::mem::take(&mut self.tree.borrow_mut().elements);
        drop(elements);
    }
}

/// Run one [`Op`] on the host's elements.
fn run(
    document: &Document,
    host: &HtmlElement,
    elements: &mut HashMap<NodeId, HtmlElement>,
    op: Op,
) -> Result<(), JsValue> {
    match op {
        Op::Create { id, spec } => {
            let element: HtmlElement = document.create_element(spec.tag.name())?.unchecked_into();
            element.set_attribute(NODE_ATTRIBUTE, &id.0.to_string())?;
            let style = element.style();
            for (property, value) in [
                ("position", "absolute"),
                ("display", "block"),
                ("margin", "0"),
                ("transform-origin", "0 0"),
            ] {
                style.set_property(property, value)?;
            }
            write(&element, None, &spec)?;
            elements.insert(id, element);
        }
        Op::Update { id, old, new } => {
            if let Some(element) = elements.get(&id) {
                write(element, Some(&old), &new)?;
            }
        }
        Op::Children { id, children } => {
            if let Some(parent) = elements.get(&id) {
                let children: Vec<&HtmlElement> = children
                    .iter()
                    .filter_map(|child| elements.get(child))
                    .collect();
                arrange(parent, &children)?;
            }
        }
        Op::Root(id) => {
            if let Some(root) = elements.get(&id)
                && host.first_element_child().as_ref() != Some(root.as_ref())
            {
                host.set_text_content(None);
                host.append_child(root)?;
            }
        }
        Op::Remove(id) => {
            if let Some(element) = elements.remove(&id) {
                element.remove();
            }
        }
        // Followed once every other change is made.
        Op::Focus(_) => {}
    }
    Ok(())
}

/// Bring `element` from `old` (nothing, for a new element) to `new`.
fn write(
    element: &HtmlElement,
    old: Option<&ElementSpec>,
    new: &ElementSpec,
) -> Result<(), JsValue> {
    let before: &[(&str, String)] = old.map_or(&[], |old| &old.attributes);
    for (name, _) in before {
        if !new.attributes.iter().any(|(kept, _)| kept == name) {
            element.remove_attribute(name)?;
        }
    }
    for (name, value) in &new.attributes {
        let was = before
            .iter()
            .find(|(had, _)| had == name)
            .map(|(_, was)| was);
        if was != Some(value) {
            element.set_attribute(name, value)?;
        }
    }
    match old {
        None if new.text.is_none() => {}
        Some(old) if old.text == new.text => {}
        _ => element.set_text_content(new.text.as_deref()),
    }
    if old.map(|old| old.placement) != Some(new.placement) {
        place(element, &new.placement)?;
    }
    Ok(())
}

fn place(element: &HtmlElement, placement: &Placement) -> Result<(), JsValue> {
    let style = element.style();
    style.set_property("left", &pixels(placement.left))?;
    style.set_property("top", &pixels(placement.top))?;
    style.set_property("width", &pixels(placement.width))?;
    style.set_property("height", &pixels(placement.height))?;
    match placement.linear {
        Some([a, b, c, d]) => {
            style.set_property("transform", &format!("matrix({a}, {b}, {c}, {d}, 0, 0)"))?;
        }
        None => {
            style.remove_property("transform")?;
        }
    }
    Ok(())
}

/// Make `children` the element children of `parent`, in order, moving only
/// the ones out of place.
fn arrange(parent: &HtmlElement, children: &[&HtmlElement]) -> Result<(), JsValue> {
    let mut cursor = parent.first_element_child();
    for child in children {
        let child: &Element = child.as_ref();
        if cursor.as_ref() == Some(child) {
            cursor = child.next_element_sibling();
            continue;
        }
        let before: Option<&web_sys::Node> = cursor.as_ref().map(AsRef::as_ref);
        parent.insert_before(child, before)?;
    }
    // What is left is no longer this parent's: a later change places it
    // under its new parent, or removes it.
    while let Some(extra) = cursor {
        cursor = extra.next_element_sibling();
        extra.remove();
    }
    Ok(())
}

fn pixels(value: f64) -> String {
    format!("{value}px")
}
