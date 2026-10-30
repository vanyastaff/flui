//! The paragraphs that take part in an enclosing `SelectionArea`.
//!
//! [`RichText`](super::RichText) (and so [`Text`](super::Text)) under a [`SelectionScope`] with a
//! registry registers itself as a [`Participant`] while mounted. The area owns the selection;
//! each participant only paints the part of it that falls on its text and, for touch handles,
//! publishes a leader layer at the selection's ends. The area reaches a participant's
//! `RenderParagraph` through its anchor to read its text and geometry.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::ops::Range;
use std::rc::Rc;

use flui_foundation::geometry::{Matrix4, Rect};
use flui_foundation::notifier::{ChangeNotifier, Listenable};
use flui_interaction::CursorIcon;
use flui_objects::{RenderParagraph, SubtreeAnchor};
use flui_painting::styling::Color;
use flui_rendering::layer::LayerLink;
use flui_rendering::pipeline::PipelineCell;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, impl_inherited_view};

use super::linked_paragraph::LinkedParagraph;
use super::rich_text::ResolvedParagraph;
use crate::anchored_box::AnchoredBox;
use crate::composited_transform::LinkLeader;
use crate::support::RefCallback;
use crate::{AnimatedBuilder, MouseRegion, Positioned, SizedBox, Stack};
use flui_painting::typography::TextLink;

/// One paragraph taking part in a `SelectionArea`, named by the area that minted it.
///
/// The area never reuses an id, so an id held after its paragraph unmounted names nothing and
/// is refused rather than reaching another paragraph. An id means something only to the area
/// that minted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectableId(NonZeroU64);

/// What a participant paints of the area's selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ParticipantPaint {
    /// The selected byte range of the paragraph's plain text.
    pub(crate) range: Option<Range<usize>>,
    /// A leader layer at each selection end on this paragraph, in its local space: a zero-width
    /// box as tall as the end's line, for a handle to follow.
    pub(crate) leaders: Vec<(LayerLink, Rect)>,
}

/// One registered paragraph.
pub(crate) struct Participant {
    id: SelectableId,
    anchor: SubtreeAnchor,
    paint: RefCell<ParticipantPaint>,
    changed: Rc<ChangeNotifier>,
}

impl Participant {
    pub(crate) fn id(&self) -> SelectableId {
        self.id
    }

    /// The participant's render node while it is mounted.
    pub(crate) fn render_id(&self) -> Option<flui_foundation::RenderId> {
        self.anchor.get()
    }

    pub(crate) fn paint(&self) -> ParticipantPaint {
        self.paint.borrow().clone()
    }

    /// Paint `paint` from the next frame; nothing rebuilds when it did not change.
    pub(crate) fn set_paint(&self, paint: ParticipantPaint) {
        if *self.paint.borrow() == paint {
            return;
        }
        let _previous = self.paint.replace(paint);
        self.changed.notify_listeners();
    }

    /// Run `f` with the laid-out `RenderParagraph` and its transform to the root. `None` when
    /// the paragraph is not mounted or laid out, or the tree is busy.
    pub(crate) fn with_paragraph<R>(
        &self,
        owner: &PipelineCell,
        f: impl FnOnce(&RenderParagraph, &Matrix4) -> Option<R>,
    ) -> Option<R> {
        let anchor = self.anchor.get()?;
        owner
            .try_with_layout(|owner| {
                let root = owner.root_id()?;
                let tree = owner.render_tree();
                // The paragraph is the first leaf under the anchor: the cursor region and the
                // stack of leaders sit between them, and a linked paragraph adds its own nodes.
                let mut node = anchor;
                let paragraph = loop {
                    let object = tree.get(node)?.as_box()?.render_object();
                    if let Some(paragraph) = object.downcast_ref::<RenderParagraph>() {
                        break paragraph;
                    }
                    node = *tree.children(node).first()?;
                };
                if !paragraph.painter().has_layout() {
                    return None;
                }
                let to_root = owner.transform_to(node, root)?;
                f(paragraph, &to_root)
            })
            .flatten()
    }
}

/// Told which participant left.
pub(crate) type RemovedHook = Rc<dyn Fn(SelectableId)>;

/// The participants of one `SelectionArea` and how it paints them.
pub(crate) struct SelectionRegistry {
    next: Cell<u64>,
    participants: RefCell<Vec<Rc<Participant>>>,
    color: Cell<Color>,
    /// Told after a participant left, so the area moves a selection end off it.
    on_removed: RefCell<Option<RemovedHook>>,
}

impl SelectionRegistry {
    pub(crate) fn new(color: Color) -> Rc<Self> {
        Rc::new(Self {
            next: Cell::new(1),
            participants: RefCell::new(Vec::new()),
            color: Cell::new(color),
            on_removed: RefCell::new(None),
        })
    }

    /// A new participant, or `None` once the area has minted every id it can; such a paragraph
    /// shows its text but takes no part in the selection.
    fn register(&self) -> Option<Rc<Participant>> {
        let serial = self.next.get();
        let id = SelectableId(NonZeroU64::new(serial)?);
        self.next.set(serial.checked_add(1)?);
        let participant = Rc::new(Participant {
            id,
            anchor: SubtreeAnchor::new(),
            paint: RefCell::new(ParticipantPaint::default()),
            changed: Rc::new(ChangeNotifier::new()),
        });
        self.participants.borrow_mut().push(Rc::clone(&participant));
        Some(participant)
    }

    fn unregister(&self, id: SelectableId) {
        let removed = {
            let mut participants = self.participants.borrow_mut();
            let before = participants.len();
            participants.retain(|participant| participant.id != id);
            before != participants.len()
        };
        let hook = self.on_removed.borrow().clone();
        if removed && let Some(hook) = hook {
            hook(id);
        }
    }

    /// The registered participants, in registration order.
    pub(crate) fn participants(&self) -> Vec<Rc<Participant>> {
        self.participants.borrow().clone()
    }

    pub(crate) fn get(&self, id: SelectableId) -> Option<Rc<Participant>> {
        self.participants
            .borrow()
            .iter()
            .find(|participant| participant.id == id)
            .cloned()
    }

    pub(crate) fn color(&self) -> Color {
        self.color.get()
    }

    /// Paint the selection in `color`, repainting each participant when it changed.
    pub(crate) fn set_color(&self, color: Color) {
        if self.color.replace(color) != color {
            for participant in self.participants() {
                participant.changed.notify_listeners();
            }
        }
    }

    pub(crate) fn set_on_removed(&self, hook: Option<RemovedHook>) {
        let _previous = self.on_removed.replace(hook);
    }
}

/// Puts the paragraphs beneath it into `registry`, or with none, keeps them out of any area
/// above.
#[derive(Clone)]
pub(crate) struct SelectionScope {
    registry: Option<Rc<SelectionRegistry>>,
    child: BoxedView,
}

impl SelectionScope {
    pub(crate) fn new(registry: Option<Rc<SelectionRegistry>>, child: impl IntoView) -> Self {
        Self {
            registry,
            child: child.into_view().boxed(),
        }
    }

    /// The registry the paragraphs at `ctx` join, if any.
    pub(crate) fn registry_of(ctx: &dyn BuildContext) -> Option<Rc<SelectionRegistry>> {
        ctx.depend_on::<Self, _>(|scope| scope.registry.clone())
            .flatten()
    }
}

impl std::fmt::Debug for SelectionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionScope")
            .field("selectable", &self.registry.is_some())
            .finish_non_exhaustive()
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

/// A paragraph that registers with `registry` while mounted and paints its part of the
/// selection.
#[derive(Clone)]
pub(super) struct SelectableParagraph {
    registry: Rc<SelectionRegistry>,
    paragraph: ResolvedParagraph,
    on_link: Option<RefCallback<TextLink>>,
}

impl SelectableParagraph {
    pub(super) fn new(
        registry: Rc<SelectionRegistry>,
        paragraph: ResolvedParagraph,
        on_link: Option<RefCallback<TextLink>>,
    ) -> Self {
        Self {
            registry,
            paragraph,
            on_link,
        }
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
            joined: self.registry.register().map(|participant| Joined {
                registry: Rc::clone(&self.registry),
                participant,
            }),
        }
    }
}

struct Joined {
    registry: Rc<SelectionRegistry>,
    participant: Rc<Participant>,
}

impl Drop for Joined {
    fn drop(&mut self) {
        self.registry.unregister(self.participant.id);
    }
}

/// A mounted [`SelectableParagraph`]: its registration, given back when it unmounts or moves
/// to another area.
pub(super) struct SelectableParagraphState {
    joined: Option<Joined>,
}

impl std::fmt::Debug for SelectableParagraphState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectableParagraphState")
            .field(
                "id",
                &self.joined.as_ref().map(|joined| joined.participant.id),
            )
            .finish()
    }
}

impl ViewState<SelectableParagraph> for SelectableParagraphState {
    fn did_update_view(&mut self, _old: &SelectableParagraph, new: &SelectableParagraph) {
        let moved = self
            .joined
            .as_ref()
            .is_none_or(|joined| !Rc::ptr_eq(&joined.registry, &new.registry));
        if moved {
            // The old registration leaves before the new one joins, so an area never holds
            // one paragraph twice.
            self.joined = None;
            self.joined = new.registry.register().map(|participant| Joined {
                registry: Rc::clone(&new.registry),
                participant,
            });
        }
    }

    fn dispose(&mut self) {
        self.joined = None;
    }

    fn build(&self, view: &SelectableParagraph, _ctx: &dyn BuildContext) -> impl IntoView {
        let Some(joined) = &self.joined else {
            return paragraph_view(view.paragraph.clone(), view.on_link.clone());
        };
        let participant = Rc::clone(&joined.participant);
        let registry = Rc::clone(&joined.registry);
        let paragraph = view.paragraph.clone();
        let on_link = view.on_link.clone();
        let listenable = Rc::clone(&participant.changed) as Rc<dyn Listenable>;
        let painted = AnimatedBuilder::new(listenable, move || {
            let paint = participant.paint();
            let mut paragraph = paragraph.clone();
            paragraph.selection = paint.range;
            paragraph.selection_color = registry.color();
            let mut layers = vec![paragraph_view(paragraph, on_link.clone())];
            layers.extend(paint.leaders.into_iter().map(|(link, rect)| {
                Positioned::new(LinkLeader::new(link, SizedBox::new(0.0, rect.height())))
                    .left(rect.left())
                    .top(rect.top())
                    .boxed()
            }));
            Stack::new(layers)
        });
        MouseRegion::new()
            .cursor(CursorIcon::Text)
            .child(AnchoredBox::new(joined.participant.anchor.clone(), painted))
            .boxed()
    }
}

fn paragraph_view(
    paragraph: ResolvedParagraph,
    on_link: Option<RefCallback<TextLink>>,
) -> BoxedView {
    match on_link {
        Some(on_link) => LinkedParagraph::new(paragraph, on_link).boxed(),
        None => paragraph.boxed(),
    }
}
