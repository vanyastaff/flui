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

mod input;

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::{Rc, Weak};

use flui_foundation::RenderId;
use flui_foundation::geometry::Rect;
use flui_foundation::panic::FirstPanic;
use flui_interaction::focus::FocusNode;
use flui_interaction::{ClipboardCapability, ClipboardHandle, KeyEventResult};
use flui_objects::{RenderParagraph, SubtreeAnchor};
use flui_painting::styling::Color;
use flui_painting::text_boundaries;
use flui_painting::typography::{TextDirection, TextLink};
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::semantics::{
    DocumentPosition, DocumentSelection, SemanticsAnchor, SemanticsRole,
    TextDirection as SemanticsTextDirection,
};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{
    BoxedView, EventCx, EventOutcome, InheritedView, LifecycleContext, PostFrameHandle,
    RebuildHandle, RebuildReason, StatefulView, View, ViewExt, ViewState, WriterSource,
    impl_inherited_view,
};

use self::input::{AreaAction, CaretMemory, PointerState, SelectionGestures};
use super::rich_text::ResolvedParagraph;
use super::selection_style::{DefaultSelectionStyle, SelectionStyle};
use crate::Semantics;
use crate::anchored_box::AnchoredBox;
use crate::interaction::{
    Actions, CopySelectionTextIntent, DismissIntent, Focus, SelectAllTextIntent,
};
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
/// A mouse, pen or trackpad selects too: a drag selects by character, a
/// double click by word and a triple click a whole text, and Shift extends
/// from the selection's anchor. The keys move the moving end (Shift with the
/// arrows, Home and End, by word with the platform's word modifier), the
/// platform's select-all and copy chords select every text and copy the
/// selection, and Esc clears it. Selecting takes the focus; a blur keeps the
/// selection and paints it with [`SelectionStyle::inactive`]. A text that
/// leaves while holding an end hands the end to the selected text beside
/// it, and the selection clears once no selected text is left; the listener
/// hears of each change.
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
    style: Option<SelectionStyle>,
}

impl std::fmt::Debug for SelectionArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionArea")
            .field("child", &self.child)
            .field("on_selection_changed", &self.on_selection_changed.is_some())
            .field("role", &self.role)
            .field("style", &self.style)
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
            style: None,
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

    /// Highlight the selection with `style`, in place of the nearest
    /// [`DefaultSelectionStyle`]'s or [`SelectionStyle::default`].
    #[must_use]
    pub fn style(mut self, style: SelectionStyle) -> Self {
        self.style = Some(style);
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
                direction: Cell::new(SemanticsTextDirection::Ltr),
                writer: RefCell::new(None),
                clipboard: RefCell::new(None),
                focus: FocusNode::with_debug_label("SelectionArea"),
                focused: Cell::new(false),
                pointer: PointerState::default(),
                post_frame: RefCell::new(None),
                clamp_pending: Cell::new(false),
                mounted: Cell::new(true),
                backward: Cell::new(false),
                caret: Cell::new(CaretMemory::default()),
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
        *self.model.writer.borrow_mut() = Some(ctx.writer_source());
        *self.model.clipboard.borrow_mut() = ctx.capability::<ClipboardCapability>().ok();
        *self.model.post_frame.borrow_mut() = ctx.post_frame_handle();
        let model = Rc::downgrade(&self.model);
        *self.model.registry.on_departure.borrow_mut() = Some(Box::new(move |anchor| {
            if let Some(model) = model.upgrade() {
                model.departed(anchor);
            }
        }));
    }

    fn did_update_view(&mut self, _old_view: &SelectionArea, new_view: &SelectionArea) {
        self.model
            .on_selection_changed
            .borrow_mut()
            .clone_from(&new_view.on_selection_changed);
    }

    fn build(&self, view: &SelectionArea, ctx: &dyn BuildContext) -> impl IntoView {
        let style = view
            .style
            .or_else(|| DefaultSelectionStyle::maybe_of(ctx))
            .unwrap_or_default();
        // The document reads its lines in the ambient direction.
        let direction = match crate::Directionality::maybe_of(ctx).unwrap_or_default() {
            TextDirection::Ltr => SemanticsTextDirection::Ltr,
            TextDirection::Rtl => SemanticsTextDirection::Rtl,
        };
        self.model.direction.set(direction);
        let content = SelectionGestures {
            model: Rc::clone(&self.model),
            child: SelectionScope {
                data: ScopeData {
                    registry: Some(Rc::clone(&self.model.registry)),
                    color: if self.model.focused.get() {
                        style.active
                    } else {
                        style.inactive
                    },
                },
                child: view.child.clone(),
            }
            .boxed(),
        };
        let (keys, focus_changed) = (Rc::downgrade(&self.model), Rc::downgrade(&self.model));
        // The area is focused by selecting in it, never by traversal; the
        // document node is what assistive technology reads.
        let focused = Focus::new(content)
            .focus_node(Rc::clone(&self.model.focus))
            .skip_traversal(true)
            .include_semantics(false)
            .on_key_event(move |cx, event| {
                keys.upgrade()
                    .map_or(KeyEventResult::Ignored, |model| model.key(cx, event))
            })
            .on_focus_change(move |_cx, focused| {
                if let Some(model) = focus_changed.upgrade() {
                    model.focus_changed(focused);
                }
            });
        let mut actions = Actions::new(focused)
            .action::<SelectAllTextIntent>(AreaAction(Rc::downgrade(&self.model)))
            .action::<CopySelectionTextIntent>(AreaAction(Rc::downgrade(&self.model)));
        // Esc clears a selection; with nothing selected the area declares no
        // dismiss, so Esc reaches the enclosing one (a modal route's pop).
        // Every commit rebuilds the area, so this follows the selection.
        if self.model.selection.borrow().is_some() {
            actions = actions.action::<DismissIntent>(AreaAction(Rc::downgrade(&self.model)));
        }
        let model = Rc::clone(&self.model);
        Semantics::new()
            .role(view.role)
            .merge_boundary(true)
            .text_direction(direction)
            .document_selection(self.model.selection.borrow().clone())
            .on_set_document_selection(move |cx, selection| model.select(cx, Some(&selection)))
            .child(actions)
    }

    fn dispose(&mut self) {
        // A handler a frame already captured finds no area to rebuild.
        self.model.rebuild.borrow_mut().take();
        self.model.mounted.set(false);
    }
}

struct SelectionModel {
    registry: Rc<SelectionRegistry>,
    selection: RefCell<Option<DocumentSelection>>,
    on_selection_changed: RefCell<Option<SelectionCallback>>,
    rebuild: RefCell<Option<RebuildHandle>>,
    pipeline: RefCell<Option<PipelineCell>>,
    /// The direction the area read as of its last build, which orders the
    /// texts along a line.
    direction: Cell<SemanticsTextDirection>,
    /// Where the listener hears of a change the pointer made, acquired in
    /// `init_state`.
    writer: RefCell<Option<WriterSource>>,
    /// Where Copy writes, acquired in `init_state`; `None` disables Copy.
    clipboard: RefCell<Option<ClipboardHandle>>,
    /// The node the area holds the focus with once the user selects in it.
    focus: Rc<FocusNode>,
    /// Whether the area holds the focus: a blur dims the highlight and
    /// keeps the selection.
    focused: Cell<bool>,
    pointer: PointerState,
    /// Where a text that leaves while holding an end has the selection
    /// moved after the frame, acquired in `init_state`.
    post_frame: RefCell<Option<PostFrameHandle>>,
    /// Whether a move after the frame is already scheduled.
    clamp_pending: Cell<bool>,
    /// Cleared when the area is disposed, so a move scheduled before then
    /// finds no area to tell.
    mounted: Cell<bool>,
    /// Whether the last committed selection runs backward, its anchor
    /// after its focus, so ends that both leave keep that direction.
    backward: Cell<bool>,
    /// What the keys remember about the moving end.
    caret: Cell<CaretMemory>,
}

impl SelectionModel {
    /// Takes `selection` as the area's, once each end is snapped into a
    /// registered text: an end naming a text this area does not hold
    /// refuses the whole request; `None` selects nothing. Every selection
    /// comes from the user (assistive technology, the pointer or a key), so
    /// a committed one takes the focus. Tells the listener after the
    /// selection is committed and no borrow is held, since it may rebuild
    /// the area or select again. A wake or focus listener that panics still
    /// lets the listener hear of the committed selection; the first failure
    /// is resumed afterwards.
    fn select(&self, cx: &mut EventCx<'_>, selection: Option<&DocumentSelection>) {
        self.commit(cx, selection, true);
    }

    /// [`select`](Self::select), taking the focus only when `from_user`
    /// and something is selected.
    fn commit(&self, cx: &mut EventCx<'_>, selection: Option<&DocumentSelection>, from_user: bool) {
        let selection = match selection {
            Some(selection) => match self.registry.snap(selection) {
                Some(snapped) => Some(snapped),
                None => return,
            },
            None => None,
        };
        if *self.selection.borrow() == selection {
            return;
        }
        self.caret.set(CaretMemory::default());
        let coverage = selection
            .as_ref()
            .and_then(|selection| self.covered(selection));
        if let Some((_, backward)) = &coverage {
            self.backward.set(*backward);
        }
        let covered = coverage.map(|(parts, _)| parts);
        let selected = covered.as_ref().map(|covered| self.joined(covered));
        let take_focus = from_user && selection.is_some();
        *self.selection.borrow_mut() = selection;
        let rebuild = self.rebuild.borrow().clone();
        let listener = self.on_selection_changed.borrow().clone();
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            if let Some(rebuild) = rebuild {
                rebuild.schedule(RebuildReason::StateChange);
            }
        });
        if take_focus && !self.focus.has_primary_focus() {
            failure.contain(|| {
                let _outcome = self.focus.request_focus();
            });
        }
        self.highlight(covered.as_deref().unwrap_or_default(), &mut failure);
        failure.contain(|| {
            if let Some(listener) = listener {
                listener(cx, selected.as_ref());
            }
        });
        failure.resume();
    }

    /// The area gained or lost the focus: it rebuilds with the active or the
    /// dimmed highlight, and keeps its selection.
    fn focus_changed(&self, focused: bool) {
        if self.focused.replace(focused) == focused {
            return;
        }
        let rebuild = self.rebuild.borrow().clone();
        if let Some(rebuild) = rebuild {
            rebuild.schedule(RebuildReason::StateChange);
        }
    }

    /// The text under `anchor` left the area. When it held an end of the
    /// selection, the selection moves after the frame, once the texts that
    /// stay are laid out: see [`keep_what_remains`](Self::keep_what_remains).
    /// Without a post-frame lane the selection keeps naming the departed
    /// text, which selects nothing until the user selects again.
    fn departed(self: &Rc<Self>, anchor: &SemanticsAnchor) {
        let holds_an_end = self.selection.borrow().as_ref().is_some_and(|selection| {
            selection.anchor().text() == anchor || selection.focus().text() == anchor
        });
        if !holds_an_end || self.clamp_pending.replace(true) {
            return;
        }
        let Some(post_frame) = self.post_frame.borrow().clone() else {
            self.clamp_pending.set(false);
            tracing::warn!("SelectionArea: no post-frame lane to move a departed selection end");
            return;
        };
        let model = Rc::downgrade(self);
        let scheduled = post_frame.schedule(move |_| {
            if let Some(model) = Weak::upgrade(&model) {
                model.keep_what_remains();
            }
        });
        if let Err(error) = scheduled {
            self.clamp_pending.set(false);
            tracing::warn!(?error, "SelectionArea: the post-frame lane is closed");
        }
    }

    /// Moves each end held by a text that left onto the edge of what the
    /// selection still highlights in the texts that stay: an end on the
    /// earlier side moves to the start of the first of them, an end on the
    /// later side to the end of the last; when both ends left, the selection
    /// keeps the direction it was committed in. Nothing left highlighted clears
    /// the selection. The listener hears of the result; the focus stays
    /// where it is.
    fn keep_what_remains(&self) {
        self.clamp_pending.set(false);
        if !self.mounted.get() {
            return;
        }
        let Some(selection) = self.selection.borrow().clone() else {
            return;
        };
        let ordered = self.ordered();
        let held =
            |at: &DocumentPosition| ordered.iter().any(|text| text.entry.anchor == *at.text());
        let (anchor_held, focus_held) = (held(selection.anchor()), held(selection.focus()));
        if anchor_held && focus_held {
            return;
        }
        let highlighted: Vec<DocumentPosition> = ordered
            .iter()
            .filter_map(|text| {
                let range = text.entry.highlight.borrow().clone()?;
                let range = range.start.min(text.text.len())..range.end.min(text.text.len());
                Some([
                    DocumentPosition::new(text.entry.anchor.clone(), range.start),
                    DocumentPosition::new(text.entry.anchor.clone(), range.end),
                ])
            })
            .flatten()
            .collect();
        let next = highlighted
            .first()
            .zip(highlighted.last())
            .map(|(first, last)| {
                let kept = if anchor_held {
                    selection.anchor()
                } else if focus_held {
                    selection.focus()
                } else {
                    let (anchor, focus) = if self.backward.get() {
                        (last, first)
                    } else {
                        (first, last)
                    };
                    return DocumentSelection::new(anchor.clone(), focus.clone());
                };
                // The kept end bounds the highlight on one side; the departed
                // end takes the other.
                let kept_earlier = kept.text() == first.text()
                    && (kept.text() != last.text() || kept.byte() <= first.byte());
                let moved = if kept_earlier { last } else { first };
                if anchor_held {
                    DocumentSelection::new(kept.clone(), moved.clone())
                } else {
                    DocumentSelection::new(moved.clone(), kept.clone())
                }
            });
        let writer = self.writer.borrow().clone();
        if let Some(writer) = writer {
            writer.write(|cx| self.commit(cx, next.as_ref(), false));
        }
    }

    /// The parts of the texts `selection` covers, in reading order, each as
    /// its entry's index and a byte range of its text; `None` when it covers
    /// none, with whether the anchor reads after the focus. Each text
    /// contributes only what its layout shows: the lines a
    /// line limit cuts are not in its runs, so they are neither reported nor
    /// highlighted.
    fn covered(&self, selection: &DocumentSelection) -> Option<(Vec<Covered>, bool)> {
        let entries = self.registry.entries.borrow();
        let order = self.reading_order(&entries);
        let place = |at: &DocumentPosition| {
            let index = entries
                .iter()
                .position(|entry| entry.anchor == *at.text())?;
            Some((order.iter().position(|&(i, _)| i == index)?, at.byte()))
        };
        let (from, to, backward) = {
            let (a, b) = (place(selection.anchor())?, place(selection.focus())?);
            if a <= b { (a, b, false) } else { (b, a, true) }
        };
        if from == to {
            return None;
        }
        Some((
            order
                .iter()
                .enumerate()
                .take(to.0 + 1)
                .skip(from.0)
                .map(|(rank, &(index, shown))| {
                    let length = entries[index].text.borrow().len();
                    let shown = shown.unwrap_or(length).min(length);
                    let start = if rank == from.0 { from.1 } else { 0 }.min(shown);
                    let end = if rank == to.0 { to.1 } else { shown }.min(shown);
                    Covered {
                        index,
                        range: start..end.max(start),
                        to_end: rank != to.0,
                    }
                })
                .collect(),
            backward,
        ))
    }

    /// The covered parts' text, joined by `'\n'`.
    fn joined(&self, covered: &[Covered]) -> SelectedText {
        let entries = self.registry.entries.borrow();
        let text = covered
            .iter()
            .map(|Covered { index, range, .. }| {
                entries[*index]
                    .text
                    .borrow()
                    .get(range.clone())
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        SelectedText { text }
    }

    /// Gives each registered text its part of `covered` to highlight, and
    /// rebuilds only the texts whose part changed. A part that runs to its
    /// text's end is kept open-ended, so a text edited while selected
    /// highlights what it holds now; the paragraph bounds it when it builds,
    /// and its layout paints only the lines it shows. A rebuild that panics
    /// stops neither the others nor the caller's listener: its failure is
    /// held in `failure`.
    fn highlight(&self, covered: &[Covered], failure: &mut FirstPanic) {
        let changed: Vec<Rc<SelectableEntry>> = {
            let entries = self.registry.entries.borrow();
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let part = covered
                        .iter()
                        .find(|part| part.index == index && !part.range.is_empty())
                        .map(|part| {
                            let end = if part.to_end {
                                usize::MAX
                            } else {
                                part.range.end
                            };
                            part.range.start..end
                        });
                    (entry.highlight.replace(part.clone()) != part).then(|| Rc::clone(entry))
                })
                .collect()
        };
        for entry in changed {
            let rebuild = entry.rebuild.borrow().clone();
            failure.contain(|| {
                if let Some(rebuild) = rebuild {
                    rebuild.schedule(RebuildReason::StateChange);
                }
            });
        }
    }

    /// The presented texts in the order a reader takes them, each as its
    /// index and the byte where its laid-out text ends (`None` when no layout
    /// says), so the text reported for a selection is the text an adapter
    /// selected and the text the pointer selects. While semantics is on, that
    /// is the last publication's order: the assembly orders a document's
    /// texts by [`reading_order`](flui_rendering::semantics::reading_order) after any sort keys, and a
    /// text it does not publish (offstage, excluded, silenced) is left out.
    /// Without it, the same function orders the texts' laid-out boxes, and
    /// texts beneath an ancestor that leaves its subtree out of semantics are
    /// left out; texts not laid out yet read last, in registration order.
    fn reading_order(&self, entries: &[Rc<SelectableEntry>]) -> Vec<(usize, Option<usize>)> {
        let pipeline = self.pipeline.borrow().clone();
        let direction = self.direction.get();
        pipeline
            .and_then(|pipeline| {
                pipeline.try_with_layout(|owner| {
                    let shown = |index: usize| {
                        paragraph_beneath(owner, &entries[index].subtree)
                            .and_then(|(_, paragraph)| shown_end(paragraph))
                    };
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
                    let presented: Vec<usize> = (0..entries.len())
                        .filter(|&index| !hidden(owner, &entries[index].subtree))
                        .collect();
                    let boxes: Vec<Rect> = presented
                        .iter()
                        .map(|&index| {
                            laid_out_box(owner, &entries[index].subtree).unwrap_or(UNPLACED)
                        })
                        .collect();
                    flui_rendering::semantics::reading_order(&boxes, direction)
                        .into_iter()
                        .map(|at| presented[at])
                        .map(|index| (index, shown(index)))
                        .collect()
                })
            })
            .unwrap_or_else(|| (0..entries.len()).map(|index| (index, None)).collect())
    }
}

/// The box of a text not laid out yet: no edge is finite, so it reads after
/// the laid-out ones.
const UNPLACED: Rect = Rect::from_ltrb(f64::NAN, f64::NAN, f64::NAN, f64::NAN);

/// The paragraph beneath `subtree` and its render node: the first render
/// object beneath the anchor that is one; link routing may wrap it, but only
/// in a few single-child boxes.
fn paragraph_beneath<'a>(
    owner: &'a PipelineOwner,
    subtree: &SubtreeAnchor,
) -> Option<(RenderId, &'a RenderParagraph)> {
    let tree = owner.render_tree();
    let mut at = subtree.get()?;
    for _ in 0..8 {
        at = *tree.children(at).first()?;
        if let Some(paragraph) = tree
            .get(at)?
            .as_box()?
            .render_object()
            .downcast_ref::<RenderParagraph>()
        {
            return Some((at, paragraph));
        }
    }
    None
}

/// Where `paragraph`'s laid-out text ends: its last shown line's end, which
/// a line limit puts before the authored text's end.
fn shown_end(paragraph: &RenderParagraph) -> Option<usize> {
    let painter = paragraph.painter();
    if !painter.has_layout() {
        return None;
    }
    painter
        .line_layouts()
        .iter()
        .map(|line| line.runs.last().map_or(line.start, |run| run.range().end))
        .max()
}

/// The box the paragraph beneath `subtree` lays its text out in, in the
/// render root's space; `None` before its layout.
fn laid_out_box(owner: &PipelineOwner, subtree: &SubtreeAnchor) -> Option<Rect> {
    let (node, paragraph) = paragraph_beneath(owner, subtree)?;
    let painter = paragraph.painter();
    if !painter.has_layout() {
        return None;
    }
    let to_root = owner.transform_to(node, owner.root_id()?)?;
    let size = painter.size();
    let rect = to_root.transform_rect(&Rect::from_ltwh(0.0, 0.0, size.width, size.height));
    rect.is_finite().then_some(rect)
}

/// Whether the text beneath `subtree` is mounted beneath an ancestor that
/// leaves its subtree out of semantics. A text not in the render tree yet is
/// not hidden: it reads after the laid-out ones.
fn hidden(owner: &PipelineOwner, subtree: &SubtreeAnchor) -> bool {
    let tree = owner.render_tree();
    let Some(mut at) = subtree.get() else {
        return false;
    };
    loop {
        let Some(node) = tree.get(at) else {
            return false;
        };
        if node.excludes_semantics_subtree() {
            return true;
        }
        let Some(parent) = tree.parent(at) else {
            return false;
        };
        at = parent;
    }
}

/// Told the anchor of a text that left an area.
type DepartureHook = Box<dyn Fn(&SemanticsAnchor)>;

/// The texts registered with one area, in registration order.
#[derive(Default)]
pub(super) struct SelectionRegistry {
    entries: RefCell<Vec<Rc<SelectableEntry>>>,
    /// Told the anchor of each text that leaves, once the entries are no
    /// longer borrowed.
    on_departure: RefCell<Option<DepartureHook>>,
}

impl SelectionRegistry {
    fn register(&self, entry: &Rc<SelectableEntry>) {
        let mut entries = self.entries.borrow_mut();
        if !entries.iter().any(|held| Rc::ptr_eq(held, entry)) {
            entries.push(Rc::clone(entry));
        }
    }

    /// Lets `entry` go, with its highlight: a text that moves to another
    /// area must not paint this one's selection there.
    fn unregister(&self, entry: &Rc<SelectableEntry>) {
        let left = {
            let mut entries = self.entries.borrow_mut();
            let before = entries.len();
            entries.retain(|held| !Rc::ptr_eq(held, entry));
            entries.len() != before
        };
        if !left {
            return;
        }
        entry.highlight.borrow_mut().take();
        if let Some(on_departure) = self.on_departure.borrow().as_ref() {
            on_departure(&entry.anchor);
        }
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
/// One registered text: the anchor its node declares, its plain text, the
/// box that measures it, the part of it the selection highlights and what
/// rebuilds it when that part changes.
struct SelectableEntry {
    anchor: SemanticsAnchor,
    text: RefCell<String>,
    subtree: SubtreeAnchor,
    highlight: RefCell<Option<Range<usize>>>,
    rebuild: RefCell<Option<RebuildHandle>>,
}

/// One text's part of a selection: its entry's index, the bytes its layout
/// shows of that part, and whether the part runs on to the text's end
/// rather than stopping at the selection's later end.
struct Covered {
    index: usize,
    range: Range<usize>,
    to_end: bool,
}

/// The selected bytes of one text and the colour that highlights them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Highlight {
    pub(super) range: Range<usize>,
    pub(super) color: Color,
}

/// Hands the area's registry and highlight colour to the texts beneath it; a
/// disabled container hands no registry.
#[derive(Clone)]
pub(super) struct SelectionScope {
    data: ScopeData,
    child: BoxedView,
}

/// What a [`SelectionScope`] hands down.
#[derive(Clone)]
pub(super) struct ScopeData {
    registry: Option<Rc<SelectionRegistry>>,
    color: Color,
}

impl std::fmt::Debug for SelectionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionScope")
            .field("enabled", &self.data.registry.is_some())
            .field("color", &self.data.color)
            .finish_non_exhaustive()
    }
}

impl SelectionScope {
    /// The registry of the area the text at `ctx` belongs to and the colour
    /// it highlights with, registering a dependency on both.
    pub(super) fn registry_of(ctx: &dyn BuildContext) -> Option<(Rc<SelectionRegistry>, Color)> {
        ctx.depend_on::<Self, _>(|scope| {
            scope
                .data
                .registry
                .clone()
                .map(|registry| (registry, scope.data.color))
        })
        .flatten()
    }
}

impl InheritedView for SelectionScope {
    type Data = ScopeData;

    fn data(&self) -> &Self::Data {
        &self.data
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        let registry_changed = match (&self.data.registry, &old.data.registry) {
            (Some(new), Some(old)) => !Rc::ptr_eq(new, old),
            (None, None) => false,
            _ => true,
        };
        registry_changed || self.data.color != old.data.color
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
            data: ScopeData {
                registry: None,
                color: Color::TRANSPARENT,
            },
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
    /// The colour the area highlights with.
    color: Color,
}

impl SelectableParagraph {
    pub(super) fn new(
        paragraph: ResolvedParagraph,
        on_link: Option<RefCallback<TextLink>>,
        registry: Rc<SelectionRegistry>,
        color: Color,
    ) -> Self {
        Self {
            paragraph,
            on_link,
            registry,
            color,
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
                highlight: RefCell::new(None),
                rebuild: RefCell::new(None),
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.entry.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
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
            highlight: self.entry.highlight.borrow().clone().and_then(|range| {
                let length = self.entry.text.borrow().len();
                let range = range.start.min(length)..range.end.min(length);
                (!range.is_empty()).then_some(Highlight {
                    range,
                    color: view.color,
                })
            }),
            ..view.paragraph.clone()
        };
        AnchoredBox::new(
            self.entry.subtree.clone(),
            paragraph.linked(view.on_link.as_ref()),
        )
    }

    fn dispose(&mut self) {
        self.entry.rebuild.borrow_mut().take();
        self.registry.unregister(&self.entry);
    }
}
