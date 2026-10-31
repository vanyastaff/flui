//! What one AccessKit node becomes in the semantics host: its element, ARIA
//! attributes, text and box, and the action a DOM event on it asks for.
//!
//! Pure Rust over [`accesskit::Node`], so the mapping is tested on the Linux
//! host; the DOM side (`dom`) only writes the difference between two
//! [`ElementSpec`]s.

use accesskit::{
    Action, ActionData, AriaCurrent, HasPopup, Live, Node, NodeId, Orientation, Point, Rect, Role,
    Toggled,
};

/// The element a node is mirrored as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    /// `<flui-node>`: an unknown tag, with no style or meaning of its own, so
    /// `role` alone says what it is.
    Node,
    /// `<a href>`, for a link with a URL: a screen reader reads the address
    /// and offers to copy or open it, which no ARIA attribute can say. A
    /// `click` on it never navigates; it is the node's action.
    Link,
    /// `<input type="range">`, for a slider a user can move: the browser
    /// adjusts it for a screen reader (VoiceOver's and TalkBack's swipes,
    /// NVDA's arrows in browse mode), which it does for no ARIA slider, and
    /// the adjusted value is the node's action.
    Range,
    /// `<input>`, for a single-line text field: a screen reader reads its
    /// value, caret and selection and edits it as a native field. While the
    /// window's text input serves the field's store, it edits the store
    /// through this element, and the store's text is its value.
    Line,
    /// `<textarea>`, for a multi-line text field, as [`Tag::Line`].
    Area,
}

impl Tag {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Node => "flui-node",
            Self::Link => "a",
            Self::Range | Self::Line => "input",
            Self::Area => "textarea",
        }
    }

    /// Whether the element is a text field the window's text input can edit
    /// through.
    pub(crate) const fn is_field(self) -> bool {
        matches!(self, Self::Line | Self::Area)
    }
}

/// Where an element sits inside its parent's element, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    /// The linear part of the node's transform, as CSS `matrix(a, b, c, d,
    /// 0, 0)` with `transform-origin: 0 0`; `None` when it is the identity.
    pub(crate) linear: Option<[f64; 4]>,
}

/// What a node's element must look like.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ElementSpec {
    pub(crate) tag: Tag,
    /// Attribute names and values, sorted by name.
    pub(crate) attributes: Vec<(&'static str, String)>,
    /// The element's own text, for a node whose name is its content: a text
    /// node before its children's elements.
    pub(crate) text: Option<String>,
    /// A [`Tag::Range`]'s or a text field's value, written to the `value`
    /// property: the attribute stops counting once the browser adjusts the
    /// input.
    pub(crate) value: Option<String>,
    pub(crate) placement: Placement,
    /// A node a platform can scroll to an offset is a scroller.
    pub(crate) scroll: Option<Scroll>,
}

/// A scroller's content and how far it is scrolled, in CSS pixels from its
/// top left corner: the node's published position less its minimum, inside
/// a content box as much larger than the element's as its extent is long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scroll {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Scroll {
    /// Whether the element's own `scrollLeft` and `scrollTop` are this
    /// scroll's, within a pixel: a browser rounds what it is given and
    /// reports whole pixels.
    pub(crate) fn is_at(&self, left: f64, top: f64) -> bool {
        (self.left - left).abs() < 1.0 && (self.top - top).abs() < 1.0
    }
}

/// The frame a node is placed in: its parent's bounds in the parent's own
/// coordinates, which the parent's element box starts at, and how far the
/// parent's element is scrolled. A scrolled node's children are published
/// where they are seen, so each is placed as far into the content as the
/// content is scrolled.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ParentFrame {
    pub(crate) origin: (f64, f64),
    pub(crate) scrolled: (f64, f64),
}

impl ParentFrame {
    /// The frame a node's children are placed in.
    pub(crate) fn of(node: &Node) -> Self {
        let origin = node
            .bounds()
            .map_or((0.0, 0.0), |bounds| (bounds.x0, bounds.y0));
        let scrolled =
            scroll(node, (0.0, 0.0)).map_or((0.0, 0.0), |scroll| (scroll.left, scroll.top));
        Self { origin, scrolled }
    }
}

/// One axis of a node's published scroll: where it is from the minimum, and
/// how long the extent is; `None` without a finite, ordered extent.
fn scroll_axis(position: Option<f64>, min: Option<f64>, max: Option<f64>) -> Option<(f64, f64)> {
    let (min, max) = (min?, max?);
    let length = max - min;
    if !(min.is_finite() && length.is_finite() && length >= 0.0) {
        return None;
    }
    let at = position
        .filter(|position| position.is_finite())
        .map_or(0.0, |position| position - min);
    Some((at.clamp(0.0, length), length))
}

/// The scroll of a node's element of `size`, for a node a platform can
/// scroll to an offset along an axis with a finite extent. The position
/// slider of a scrollable publishes the same range and takes no offset, so
/// it stays a slider.
fn scroll(node: &Node, size: (f64, f64)) -> Option<Scroll> {
    if !node.supports_action(Action::SetScrollOffset) {
        return None;
    }
    let x = scroll_axis(node.scroll_x(), node.scroll_x_min(), node.scroll_x_max());
    let y = scroll_axis(node.scroll_y(), node.scroll_y_min(), node.scroll_y_max());
    if x.is_none() && y.is_none() {
        return None;
    }
    let (left, wide) = x.unwrap_or((0.0, 0.0));
    let (top, tall) = y.unwrap_or((0.0, 0.0));
    Some(Scroll {
        left,
        top,
        width: size.0 + wide,
        height: size.1 + tall,
    })
}

/// The offset a scroller's element scrolled to by the browser asks for, in
/// the order the node publishes its position: its minimum plus how far the
/// element is scrolled, on each axis the node scrolls along, and its
/// published position on the other.
pub(crate) fn scroll_offset(node: &Node, left: f64, top: f64) -> Option<Point> {
    scroll(node, (0.0, 0.0))?;
    let along = |position: Option<f64>, min: Option<f64>, max: Option<f64>, scrolled: f64| {
        let finite = |value: &f64| value.is_finite();
        match scroll_axis(position, min, max) {
            Some((_, length)) => min.map(|min| min + scrolled.clamp(0.0, length)),
            None => position.filter(finite),
        }
        .filter(finite)
        .unwrap_or(0.0)
    };
    Some(Point::new(
        along(
            node.scroll_x(),
            node.scroll_x_min(),
            node.scroll_x_max(),
            left,
        ),
        along(
            node.scroll_y(),
            node.scroll_y_min(),
            node.scroll_y_max(),
            top,
        ),
    ))
}

/// The DOM `id` each mirrored element gets, unique on the page: a scope per
/// semantics host, then the node's identifier. A relation between nodes
/// (`aria-labelledby`, `aria-controls`, …) names its targets by these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ElementIds {
    scope: String,
}

impl ElementIds {
    pub(crate) fn new(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
        }
    }

    /// The `id` of `node`'s element.
    pub(crate) fn of(&self, node: NodeId) -> String {
        format!("{}-{}", self.scope, node.0)
    }

    /// An IDREF list: the `id`s of `nodes`' elements, space-separated.
    fn list(&self, nodes: &[NodeId]) -> String {
        let ids: Vec<String> = nodes.iter().map(|&node| self.of(node)).collect();
        ids.join(" ")
    }
}

/// Whether a node is mirrored at all. A `TextRun` is the text of its parent's
/// element, never an element of its own.
pub(crate) fn is_mirrored(node: &Node) -> bool {
    node.role() != Role::TextRun
}

/// The element for node `id` placed in `parent`. `root` drops the node's own
/// transform: the root carries the device pixel ratio (`Affine::scale`),
/// and CSS pixels are already logical.
pub(crate) fn describe(
    id: NodeId,
    node: &Node,
    parent: ParentFrame,
    root: bool,
    ids: &ElementIds,
) -> ElementSpec {
    let role = aria_role(node.role());
    let field = text_field(node);
    let mut attributes = vec![("id", ids.of(id))];
    // A text field's own element says what it is.
    if let Some(role) = role.filter(|_| field.is_none()) {
        attributes.push(("role", role.to_owned()));
    }
    // Static text is its content, children or not (a labelled `Semantics`
    // wrapper is a `Label` too, and a generic element can carry no
    // `aria-label`), and its whole text is its value: the semantics
    // translation joins a label and a value there. Other roles named by their
    // content are when they are leaves.
    let static_text = node.role() == Role::Label;
    let name_is_content =
        static_text || (names_from_content(node.role()) && node.children().is_empty());
    let text = if static_text {
        node.value().or_else(|| node.label()).map(str::to_owned)
    } else if name_is_content {
        node.label().or_else(|| node.value()).map(str::to_owned)
    } else {
        None
    };
    if !name_is_content
        && role.is_some()
        && let Some(label) = node.label()
    {
        attributes.push(("aria-label", label.to_owned()));
    }
    if let Some(description) = node.description() {
        attributes.push(("aria-description", description.to_owned()));
    }
    if let Some(role_description) = node.role_description() {
        attributes.push(("aria-roledescription", role_description.to_owned()));
    }
    if let Some(author_id) = node.author_id() {
        attributes.push(("data-flui-id", author_id.to_owned()));
    }
    states(node, role, &mut attributes);
    relations(node, ids, &mut attributes);
    let slider = movable_slider(node);
    let tag = if slider.is_some() {
        Tag::Range
    } else if let Some((tag, _)) = field {
        tag
    } else if node.role() == Role::Link && node.url().is_some() {
        Tag::Link
    } else {
        Tag::Node
    };
    let mut value = None;
    match (tag, slider) {
        (Tag::Range, Some(slider)) => {
            attributes.extend([
                ("type", "range".to_owned()),
                ("min", slider.min.to_string()),
                ("max", slider.max.to_string()),
                (
                    "step",
                    slider
                        .step
                        .map_or_else(|| "any".to_owned(), |step| step.to_string()),
                ),
            ]);
            if let Some(text) = node.value() {
                attributes.push(("aria-valuetext", text.to_owned()));
            }
            value = Some(slider.value.to_string());
        }
        (Tag::Link, _) => {
            attributes.extend(node.url().map(|url| ("href", url.to_owned())));
            range(node, &mut attributes);
        }
        (Tag::Line | Tag::Area, _) => {
            attributes.extend(
                field
                    .and_then(|(_, kind)| kind)
                    .map(|kind| ("type", kind.to_owned())),
            );
            if node.is_read_only() {
                attributes.push(("readonly", String::new()));
            }
            value = Some(node.value().unwrap_or_default().to_owned());
        }
        _ => range(node, &mut attributes),
    }
    live(node, &mut attributes);
    // Focusable by script and by assistive technology, never by the browser's
    // Tab order: FLUI's focus traversal moves it. A link and an input are in
    // the Tab order by themselves.
    if tag != Tag::Node || node.supports_action(Action::Focus) {
        attributes.push(("tabindex", "-1".to_owned()));
    }
    attributes.sort_by_key(|&(name, _)| name);
    let placement = placement(node, parent, root);
    ElementSpec {
        tag,
        attributes,
        // A text field's text would be its default value.
        text: text.filter(|_| tag == Tag::Node || tag == Tag::Link),
        value,
        placement,
        scroll: scroll(node, (placement.width, placement.height)),
    }
}

/// How far from a whole number of steps a value may be and still be on its
/// slider's step, relative to the count: the division rounds.
const STEP_TOLERANCE: f64 = 1e-9;

/// The element a text field is mirrored as, and an `<input>`'s `type`: one
/// whose caret and selection a script can set, so the window's text input can
/// edit through it (an `email` or `number` input has neither), and that keeps
/// the value it is given (a `url` input strips the whitespace around it, which
/// the store holds; the editor's `inputmode` and `autocomplete` still say it
/// is an address). A field of dates or times, a combo box and a spin button
/// stay ARIA ones, served on the canvas.
fn text_field(node: &Node) -> Option<(Tag, Option<&'static str>)> {
    let kind = match node.role() {
        Role::MultilineTextInput => return Some((Tag::Area, None)),
        Role::TextInput | Role::EmailInput | Role::NumberInput | Role::UrlInput => "text",
        Role::SearchInput => "search",
        Role::PasswordInput => "password",
        Role::PhoneNumberInput => "tel",
        _ => return None,
    };
    Some((Tag::Line, Some(kind)))
}

/// A slider's range as an `<input type="range">` holds it; `step` is `None`
/// where the input takes any value.
struct Slider {
    value: f64,
    min: f64,
    max: f64,
    step: Option<f64>,
}

/// The range of a slider a user can move: a leaf whose node takes a value or
/// a step, with a finite value inside a finite, ordered range. Any other
/// slider stays an ARIA one, read but not adjusted.
fn movable_slider(node: &Node) -> Option<Slider> {
    let movable = [Action::SetValue, Action::Increment, Action::Decrement]
        .into_iter()
        .any(|action| node.supports_action(action));
    if node.role() != Role::Slider || !node.children().is_empty() || !movable || node.is_read_only()
    {
        return None;
    }
    let finite = |value: Option<f64>| value.filter(|value| value.is_finite());
    let (min, max) = (
        finite(node.min_numeric_value())?,
        finite(node.max_numeric_value())?,
    );
    let value = finite(node.numeric_value())?;
    if min > max {
        return None;
    }
    let value = value.clamp(min, max);
    // A range input snaps its value to its step: a value or a maximum the step
    // does not reach from the minimum would be shown as another, or never
    // reached, so the input takes any.
    let on_step = |step: f64, at: f64| {
        let steps = (at - min) / step;
        (steps - steps.round()).abs() <= STEP_TOLERANCE * steps.abs().max(1.0)
    };
    let step = finite(node.numeric_value_step())
        .filter(|step| *step > 0.0)
        .filter(|&step| on_step(step, value) && on_step(step, max));
    Some(Slider {
        value,
        min,
        max,
        step,
    })
}

/// What a slider's input moved to `value` by the browser asks for: the value
/// itself where the node takes one, otherwise a step towards it.
pub(crate) fn slider_request(node: &Node, value: f64) -> Option<(Action, Option<ActionData>)> {
    if !value.is_finite() {
        return None;
    }
    if node.supports_action(Action::SetValue) {
        return Some((Action::SetValue, Some(ActionData::NumericValue(value))));
    }
    let current = node.numeric_value()?;
    let action = if value > current {
        Action::Increment
    } else if value < current {
        Action::Decrement
    } else {
        return None;
    };
    node.supports_action(action).then_some((action, None))
}

/// The node's relations, as IDREF lists of the targets' elements.
fn relations(node: &Node, ids: &ElementIds, attributes: &mut Vec<(&'static str, String)>) {
    // A node named directly is named by that, as AccessKit's own name reads
    // it: `aria-labelledby` would win over `aria-label` in the browser.
    let labelled_by = if node.label().is_none() {
        node.labelled_by()
    } else {
        &[]
    };
    for (name, targets) in [
        ("aria-labelledby", labelled_by),
        ("aria-describedby", node.described_by()),
        ("aria-controls", node.controls()),
        ("aria-details", node.details()),
        ("aria-flowto", node.flow_to()),
    ] {
        if !targets.is_empty() {
            attributes.push((name, ids.list(targets)));
        }
    }
}

/// Whether `node` takes text from the keyboard or an input method. One
/// mirrored as a text field ([`Tag::is_field`]) is edited through its
/// element; the window's text input serves any other on the canvas (an
/// `EditContext` or the hidden editor), so the page's focus stays there
/// rather than on the node's element, which has no input surface.
pub(crate) fn takes_text(node: &Node) -> bool {
    matches!(
        node.role(),
        Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::DateInput
            | Role::DateTimeInput
            | Role::WeekInput
            | Role::MonthInput
            | Role::TimeInput
            | Role::EmailInput
            | Role::NumberInput
            | Role::PasswordInput
            | Role::PhoneNumberInput
            | Role::UrlInput
            | Role::EditableComboBox
            | Role::SpinButton
    )
}

/// The action a DOM event of `kind` on `node`'s element asks for: `click`,
/// `focusin` or `contextmenu`.
pub(crate) fn event_action(kind: &str, node: &Node) -> Option<Action> {
    let supported = |action| node.supports_action(action).then_some(action);
    match kind {
        "click" => click_action(node),
        "focusin" => supported(Action::Focus),
        "contextmenu" => supported(Action::ShowContextMenu),
        _ => None,
    }
}

/// The action a `click` on `node`'s element asks for.
fn click_action(node: &Node) -> Option<Action> {
    if node.supports_action(Action::Click) {
        Some(Action::Click)
    } else if node.supports_action(Action::Expand) && node.is_expanded() != Some(true) {
        Some(Action::Expand)
    } else if node.supports_action(Action::Collapse) {
        Some(Action::Collapse)
    } else {
        None
    }
}

fn states(node: &Node, role: Option<&str>, attributes: &mut Vec<(&'static str, String)>) {
    let flag = |value: bool| if value { "true" } else { "false" }.to_owned();
    if node.is_disabled() {
        attributes.push(("aria-disabled", flag(true)));
    }
    if node.is_hidden() {
        attributes.push(("aria-hidden", flag(true)));
    }
    if node.is_required() {
        attributes.push(("aria-required", flag(true)));
    }
    if node.is_read_only() {
        attributes.push(("aria-readonly", flag(true)));
    }
    if node.is_modal() {
        attributes.push(("aria-modal", flag(true)));
    }
    if let Some(toggled) = node.toggled() {
        let value = match toggled {
            Toggled::False => "false",
            Toggled::True => "true",
            Toggled::Mixed => "mixed",
        };
        // A toggled button is a toggle button; the others check.
        let name = if role == Some("button") {
            "aria-pressed"
        } else {
            "aria-checked"
        };
        attributes.push((name, value.to_owned()));
    }
    if let Some(expanded) = node.is_expanded() {
        attributes.push(("aria-expanded", flag(expanded)));
    }
    if let Some(selected) = node.is_selected() {
        attributes.push(("aria-selected", flag(selected)));
    }
    // AccessKit counts levels and positions from zero (the semantics
    // translation subtracts one), ARIA from one.
    if let Some(level) = node.level() {
        attributes.push(("aria-level", level.saturating_add(1).to_string()));
    }
    if let Some(size) = node.size_of_set() {
        attributes.push(("aria-setsize", size.to_string()));
    }
    if let Some(position) = node.position_in_set() {
        attributes.push(("aria-posinset", position.saturating_add(1).to_string()));
    }
    if let Some(popup) = node.has_popup() {
        let value = match popup {
            HasPopup::Menu => "menu",
            HasPopup::Listbox => "listbox",
            HasPopup::Tree => "tree",
            HasPopup::Grid => "grid",
            HasPopup::Dialog => "dialog",
        };
        attributes.push(("aria-haspopup", value.to_owned()));
    }
    if let Some(current) = node.aria_current() {
        let value = match current {
            AriaCurrent::False => "false",
            AriaCurrent::True => "true",
            AriaCurrent::Page => "page",
            AriaCurrent::Step => "step",
            AriaCurrent::Location => "location",
            AriaCurrent::Date => "date",
            AriaCurrent::Time => "time",
        };
        attributes.push(("aria-current", value.to_owned()));
    }
    if let Some(orientation) = node.orientation() {
        let value = match orientation {
            Orientation::Horizontal => "horizontal",
            Orientation::Vertical => "vertical",
        };
        attributes.push(("aria-orientation", value.to_owned()));
    }
}

fn range(node: &Node, attributes: &mut Vec<(&'static str, String)>) {
    if !matches!(
        node.role(),
        Role::Slider | Role::ProgressIndicator | Role::Meter | Role::ScrollBar | Role::SpinButton
    ) {
        return;
    }
    for (name, value) in [
        ("aria-valuenow", node.numeric_value()),
        ("aria-valuemin", node.min_numeric_value()),
        ("aria-valuemax", node.max_numeric_value()),
    ] {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            attributes.push((name, value.to_string()));
        }
    }
    if let Some(text) = node.value() {
        attributes.push(("aria-valuetext", text.to_owned()));
    }
}

fn live(node: &Node, attributes: &mut Vec<(&'static str, String)>) {
    if let Some(live) = node.live() {
        let value = match live {
            Live::Off => "off",
            Live::Polite => "polite",
            Live::Assertive => "assertive",
        };
        attributes.push(("aria-live", value.to_owned()));
        if node.is_live_atomic() {
            attributes.push(("aria-atomic", "true".to_owned()));
        }
    }
    if node.is_busy() {
        attributes.push(("aria-busy", "true".to_owned()));
    }
}

/// The node's box in its parent element. A node's transform maps its own
/// coordinates into its parent's, `D(x) = A·x + t`; the element's box starts
/// at the node's bounds origin `b`, the parent's at `p`. With
/// `transform-origin: 0 0` the element is at `A·b + t − p`, transformed by
/// `A` alone, and as far into a scrolled parent's content `s` as it is
/// scrolled: `A·b + t − p + s`.
fn placement(node: &Node, parent: ParentFrame, root: bool) -> Placement {
    let bounds = node.bounds().unwrap_or(Rect::ZERO);
    let coefficients = if root {
        None
    } else {
        node.transform().map(|transform| transform.as_coeffs())
    };
    let [xx, yx, xy, yy, dx, dy] = coefficients.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let (x0, y0) = (bounds.x0, bounds.y0);
    let left = xx * x0 + xy * y0 + dx - parent.origin.0 + parent.scrolled.0;
    let top = yx * x0 + yy * y0 + dy - parent.origin.1 + parent.scrolled.1;
    let linear = [xx, yx, xy, yy];
    Placement {
        left: finite_or_zero(left),
        top: finite_or_zero(top),
        width: finite_or_zero(bounds.width()).max(0.0),
        height: finite_or_zero(bounds.height()).max(0.0),
        linear: (linear != [1.0, 0.0, 0.0, 1.0] && linear.iter().all(|value| value.is_finite()))
            .then_some(linear),
    }
}

/// A non-finite coordinate is never written into a style: the element stays
/// at its parent's origin instead.
fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

/// Roles whose accessible name is their text content when they have no
/// children of their own.
fn names_from_content(role: Role) -> bool {
    matches!(
        role,
        Role::Label
            | Role::Paragraph
            | Role::Heading
            | Role::Button
            | Role::DefaultButton
            | Role::Link
            | Role::Cell
            | Role::GridCell
            | Role::ColumnHeader
            | Role::RowHeader
            | Role::ListItem
            | Role::ListBoxOption
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::Tab
            | Role::TreeItem
            | Role::Tooltip
            | Role::Caption
            | Role::Strong
            | Role::Emphasis
            | Role::Code
            | Role::Time
    )
}

/// The ARIA role for an AccessKit role; `None` where the element needs no
/// role (text, generic containers) or ARIA has none.
pub(crate) const fn aria_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Button | Role::DefaultButton => "button",
        Role::CheckBox => "checkbox",
        Role::RadioButton => "radio",
        Role::Switch => "switch",
        Role::Link => "link",
        Role::Heading => "heading",
        Role::Paragraph => "paragraph",
        Role::Image => "img",
        Role::List => "list",
        Role::ListItem => "listitem",
        Role::ListBox => "listbox",
        Role::ListBoxOption | Role::MenuListOption => "option",
        Role::Menu | Role::MenuListPopup => "menu",
        Role::MenuBar => "menubar",
        Role::MenuItem => "menuitem",
        Role::MenuItemCheckBox => "menuitemcheckbox",
        Role::MenuItemRadio => "menuitemradio",
        Role::Tab => "tab",
        Role::TabList => "tablist",
        Role::TabPanel => "tabpanel",
        Role::Dialog => "dialog",
        Role::AlertDialog => "alertdialog",
        Role::Alert => "alert",
        Role::Status => "status",
        Role::Log => "log",
        Role::Marquee => "marquee",
        Role::Timer => "timer",
        Role::Slider => "slider",
        Role::ProgressIndicator => "progressbar",
        Role::Meter => "meter",
        Role::ScrollBar => "scrollbar",
        Role::SpinButton => "spinbutton",
        Role::ComboBox | Role::EditableComboBox => "combobox",
        Role::TextInput
        | Role::MultilineTextInput
        | Role::DateInput
        | Role::DateTimeInput
        | Role::WeekInput
        | Role::MonthInput
        | Role::TimeInput
        | Role::EmailInput
        | Role::NumberInput
        | Role::PasswordInput
        | Role::PhoneNumberInput
        | Role::UrlInput => "textbox",
        Role::SearchInput => "searchbox",
        Role::Table | Role::ListGrid => "table",
        Role::Row => "row",
        Role::RowGroup => "rowgroup",
        Role::Cell => "cell",
        Role::GridCell => "gridcell",
        Role::ColumnHeader => "columnheader",
        Role::RowHeader => "rowheader",
        Role::Grid => "grid",
        Role::TreeGrid => "treegrid",
        Role::Tree => "tree",
        Role::TreeItem => "treeitem",
        Role::Group | Role::Pane | Role::Details => "group",
        Role::RadioGroup => "radiogroup",
        Role::Toolbar => "toolbar",
        Role::Tooltip => "tooltip",
        Role::Navigation => "navigation",
        Role::Main => "main",
        Role::Banner | Role::Header => "banner",
        Role::ContentInfo | Role::Footer => "contentinfo",
        Role::Complementary => "complementary",
        Role::Region | Role::Section => "region",
        Role::Form => "form",
        Role::Search => "search",
        Role::Article => "article",
        Role::Document | Role::Terminal => "document",
        Role::Application => "application",
        Role::Feed => "feed",
        Role::Figure => "figure",
        Role::Note => "note",
        Role::Definition => "definition",
        Role::Term => "term",
        Role::Math => "math",
        Role::Splitter => "separator",
        Role::Blockquote => "blockquote",
        Role::Caption | Role::FigureCaption => "caption",
        Role::Code => "code",
        Role::Strong => "strong",
        Role::Emphasis => "emphasis",
        Role::Time => "time",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, Affine, Node, NodeId, Rect, Role, Toggled};

    use super::{ElementIds, ParentFrame, Scroll, describe, takes_text};

    fn attribute<'a>(spec: &'a super::ElementSpec, name: &str) -> Option<&'a str> {
        spec.attributes
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Each node shape against the element, attributes and text it is
    /// mirrored as.
    #[test]
    fn a_node_is_described_as_its_element() {
        struct Case {
            name: &'static str,
            node: Node,
            expect: &'static [(&'static str, Option<&'static str>)],
            text: Option<&'static str>,
        }
        let button = {
            let mut node = Node::new(Role::Button);
            node.set_label("Increment");
            node.set_author_id("increment");
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
            node
        };
        let toggle_button = {
            let mut node = Node::new(Role::Button);
            node.set_label("Bold");
            node.set_toggled(Toggled::True);
            node
        };
        let checkbox = {
            let mut node = Node::new(Role::CheckBox);
            node.set_label("Agree");
            node.set_toggled(Toggled::Mixed);
            node.set_disabled();
            node
        };
        let static_text = {
            let mut node = Node::new(Role::Label);
            node.set_value("Count 0");
            node
        };
        let labelled_value = {
            let mut node = Node::new(Role::Label);
            node.set_label("Total");
            node.set_value("Total\n42");
            node
        };
        let labelled_wrapper = {
            let mut node = Node::new(Role::Label);
            node.set_label("Shipping");
            node.set_value("Shipping");
            node.push_child(accesskit::NodeId(9));
            node
        };
        let container = {
            let mut node = Node::new(Role::GenericContainer);
            node.set_label("dropped");
            node
        };
        let heading = {
            let mut node = Node::new(Role::Heading);
            node.set_label("Settings");
            node.set_level(1);
            node
        };
        let slider = {
            let mut node = Node::new(Role::Slider);
            node.set_label("Volume");
            node.set_numeric_value(30.0);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(f64::NAN);
            node.set_value("30%");
            node
        };
        let item = {
            let mut node = Node::new(Role::ListItem);
            node.set_label("Second");
            node.set_size_of_set(3);
            node.set_position_in_set(1);
            node.set_selected(true);
            node
        };
        let group_with_child = {
            let mut node = Node::new(Role::Group);
            node.set_label("Options");
            node.push_child(accesskit::NodeId(9));
            node
        };
        let labelled_check_box = {
            let mut node = Node::new(Role::CheckBox);
            node.set_labelled_by(vec![NodeId(3), NodeId(4)]);
            node.set_described_by(vec![NodeId(5)]);
            node.set_controls(vec![NodeId(6)]);
            node
        };
        let named_and_labelled = {
            let mut node = Node::new(Role::CheckBox);
            node.set_label("Remember me");
            node.set_labelled_by(vec![NodeId(3)]);
            node
        };
        let cases = [
            Case {
                name: "a button is named by its text and found by its identifier",
                node: button,
                expect: &[
                    ("role", Some("button")),
                    ("data-flui-id", Some("increment")),
                    ("tabindex", Some("-1")),
                    ("aria-label", None),
                ],
                text: Some("Increment"),
            },
            Case {
                name: "a toggled button is pressed, not checked",
                node: toggle_button,
                expect: &[("aria-pressed", Some("true")), ("aria-checked", None)],
                text: Some("Bold"),
            },
            Case {
                name: "a mixed, disabled checkbox",
                node: checkbox,
                expect: &[
                    ("role", Some("checkbox")),
                    ("aria-label", Some("Agree")),
                    ("aria-checked", Some("mixed")),
                    ("aria-disabled", Some("true")),
                    ("tabindex", None),
                ],
                text: None,
            },
            Case {
                name: "static text is its own content, with no role",
                node: static_text,
                expect: &[("role", None), ("aria-label", None)],
                text: Some("Count 0"),
            },
            Case {
                name: "static text reads its whole published value, label included",
                node: labelled_value,
                expect: &[("role", None), ("aria-label", None)],
                text: Some("Total\n42"),
            },
            Case {
                name: "a labelled wrapper keeps its text beside its children",
                node: labelled_wrapper,
                expect: &[("role", None), ("aria-label", None)],
                text: Some("Shipping"),
            },
            Case {
                name: "a generic container takes no prohibited name",
                node: container,
                expect: &[("role", None), ("aria-label", None)],
                text: None,
            },
            Case {
                name: "a heading's zero-based level is one-based in ARIA",
                node: heading,
                expect: &[("role", Some("heading")), ("aria-level", Some("2"))],
                text: Some("Settings"),
            },
            Case {
                name: "a slider publishes its finite range values and value text",
                node: slider,
                expect: &[
                    ("aria-valuenow", Some("30")),
                    ("aria-valuemin", Some("0")),
                    ("aria-valuemax", None),
                    ("aria-valuetext", Some("30%")),
                ],
                text: None,
            },
            Case {
                name: "a list item says where it is in its set, counted from one",
                node: item,
                expect: &[
                    ("aria-setsize", Some("3")),
                    ("aria-posinset", Some("2")),
                    ("aria-selected", Some("true")),
                ],
                text: Some("Second"),
            },
            Case {
                name: "a node with children is named by its label, not its content",
                node: group_with_child,
                expect: &[("role", Some("group")), ("aria-label", Some("Options"))],
                text: None,
            },
            Case {
                name: "a node labelled by others names its targets' elements",
                node: labelled_check_box,
                expect: &[
                    ("role", Some("checkbox")),
                    ("aria-label", None),
                    ("aria-labelledby", Some("t-3 t-4")),
                    ("aria-describedby", Some("t-5")),
                    ("aria-controls", Some("t-6")),
                ],
                text: None,
            },
            Case {
                name: "a node named directly keeps that name over its labelling nodes",
                node: named_and_labelled,
                expect: &[
                    ("aria-label", Some("Remember me")),
                    ("aria-labelledby", None),
                ],
                text: None,
            },
        ];
        for case in cases {
            let spec = describe(
                NodeId(7),
                &case.node,
                ParentFrame::default(),
                false,
                &ElementIds::new("t"),
            );
            for &(name, value) in case.expect {
                assert_eq!(attribute(&spec, name), value, "{}: {name}", case.name);
            }
            assert_eq!(spec.text.as_deref(), case.text, "{}: text", case.name);
            assert_eq!(spec.tag.name(), "flui-node", "{}: tag", case.name);
            assert_eq!(attribute(&spec, "id"), Some("t-7"), "{}: id", case.name);
            assert!(
                spec.attributes.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "{}: attributes are sorted and unique: {:?}",
                case.name,
                spec.attributes
            );
        }
    }

    /// The box an element gets inside its parent's, for the transforms a tree
    /// carries.
    #[test]
    fn an_element_is_placed_in_its_parent_s_box() {
        struct Case {
            name: &'static str,
            bounds: Rect,
            transform: Option<Affine>,
            parent: (f64, f64),
            scrolled: (f64, f64),
            root: bool,
            expect: (f64, f64, f64, f64, Option<[f64; 4]>),
        }
        let cases = [
            Case {
                name: "bounds in the parent's space, offset by the parent's origin",
                bounds: Rect::new(30.0, 40.0, 130.0, 60.0),
                transform: None,
                parent: (10.0, 10.0),
                scrolled: (0.0, 0.0),
                root: false,
                expect: (20.0, 30.0, 100.0, 20.0, None),
            },
            Case {
                name: "a translation moves the box and leaves no CSS transform",
                bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
                transform: Some(Affine::translate((5.0, -7.0))),
                parent: (0.0, 0.0),
                scrolled: (0.0, 0.0),
                root: false,
                expect: (5.0, -7.0, 50.0, 50.0, None),
            },
            Case {
                name: "the root's device-pixel scale is dropped",
                bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
                transform: Some(Affine::scale(2.0)),
                parent: (0.0, 0.0),
                scrolled: (0.0, 0.0),
                root: true,
                expect: (0.0, 0.0, 800.0, 600.0, None),
            },
            Case {
                name: "a scale below the root becomes a CSS matrix at the scaled origin",
                bounds: Rect::new(10.0, 0.0, 20.0, 10.0),
                transform: Some(Affine::scale(2.0)),
                parent: (0.0, 0.0),
                scrolled: (0.0, 0.0),
                root: false,
                expect: (20.0, 0.0, 10.0, 10.0, Some([2.0, 0.0, 0.0, 2.0])),
            },
            Case {
                name: "a scrolled parent's child is as far into its content as it is scrolled",
                bounds: Rect::new(0.0, -30.0, 100.0, 10.0),
                transform: None,
                parent: (0.0, 0.0),
                scrolled: (0.0, 120.0),
                root: false,
                expect: (0.0, 90.0, 100.0, 40.0, None),
            },
            Case {
                name: "non-finite geometry is never published",
                bounds: Rect::new(f64::NAN, 0.0, f64::INFINITY, 10.0),
                transform: None,
                parent: (0.0, 0.0),
                scrolled: (0.0, 0.0),
                root: false,
                expect: (0.0, 0.0, 0.0, 10.0, None),
            },
        ];
        for case in cases {
            let mut node = Node::new(Role::GenericContainer);
            node.set_bounds(case.bounds);
            if let Some(transform) = case.transform {
                node.set_transform(transform);
            }
            let spec = describe(
                NodeId(7),
                &node,
                ParentFrame {
                    origin: case.parent,
                    scrolled: case.scrolled,
                },
                case.root,
                &ElementIds::new("t"),
            );
            let placement = spec.placement;
            assert_eq!(
                (
                    placement.left,
                    placement.top,
                    placement.width,
                    placement.height,
                    placement.linear
                ),
                case.expect,
                "{}",
                case.name
            );
        }
    }

    /// A link with a URL, a slider a user can move and a scroller are
    /// mirrored as the elements the browser adjusts and reads for them.
    #[test]
    fn a_node_the_browser_must_act_on_is_its_native_element() {
        let link = {
            let mut node = Node::new(Role::Link);
            node.set_label("Docs");
            node.set_url("https://example.com/docs");
            node.add_action(Action::Click);
            node
        };
        let slider = |read_only: bool| {
            let mut node = Node::new(Role::Slider);
            node.set_label("Volume");
            node.set_numeric_value(30.0);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(100.0);
            node.set_numeric_value_step(5.0);
            node.set_value("30%");
            node.add_action(Action::SetValue);
            if read_only {
                node.set_read_only();
            }
            node
        };
        let describe = |node: &Node| {
            describe(
                NodeId(7),
                node,
                ParentFrame::default(),
                false,
                &ElementIds::new("t"),
            )
        };

        let anchor = describe(&link);
        assert_eq!(anchor.tag.name(), "a");
        assert_eq!(attribute(&anchor, "href"), Some("https://example.com/docs"));
        assert_eq!(
            attribute(&anchor, "tabindex"),
            Some("-1"),
            "never in the Tab order"
        );
        assert_eq!(anchor.text.as_deref(), Some("Docs"));
        let mut without_url = link;
        without_url.clear_url();
        assert_eq!(describe(&without_url).tag.name(), "flui-node");

        let input = describe(&slider(false));
        assert_eq!(input.tag.name(), "input");
        for (name, value) in [
            ("type", Some("range")),
            ("min", Some("0")),
            ("max", Some("100")),
            ("step", Some("5")),
            ("aria-valuetext", Some("30%")),
            ("aria-label", Some("Volume")),
            ("aria-valuenow", None),
            ("tabindex", Some("-1")),
        ] {
            assert_eq!(attribute(&input, name), value, "range input: {name}");
        }
        assert_eq!(input.value.as_deref(), Some("30"));
        let mut off_step = slider(false);
        off_step.set_numeric_value(32.0);
        let input = describe(&off_step);
        assert_eq!(
            attribute(&input, "step"),
            Some("any"),
            "a value off the step is not snapped"
        );
        assert_eq!(input.value.as_deref(), Some("32"));
        let mut short_range = slider(false);
        short_range.set_max_numeric_value(32.0);
        assert_eq!(
            attribute(&describe(&short_range), "step"),
            Some("any"),
            "a maximum off the step stays reachable"
        );
        let read_only = describe(&slider(true));
        assert_eq!(
            read_only.tag.name(),
            "flui-node",
            "a slider nobody moves is read"
        );
        assert_eq!(attribute(&read_only, "aria-valuenow"), Some("30"));

        let mut list = Node::new(Role::ScrollView);
        list.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
        list.set_scroll_y(150.0);
        list.set_scroll_y_min(50.0);
        list.set_scroll_y_max(450.0);
        assert_eq!(
            describe(&list).scroll,
            None,
            "a scroll nothing can set is placed only"
        );
        list.add_action(Action::SetScrollOffset);
        assert_eq!(
            describe(&list).scroll,
            Some(Scroll {
                left: 0.0,
                top: 100.0,
                width: 200.0,
                height: 500.0
            })
        );
    }

    /// A text field is the native field of its kind, named and valued as
    /// published, with no ARIA role and no text of its own; one whose caret
    /// a script cannot set, or a field of dates, stays an ARIA textbox.
    #[test]
    fn a_text_field_is_a_native_field_of_its_kind() {
        let field = |role: Role| {
            let mut node = Node::new(role);
            node.set_label("Name");
            node.set_value("Ada");
            node.add_action(Action::Focus);
            describe(
                NodeId(7),
                &node,
                ParentFrame::default(),
                false,
                &ElementIds::new("t"),
            )
        };
        for (role, tag, kind) in [
            (Role::TextInput, "input", Some("text")),
            (Role::EmailInput, "input", Some("text")),
            (Role::NumberInput, "input", Some("text")),
            (Role::SearchInput, "input", Some("search")),
            (Role::PasswordInput, "input", Some("password")),
            (Role::PhoneNumberInput, "input", Some("tel")),
            (Role::UrlInput, "input", Some("text")),
            (Role::MultilineTextInput, "textarea", None),
        ] {
            let spec = field(role);
            assert_eq!(spec.tag.name(), tag, "{role:?}");
            assert!(spec.tag.is_field(), "{role:?}");
            assert_eq!(attribute(&spec, "type"), kind, "{role:?}");
            assert_eq!(
                attribute(&spec, "role"),
                None,
                "{role:?}: the element says it"
            );
            assert_eq!(attribute(&spec, "aria-label"), Some("Name"), "{role:?}");
            assert_eq!(attribute(&spec, "tabindex"), Some("-1"), "{role:?}");
            assert_eq!(spec.value.as_deref(), Some("Ada"), "{role:?}");
            assert_eq!(spec.text, None, "{role:?}: no default value");
        }
        for role in [Role::DateInput, Role::SpinButton, Role::EditableComboBox] {
            let spec = field(role);
            assert_eq!(spec.tag, super::Tag::Node, "{role:?}");
            assert!(!spec.tag.is_field(), "{role:?}");
        }

        let mut read_only = Node::new(Role::TextInput);
        read_only.set_read_only();
        let spec = describe(
            NodeId(7),
            &read_only,
            ParentFrame::default(),
            false,
            &ElementIds::new("t"),
        );
        assert_eq!(attribute(&spec, "readonly"), Some(""));
        assert_eq!(spec.value.as_deref(), Some(""), "an empty field is emptied");
    }

    /// What the browser moving a range input or scrolling a scroller asks
    /// the node for.
    #[test]
    fn the_browser_adjusting_an_element_asks_for_the_node_s_action() {
        use accesskit::{ActionData, Point};

        use super::{scroll_offset, slider_request};

        let mut stepped = Node::new(Role::Slider);
        stepped.set_numeric_value(30.0);
        stepped.add_action(Action::Increment);
        stepped.add_action(Action::Decrement);
        assert_eq!(
            slider_request(&stepped, 35.0),
            Some((Action::Increment, None))
        );
        assert_eq!(
            slider_request(&stepped, 25.0),
            Some((Action::Decrement, None))
        );
        assert_eq!(slider_request(&stepped, 30.0), None);
        assert_eq!(slider_request(&stepped, f64::NAN), None);
        let mut set = stepped.clone();
        set.add_action(Action::SetValue);
        assert_eq!(
            slider_request(&set, 42.0),
            Some((Action::SetValue, Some(ActionData::NumericValue(42.0))))
        );

        let mut list = Node::new(Role::ScrollView);
        list.set_scroll_x(7.0);
        list.set_scroll_y(150.0);
        list.set_scroll_y_min(50.0);
        list.set_scroll_y_max(450.0);
        assert_eq!(scroll_offset(&list, 0.0, 10.0), None, "no offset is taken");
        list.add_action(Action::SetScrollOffset);
        assert_eq!(
            scroll_offset(&list, 3.0, 200.0),
            Some(Point::new(7.0, 250.0)),
            "the scrolled axis from its minimum, the other as published"
        );
        assert_eq!(
            scroll_offset(&list, 0.0, 900.0),
            Some(Point::new(7.0, 450.0)),
            "never past the extent"
        );
        let scroll = super::Scroll {
            left: 0.0,
            top: 100.0,
            width: 0.0,
            height: 0.0,
        };
        assert!(scroll.is_at(0.2, 100.9));
        assert!(!scroll.is_at(0.0, 101.0));
    }

    /// The roles the window's text input serves on the canvas.
    #[test]
    fn a_node_that_takes_text_is_served_on_the_canvas() {
        for (role, takes) in [
            (Role::TextInput, true),
            (Role::MultilineTextInput, true),
            (Role::PasswordInput, true),
            (Role::EditableComboBox, true),
            (Role::SpinButton, true),
            (Role::Button, false),
            (Role::ComboBox, false),
            (Role::Label, false),
        ] {
            assert_eq!(takes_text(&Node::new(role)), takes, "{role:?}");
        }
    }

    #[test]
    fn a_dom_event_asks_for_what_the_node_supports() {
        use super::event_action;

        let mut expandable = Node::new(Role::Button);
        expandable.add_action(Action::Expand);
        expandable.add_action(Action::Collapse);
        assert_eq!(event_action("click", &expandable), Some(Action::Expand));
        expandable.set_expanded(true);
        assert_eq!(event_action("click", &expandable), Some(Action::Collapse));
        expandable.add_action(Action::Click);
        assert_eq!(event_action("click", &expandable), Some(Action::Click));
        assert_eq!(event_action("focusin", &expandable), None);
        assert_eq!(event_action("contextmenu", &expandable), None);

        let mut field = Node::new(Role::TextInput);
        field.add_action(Action::Focus);
        field.add_action(Action::ShowContextMenu);
        assert_eq!(event_action("focusin", &field), Some(Action::Focus));
        assert_eq!(
            event_action("contextmenu", &field),
            Some(Action::ShowContextMenu)
        );
        assert_eq!(event_action("click", &field), None);
        assert_eq!(event_action("click", &Node::new(Role::Label)), None);
    }
}
