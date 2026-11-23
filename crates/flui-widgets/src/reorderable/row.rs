//! One row of a list or grid: its drag handle, its drop target, its long press and
//! the plate a label-less drag shows.

use std::rc::Rc;

use flui_foundation::ViewKey;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::drag::{DragItem, DragReach, DragView};
use flui_interaction::gesture::MultiDragAxis;
use flui_interaction::gesture::PanClaim;
use flui_objects::SubtreeAnchor;
use flui_painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_platform::input::capabilities::PointerKindSet;

use crate::__private::AnchoredBox;
use crate::interaction::FeedbackFrame;
use crate::{DecoratedBox, DefaultFocusRingStyle, Opacity};
use flui_foundation::TextDirection;
use flui_interaction::pointer::HitTestBehavior;
use flui_platform::transfer::data::TransferActions;
use flui_view::element::ElementKind;
use flui_view::environment::GestureArenaScope;
use flui_view::prelude::*;

use crate::interaction::actions::{ambient_action_chain, resolve};
use crate::interaction::{
    Actions, Alternative, ControlSpec, GestureAlternatives, Shortcuts, ShowContextMenuIntent,
};
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, Directionality, WidgetsLocalizations,
};
use crate::{
    Center, ColoredBox, Column, ContextMenuArea, Draggable, DropTarget, Expanded, GestureDetector,
    Listener, PanClaimRegion, Row, SizedBox, Text,
};

use super::config::{Config, Layout};
use super::moves::{RowMoves, arrow, moves};
use super::session::{RowPayload, Shared};
use super::types::{ReorderHandles, ReorderableItem};

/// Makes a row's drag handles: each one drags the row it was made for.
pub(super) struct HandleSource {
    pub(super) list: Rc<Shared>,
    pub(super) key: Rc<dyn ViewKey>,
    pub(super) index: usize,
    pub(super) feedback: BoxedView,
    /// Where the row's content is published: the box the proxy shows and
    /// the row lands in.
    pub(super) content: SubtreeAnchor,
    pub(super) menu: bool,
    /// Where a press on the handle moves to drag ([`Config::handle_axis`]).
    pub(super) axis: MultiDragAxis,
}

impl HandleSource {
    /// A draggable of the row, started by `devices`; its child sits in the
    /// row's content at the place `child_offset` reads as a drag starts.
    pub(super) fn draggable(
        &self,
        child: BoxedView,
        devices: PointerKindSet,
        child_offset: Rc<dyn Fn() -> Offset<f64>>,
    ) -> Draggable {
        let list = Rc::clone(&self.list);
        let key = Rc::clone(&self.key);
        let (started, started_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (ended, ended_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (shown, shown_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (landed, landed_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (feedback, index) = (self.feedback.clone(), self.index);
        Draggable::bare(
            move || {
                vec![DragItem::new().with_value(RowPayload {
                    list: Rc::clone(&list),
                    key: Rc::clone(&key),
                })]
            },
            child,
        )
        .devices(devices)
        .reach(DragReach::Window)
        .feedback(move || shown.proxy(feedback.clone(), index, &*shown_key))
        .feedback_frame(FeedbackFrame {
            child_offset,
            ended: Rc::new(move |proxy| landed.proxy_ended(&*landed_key, proxy)),
        })
        .on_drag_started(move |_ctx| started.started(&started_key))
        // A drag that ends anywhere, dropped or not, is over no row now.
        .on_drag_end(move |_ctx, _outcome| {
            ended.ended(&ended_key);
        })
    }

    pub(super) fn handle(&self, child: BoxedView) -> BoxedView {
        // All of the handle's box takes the press, not only what it paints,
        // and a press that moves along the list drags the row: on the list's
        // axis the handle is deeper than the list's own scroll, so it wins.
        // A grid's handle drags on a press that moves any way, and keeps the
        // contact from the grid's scroll, which would take any motion along
        // its axis from a free drag (ADR-0208 rule 1).
        let child = Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(child)
            .boxed();
        // The proxy lifts from the row's content, not from the handle: the
        // handle's place in the content is read as the drag starts.
        let at = SubtreeAnchor::new();
        let child = AnchoredBox::new(at.clone(), child).boxed();
        let (list, content) = (Rc::clone(&self.list), self.content.clone());
        let child_offset =
            Rc::new(
                move || match (list.painted_at(&at), list.painted_at(&content)) {
                    (Some(handle), Some(content)) => handle - content,
                    _ => Offset::ZERO,
                },
            );
        let handle = self
            .draggable(child, PointerKindSet::all(), child_offset)
            .axis(self.axis)
            .boxed();
        let handle = match self.axis {
            MultiDragAxis::Free => PanClaimRegion::new(PanClaim::None, handle).boxed(),
            MultiDragAxis::Horizontal | MultiDragAxis::Vertical => handle,
        };
        if self.menu {
            DragHandle { child: handle }.boxed()
        } else {
            handle
        }
    }
}

/// A handle whose tap opens its row's menu.
#[derive(Clone)]
pub(super) struct DragHandle {
    pub(super) child: BoxedView,
}

impl View for DragHandle {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for DragHandle {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let chain = ambient_action_chain(ctx);
        GestureDetector::new()
            .on_tap(move |ctx| {
                if let Some(open) = chain
                    .as_ref()
                    .and_then(|chain| resolve(chain, &ShowContextMenuIntent))
                {
                    open.invoke(ctx, &ShowContextMenuIntent);
                }
            })
            .child(self.child.clone())
    }
}

/// The handle the list puts before a row's content: three bars, centred in
/// a square as large as the platform's smallest touch target.
pub(super) fn default_handle() -> BoxedView {
    let bar = || {
        SizedBox::new(18.0, 2.0)
            .child(ColoredBox::new(Color::rgb(0x49, 0x45, 0x4F)))
            .boxed()
    };
    let gap = || SizedBox::new(18.0, 3.0).boxed();
    Center::new()
        .child(
            Column::new(vec![bar(), gap(), bar(), gap(), bar()])
                .main_axis_size(flui_objects::MainAxisSize::Min),
        )
        .boxed()
}

/// `content` after `handle`, which sits in a square `side` wide (the touch
/// minimum) at the row's reading start in a vertical list, and on top in a
/// horizontal list or a grid's tile.
///
/// The handle comes before the content because keyboard focus in the row
/// reveals the row's menu control at the trailing edge, where it would
/// cover a handle.
pub(super) fn beside_handle(
    content: BoxedView,
    handle: BoxedView,
    side: f64,
    view: &Config,
) -> BoxedView {
    let handle = SizedBox::new(side, side).child(handle).boxed();
    let content = Expanded::new(content).boxed();
    match (&view.layout, view.scroll_direction) {
        (Layout::List { .. }, Axis::Vertical) => Row::new(vec![handle, content]).boxed(),
        (Layout::List { .. }, Axis::Horizontal) | (Layout::Grid { .. }, _) => {
            Column::new(vec![handle, content]).boxed()
        }
    }
}

/// One row: its moves, its menu or controls, the drop target it is and the
/// long press that drags it.
#[derive(Clone, StatelessView)]
pub(super) struct ReorderRow {
    /// The item's index in the app's model, wherever the row is shown.
    pub(super) index: usize,
    #[view(key)]
    pub(super) key: Rc<dyn ViewKey>,
    pub(super) list: Rc<Shared>,
    /// Whether the row is the gap the lead drag's item leaves: laid out and
    /// still a drop target, but not painted.
    pub(super) hidden: bool,
    /// Where the row's content is published, kept for the item.
    pub(super) anchor: SubtreeAnchor,
}

impl StatelessView for ReorderRow {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let view = self.list.view.borrow().clone();
        let index = self.index;
        let menu = view.controls.is_none();
        let item = |handle: &HandleSource| {
            (view.item)(&ReorderableItem {
                index,
                key: &*self.key,
                handle,
            })
        };
        // What the drag shows under the pointer. Never the row's own content
        // built again: that would mount a second copy of whatever the row
        // owns alone (a field's focus node, a controller).
        let anchor = self.anchor.clone();
        let bare_source = HandleSource {
            list: Rc::clone(&self.list),
            key: Rc::clone(&self.key),
            index,
            feedback: SizedBox::shrink().boxed(),
            content: anchor.clone(),
            menu,
            axis: view.handle_axis(),
        };
        let feedback = match &view.feedback {
            Some(feedback) => feedback(&ReorderableItem {
                index,
                key: &*self.key,
                handle: &bare_source,
            }),
            None => match view.item_label.as_ref().map(|label| label(index)) {
                Some(label) => Text::new(label).boxed(),
                None => RowPlate {
                    list: Rc::clone(&self.list),
                    anchor: anchor.clone(),
                    measured: Rc::default(),
                }
                .boxed(),
            },
        };
        let source = HandleSource {
            feedback,
            ..bare_source
        };
        let content = item(&source);
        let rtl = Directionality::maybe_of(ctx) == Some(TextDirection::Rtl);
        let content = match view.handles {
            ReorderHandles::Default => {
                let side = GestureArenaScope::settings_or_default_of(ctx)
                    .min_target_for(flui_platform::input::pointer::PointerKind::Touch);
                let handle = source.handle(default_handle());
                beside_handle(content, handle, side, &view)
            }
            ReorderHandles::Custom => content,
        };
        let content = if view.long_press {
            source
                .draggable(
                    content,
                    PointerKindSet::TOUCH | PointerKindSet::PEN,
                    Rc::new(|| Offset::ZERO),
                )
                .trigger(flui_interaction::drag::DragTrigger::LongPress)
                .boxed()
        } else {
            content
        };

        // The row publishes its render node, so a drag without a label shows
        // a plate of the row's size, the proxy lifts from it and the row
        // lands from where the proxy was let go.
        let content = AnchoredBox::new(anchor, content).boxed();
        let token = Rc::clone(&self.list);
        let hover = Rc::clone(&self.list);
        let row = Rc::clone(&self.key);
        let target = DropTarget::bare(
            move |drag: &DragView<'_>| {
                let payload = drag.value::<RowPayload>()?;
                if !Rc::ptr_eq(&payload.list, &token) {
                    return None;
                }
                hover.over_row(&payload.key, &row);
                Some(TransferActions::MOVE)
            },
            move |_hover| content.clone(),
        )
        .on_drop({
            let list = Rc::clone(&self.list);
            move |ctx, drag, _at, _action| {
                if let Some(payload) = drag.value::<RowPayload>()
                    && Rc::ptr_eq(&payload.list, &list)
                {
                    list.dropped(ctx, &*payload.key);
                }
            }
        });

        let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
        let strings: &dyn WidgetsLocalizations = localizations
            .as_deref()
            .map_or(&DefaultWidgetsLocalizations, |boxed| &**boxed);
        let moves = moves(strings, &view, self.list.grid.get(), index, rtl);
        // The menu, controls and assistive actions offer the moves that apply
        // now; the keys are bound for every move, and the action decides when
        // it runs from where the model has the row then.
        let commands = moves
            .iter()
            .filter(|(_, _, _, applies)| *applies)
            .map(|(command, intent, key, _)| (command.clone(), *intent, key.clone()))
            .collect::<Vec<_>>();
        let label = view.item_label.as_ref().map(|label| label(index));
        let row = match commands.split_first() {
            None => target.boxed(),
            Some(((first, _, _), more)) if menu => {
                let area = ContextMenuArea::try_new(
                    first.clone(),
                    more.iter().map(|(command, _, _)| command.clone()),
                )
                .expect("BUG: the move commands have distinct labels and keys");
                match &label {
                    Some(label) => area.target(label.clone()),
                    None => area,
                }
                .child(target)
                .boxed()
            }
            Some(((first, intent, _), more)) => {
                let placement = view.controls.unwrap_or_default();
                let spec = more.iter().fold(
                    ControlSpec::new(first.clone(), Text::new(arrow(*intent))).placement(placement),
                    |spec, (command, intent, _)| {
                        spec.control(command.clone(), Text::new(arrow(*intent)))
                    },
                );
                let alternatives = GestureAlternatives::try_new(Alternative::VisibleControl(spec))
                    .expect("BUG: the move commands have distinct labels and keys");
                match &label {
                    Some(label) => alternatives.target(label.clone()),
                    None => alternatives,
                }
                .child(target)
                .boxed()
            }
        };
        // The moves' Alt keys are bound by the row, not as the commands'
        // keys: ADR-0216 §1 keeps arrows, Home and End from gesture commands
        // for text editing, and a text field inside the row still takes them
        // first, since it handles its keys before they reach the row.
        let row = moves
            .iter()
            .fold(Shortcuts::new(row), |shortcuts, (_, intent, key, _)| {
                shortcuts.shortcut(key.clone(), *intent)
            });
        let row = Actions::new(row).action(RowMoves {
            list: Rc::clone(&self.list),
            key: Rc::clone(&self.key),
            rtl,
        });
        // Always the same wrapper, so the row keeps its state when it becomes
        // the gap and when it lands.
        Opacity::new(if self.hidden { 0.0 } else { 1.0 }).child(row)
    }
}

/// What a drag of a row without a label shows: a plate the row's size in the
/// focus ring's colors, so the drag is seen; nothing while the row is not
/// laid out.
#[derive(Clone)]
pub(super) struct RowPlate {
    pub(super) list: Rc<Shared>,
    pub(super) anchor: SubtreeAnchor,
    /// The row's size as last measured: a rebuild of the row during the
    /// drag publishes it under a new anchor, and the plate keeps its size.
    pub(super) measured: Rc<std::cell::Cell<Option<Size>>>,
}

impl View for RowPlate {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for RowPlate {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let measured = self.list.row_size(&self.anchor).or(self.measured.get());
        self.measured.set(measured);
        let Some(size) = measured else {
            return SizedBox::shrink().boxed();
        };
        let style = DefaultFocusRingStyle::of(ctx);
        let plate = BoxDecoration::new()
            .set_color(Some(style.inner.with_alpha(0xCC)))
            .set_border(Some(Border::all(BorderSide::new(
                style.outer,
                style.outer_width.max(1.0),
                BorderStyle::Solid,
            ))));
        SizedBox::new(size.width, size.height)
            .child(DecoratedBox::new(plate))
            .boxed()
    }
}
