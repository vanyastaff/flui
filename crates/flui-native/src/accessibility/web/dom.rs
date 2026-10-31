//! The semantics host in the page: the adapter a canvas window hands out.
//!
//! Before assistive technology asks, the page holds one element beside the
//! canvas, a placeholder button only a screen reader finds; the browser
//! tells a page nothing of a screen reader otherwise. Activating it puts the
//! semantics host in its place, `position: fixed` over the canvas with
//! `pointer-events: none` and `opacity: 0`, so the pointer still reaches the
//! canvas, FLUI keeps hit-testing it and nothing paints over it, and tells
//! the composition root, which publishes the whole tree. Nothing deactivates
//! it: the browser does not say when a screen reader leaves.
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

use accesskit::{
    Action, ActionData, ActionRequest, NodeId, TextPosition, TextSelection, TreeId, TreeUpdate,
};
use flui_platform::keyboard::KeyState;
use flui_semantics::AssistiveFocusSupport;
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    Document, Element, HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlTextAreaElement,
};

use super::aria::{self, ElementIds, ElementSpec, Placement, Scroll, Tag};
use super::mirror::{Mirror, Op, Refused};
use crate::accessibility::bridge::BridgeShared;
use crate::owner::slot::OwnerSlot;
use crate::owner::web::listener::{CanvasListener, viewport_listeners, warn_failure};
use crate::owner::web::{Accepting, CanvasEditor, CanvasKeys, CanvasRef, FocusedField};

/// The attribute that names the node an element mirrors.
const NODE_ATTRIBUTE: &str = "data-flui-node";

/// The empty element whose size is a scroller's content.
const EXTENT_TAG: &str = "flui-scroll-extent";
/// A scroller's own style: it scrolls with no scrollbar, which would take
/// part of the box the node's viewport is, and its offsets run from the
/// left edge whatever direction the page inherits, as the node's scroll
/// does.
const SCROLLER_STYLE: [(&str, &str); 3] = [
    ("overflow", "auto"),
    ("scrollbar-width", "none"),
    ("direction", "ltr"),
];
/// How far from 1 the canvas's own scale may be before the host takes it:
/// layout rounds a box to a fraction of a pixel.
const SCALE_TOLERANCE: f64 = 1e-3;

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
    /// The text input stopped serving a store through this field: it shows
    /// its published value again.
    Released(HtmlElement),
    /// The page rendered the live regions created since the last time:
    /// write their text.
    Speak,
    /// The page's focus entered or left the host from outside the canvas's
    /// focus area.
    SyncFocus,
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
    /// The field the text input edits through, which the tree's focus names.
    field: Rc<FocusedField>,
    stage: RefCell<Stage>,
    tree: RefCell<Tree>,
    work: RefCell<VecDeque<Work>>,
    running: Cell<bool>,
    /// The composition root was asked for the whole tree after a refused
    /// update and has not answered with one that stands yet.
    asked_again: Cell<bool>,
    /// The host is not shown, or not yet: it has no scroll to keep.
    host_hidden: Cell<bool>,
    /// Watches the canvas's own `style`, `class` and `hidden` for a change
    /// that shows or hides it; dropped when the window closes.
    canvas_style: RefCell<Option<StyleObserver>>,
    /// Live regions created empty, whose text is written once the page has
    /// rendered them: a region that enters the page with its text is not
    /// read out.
    unspoken: RefCell<Vec<NodeId>>,
    /// The frames that wait for them to render.
    rendered: FrameWait,
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
            field: Rc::clone(&window.field),
            stage: RefCell::new(Stage::Retired),
            tree: RefCell::new(Tree {
                mirror: Mirror::new(ElementIds::new(scope())),
                elements: HashMap::new(),
            }),
            work: RefCell::default(),
            running: Cell::new(false),
            asked_again: Cell::new(false),
            host_hidden: Cell::new(true),
            canvas_style: RefCell::new(None),
            unspoken: RefCell::default(),
            rendered: FrameWait::new({
                let this = this.clone();
                move || {
                    if let Some(this) = this.upgrade() {
                        this.submit(Work::Speak);
                    }
                }
            }),
        });
        let placeholder = page.placeholder()?;
        *page.stage.borrow_mut() = Stage::Placeholder(placeholder);
        let this = page.this.clone();
        *page.canvas_style.borrow_mut() = Some(StyleObserver::try_new(&page.canvas, move || {
            if let Some(this) = this.upgrade() {
                this.submit(Work::Place);
            }
        })?);
        page.place();
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
            // Painted by the canvas only: the elements keep their boxes and
            // their place in the accessibility tree, and paint nothing over
            // it, focus rings included.
            ("opacity", "0"),
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
        let this = self.this.clone();
        listeners.push(self.listen(target, "input", move |event| {
            if let Some(this) = this.upgrade() {
                this.input_changed(&event);
            }
        })?);
        // The page has one selection, and tells the document of its changes.
        let this = self.this.clone();
        listeners.push(
            self.listen(self.document.as_ref(), "selectionchange", move |_| {
                if let Some(this) = this.upgrade() {
                    this.selection_changed();
                }
            })?,
        );
        // `scroll` does not bubble: the host sees its scrollers' in the
        // capture phase.
        let this = self.this.clone();
        listeners.push(
            CanvasListener::install_in_phase(
                target,
                "scroll",
                &self.accepting,
                true,
                move |event| {
                    if let Some(this) = this.upgrade() {
                        this.scrolled(&event);
                    }
                },
            )
            .map_err(|error| JsValue::from_str(&error.to_string()))?,
        );
        // The window reads its focus again when it leaves or enters the host
        // from outside the canvas's focus area. The window's handler runs
        // from the queue: a mirrored element removed while it holds focus
        // fires `focusout` while the tree is borrowed.
        for kind in ["focusin", "focusout"] {
            let this = self.this.clone();
            listeners.push(self.listen(target, kind, move |event| {
                if let Some(this) = this.upgrade()
                    && !this.editor.is_handoff(&event)
                {
                    this.submit(Work::SyncFocus);
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
            Work::SyncFocus => self.editor.sync(),
            Work::Released(element) => self.show_published(&element),
            Work::Speak => self.speak(),
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
        let this = self.this.clone();
        self.field
            .on_release(Some(Rc::new(move |element: &HtmlElement| {
                if let Some(this) = this.upgrade() {
                    this.submit(Work::Released(element.clone()));
                }
            })));
        let replaced = std::mem::replace(&mut *self.stage.borrow_mut(), Stage::Host(host));
        // Removing the focused placeholder would leave the page's focus on
        // the body, where no key reaches the window.
        if placeholder_focused {
            self.focus_canvas();
        }
        if let Stage::Placeholder(placeholder) = replaced {
            placeholder.element.remove();
        }
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
        // The tree's focus is followed only while the page's focus is the
        // window's: on the canvas or in the host, before this update removed
        // an element that held it.
        let focus_was_in_window = self.holds_focus(&host) || self.canvas_focused();
        let mut focus_moved = false;
        let mut scrolled = Vec::new();
        let mut shown = None;
        let mut created_live = false;
        {
            let mut tree = self.tree.borrow_mut();
            let mut unspoken = self.unspoken.borrow_mut();
            for op in ops {
                let op = hold_live_text(op, &mut unspoken, &mut created_live);
                if let Op::Select(selection) = &op {
                    shown = Some(*selection);
                }
                if matches!(op, Op::Focus(_)) {
                    focus_moved = true;
                }
                if let Err(error) = run(
                    &self.document,
                    &host,
                    &self.field,
                    &mut tree.elements,
                    &mut scrolled,
                    op,
                ) {
                    warn_failure("the semantics host could not be changed", &error);
                }
            }
        }
        self.place();
        // Scrolled once placed and shown: a hidden element has no box.
        for (element, scroll) in &scrolled {
            scroll_to(element, scroll);
        }
        // Once every element is written: the text it lies in is in place.
        if let Some(selection) = shown {
            self.show_selection(selection);
        }
        if created_live {
            self.rendered.arm();
        }
        let field = self.focused_field();
        self.field.name(field.as_ref());
        if focus_was_in_window && (focus_moved || !self.holds_focus(&host)) {
            self.follow_focus();
        }
    }

    /// Show a field's published value again: the store it was served
    /// through held its own, and the tree may have published another since.
    fn show_published(&self, element: &HtmlElement) {
        if self.field.serves(element) {
            return;
        }
        let value = {
            let Ok(tree) = self.tree.try_borrow() else {
                return;
            };
            let Some(node) = element
                .get_attribute(NODE_ATTRIBUTE)
                .and_then(|node| node.parse::<u64>().ok())
            else {
                return;
            };
            let mirrored = tree.elements.get(&NodeId(node));
            if !mirrored.is_some_and(|mirrored| mirrored.is_same_node(Some(element.as_ref()))) {
                return;
            }
            tree.mirror
                .spec(NodeId(node))
                .and_then(|spec| spec.value.clone())
        };
        if let Some(value) = value {
            set_value(element, &value);
        }
    }

    /// The page has rendered the live regions created empty: write the
    /// text their nodes hold now, so it is read out as a change. A region
    /// removed in the meantime is no longer waiting.
    fn speak(&self) {
        let unspoken = std::mem::take(&mut *self.unspoken.borrow_mut());
        let Ok(tree) = self.tree.try_borrow() else {
            return;
        };
        for id in unspoken {
            let (Some(element), Some(spec)) = (tree.elements.get(&id), tree.mirror.spec(id)) else {
                continue;
            };
            if let Err(error) = write_text(element, spec.text.as_deref()) {
                warn_failure("a live region's text could not be written", &error);
            }
        }
    }

    /// The element of the tree's focus when it is mirrored as a text field.
    fn focused_field(&self) -> Option<HtmlElement> {
        let tree = self.tree.borrow();
        let focus = tree.mirror.focus()?;
        tree.mirror
            .spec(focus)
            .is_some_and(|spec| spec.tag.is_field())
            .then(|| tree.elements.get(&focus).cloned())
            .flatten()
    }

    /// Move the page's focus to the tree's focus. A focus whose element
    /// takes no DOM focus (a root without `Action::Focus`), or a node that
    /// takes text with no field of its own, which the window's text input
    /// serves on the canvas, puts the page's focus on the canvas, off any
    /// element it was on before.
    fn follow_focus(&self) {
        let element = {
            let tree = self.tree.borrow();
            if tree.mirror.is_empty() {
                return;
            }
            tree.mirror
                .focus()
                .filter(|&focus| {
                    let field = tree
                        .mirror
                        .spec(focus)
                        .is_some_and(|spec| spec.tag.is_field());
                    field || !tree.mirror.node(focus).is_some_and(aria::takes_text)
                })
                .and_then(|focus| tree.elements.get(&focus).cloned())
        };
        if let Some(element) = element {
            if let Err(error) = element.focus_with_options(&unscrolled()) {
                warn_failure("the focused node's element could not be focused", &error);
            }
            let target: &Element = element.as_ref();
            if self.document.active_element().as_ref() == Some(target) {
                return;
            }
        }
        self.focus_canvas();
    }

    /// Put the page's focus on the canvas.
    fn focus_canvas(&self) {
        if self.canvas_focused() {
            return;
        }
        if let Err(error) = self.canvas.focus_with_options(&unscrolled()) {
            warn_failure("the canvas could not be focused", &error);
        }
    }

    fn canvas_focused(&self) -> bool {
        let canvas: &Element = self.canvas.as_ref();
        self.document.active_element().as_ref() == Some(canvas)
    }

    /// A `click`, `focusin` or `contextmenu` on an element of the host,
    /// turned into the action its node supports.
    fn node_event(&self, kind: &str, event: &web_sys::Event) {
        // A link's address is read, never followed: the node's action is
        // what a click on it does.
        if kind == "click" && closest(event, "a[href]").is_some() {
            event.prevent_default();
        }
        let Some((node, _)) = event_node(event) else {
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

    /// The browser changed an input's value. A slider's range input moved
    /// for a screen reader: the value, or a step towards it, is asked of the
    /// node. The input shows the node's value again until the tree publishes
    /// the moved one, so a move the node refuses leaves nothing behind. A text
    /// field the window's text input edits through is the store's, which
    /// takes the edit; one it does not is shown as published again, since
    /// only the store edits the text.
    fn input_changed(&self, event: &web_sys::Event) {
        let Some((node, element)) = event_node(event) else {
            return;
        };
        if self.field.serves(&element) {
            return;
        }
        let Ok(tree) = self.tree.try_borrow() else {
            return;
        };
        let Some(spec) = tree.mirror.spec(node) else {
            return;
        };
        let published = spec.value.clone();
        let request = match spec.tag {
            Tag::Range => element
                .dyn_ref::<HtmlInputElement>()
                .map(HtmlInputElement::value_as_number)
                .and_then(|value| {
                    let mirrored = tree.mirror.node(node)?;
                    aria::slider_request(mirrored, value)
                }),
            Tag::Line | Tag::Area => None,
            Tag::Node | Tag::Link => return,
        };
        drop(tree);
        if let Some(published) = published {
            set_value(&element, &published);
        }
        if let Some((action, data)) = request {
            self.submit(Work::Action(ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: node,
                data,
            }));
        }
    }

    /// The browser scrolled a scroller, for a screen reader's swipe or to
    /// reveal the element its cursor moved to: the offset it is scrolled to
    /// is asked of the node, and the scroller goes back to the published
    /// offset until the node publishes the new one, so a scroll the node
    /// refuses leaves nothing behind. A scroll to where the node already is
    /// (the host's own, after a publish) asks for nothing.
    fn scrolled(&self, event: &web_sys::Event) {
        let Some((node, element)) = event_node(event) else {
            return;
        };
        let (left, top) = (
            f64::from(element.scroll_left()),
            f64::from(element.scroll_top()),
        );
        let Ok(tree) = self.tree.try_borrow() else {
            return;
        };
        let Some(scroll) = tree.mirror.spec(node).and_then(|spec| spec.scroll) else {
            return;
        };
        if scroll.is_at(left, top) {
            return;
        }
        let offset = tree
            .mirror
            .node(node)
            .and_then(|mirrored| aria::scroll_offset(mirrored, left, top));
        drop(tree);
        // The node scrolls, or refuses to: until it publishes where it is, the
        // scroller stays where it was published, with its children.
        scroll_to(&element, &scroll);
        if let Some(offset) = offset {
            self.submit(Work::Action(ActionRequest {
                action: Action::SetScrollOffset,
                target_tree: TreeId::ROOT,
                target_node: node,
                data: Some(ActionData::SetScrollOffset(offset)),
            }));
        }
    }

    /// Show the selection static text published, or none, as the page's
    /// selection. The page's own selection, in text outside the host, is
    /// left alone: the host takes the page's selection only while it is
    /// empty, a caret or already in the host.
    fn show_selection(&self, selection: Option<TextSelection>) {
        let Some(host) = self.host_element() else {
            return;
        };
        let Ok(Some(page)) = self.document.get_selection() else {
            return;
        };
        let in_host = selection_in(&page, &host);
        if !(in_host || page.range_count() == 0 || page.is_collapsed()) {
            return;
        }
        let ends = {
            let Ok(tree) = self.tree.try_borrow() else {
                return;
            };
            selection.and_then(|selection| {
                Some((
                    text_at(&tree, selection.anchor)?,
                    text_at(&tree, selection.focus)?,
                ))
            })
        };
        let shown = match ends {
            Some(((anchor, anchor_offset), (focus, focus_offset))) => {
                page.set_base_and_extent(&anchor, anchor_offset, &focus, focus_offset)
            }
            // A caret elsewhere on the page stays where it is.
            None if in_host => page.remove_all_ranges(),
            None => Ok(()),
        };
        if let Err(error) = shown {
            warn_failure("the page's selection could not be set", &error);
        }
    }

    /// The page's selection changed, for a screen reader selecting static
    /// text: a selection with both ends in text that publishes runs is
    /// asked of the nearest node over both that takes `SetTextSelection`,
    /// and the page shows the published selection again until the tree
    /// publishes the new one, so a selection the node refuses leaves
    /// nothing behind. The published selection itself (the host's own,
    /// after a publish) asks for nothing.
    fn selection_changed(&self) {
        let Some(host) = self.host_element() else {
            return;
        };
        let Ok(Some(page)) = self.document.get_selection() else {
            return;
        };
        let (Some(anchor), Some(focus)) = (
            text_end(&host, page.anchor_node(), page.anchor_offset()),
            text_end(&host, page.focus_node(), page.focus_offset()),
        ) else {
            return;
        };
        let Ok(tree) = self.tree.try_borrow() else {
            return;
        };
        let published = tree.mirror.selection();
        let published_ends = published.and_then(|selection| {
            Some((
                tree.mirror.dom_position(selection.anchor)?,
                tree.mirror.dom_position(selection.focus)?,
            ))
        });
        if published_ends == Some((anchor, focus)) {
            return;
        }
        let request = (|| {
            let target = tree.mirror.selection_target(anchor.0, focus.0)?;
            let selection = TextSelection {
                anchor: tree.mirror.text_position(anchor.0, anchor.1)?,
                focus: tree.mirror.text_position(focus.0, focus.1)?,
            };
            Some((target, selection))
        })();
        drop(tree);
        let Some((target, selection)) = request else {
            return;
        };
        self.show_selection(published);
        self.submit(Work::Action(ActionRequest {
            action: Action::SetTextSelection,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: Some(ActionData::SetTextSelection(selection)),
        }));
    }

    /// Show what stands beside the canvas only while the canvas is shown,
    /// and put the host over the canvas's drawable box in the viewport, at
    /// the canvas's layout size, which the tree's CSS pixels are in.
    ///
    /// The host is the canvas's sibling, so an ancestor that hides the canvas
    /// hides it too; the canvas's own `display` and `visibility` are copied.
    /// The drawable box is the client box, inside the canvas's border, as the
    /// window's layout size reads it. A transformed ancestor of the canvas
    /// (the host's too) makes the host's `fixed` box relative to that
    /// ancestor and scales it with the canvas: the host is measured where it
    /// landed and moved by the difference, in its own scale. A scale on the
    /// canvas alone is the host's own `transform`, from its top left corner.
    /// A rotation is not followed.
    fn place(&self) {
        let (element, host) = match &*self.stage.borrow() {
            Stage::Placeholder(placeholder) => (placeholder.element.clone(), false),
            Stage::Host(host) => (host.element.clone(), true),
            Stage::Retired => return,
        };
        let style = element.style();
        let hidden = self.canvas_hidden();
        let shown = if hidden {
            style.set_property("display", "none")
        } else if host {
            style.set_property("display", "block")
        } else {
            style.remove_property("display").map(drop)
        };
        if let Err(error) = shown {
            warn_failure("the semantics host could not be shown or hidden", &error);
        }
        if hidden || !host {
            self.host_hidden.set(true);
            return;
        }
        // A hidden element keeps no scroll: the host shown again scrolls
        // each scroller back to where its node is.
        if self.host_hidden.replace(false) {
            self.restore_scrolls();
        }
        let canvas = self.canvas.get_bounding_client_rect();
        let width = f64::from(self.canvas.client_width());
        let height = f64::from(self.canvas.client_height());
        let ratio = |landed: f64, laid_out: f64| {
            let ratio = landed / laid_out;
            if ratio.is_finite() && ratio > 0.0 {
                ratio
            } else {
                1.0
            }
        };
        // The canvas's whole scale in the viewport, and where its client box
        // starts there.
        let total = (
            ratio(canvas.width(), f64::from(self.canvas.offset_width())),
            ratio(canvas.height(), f64::from(self.canvas.offset_height())),
        );
        let origin = (
            canvas.left() + f64::from(self.canvas.client_left()) * total.0,
            canvas.top() + f64::from(self.canvas.client_top()) * total.1,
        );
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
        let placed = style
            .remove_property("transform")
            .and_then(|_| set(origin.0, origin.1));
        if let Err(error) = placed {
            warn_failure("the semantics host could not be placed", &error);
            return;
        }
        let placed = element.get_bounding_client_rect();
        // The ancestors' scale, which the host shares, and the canvas's own.
        let shared = (ratio(placed.width(), width), ratio(placed.height(), height));
        let own = (total.0 / shared.0, total.1 / shared.1);
        let left = origin.0 + (origin.0 - placed.left()) / shared.0;
        let top = origin.1 + (origin.1 - placed.top()) / shared.1;
        let moved = (left - origin.0).abs() > f64::EPSILON || (top - origin.1).abs() > f64::EPSILON;
        let scaled = (own.0 - 1.0).abs() > SCALE_TOLERANCE || (own.1 - 1.0).abs() > SCALE_TOLERANCE;
        let mut placed = Ok(());
        if moved {
            placed = set(left, top);
        }
        if scaled {
            placed = placed
                .and_then(|()| style.set_property("transform-origin", "0 0"))
                .and_then(|()| {
                    style.set_property("transform", &format!("scale({}, {})", own.0, own.1))
                });
        }
        if let Err(error) = placed {
            warn_failure("the semantics host could not be placed", &error);
        }
    }

    /// Scroll every scroller to its node's scroll.
    fn restore_scrolls(&self) {
        let scrollers: Vec<(HtmlElement, Scroll)> = {
            let Ok(tree) = self.tree.try_borrow() else {
                return;
            };
            tree.elements
                .iter()
                .filter_map(|(&id, element)| {
                    let scroll = tree.mirror.spec(id)?.scroll?;
                    Some((element.clone(), scroll))
                })
                .collect()
        };
        for (element, scroll) in &scrollers {
            scroll_to(element, scroll);
        }
    }

    /// Whether the canvas is hidden: by its own style, or by an ancestor's
    /// `display: none`, which leaves it no box.
    fn canvas_hidden(&self) -> bool {
        let Some(computed) = self
            .document
            .default_view()
            .and_then(|window| window.get_computed_style(&self.canvas).ok().flatten())
        else {
            return false;
        };
        let read = |property| computed.get_property_value(property).unwrap_or_default();
        read("display") == "none"
            || read("visibility") != "visible"
            || self.canvas.get_client_rects().length() == 0
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
        self.field.on_release(None);
        self.rendered.cancel();
        self.unspoken.borrow_mut().clear();
        // The text input goes back to an editor of its own.
        self.field.name(None);
        match stage {
            Stage::Placeholder(placeholder) => placeholder.element.remove(),
            Stage::Host(host) => {
                self.editor.set_semantics(None);
                host.element.remove();
            }
            Stage::Retired => {}
        }
        drop(self.canvas_style.borrow_mut().take());
        let elements = std::mem::take(&mut self.tree.borrow_mut().elements);
        drop(elements);
    }
}

/// The nearest element at or above `event`'s target that matches
/// `selector`.
fn closest(event: &web_sys::Event, selector: &str) -> Option<Element> {
    event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
        .and_then(|target| target.closest(selector).ok().flatten())
}

/// `op` as the host runs it while live regions wait to be rendered empty: a
/// live region created or replaced with text is created without it and
/// added to `unspoken` (`created` is then set), a waiting region's text is
/// not written before then, and a removed one waits no longer.
fn hold_live_text(op: Op, unspoken: &mut Vec<NodeId>, created: &mut bool) -> Op {
    let mut hold = |id: NodeId, spec: ElementSpec| {
        if spec.is_live() && spec.text.is_some() {
            if !unspoken.contains(&id) {
                unspoken.push(id);
            }
            *created = true;
            ElementSpec { text: None, ..spec }
        } else {
            spec
        }
    };
    match op {
        Op::Create { id, spec } => Op::Create {
            id,
            spec: hold(id, spec),
        },
        Op::Replace { id, spec } => Op::Replace {
            id,
            spec: hold(id, spec),
        },
        Op::Update { id, old, new } if unspoken.contains(&id) => Op::Update {
            id,
            old: ElementSpec { text: None, ..old },
            new: ElementSpec { text: None, ..new },
        },
        Op::Remove(id) => {
            unspoken.retain(|&waiting| waiting != id);
            Op::Remove(id)
        }
        op => op,
    }
}

/// The closure a [`FrameWait`] hands the browser, which asks for its next
/// frame from inside itself.
type FrameSlot = RefCell<Option<Closure<dyn Fn()>>>;

/// Two animation frames, then the body: the page has rendered once in
/// between. A background tab renders no frame, so the body waits until the
/// tab is shown again; nobody hears a live region of a hidden tab.
struct FrameWait {
    slot: Rc<FrameSlot>,
    state: Rc<FrameState>,
}

#[derive(Default)]
struct FrameState {
    /// The frames still to come before the body runs.
    remaining: Cell<u8>,
    /// The frame requested and not yet run.
    pending: Cell<Option<i32>>,
}

impl FrameWait {
    fn new(body: impl Fn() + 'static) -> Self {
        let slot: Rc<FrameSlot> = Rc::default();
        let state: Rc<FrameState> = Rc::default();
        let again = Rc::downgrade(&slot);
        let frames = Rc::clone(&state);
        let closure = Closure::<dyn Fn()>::new(move || {
            frames.pending.set(None);
            let remaining = frames.remaining.get().saturating_sub(1);
            frames.remaining.set(remaining);
            if remaining > 0 {
                if let Some(slot) = again.upgrade() {
                    request_frame(&slot, &frames);
                }
            } else {
                body();
            }
        });
        *slot.borrow_mut() = Some(closure);
        Self { slot, state }
    }

    /// Run the body after two frames, unless it is waiting for them already.
    fn arm(&self) {
        if self.state.pending.get().is_some() {
            return;
        }
        self.state.remaining.set(2);
        request_frame(&self.slot, &self.state);
    }

    /// Drop the frame waited for, if any.
    fn cancel(&self) {
        if let (Some(id), Some(window)) = (self.state.pending.take(), web_sys::window()) {
            let _ = window.cancel_animation_frame(id);
        }
    }
}

impl Drop for FrameWait {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn request_frame(slot: &FrameSlot, state: &FrameState) {
    let (Some(window), Some(closure)) = (
        web_sys::window(),
        slot.borrow()
            .as_ref()
            .map(|closure| closure.as_ref().clone()),
    ) else {
        return;
    };
    match window.request_animation_frame(closure.unchecked_ref()) {
        Ok(id) => state.pending.set(Some(id)),
        Err(error) => warn_failure("no animation frame for the live regions", &error),
    }
}

/// Whether both ends of the page's selection are in `host`.
fn selection_in(page: &web_sys::Selection, host: &HtmlElement) -> bool {
    let inside = |node: Option<web_sys::Node>| node.is_some_and(|node| host.contains(Some(&node)));
    inside(page.anchor_node()) && inside(page.focus_node())
}

/// The text node `position` lies in and the UTF-16 offset into it: the
/// text that leads its paragraph's element.
fn text_at(tree: &Tree, position: TextPosition) -> Option<(web_sys::Node, u32)> {
    let (paragraph, offset) = tree.mirror.dom_position(position)?;
    let text = tree
        .elements
        .get(&paragraph)?
        .first_child()
        .filter(|child| child.node_type() == web_sys::Node::TEXT_NODE)?;
    Some((text, offset))
}

/// The paragraph and the UTF-16 offset an end of the page's selection names,
/// when it lies in the text that leads a mirrored element in `host`.
fn text_end(host: &HtmlElement, node: Option<web_sys::Node>, offset: u32) -> Option<(NodeId, u32)> {
    let node = node.filter(|node| node.node_type() == web_sys::Node::TEXT_NODE)?;
    let element = node.parent_element()?;
    if !host.contains(Some(&element)) || element.first_child().as_ref() != Some(&node) {
        return None;
    }
    let id = element.get_attribute(NODE_ATTRIBUTE)?.parse::<u64>().ok()?;
    Some((NodeId(id), offset))
}

/// The mirrored element `event` happened on, and its node.
fn event_node(event: &web_sys::Event) -> Option<(NodeId, Element)> {
    let element = closest(event, "[data-flui-node]")?;
    let node = element.get_attribute(NODE_ATTRIBUTE)?.parse::<u64>().ok()?;
    Some((NodeId(node), element))
}

/// Run one [`Op`] on the host's elements. An element whose scroll changed is
/// added to `scrolled`, to be scrolled once every element is in place: a
/// detached element has no box to scroll.
fn run(
    document: &Document,
    host: &HtmlElement,
    field: &FocusedField,
    elements: &mut HashMap<NodeId, HtmlElement>,
    scrolled: &mut Vec<(HtmlElement, Scroll)>,
    op: Op,
) -> Result<(), JsValue> {
    match op {
        Op::Create { id, spec } => {
            let element = create(document, id, &spec)?;
            if let Some(scroll) = spec.scroll {
                scrolled.push((element.clone(), scroll));
            }
            elements.insert(id, element);
        }
        Op::Replace { id, spec } => {
            let element = create(document, id, &spec)?;
            if let Some(old) = elements.get(&id) {
                // The children's elements move over; the old element's own
                // text and scroll extent stay behind with it.
                let mut child = old.first_element_child();
                while let Some(current) = child {
                    child = current.next_element_sibling();
                    if current.has_attribute(NODE_ATTRIBUTE) {
                        element.append_child(&current)?;
                    }
                }
                let focused = document.active_element().as_ref() == Some(old.as_ref());
                old.replace_with_with_node_1(&element)?;
                if focused {
                    element.focus_with_options(&unscrolled())?;
                }
            }
            if let Some(scroll) = spec.scroll {
                scrolled.push((element.clone(), scroll));
            }
            elements.insert(id, element);
        }
        Op::Update { id, old, new } => {
            if let Some(element) = elements.get(&id) {
                write(element, Some(&old), &new, field.serves(element))?;
                if let Some(scroll) = new.scroll.filter(|scroll| old.scroll != Some(*scroll)) {
                    scrolled.push((element.clone(), scroll));
                }
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
        Op::Detach(id) => {
            if let Some(element) = elements.get(&id) {
                element.remove();
            }
        }
        Op::Remove(id) => {
            if let Some(element) = elements.remove(&id) {
                element.remove();
            }
        }
        // Followed once every other change is made.
        Op::Focus(_) | Op::Select(_) => {}
    }
    Ok(())
}

/// A detached element for node `id`, brought to `spec`.
fn create(document: &Document, id: NodeId, spec: &ElementSpec) -> Result<HtmlElement, JsValue> {
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
    if matches!(spec.tag, Tag::Range | Tag::Line | Tag::Area) {
        // The input's own look would size it; its box is the node's.
        for (property, value) in [
            ("appearance", "none"),
            ("padding", "0"),
            ("border", "0"),
            ("box-sizing", "border-box"),
            ("resize", "none"),
        ] {
            style.set_property(property, value)?;
        }
    }
    write(&element, None, spec, false)?;
    Ok(element)
}

/// Scroll `element` to `scroll`, unless it is there: the browser answers a
/// scroll with a `scroll` event, which then asks for nothing.
fn scroll_to(element: &Element, scroll: &Scroll) {
    if !scroll.is_at(
        f64::from(element.scroll_left()),
        f64::from(element.scroll_top()),
    ) {
        element.scroll_to_with_x_and_y(scroll.left, scroll.top);
    }
}

/// Bring `element` from `old` (nothing, for a new element) to `new`. The
/// value of a field the window's text input edits through (`served`) is the
/// store's, which the text input writes; the published one, which can lag
/// it (a composition in progress), is left out.
fn write(
    element: &HtmlElement,
    old: Option<&ElementSpec>,
    new: &ElementSpec,
    served: bool,
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
    if old.map(|old| &old.text) != Some(&new.text) {
        write_text(element, new.text.as_deref())?;
    }
    // An input sanitizes its value against its range as `min`, `max` and
    // `step` are written, so a changed attribute writes the value again.
    if !served
        && old.map(|old| (&old.value, &old.attributes)) != Some((&new.value, &new.attributes))
        && let Some(value) = &new.value
    {
        set_value(element, value);
    }
    if old.map(|old| old.placement) != Some(new.placement) {
        place(element, &new.placement)?;
    }
    if old.map(|old| old.scroll) != Some(new.scroll) {
        write_extent(element, new.scroll.as_ref())?;
    }
    Ok(())
}

/// Write `value` to an input's or a textarea's `value` property.
fn set_value(element: &Element, value: &str) {
    if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        input.set_value(value);
    } else if let Some(area) = element.dyn_ref::<HtmlTextAreaElement>() {
        area.set_value(value);
    }
}

/// Make `element` a scroller over content of `scroll`'s size, or a plain
/// box again. The content's size is an empty element of that size, the
/// last child, which no arrangement of the children's elements moves.
fn write_extent(element: &HtmlElement, scroll: Option<&Scroll>) -> Result<(), JsValue> {
    let style = element.style();
    let extent = element.query_selector(&format!(":scope > {EXTENT_TAG}"))?;
    let Some(scroll) = scroll else {
        for property in SCROLLER_STYLE.map(|(property, _)| property) {
            style.remove_property(property)?;
        }
        if let Some(extent) = extent {
            extent.remove();
        }
        return Ok(());
    };
    for (property, value) in SCROLLER_STYLE {
        style.set_property(property, value)?;
    }
    let extent: HtmlElement = if let Some(extent) = extent {
        extent.unchecked_into()
    } else {
        let document = element
            .owner_document()
            .ok_or_else(|| JsValue::from_str("the element has no document"))?;
        let extent: HtmlElement = document.create_element(EXTENT_TAG)?.unchecked_into();
        extent.set_attribute("aria-hidden", "true")?;
        let style = extent.style();
        for (property, value) in [
            ("position", "absolute"),
            ("display", "block"),
            ("left", "0"),
            ("top", "0"),
            ("visibility", "hidden"),
        ] {
            style.set_property(property, value)?;
        }
        element.append_child(&extent)?;
        extent
    };
    let style = extent.style();
    style.set_property("width", &pixels(scroll.width))?;
    style.set_property("height", &pixels(scroll.height))?;
    Ok(())
}

/// Make `text` the text node that leads `element`'s children, or remove it;
/// the children's elements after it are left where they are.
fn write_text(element: &HtmlElement, text: Option<&str>) -> Result<(), JsValue> {
    let leading = element
        .first_child()
        .filter(|node| node.node_type() == web_sys::Node::TEXT_NODE);
    match (leading, text) {
        (Some(node), Some(text)) => node.set_node_value(Some(text)),
        (Some(node), None) => {
            element.remove_child(&node)?;
        }
        (None, Some(text)) => element.prepend_with_str_1(text)?,
        (None, None) => {}
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
/// the ones out of place. A scroller's extent is not a node's and stays.
fn arrange(parent: &HtmlElement, children: &[&HtmlElement]) -> Result<(), JsValue> {
    let mirrored = |mut element: Option<Element>| {
        while let Some(current) = &element
            && !current.has_attribute(NODE_ATTRIBUTE)
        {
            element = current.next_element_sibling();
        }
        element
    };
    let mut cursor = mirrored(parent.first_element_child());
    for child in children {
        let child: &Element = child.as_ref();
        if cursor.as_ref() == Some(child) {
            cursor = mirrored(child.next_element_sibling());
            continue;
        }
        let before: Option<&web_sys::Node> = cursor.as_ref().map(AsRef::as_ref);
        parent.insert_before(child, before)?;
    }
    // What is left is no longer this parent's: a later change places it
    // under its new parent, or removes it.
    while let Some(extra) = cursor {
        cursor = mirrored(extra.next_element_sibling());
        extra.remove();
    }
    Ok(())
}

/// A `MutationObserver` on the attributes that can show or hide the canvas,
/// and a `ResizeObserver` on its box, which an ancestor hidden or shown
/// takes away or gives back; both disconnected when dropped.
struct StyleObserver {
    observer: web_sys::MutationObserver,
    resized: web_sys::ResizeObserver,
    _callback: Closure<dyn Fn()>,
}

impl StyleObserver {
    fn try_new(canvas: &HtmlCanvasElement, changed: impl Fn() + 'static) -> Result<Self, JsValue> {
        let callback = Closure::<dyn Fn()>::new(changed);
        let observer = web_sys::MutationObserver::new(callback.as_ref().unchecked_ref())?;
        let options = web_sys::MutationObserverInit::new();
        options.set_attributes(true);
        let filter: js_sys::Array = ["style", "class", "hidden"]
            .into_iter()
            .map(JsValue::from_str)
            .collect();
        options.set_attribute_filter(&filter);
        observer.observe_with_options(canvas, &options)?;
        let resized = match web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) {
            Ok(resized) => resized,
            Err(error) => {
                observer.disconnect();
                return Err(error);
            }
        };
        resized.observe(canvas);
        Ok(Self {
            observer,
            resized,
            _callback: callback,
        })
    }
}

impl Drop for StyleObserver {
    fn drop(&mut self) {
        self.observer.disconnect();
        self.resized.disconnect();
    }
}

/// A scope for one host's element `id`s that no other host on the page
/// shares: random, as two canvases' hosts never learn of each other.
fn scope() -> String {
    // 53 random bits, the most a JS number holds exactly.
    let bits = (js_sys::Math::random() * 9_007_199_254_740_992.0) as u64;
    format!("flui-a11y-{bits:x}")
}

/// Focus that leaves the page where it is scrolled.
fn unscrolled() -> web_sys::FocusOptions {
    let options = web_sys::FocusOptions::new();
    options.set_prevent_scroll(true);
    options
}

fn pixels(value: f64) -> String {
    format!("{value}px")
}
