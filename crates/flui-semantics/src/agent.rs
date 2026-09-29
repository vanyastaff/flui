//! The semantics tree as an agent reads it: the ADR-0080 wire [`Node`]s
//! projected from the AccessKit tree this crate publishes, and a wire
//! [`ActionRequest`] resolved back into a [`SemanticsActionRequest`].
//!
//! # One derivation for both backends
//!
//! The projection starts from [`SemanticsOwner::to_accesskit_tree_update`],
//! exactly what the platform adapter is handed, and reads it through
//! `accesskit_consumer`, the layer every AccessKit adapter sits on. Filtering
//! (a hidden subtree dropped, a `GenericContainer` lifted into its parent),
//! the name a static text is given, and which UI Automation patterns a node
//! offers are therefore the adapter's own predicates, not a second copy of
//! them. What UI Automation reports for a node is what the desktop server
//! reads through the OS; the in-process read reports the same role, name,
//! state and actions without the OS in between (ADR-0095 §2, as amended there:
//! the mapping lives here, because only this crate sees both the flags and
//! AccessKit).
//!
//! # Roles
//!
//! [`wire_role`] folds an AccessKit role to the wire role the desktop server
//! would report for it on Windows: the UI Automation control type
//! `accesskit_windows` 0.35 gives it, read through the desktop server's
//! control-type table, with the two refinements that server applies (a
//! password field, a dialog) and the ARIA roles it restores (cells, rows,
//! headers, a switch). `native_role` keeps the AccessKit name, so nothing the
//! fold erases is lost.
//!
//! # Rectangles
//!
//! A node's bounds are its published AccessKit bounds scaled to physical
//! pixels and covered by whole pixels, measured from the window's drawing
//! surface ([`Coordinates::Surface`]): the realm knows no window position.

use std::panic::{AssertUnwindSafe, catch_unwind};

use accesskit::{Action, NodeId, Toggled, TreeId};
use accesskit_consumer::{FilterResult, NodeRef, common_filter};
use flui_foundation::geometry::{DevicePixelRatio, Rect as LogicalRect, device_rect_covering};
use flui_protocol::{
    ActionName, ActionRequest, Checked, Coordinates, ElementId, ErrorCode, Node, ReadQuery, Rect,
    Role, Tree, WindowId,
};

use crate::action::{ActionArgs, SemanticsAction, SemanticsActionRequest};
use crate::identity::AccessibilityNodeId;
use crate::owner::SemanticsOwner;

/// Where a presentation's tree sits: the window it is the root of, and the
/// scale from its logical pixels to the surface's physical ones.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Placement {
    /// Physical pixels per logical pixel.
    pub device_pixel_ratio: DevicePixelRatio,
    /// The handle the root of the tree carries as its `window`.
    pub window: WindowId,
}

impl Placement {
    /// The tree of `window`, drawn at `device_pixel_ratio`.
    #[must_use]
    pub fn new(device_pixel_ratio: DevicePixelRatio, window: WindowId) -> Self {
        Self {
            device_pixel_ratio,
            window,
        }
    }
}

/// Why a wire read failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WireReadError {
    /// No semantics tree has been assembled yet.
    #[error("no semantics tree has been assembled yet")]
    NoTree,
    /// The read was scoped to an element the tree does not show.
    #[error("element {element} is not in the tree")]
    NotFound {
        /// The scope the read asked for.
        element: ElementId,
    },
    /// The published tree could not be read (a duplicate child, a child with
    /// no node): the same tree would fail in the platform adapter.
    #[error("the published semantics tree is malformed")]
    Malformed,
}

impl WireReadError {
    /// The ADR-0080 code for this error.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NoTree => ErrorCode::Busy,
            Self::NotFound { .. } => ErrorCode::Gone,
            Self::Malformed => ErrorCode::Platform,
        }
    }
}

/// Why a wire action was refused before reaching any handler.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WireActionError {
    /// No semantics tree has been assembled yet.
    #[error("no semantics tree has been assembled yet")]
    NoTree,
    /// The element is not in the tree: it left it, or it never was.
    #[error("element {element} is not in the tree")]
    NotFound {
        /// The element the action addressed.
        element: ElementId,
    },
    /// The element refuses interaction.
    #[error("element {element} is disabled")]
    Disabled {
        /// The element the action addressed.
        element: ElementId,
    },
    /// The element does not advertise the action now: it never offered it,
    /// or its state no longer allows it (`expand` on an expanded element).
    #[error("element {element} does not offer {action}")]
    ActionUnsupported {
        /// The element the action addressed.
        element: ElementId,
        /// The action it does not offer.
        action: ActionName,
    },
    /// `set_value` without a value, or a value on another action.
    #[error("{reason}")]
    InvalidArgument {
        /// What is wrong with the request.
        reason: &'static str,
    },
    /// The published tree could not be read.
    #[error("the published semantics tree is malformed")]
    Malformed,
}

impl WireActionError {
    /// The ADR-0080 code for this error.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NoTree => ErrorCode::Busy,
            Self::NotFound { .. } => ErrorCode::Gone,
            Self::Disabled { .. } => ErrorCode::Disabled,
            Self::ActionUnsupported { .. } => ErrorCode::ActionUnsupported,
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::Malformed => ErrorCode::Platform,
        }
    }
}

/// The FLUI action a wire action reaches, as AccessKit's Windows adapter
/// routes the UI Automation call behind it: `invoke`, `toggle` and `select`
/// click, which is FLUI's tap; `expand` and `collapse` toggle an expandable
/// node through its tap handler (mapping decision 5); `set_value` sets text
/// (mapping decision 3); `scroll_into_view` is `ShowOnScreen`.
///
/// `None` for a wire action FLUI has no route for; `ActionName` is
/// `#[non_exhaustive]`, so a tool added to the vocabulary lands here.
#[must_use]
pub fn semantics_action_for_wire(action: ActionName) -> Option<SemanticsAction> {
    Some(match action {
        ActionName::Invoke
        | ActionName::Toggle
        | ActionName::Select
        | ActionName::Expand
        | ActionName::Collapse => SemanticsAction::Tap,
        ActionName::SetValue => SemanticsAction::SetText,
        ActionName::Focus => SemanticsAction::Focus,
        ActionName::ScrollIntoView => SemanticsAction::ShowOnScreen,
        _ => return None,
    })
}

/// The wire role the desktop server reports for a node AccessKit publishes as
/// `role` on Windows: UI Automation's control type for it
/// (`accesskit_windows` 0.35, `NodeWrapper::control_type`), named by the
/// desktop server's table (`tools/desktop-mcp`, `uia.rs` `role_of`), with the
/// ARIA roles that server restores (`role.rs` `role_from_aria`) and its
/// password and dialog refinements (`uia.rs` `refine_role`).
///
/// The match names every AccessKit role: `accesskit::Role` is not
/// `#[non_exhaustive]`, so a release that adds one stops this compiling until
/// its fold is decided.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one arm per UI Automation control type, naming every AccessKit role"
)]
pub(crate) fn wire_role(role: accesskit::Role) -> Role {
    use accesskit::Role as A;
    match role {
        // Custom.
        A::Unknown | A::TextRun | A::PdfActionableHighlight => Role::Unknown,
        // DataItem, with the ARIA roles that tell its kinds apart.
        A::Cell => Role::Cell,
        A::GridCell => Role::GridCell,
        A::RowHeader => Role::RowHeader,
        A::ColumnHeader => Role::ColumnHeader,
        A::Row | A::LayoutTableCell | A::LayoutTableRow => Role::Row,
        // Text.
        A::Label
        | A::Abbr
        | A::Alert
        | A::Caption
        | A::Code
        | A::Emphasis
        | A::FigureCaption
        | A::Heading
        | A::Legend
        | A::LineBreak
        | A::Mark
        | A::Marquee
        | A::RubyAnnotation
        | A::Strong
        | A::Time => Role::Label,
        // Image.
        A::Image | A::Canvas | A::SvgRoot | A::GraphicsSymbol | A::DocCover => Role::Image,
        // Hyperlink.
        A::Link | A::DocBackLink | A::DocBiblioRef | A::DocGlossRef | A::DocNoteRef => Role::Link,
        // ListItem.
        A::ListItem
        | A::ListBoxOption
        | A::MenuListOption
        | A::Term
        | A::DocBiblioEntry
        | A::DocEndnote
        | A::DocFootnote => Role::ListItem,
        A::TreeItem => Role::TreeItem,
        A::MenuItem => Role::MenuItem,
        A::CheckBox | A::MenuItemCheckBox => Role::CheckBox,
        A::RadioButton | A::MenuItemRadio => Role::RadioButton,
        // Edit, a password field refined by `IsPassword`.
        A::PasswordInput => Role::PasswordInput,
        A::TextInput
        | A::MultilineTextInput
        | A::SearchInput
        | A::DateInput
        | A::DateTimeInput
        | A::WeekInput
        | A::MonthInput
        | A::TimeInput
        | A::EmailInput
        | A::NumberInput
        | A::PhoneNumberInput
        | A::UrlInput => Role::TextInput,
        // Button, a switch restored by its ARIA role.
        A::Switch => Role::Switch,
        A::Button | A::DefaultButton | A::ColorWell | A::DisclosureTriangle => Role::Button,
        // Pane.
        A::Pane
        | A::Application
        | A::EmbeddedObject
        | A::ImeCandidate
        | A::Keyboard
        | A::ScrollView
        | A::TabPanel
        | A::Timer
        | A::TitleBar
        | A::GraphicsObject => Role::Pane,
        // List.
        A::List | A::DescriptionList | A::ListBox | A::MenuListPopup => Role::List,
        // Table.
        A::Table | A::LayoutTable => Role::Table,
        A::Menu => Role::Menu,
        A::MenuBar => Role::MenuBar,
        // Window, a dialog refined by `IsDialog`.
        A::Dialog | A::AlertDialog => Role::Dialog,
        A::Window => Role::Window,
        A::ComboBox | A::EditableComboBox => Role::ComboBox,
        // Document.
        A::Document
        | A::Terminal
        | A::Iframe
        | A::RootWebArea
        | A::WebView
        | A::PdfRoot
        | A::GraphicsDocument => Role::Document,
        // DataGrid.
        A::Grid | A::TreeGrid | A::ListGrid => Role::Grid,
        A::Meter | A::ProgressIndicator => Role::ProgressIndicator,
        A::ScrollBar => Role::ScrollBar,
        A::Slider => Role::Slider,
        A::SpinButton => Role::SpinButton,
        // Separator.
        A::Splitter | A::DocPageBreak => Role::Splitter,
        A::Status => Role::Status,
        A::Tab => Role::Tab,
        A::TabList => Role::TabList,
        A::Toolbar => Role::Toolbar,
        A::Tooltip => Role::Tooltip,
        A::Tree => Role::Tree,
        // Group.
        A::ListMarker
        | A::Paragraph
        | A::GenericContainer
        | A::RowGroup
        | A::Article
        | A::Audio
        | A::Banner
        | A::Blockquote
        | A::Caret
        | A::Complementary
        | A::Comment
        | A::ContentDeletion
        | A::ContentInsertion
        | A::ContentInfo
        | A::Definition
        | A::Details
        | A::Feed
        | A::Figure
        | A::Footer
        | A::Form
        | A::Group
        | A::Header
        | A::IframePresentational
        | A::Log
        | A::Main
        | A::Math
        | A::Navigation
        | A::Note
        | A::PluginObject
        | A::RadioGroup
        | A::Region
        | A::Ruby
        | A::Search
        | A::Section
        | A::SectionFooter
        | A::SectionHeader
        | A::Suggestion
        | A::Video
        | A::DocAbstract
        | A::DocAcknowledgements
        | A::DocAfterword
        | A::DocAppendix
        | A::DocBibliography
        | A::DocChapter
        | A::DocColophon
        | A::DocConclusion
        | A::DocCredit
        | A::DocCredits
        | A::DocDedication
        | A::DocEndnotes
        | A::DocEpigraph
        | A::DocEpilogue
        | A::DocErrata
        | A::DocExample
        | A::DocForeword
        | A::DocGlossary
        | A::DocIndex
        | A::DocIntroduction
        | A::DocNotice
        | A::DocPageFooter
        | A::DocPageHeader
        | A::DocPageList
        | A::DocPart
        | A::DocPreface
        | A::DocPrologue
        | A::DocPullquote
        | A::DocQna
        | A::DocSubtitle
        | A::DocTip
        | A::DocToc => Role::Group,
    }
}

/// Whether UI Automation offers the `SelectionItem` pattern for `node`
/// (`accesskit_windows` 0.35, `is_selection_item_pattern_supported`).
fn offers_selection_item(node: &NodeRef<'_>) -> bool {
    use accesskit::Role as A;
    match node.role() {
        A::RadioButton | A::MenuItemRadio => {
            matches!(node.toggled(), Some(Toggled::True | Toggled::False))
        }
        A::ListBoxOption | A::ListItem | A::MenuListOption | A::Tab | A::TreeItem => {
            node.is_selected().is_some()
        }
        A::GridCell => true,
        _ => false,
    }
}

/// Whether the `SelectionItem` pattern reports `node` selected
/// (`accesskit_windows` 0.35, `is_selected`).
fn reads_selected(node: &NodeRef<'_>) -> bool {
    match node.role() {
        accesskit::Role::RadioButton | accesskit::Role::MenuItemRadio => {
            node.toggled() == Some(Toggled::True)
        }
        _ => node.is_selected().unwrap_or(false),
    }
}

/// The tools that act on `node`, in the order the desktop server lists them:
/// its UI Automation patterns (`Invoke`, `Toggle`, `Value`, `SelectionItem`,
/// `ExpandCollapse`, `ScrollItem`), then focus.
///
/// `set_value`, `expand`, `collapse`, `scroll_into_view` and `focus` are
/// listed only when the node carries the AccessKit action that performs them,
/// so an advertised action always reaches a handler: UI Automation offers the
/// `Value` pattern on any node with a value, but calling it on one with no
/// `SetValue` action does nothing.
fn wire_actions(node: &NodeRef<'_>) -> Vec<ActionName> {
    let supports = |action| node.supports_action(action, &common_filter);
    let mut actions = Vec::new();
    if node.is_invocable(&common_filter) {
        actions.push(ActionName::Invoke);
    }
    if node.toggled().is_some() && !offers_selection_item(node) {
        actions.push(ActionName::Toggle);
    }
    if supports(Action::SetValue) && !node.is_read_only() {
        actions.push(ActionName::SetValue);
    }
    if offers_selection_item(node) {
        actions.push(ActionName::Select);
    }
    if node.supports_expand_collapse() {
        // Only the transition the state allows, as the desktop server lists
        // it; a leaf offers neither.
        match node.data().is_expanded() {
            Some(false) if supports(Action::Expand) => actions.push(ActionName::Expand),
            Some(true) if supports(Action::Collapse) => actions.push(ActionName::Collapse),
            _ => {}
        }
    }
    if supports(Action::ScrollIntoView) {
        actions.push(ActionName::ScrollIntoView);
    }
    if supports(Action::Focus) {
        actions.push(ActionName::Focus);
    }
    actions
}

/// A read's budget and what it left out.
struct Walk<'q> {
    query: &'q ReadQuery,
    placement: Placement,
    remaining: usize,
    truncated: bool,
    /// Whether the node AccessKit is told is focused really claims focus:
    /// AccessKit falls back to the root when nothing does.
    root_claims_focus: bool,
}

impl Walk<'_> {
    fn rect(&self, node: &NodeRef<'_>) -> Option<Rect> {
        let bounds = node.bounding_box()?;
        let logical = LogicalRect::from_ltrb(bounds.x0, bounds.y0, bounds.x1, bounds.y1);
        let device = device_rect_covering(logical, self.placement.device_pixel_ratio);
        Some(Rect {
            x: device.left(),
            y: device.top(),
            width: device.width().max(0).unsigned_abs(),
            height: device.height().max(0).unsigned_abs(),
        })
    }

    /// `node` as a wire node, its filtered children below it within the
    /// depth and node budget.
    fn node(&mut self, node: &NodeRef<'_>, id: ElementId, depth: usize) -> Node {
        let mut out = wire_node(node, id, self.root_claims_focus);
        out.rect = self.rect(node);
        let children: Vec<NodeRef<'_>> = node.filtered_children(common_filter).collect();
        if children.is_empty() {
            return out;
        }
        if self.query.max_depth.is_some_and(|max| depth >= max) {
            out.omitted_children = Some(children.len());
            self.truncated = true;
            return out;
        }
        for (index, child) in children.iter().enumerate() {
            if self.remaining == 0 {
                out.omitted_children = Some(children.len() - index);
                self.truncated = true;
                break;
            }
            let Some(child_id) = element_id(child) else {
                continue;
            };
            self.remaining -= 1;
            let child = self.node(child, child_id, depth + 1);
            out.children.push(child);
        }
        out
    }
}

/// The wire handle of a published node: its generational accessibility id.
fn element_id(node: &NodeRef<'_>) -> Option<ElementId> {
    ElementId::from_u64(node.locate().0.0)
}

/// `node`'s own fields, without children or bounds.
fn wire_node(node: &NodeRef<'_>, id: ElementId, root_claims_focus: bool) -> Node {
    let ak_role = node.role();
    let mut out = Node::new(id, wire_role(ak_role), format!("{ak_role:?}"));
    // UI Automation's `Name`: a static text is named by its value.
    out.name = if node.label_comes_from_value() {
        node.value()
    } else {
        node.label()
    }
    .filter(|name| !name.is_empty());
    // The `Value` pattern's value: not for a static text, whose value is its
    // name, and never a password field's.
    if !node.label_comes_from_value() && ak_role != accesskit::Role::PasswordInput {
        out.value = node.value();
    }
    out.disabled = node.is_disabled();
    out.focused = node.is_focused() && (!node.is_root() || root_claims_focus);
    out.focusable = node.is_focusable(&common_filter);
    if node.toggled().is_some() && !offers_selection_item(node) {
        out.checked = node.toggled().map(|state| match state {
            Toggled::True => Checked::True,
            Toggled::False => Checked::False,
            Toggled::Mixed => Checked::Mixed,
        });
    }
    if node.supports_expand_collapse() {
        out.expanded = node.data().is_expanded();
    }
    if offers_selection_item(node) {
        out.selected = Some(reads_selected(node));
    }
    out.actions = wire_actions(node);
    out
}

impl SemanticsOwner {
    /// Builds the AccessKit tree the adapter would be handed and runs `read`
    /// over it, with whether the root really claims focus.
    fn with_published<R>(
        &self,
        read: impl FnOnce(&accesskit_consumer::TreeState, bool) -> R,
    ) -> Result<R, Published> {
        let update = self
            .to_accesskit_tree_update(None)
            .ok_or(Published::NoTree)?;
        let root_claims_focus = self
            .tree()
            .root()
            .and_then(|root| self.tree().get(root))
            .is_some_and(|root| root.config().is_focused());
        // `accesskit_consumer` panics on a tree that breaks its contract (a
        // duplicate child, a child with no node). The adapter would panic on
        // the same update; a read reports it instead of unwinding through
        // the caller's owner turn.
        let tree = catch_unwind(AssertUnwindSafe(|| {
            accesskit_consumer::Tree::new(update, true)
        }))
        .map_err(|_| Published::Malformed)?;
        Ok(read(tree.state(), root_claims_focus))
    }

    /// The tree as an agent reads it: ADR-0080's wire nodes, projected from
    /// the AccessKit tree this owner publishes (see the module doc).
    ///
    /// # Errors
    ///
    /// [`WireReadError::NoTree`] before the first assembly,
    /// [`WireReadError::NotFound`] when `query.root` names no element in the
    /// tree, [`WireReadError::Malformed`] when the published tree breaks
    /// AccessKit's contract.
    pub fn read_wire(
        &self,
        query: &ReadQuery,
        placement: Placement,
    ) -> Result<Tree, WireReadError> {
        let read = self.with_published(|state, root_claims_focus| {
            let (start, id, is_window_root) = match query.root {
                None => {
                    let root = state.root();
                    let id = element_id(&root).ok_or(WireReadError::Malformed)?;
                    (root, id, true)
                }
                Some(id) => {
                    let node = state
                        .node_by_tree_local_id(NodeId(id.get()), TreeId::ROOT)
                        .filter(|node| common_filter(node) == FilterResult::Include)
                        .ok_or(WireReadError::NotFound { element: id })?;
                    (node, id, node.is_root())
                }
            };
            let mut walk = Walk {
                query,
                placement,
                remaining: query.max_nodes.unwrap_or(usize::MAX),
                truncated: false,
                root_claims_focus,
            };
            if walk.remaining == 0 {
                return Ok(Tree::new(Vec::new(), true, Coordinates::Surface));
            }
            walk.remaining -= 1;
            let mut root = walk.node(&start, id, 0);
            if is_window_root {
                root.window = Some(placement.window);
            }
            Ok(Tree::new(vec![root], walk.truncated, Coordinates::Surface))
        });
        match read {
            Ok(result) => result,
            Err(Published::NoTree) => Err(WireReadError::NoTree),
            Err(Published::Malformed) => Err(WireReadError::Malformed),
        }
    }

    /// Resolves a wire action into the FLUI action it performs, checked
    /// against the element as the wire shows it now: the element is in the
    /// tree, it is not disabled, and it advertises the action. `expand` and
    /// `collapse` are advertised only toward the state the element lacks, so
    /// a second `expand` before the next frame is refused here rather than
    /// toggling the element back (mapping decision 7).
    ///
    /// # Errors
    ///
    /// See [`WireActionError`].
    pub fn resolve_wire_action(
        &self,
        request: &ActionRequest,
    ) -> Result<SemanticsActionRequest, WireActionError> {
        let element = request.element;
        let checked = self.with_published(|state, root_claims_focus| {
            let node = state
                .node_by_tree_local_id(NodeId(element.get()), TreeId::ROOT)
                .filter(|node| common_filter(node) == FilterResult::Include)
                .ok_or(WireActionError::NotFound { element })?;
            let wire = wire_node(&node, element, root_claims_focus);
            if wire.disabled {
                return Err(WireActionError::Disabled { element });
            }
            if !wire.actions.contains(&request.action) {
                return Err(WireActionError::ActionUnsupported {
                    element,
                    action: request.action,
                });
            }
            Ok(())
        });
        match checked {
            Ok(result) => result?,
            Err(Published::NoTree) => return Err(WireActionError::NoTree),
            Err(Published::Malformed) => return Err(WireActionError::Malformed),
        }

        let arguments = match (request.action, &request.value) {
            (ActionName::SetValue, Some(text)) => Some(ActionArgs::SetText { text: text.clone() }),
            (ActionName::SetValue, None) => {
                return Err(WireActionError::InvalidArgument {
                    reason: "set_value needs a value",
                });
            }
            (_, Some(_)) => {
                return Err(WireActionError::InvalidArgument {
                    reason: "only set_value takes a value",
                });
            }
            (_, None) => None,
        };
        let action = semantics_action_for_wire(request.action).ok_or(
            WireActionError::ActionUnsupported {
                element,
                action: request.action,
            },
        )?;
        let node_id = AccessibilityNodeId::from_u64(element.get())
            .ok_or(WireActionError::NotFound { element })?;
        Ok(SemanticsActionRequest {
            node_id,
            action,
            arguments,
        })
    }
}

/// Why no published tree could be read.
enum Published {
    NoTree,
    Malformed,
}

#[cfg(test)]
mod tests;
