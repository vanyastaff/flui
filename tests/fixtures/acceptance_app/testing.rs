//! Driving the acceptance application headless, the way its rows do: start
//! it over a storage, restart it over the same storage, close it or end its
//! session, give it the user's taps and keys, and read where the keyboard
//! focus is.
//!
//! A run is a [`Tester`]: it finds what is on screen in the window's
//! semantics tree, acts through the window's own input path, and reads the
//! render and widget trees as an agent inspects them. The application runs
//! under [`Controlled`], a root of its own that does for the rows what a host
//! application does for itself: rebuild from the root, drop the keyboard
//! focus, and say where the focus is.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui::foundation::RebuildReason;
use flui::interaction::FocusManager;
use flui::platform::ImeEvent;
use flui::platform::keyboard::{Modifiers, NamedKey};
use flui::prelude::*;
use flui::testing::{
    InspectNode, InspectValue, Locator, Match, MemoryStorage, Node, Role, TestWindow, Tester,
    TextMatch, find,
};

use crate::app::AcceptanceApp;

/// The headless window the application runs in.
pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 720;

/// Longer than any page transition, so a route change has finished.
const SETTLE_STEP: Duration = Duration::from_millis(50);
const SETTLE_STEPS: usize = 8;

/// One run of the application: the tester driving it and the handles its
/// root gives the rows.
pub struct AppRun {
    tester: Tester,
    control: Rc<Control>,
    /// What [`storage`](Self::storage) hands a run without storage.
    detached: MemoryStorage,
}

/// What [`Controlled`] hands the rows once it is mounted: its own rebuild
/// handle and the window's focus manager.
#[derive(Default)]
struct Control {
    rebuild: RefCell<Option<RebuildHandle>>,
    focus: RefCell<Option<Rc<FocusManager>>>,
}

impl Control {
    fn focus(&self) -> Rc<FocusManager> {
        Rc::clone(
            self.focus
                .borrow()
                .as_ref()
                .expect("BUG: the root takes its focus manager when it is mounted"),
        )
    }
}

/// The application under a root the rows control.
#[derive(Clone, StatefulView)]
struct Controlled {
    app: AcceptanceApp,
    control: Rc<Control>,
}

struct ControlledState {
    control: Rc<Control>,
}

impl StatefulView for Controlled {
    type State = ControlledState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        // A restart mounts a new root over the same control.
        *self.control.focus.borrow_mut() = Some(ctx.focus_manager());
        ControlledState {
            control: Rc::clone(&self.control),
        }
    }
}

impl ViewState<Controlled> for ControlledState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.control.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &Controlled, _ctx: &dyn BuildContext) -> impl IntoView {
        view.app.clone()
    }
}

impl AppRun {
    /// Start `app` over a fresh, empty storage.
    pub fn start(app: AcceptanceApp) -> Self {
        Self::start_over(app, MemoryStorage::new())
    }

    /// Start `app` over `storage`, as a launch over what earlier runs left.
    pub fn start_over(app: AcceptanceApp, storage: MemoryStorage) -> Self {
        Self::launch(app, TestWindow::new(WIDTH, HEIGHT).storage(storage))
    }

    /// Start `app` on a host with no storage at all, as a platform without
    /// files runs it. [`storage`](Self::storage) is an empty one nothing
    /// reads or writes.
    pub fn start_without_storage(app: AcceptanceApp) -> Self {
        Self::launch(app, TestWindow::new(WIDTH, HEIGHT).without_storage())
    }

    fn launch(app: AcceptanceApp, window: TestWindow) -> Self {
        let control = Rc::new(Control::default());
        let root = Controlled {
            app,
            control: Rc::clone(&control),
        };
        let mut run = Self {
            tester: Tester::mount_in(root, window),
            control,
            detached: MemoryStorage::new(),
        };
        run.settle();
        run
    }

    /// Stop this run and launch the same application over the same storage.
    /// The process is killed: what the run had not written is lost, and the
    /// old tree is gone before the new one mounts, as across a process
    /// restart.
    #[must_use]
    pub fn restart(self) -> Self {
        let Self {
            tester,
            control,
            detached,
        } = self;
        let mut run = Self {
            tester: tester.relaunch_after_kill(),
            control,
            detached,
        };
        run.settle();
        run
    }

    /// The storage this run reads and writes; an empty one nothing reads or
    /// writes for a run without storage.
    pub fn storage(&self) -> &MemoryStorage {
        self.tester.storage().unwrap_or(&self.detached)
    }

    /// The user closes the window, then the run settles.
    pub fn request_close(&mut self) {
        let _closed = self.tester.request_close();
        self.settle();
    }

    /// The user closes the window; `true` when it closed at once.
    pub fn try_close_now(&mut self) -> bool {
        self.tester.request_close()
    }

    /// The user closes the last window and the application exits: what the
    /// run holds is dropped, as the runner drops the UI runtime.
    pub fn close(mut self) {
        self.request_close();
        drop(self);
    }

    /// Whether the window has closed: at once, or once what held its close
    /// was released or decided.
    pub fn is_closed(&self) -> bool {
        self.tester.is_closed()
    }

    /// The operating system ends the session.
    pub fn end_session(&mut self) {
        self.tester.end_session();
        self.settle();
    }

    /// Let virtual time pass until loads, transitions and lazy rows are done.
    pub fn settle(&mut self) {
        for _ in 0..SETTLE_STEPS {
            self.tester.pump_for(SETTLE_STEP);
        }
        // Lazy rows build after the paint that demanded them, and lay out on
        // the frame after.
        self.tester.pump();
        self.tester.pump();
    }

    /// One frame at the current time.
    pub fn pump(&mut self) {
        self.tester.pump();
    }

    /// Rebuild the application from its root, as a parent's rebuild would,
    /// with nothing changed, then settle.
    pub fn rebuild(&mut self) {
        self.control
            .rebuild
            .borrow()
            .as_ref()
            .expect("BUG: the root takes its rebuild handle when it is mounted")
            .schedule(RebuildReason::StateChange);
        self.settle();
    }

    /// Whether `text` is on screen: its element's centre inside the window and
    /// the bounds of every element above it. A row built off screen, a covered
    /// or a departed page's text is not.
    pub fn shows(&mut self, text: &str) -> bool {
        self.any(&find::text(text))
    }

    /// Whether text starting with `prefix` is on screen, as [`Self::shows`]
    /// reads it, for a message that ends with a detail the row does not
    /// pin, such as an error's description.
    pub fn shows_starting(&mut self, prefix: &str) -> bool {
        self.any(&Locator::default().name_matching(TextMatch::prefix(prefix)))
    }

    /// Whether a laid-out paragraph holds exactly `text`, for text that is
    /// drawn but is no element of its own, such as a field's error.
    pub fn lays_out(&mut self, text: &str) -> bool {
        let reading = self.tester.inspect_render();
        let mut found = false;
        walk(&reading.roots, &mut |node| {
            found |= node.kind == "RenderParagraph"
                && matches!(field(node, "text"), Some(InspectValue::Str(laid)) if laid == text);
        });
        found
    }

    /// How many widgets of the view type named `kind` are mounted.
    pub fn count_views(&mut self, kind: &str) -> usize {
        let reading = self.tester.inspect_widgets();
        let mut count = 0;
        walk(&reading.roots, &mut |node| {
            if node.kind == kind {
                count += 1;
            }
        });
        count
    }

    /// Tap the on-screen `text`, then settle.
    ///
    /// # Panics
    ///
    /// When `text` is not on screen.
    pub fn tap(&mut self, text: &str) {
        self.tester.tap(&find::text(text));
        self.settle();
    }

    /// Type `text` into the focused field, one key per character, then
    /// settle.
    pub fn type_text(&mut self, text: &str) {
        self.type_keys(text);
        self.settle();
    }

    /// Tap the Title field, focusing it, then settle.
    ///
    /// # Panics
    ///
    /// When no Title field is on screen.
    pub fn focus_title(&mut self) {
        self.tester.tap(&title_field());
        self.settle();
    }

    /// Deliver `event` from the platform's input method, then settle.
    ///
    /// # Panics
    ///
    /// On an event the window's input path does not take: a composition
    /// whose cursor selects a range or does not fall between characters, or
    /// the session ending while a composition is shown (the platform would
    /// end it there; a row commits or cancels it first).
    pub fn ime(&mut self, event: ImeEvent) {
        match event {
            ImeEvent::Preedit { text, .. } if text.is_empty() => {
                self.tester.cancel_composition();
            }
            ImeEvent::Preedit { text, cursor } => {
                let caret = cursor.map(|(start, end)| {
                    assert_eq!(start, end, "a composition's cursor is a caret");
                    let before = text
                        .get(..start)
                        .expect("a composition's caret falls between characters");
                    u32::try_from(before.chars().count()).expect("a row's composition is short")
                });
                self.tester.compose(&text, caret);
            }
            ImeEvent::Commit(text) => self.tester.commit_composition(&text),
            ImeEvent::Disabled => assert!(
                self.title_composing().is_none(),
                "the input method's session ends with no composition shown"
            ),
            // The session opens with the composition it brings.
            ImeEvent::Enabled => {}
            other => panic!("the window's input path takes no {other:?}"),
        }
        self.settle();
    }

    /// The byte range of the Title field's composition, if one is shown.
    pub fn title_composing(&mut self) -> Option<std::ops::Range<u64>> {
        let start = self.editable("composing_start")?;
        let end = self.editable("composing_end")?;
        match (start, end) {
            (InspectValue::Uint(start), InspectValue::Uint(end)) => Some(start..end),
            _ => None,
        }
    }

    /// Whether the Title field shows its caret.
    pub fn title_shows_caret(&mut self) -> bool {
        match self.editable("show_caret") {
            Some(InspectValue::Bool(shown)) => shown,
            other => panic!("an editable reports whether it shows its caret: {other:?}"),
        }
    }

    /// Where the Title field's caret is, as a byte offset into its text.
    pub fn title_caret(&mut self) -> u64 {
        match self.editable("caret_byte_offset") {
            Some(InspectValue::Uint(caret)) => caret,
            other => panic!("an editable reports its caret: {other:?}"),
        }
    }

    /// Focus the Title field and replace its text by typing `text`.
    ///
    /// # Panics
    ///
    /// When no Title field is on screen.
    pub fn replace_title(&mut self, text: &str) {
        self.focus_title();
        self.tester.press_keys(&["Control+a", "Backspace"]);
        self.type_keys(text);
        self.settle();
    }

    /// The text the on-screen Title field holds.
    pub fn title(&mut self) -> String {
        self.tester.read(&title_field()).value.unwrap_or_default()
    }

    /// Press and release `key`, then settle.
    pub fn key(&mut self, key: NamedKey) {
        self.chord(key, Modifiers::NONE);
    }

    /// Press and release `key` with `modifiers` held, then settle.
    pub fn chord(&mut self, key: NamedKey, modifiers: Modifiers) {
        let mut chord = String::new();
        for (modifier, name) in [
            (Modifiers::CONTROL, "Control"),
            (Modifiers::ALT, "Alt"),
            (Modifiers::SHIFT, "Shift"),
            (Modifiers::META, "Meta"),
        ] {
            if modifiers.contains(modifier) {
                chord.push_str(name);
                chord.push('+');
            }
        }
        chord.push_str(key.as_str());
        self.tester.press_keys(&[&chord]);
        self.settle();
    }

    /// Tab, then settle.
    pub fn tab(&mut self) {
        self.key(NamedKey::Tab);
    }

    /// Shift+Tab, then settle.
    pub fn shift_tab(&mut self) {
        self.chord(NamedKey::Tab, Modifiers::SHIFT);
    }

    /// The Back chord of the platform-neutral conventions a headless window
    /// follows: Alt+Left.
    pub fn back_key(&mut self) {
        self.chord(NamedKey::ArrowLeft, Modifiers::ALT);
    }

    /// Hold `key` down through `repeats` auto-repeats, release it, then
    /// settle.
    pub fn hold(&mut self, key: NamedKey, repeats: usize) {
        let key = key.as_str();
        for _ in 0..=repeats {
            self.tester.key_down(key);
        }
        self.tester.key_up(key);
        self.settle();
    }

    /// The window becomes hidden (`false`) or shown again; the row settles.
    pub fn set_window_visible(&mut self, visible: bool) {
        self.tester.set_window_visible(visible);
    }

    /// The window loses (`false`) or regains its OS focus, then settles.
    pub fn set_window_focused(&mut self, focused: bool) {
        self.tester.set_window_focused(focused);
        self.settle();
    }

    /// Drop the keyboard focus, as a fresh window has none.
    pub fn unfocus(&mut self) {
        self.control.focus().unfocus();
        self.settle();
    }

    /// The name of the element the semantics tree reports focused.
    ///
    /// # Panics
    ///
    /// When the tree's focused node is not where the primary focus is: after
    /// every step the two name the same element.
    pub fn focused(&mut self) -> Option<String> {
        let tree = self.tester.semantics();
        let mut focused = None;
        walk_semantics(&tree.roots, &mut |node| {
            if node.focused && node.role != Role::Window {
                focused = Some(node.clone());
            }
        });
        let primary = self.control.focus().primary_focus();
        match (&focused, &primary) {
            (None, None) => {}
            (Some(node), Some(primary)) => {
                let rect = primary.rect();
                let (x, y) = (
                    (rect.left() + rect.right()) / 2.0,
                    (rect.top() + rect.bottom()) / 2.0,
                );
                // A headless window's device pixels are its logical pixels.
                let bounds = node
                    .surface_rect
                    .or(node.rect)
                    .expect("BUG: a focused node has bounds");
                let (left, top) = (f64::from(bounds.x), f64::from(bounds.y));
                let (right, bottom) = (
                    left + f64::from(bounds.width),
                    top + f64::from(bounds.height),
                );
                assert!(
                    (left..=right).contains(&x) && (top..=bottom).contains(&y),
                    "the semantics tree's focus {:?} is not the primary focus at {rect:?}",
                    node.name
                );
            }
            _ => panic!(
                "the semantics tree's focus ({:?}) and the primary focus ({}) disagree",
                focused.as_ref().map(|node| node.name.clone()),
                if primary.is_some() { "set" } else { "none" }
            ),
        }
        focused.and_then(|node| node.name)
    }

    /// Whether the focused element shows the focus indicator.
    pub fn shows_focus_indicator(&self) -> bool {
        self.control
            .focus()
            .primary_focus()
            .is_some_and(|node| node.has_visible_focus())
    }

    /// The notes list's scroll offset, as its viewport last laid out or was
    /// given it: the list is the only scrolling view.
    pub fn list_offset(&mut self) -> f64 {
        let reading = self.tester.inspect_render();
        let mut offsets = Vec::new();
        walk(&reading.roots, &mut |node| {
            if node.kind == "RenderViewport" {
                offsets.push(field(node, "scroll_offset"));
            }
        });
        assert_eq!(offsets.len(), 1, "one notes list is mounted");
        match offsets[0] {
            Some(InspectValue::Float(offset)) => offset,
            Some(InspectValue::Int(offset)) => offset as f64,
            Some(InspectValue::Uint(offset)) => offset as f64,
            ref other => panic!("a viewport reports its scroll offset: {other:?}"),
        }
    }

    fn type_keys(&mut self, text: &str) {
        let keys: Vec<String> = text.chars().map(String::from).collect();
        let chords: Vec<&str> = keys.iter().map(String::as_str).collect();
        if !chords.is_empty() {
            self.tester.press_keys(&chords);
        }
    }

    /// Whether an element `locator` matches has its centre inside the window
    /// and inside the bounds of every element above it, as a pointer at that
    /// centre would reach it: a row the list scrolled out of its viewport, or
    /// built in its cache, lies outside the list's bounds.
    fn any(&mut self, locator: &Locator) -> bool {
        let found: Vec<Match> = self.tester.find_all(locator);
        if found.is_empty() {
            return false;
        }
        let tree = self.tester.semantics();
        let mut shown = false;
        walk_shown(&tree.roots, None, &mut |node| {
            shown |= found.iter().any(|found| found.node.id == node.id);
        });
        shown
    }

    /// The Title field's render-object field `name`.
    fn editable(&mut self, name: &str) -> Option<InspectValue> {
        let reading = self.tester.inspect_render();
        let mut editables = Vec::new();
        walk(&reading.roots, &mut |node| {
            if node.kind == "RenderEditable" {
                editables.push(field(node, name));
            }
        });
        assert_eq!(editables.len(), 1, "one Title field on screen");
        editables.remove(0)
    }
}

/// The Title field.
fn title_field() -> Locator {
    find::role(Role::TextInput)
}

fn field(node: &InspectNode, name: &str) -> Option<InspectValue> {
    node.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.clone())
}

fn walk(nodes: &[InspectNode], visit: &mut impl FnMut(&InspectNode)) {
    for node in nodes {
        visit(node);
        walk(&node.children, visit);
    }
}

fn walk_semantics(nodes: &[Node], visit: &mut impl FnMut(&Node)) {
    for node in nodes {
        visit(node);
        walk_semantics(&node.children, visit);
    }
}

/// Left, top, right and bottom, in the window's pixels.
type Bounds = (f64, f64, f64, f64);

fn bounds(node: &Node) -> Option<Bounds> {
    let rect = node.surface_rect.or(node.rect)?;
    let (left, top) = (f64::from(rect.x), f64::from(rect.y));
    Some((
        left,
        top,
        left + f64::from(rect.width),
        top + f64::from(rect.height),
    ))
}

/// Visit each node whose centre lies inside `clip` and inside the bounds of
/// every node above it.
fn walk_shown(nodes: &[Node], clip: Option<Bounds>, visit: &mut impl FnMut(&Node)) {
    for node in nodes {
        let own = bounds(node);
        let inside = match (own, clip) {
            (Some((left, top, right, bottom)), Some((cl, ct, cr, cb))) => {
                let (x, y) = ((left + right) / 2.0, (top + bottom) / 2.0);
                (cl..=cr).contains(&x) && (ct..=cb).contains(&y)
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        if !inside {
            continue;
        }
        visit(node);
        let clip = match (own, clip) {
            (Some((left, top, right, bottom)), Some((cl, ct, cr, cb))) => {
                Some((left.max(cl), top.max(ct), right.min(cr), bottom.min(cb)))
            }
            (own, clip) => own.or(clip),
        };
        walk_shown(&node.children, clip, visit);
    }
}
