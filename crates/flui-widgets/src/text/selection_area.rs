//! [`SelectionArea`]: static text beneath it becomes selectable as one
//! document, by assistive technology now, and by pointer and keyboard on the
//! same model (ADR-0214).
//!
//! Each [`Text`](crate::Text) or [`RichText`](crate::RichText) beneath an
//! area registers itself through the inherited [`SelectionScope`] under a
//! [`SemanticsAnchor`] of its own. The paragraph publishes its runs and
//! declares that anchor (`RenderParagraph::set_selectable`); the area's node
//! is a merge boundary with the `Document` role that publishes the selection
//! as positions in those runs and takes `SetTextSelection` for the whole
//! document. The selection is held as two ([anchor], byte) ends, so it names
//! a text by identity, never by its place or its words.
//!
//! [anchor]: SemanticsAnchor

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use flui_foundation::panic::FirstPanic;
use flui_objects::{RenderParagraph, SubtreeAnchor};
use flui_painting::text_boundaries;
use flui_painting::typography::TextLink;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::semantics::{
    DocumentPosition, DocumentSelection, SemanticsAnchor, SemanticsRole,
};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{
    BoxedView, EventCx, EventOutcome, InheritedView, LifecycleContext, RebuildHandle,
    RebuildReason, StatefulView, View, ViewExt, ViewState, impl_inherited_view,
};

use super::rich_text::ResolvedParagraph;
use crate::Semantics;
use crate::anchored_box::AnchoredBox;
use crate::support::RefCallback;

/// What a [`SelectionArea`] holds selected, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SelectedText {
    /// The selected text, with the texts it spans joined by `'\n'`.
    pub text: String,
}

/// Told the selected text after each change of the selection, `None` once
/// nothing, or only a caret, is selected.
type SelectionCallback = Rc<dyn Fn(&mut EventCx<'_>, Option<&SelectedText>)>;

/// Makes the static text beneath it selectable as one document.
///
/// Every [`Text`](crate::Text) and [`RichText`](crate::RichText) beneath the
/// area joins it, except beneath a [`SelectionContainer::disabled`]. A
/// selection runs from one text to another in the order the published
/// document reads them, and every text between its two ends is selected
/// whole.
///
/// Assistive technology reads the texts by character and word and moves the
/// selection (ADR-0214): the area is a `Document` node that publishes the
/// selection, and the texts publish their runs. Text outside an area
/// publishes none of this and costs nothing more.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = SelectionArea::new(Column::new((Text::new("Terms"), Text::new("Read them."))))
///     .on_selection_changed(|_cx, selected| {
///         if let Some(selected) = selected {
///             println!("selected {:?}", selected.text);
///         }
///     });
/// ```
#[derive(Clone)]
pub struct SelectionArea {
    child: BoxedView,
    on_selection_changed: Option<SelectionCallback>,
    role: SemanticsRole,
}

impl std::fmt::Debug for SelectionArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionArea")
            .field("child", &self.child)
            .field("on_selection_changed", &self.on_selection_changed.is_some())
            .field("role", &self.role)
            .finish()
    }
}

impl SelectionArea {
    /// Make the static text in `child` selectable.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            on_selection_changed: None,
            role: SemanticsRole::Document,
        }
    }

    /// Call `handler` with the selected text after each change of the
    /// selection, or with `None` once nothing, or only a caret, is selected.
    /// It runs after the selection is committed, so it may rebuild the area.
    #[must_use]
    pub fn on_selection_changed<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, Option<&SelectedText>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_selection_changed = Some(Rc::new(move |cx, selected| {
            handler(cx, selected).report();
        }));
        self
    }

    /// Set the area's accessible role (default [`SemanticsRole::Document`]).
    /// It must be a role assistive technology reads text ranges on; any
    /// other is a bug, reported in debug builds and published as `Document`.
    #[must_use]
    pub fn semantics_role(mut self, role: SemanticsRole) -> Self {
        let supported = matches!(role, SemanticsRole::Document);
        debug_assert!(
            supported,
            "BUG: a SelectionArea's role must support text ranges, not {role:?}"
        );
        self.role = if supported {
            role
        } else {
            SemanticsRole::Document
        };
        self
    }
}

impl View for SelectionArea {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for SelectionArea {
    type State = SelectionAreaState;

    fn create_state(&self) -> Self::State {
        SelectionAreaState {
            model: Rc::new(SelectionModel {
                registry: Rc::new(SelectionRegistry::default()),
                selection: RefCell::new(None),
                on_selection_changed: RefCell::new(self.on_selection_changed.clone()),
                rebuild: RefCell::new(None),
                pipeline: RefCell::new(None),
            }),
        }
    }
}

/// The area's selection model, shared with the semantics handler that moves
/// it.
#[doc(hidden)]
pub struct SelectionAreaState {
    model: Rc<SelectionModel>,
}

impl std::fmt::Debug for SelectionAreaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionAreaState")
            .field("selection", &self.model.selection.borrow())
            .field("texts", &self.model.registry.entries.borrow().len())
            .finish()
    }
}

impl ViewState<SelectionArea> for SelectionAreaState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.model.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
        *self.model.pipeline.borrow_mut() = ctx.pipeline_owner();
    }

    fn did_update_view(&mut self, _old_view: &SelectionArea, new_view: &SelectionArea) {
        self.model
            .on_selection_changed
            .borrow_mut()
            .clone_from(&new_view.on_selection_changed);
    }

    fn build(&self, view: &SelectionArea, _ctx: &dyn BuildContext) -> impl IntoView {
        let model = Rc::clone(&self.model);
        Semantics::new()
            .role(view.role)
            .merge_boundary(true)
            .document_selection(self.model.selection.borrow().clone())
            .on_set_document_selection(move |cx, selection| model.select(cx, &selection))
            .child(SelectionScope {
                registry: Some(Rc::clone(&self.model.registry)),
                child: view.child.clone(),
            })
    }

    fn dispose(&mut self) {
        // A handler a frame already captured finds no area to rebuild.
        self.model.rebuild.borrow_mut().take();
    }
}

struct SelectionModel {
    registry: Rc<SelectionRegistry>,
    selection: RefCell<Option<DocumentSelection>>,
    on_selection_changed: RefCell<Option<SelectionCallback>>,
    rebuild: RefCell<Option<RebuildHandle>>,
    pipeline: RefCell<Option<PipelineCell>>,
}

impl SelectionModel {
    /// Takes `selection` as the area's, once each end is snapped into a
    /// registered text: an end naming a text this area does not hold
    /// refuses the whole request. Tells the listener after the selection is
    /// committed and no borrow is held, since it may rebuild the area or
    /// select again. A wake that panics still lets the listener hear of the
    /// committed selection; the first failure is resumed afterwards.
    fn select(&self, cx: &mut EventCx<'_>, selection: &DocumentSelection) {
        let Some(selection) = self.registry.snap(selection) else {
            return;
        };
        if self.selection.borrow().as_ref() == Some(&selection) {
            return;
        }
        let selected = self.selected_text(&selection);
        *self.selection.borrow_mut() = Some(selection);
        let rebuild = self.rebuild.borrow().clone();
        let listener = self.on_selection_changed.borrow().clone();
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            if let Some(rebuild) = rebuild {
                rebuild.schedule(RebuildReason::StateChange);
            }
        });
        failure.contain(|| {
            if let Some(listener) = listener {
                listener(cx, selected.as_ref());
            }
        });
        failure.resume();
    }

    /// The text `selection` covers, in reading order; `None` when it covers
    /// none. Each text contributes only what its layout shows: the lines a
    /// line limit cuts are not in its runs, so they are not reported either.
    fn selected_text(&self, selection: &DocumentSelection) -> Option<SelectedText> {
        let entries = self.registry.entries.borrow();
        let order = self.reading_order(&entries);
        let place = |at: &DocumentPosition| {
            let index = entries
                .iter()
                .position(|entry| entry.anchor == *at.text())?;
            Some((order.iter().position(|&(i, _)| i == index)?, at.byte()))
        };
        let (from, to) = {
            let (a, b) = (place(selection.anchor())?, place(selection.focus())?);
            if a <= b { (a, b) } else { (b, a) }
        };
        if from == to {
            return None;
        }
        let mut text = String::new();
        for (rank, &(index, shown)) in order.iter().enumerate().take(to.0 + 1).skip(from.0) {
            let content = entries[index].text.borrow();
            let shown = shown.unwrap_or(content.len()).min(content.len());
            let start = if rank == from.0 { from.1 } else { 0 }.min(shown);
            let end = if rank == to.0 { to.1 } else { shown }.min(shown);
            if rank > from.0 {
                text.push('\n');
            }
            text.push_str(content.get(start..end).unwrap_or_default());
        }
        Some(SelectedText { text })
    }

    /// The presented texts in the order the published document reads them,
    /// each as its index and the byte where its laid-out text ends (`None`
    /// when no layout says), so the text reported for a selection is the
    /// text an adapter selected. While semantics is on, that is the last
    /// publication's order, sort keys included, and a text it does not
    /// publish (offstage, excluded, silenced) is left out. Without it, the
    /// order is place in the render tree, and texts beneath an ancestor that
    /// leaves its subtree out of semantics are left out; texts not yet in
    /// the render tree keep registration order after the placed ones.
    ///
    /// Geometric reading order replaces these when pointer selection lands,
    /// for the model and the assembly at once.
    fn reading_order(&self, entries: &[Rc<SelectableEntry>]) -> Vec<(usize, Option<usize>)> {
        let pipeline = self.pipeline.borrow().clone();
        pipeline
            .and_then(|pipeline| {
                pipeline.try_with_layout(|owner| {
                    let shown = |index: usize| shown_end(owner, &entries[index].subtree);
                    if let Some(published) = owner.semantics_owner().and_then(|semantics| {
                        entries
                            .iter()
                            .find_map(|entry| semantics.document_texts(&entry.anchor))
                    }) {
                        return published
                            .iter()
                            .filter_map(|anchor| {
                                entries.iter().position(|entry| entry.anchor == *anchor)
                            })
                            .map(|index| (index, shown(index)))
                            .collect();
                    }
                    let places: Vec<Placement> = entries
                        .iter()
                        .map(|entry| Placement::of(owner, &entry.subtree))
                        .collect();
                    let mut order: Vec<usize> = (0..entries.len())
                        .filter(|&index| places[index] != Placement::Hidden)
                        .collect();
                    order.sort_by(|&a, &b| match (&places[a], &places[b]) {
                        (Placement::At(a), Placement::At(b)) => a.cmp(b),
                        (Placement::At(_), _) => Ordering::Less,
                        (_, Placement::At(_)) => Ordering::Greater,
                        _ => Ordering::Equal,
                    });
                    order
                        .into_iter()
                        .map(|index| (index, shown(index)))
                        .collect()
                })
            })
            .unwrap_or_else(|| (0..entries.len()).map(|index| (index, None)).collect())
    }
}

/// Where the laid-out text of the paragraph beneath `subtree` ends: its last
/// shown line's end, which a line limit puts before the authored text's end.
fn shown_end(owner: &PipelineOwner, subtree: &SubtreeAnchor) -> Option<usize> {
    let tree = owner.render_tree();
    let mut at = subtree.get()?;
    // The paragraph is the first render object beneath the anchor that is
    // one; link routing may wrap it, but only in a few single-child boxes.
    for _ in 0..8 {
        at = *tree.children(at).first()?;
        let Some(paragraph) = tree
            .get(at)?
            .as_box()?
            .render_object()
            .downcast_ref::<RenderParagraph>()
        else {
            continue;
        };
        let painter = paragraph.painter();
        if !painter.has_layout() {
            return None;
        }
        return painter
            .line_layouts()
            .iter()
            .map(|line| line.runs.last().map_or(line.start, |run| run.range().end))
            .max();
    }
    None
}

/// Where a registered text stands in the presented document.
#[derive(Clone, Debug, PartialEq)]
enum Placement {
    /// The child indices from the render root to the text's box.
    At(Vec<usize>),
    /// Mounted, but beneath an ancestor that excludes it from semantics.
    Hidden,
    /// Not in the render tree.
    Unplaced,
}

impl Placement {
    fn of(owner: &PipelineOwner, subtree: &SubtreeAnchor) -> Self {
        let Some(id) = subtree.get() else {
            return Self::Unplaced;
        };
        let tree = owner.render_tree();
        let mut path = Vec::new();
        let mut at = id;
        loop {
            let Some(node) = tree.get(at) else {
                return Self::Unplaced;
            };
            if node.excludes_semantics_subtree() {
                return Self::Hidden;
            }
            let Some(parent) = tree.parent(at) else {
                break;
            };
            let Some(index) = tree.children(parent).iter().position(|&child| child == at) else {
                return Self::Unplaced;
            };
            path.push(index);
            at = parent;
        }
        path.reverse();
        Self::At(path)
    }
}

/// The texts registered with one area, in registration order.
#[derive(Default)]
pub(super) struct SelectionRegistry {
    entries: RefCell<Vec<Rc<SelectableEntry>>>,
}

impl SelectionRegistry {
    fn register(&self, entry: &Rc<SelectableEntry>) {
        let mut entries = self.entries.borrow_mut();
        if !entries.iter().any(|held| Rc::ptr_eq(held, entry)) {
            entries.push(Rc::clone(entry));
        }
    }

    fn unregister(&self, entry: &Rc<SelectableEntry>) {
        self.entries
            .borrow_mut()
            .retain(|held| !Rc::ptr_eq(held, entry));
    }

    /// `selection` with each end in a registered text, its byte clamped to
    /// that text's length and moved down to a grapheme boundary, so no end
    /// splits a user-perceived character; `None` when an end names a text
    /// this area does not hold.
    fn snap(&self, selection: &DocumentSelection) -> Option<DocumentSelection> {
        let entries = self.entries.borrow();
        let snap = |at: &DocumentPosition| {
            let entry = entries.iter().find(|entry| entry.anchor == *at.text())?;
            let text = entry.text.borrow();
            let byte = at.byte().min(text.len());
            let byte = if text_boundaries::is_grapheme_boundary(&text, byte) {
                byte
            } else {
                text_boundaries::previous_grapheme_boundary(&text, byte)
            };
            Some(DocumentPosition::new(at.text().clone(), byte))
        };
        Some(DocumentSelection::new(
            snap(selection.anchor())?,
            snap(selection.focus())?,
        ))
    }
}

/// One registered text: the anchor its node declares, its plain text, and
/// the box that measures it.
struct SelectableEntry {
    anchor: SemanticsAnchor,
    text: RefCell<String>,
    subtree: SubtreeAnchor,
}

/// Hands the area's registry to the texts beneath it; a disabled container
/// hands none.
#[derive(Clone)]
pub(super) struct SelectionScope {
    registry: Option<Rc<SelectionRegistry>>,
    child: BoxedView,
}

impl std::fmt::Debug for SelectionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionScope")
            .field("enabled", &self.registry.is_some())
            .finish_non_exhaustive()
    }
}

impl SelectionScope {
    /// The registry of the area the text at `ctx` belongs to, registering a
    /// dependency on it.
    pub(super) fn registry_of(ctx: &dyn BuildContext) -> Option<Rc<SelectionRegistry>> {
        ctx.depend_on::<Self, _>(|scope| scope.registry.clone())
            .flatten()
    }
}

impl InheritedView for SelectionScope {
    type Data = Option<Rc<SelectionRegistry>>;

    fn data(&self) -> &Self::Data {
        &self.registry
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        match (&self.registry, &old.registry) {
            (Some(new), Some(old)) => !Rc::ptr_eq(new, old),
            (None, None) => false,
            _ => true,
        }
    }
}

impl_inherited_view!(SelectionScope);

/// Takes a subtree out of the enclosing [`SelectionArea`]: its text is plain
/// static text again, as on a button's label.
#[derive(Clone, Debug)]
pub struct SelectionContainer {
    child: BoxedView,
}

impl SelectionContainer {
    /// `child`, whose text no enclosing area selects.
    pub fn disabled(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }
}

impl View for SelectionContainer {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for SelectionContainer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SelectionScope {
            registry: None,
            child: self.child.clone(),
        }
    }
}

/// A text beneath a [`SelectionArea`], registered with it while mounted.
#[derive(Clone)]
pub(super) struct SelectableParagraph {
    paragraph: ResolvedParagraph,
    on_link: Option<RefCallback<TextLink>>,
    registry: Rc<SelectionRegistry>,
}

impl SelectableParagraph {
    pub(super) fn new(
        paragraph: ResolvedParagraph,
        on_link: Option<RefCallback<TextLink>>,
        registry: Rc<SelectionRegistry>,
    ) -> Self {
        Self {
            paragraph,
            on_link,
            registry,
        }
    }

    fn plain_text(&self) -> String {
        self.paragraph.authored.text.to_plain_text()
    }
}

impl std::fmt::Debug for SelectableParagraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectableParagraph")
            .field("paragraph", &self.paragraph)
            .finish_non_exhaustive()
    }
}

impl View for SelectableParagraph {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for SelectableParagraph {
    type State = SelectableParagraphState;

    fn create_state(&self) -> Self::State {
        SelectableParagraphState {
            entry: Rc::new(SelectableEntry {
                anchor: SemanticsAnchor::new(),
                text: RefCell::new(self.plain_text()),
                subtree: SubtreeAnchor::new(),
            }),
            registry: Rc::clone(&self.registry),
        }
    }
}

#[doc(hidden)]
pub struct SelectableParagraphState {
    entry: Rc<SelectableEntry>,
    registry: Rc<SelectionRegistry>,
}

impl std::fmt::Debug for SelectableParagraphState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectableParagraphState")
            .field("text", &self.entry.text.borrow())
            .finish_non_exhaustive()
    }
}

impl ViewState<SelectableParagraph> for SelectableParagraphState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        self.registry.register(&self.entry);
    }

    fn did_update_view(&mut self, _old_view: &SelectableParagraph, new_view: &SelectableParagraph) {
        *self.entry.text.borrow_mut() = new_view.plain_text();
        if !Rc::ptr_eq(&self.registry, &new_view.registry) {
            self.registry.unregister(&self.entry);
            self.registry = Rc::clone(&new_view.registry);
            self.registry.register(&self.entry);
        }
    }

    fn build(&self, view: &SelectableParagraph, _ctx: &dyn BuildContext) -> impl IntoView {
        let paragraph = ResolvedParagraph {
            selectable: Some(self.entry.anchor.clone()),
            ..view.paragraph.clone()
        };
        AnchoredBox::new(
            self.entry.subtree.clone(),
            paragraph.linked(view.on_link.as_ref()),
        )
    }

    fn dispose(&mut self) {
        self.registry.unregister(&self.entry);
    }
}
