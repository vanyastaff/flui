//! A controlled expandable section with one focusable header.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{Point, Size};
use flui_interaction::routing::FocusNode;
use flui_objects::{CrossAxisAlignment, MainAxisSize};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_painting::{Canvas, Paint};
use flui_rendering::delegates::CustomPainter;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, RebuildHandle, RebuildReason, WriterSource};

use crate::support::retirement::Terminal;
use crate::support::{ValueCallback, value_callback};
use crate::{
    Action, ActionOutcome, Actions, ActivateIntent, ButtonActivateIntent, ClipRect, Column,
    CustomPaint, Directionality, Flexible, Focus, GestureDetector, Intent, LayoutBuilder,
    MergeSemantics, Row, Semantics,
};

/// The application-owned state of a [`Disclosure`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExpansionState {
    /// Show only the header.
    #[default]
    Collapsed,
    /// Show the header and body.
    Expanded,
}

impl ExpansionState {
    fn toggled(self) -> Self {
        match self {
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Collapsed,
        }
    }
}

/// An expandable section whose state belongs to its application.
///
/// A header tap, Enter or Space proposes the opposite state through
/// [`on_changed`](Self::on_changed). The section changes only when its owner
/// rebuilds it with that state. Without a callback the header is disabled.
/// Assistive expand/collapse requests propose their explicit target only when
/// it differs from the current state. The collapsed body is unmounted, so its
/// layout, paint, input, focus and accessibility contributions are absent.
/// The platform advertises Expand while collapsed and Collapse while expanded;
/// a retained request that arrives after that transition is an idempotent no-op.
///
/// The indicator follows the ambient reading direction unless
/// [`text_direction`](Self::text_direction) explicitly overrides it. The
/// section clips its paint and hit targets to its allocated bounds, including
/// constraints smaller than the preferred indicator size.
/// The header should be passive visual content. Its accessible text is merged
/// into the control unless [`label`](Self::label) supplies an explicit name;
/// an explicit name excludes descendant header semantics, leaving the body
/// as a separate accessible subtree.
#[derive(StatefulView)]
pub struct Disclosure {
    state: ExpansionState,
    header: Terminal<BoxedView>,
    body: Terminal<BoxedView>,
    on_changed: Terminal<Option<ValueCallback<ExpansionState>>>,
    direction: Option<TextDirection>,
    indicator_color: Color,
    label: Terminal<Option<String>>,
}

impl std::fmt::Debug for Disclosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Disclosure")
            .field("state", &self.state)
            .field("enabled", &self.on_changed.is_some())
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

impl Clone for Disclosure {
    fn clone(&self) -> Self {
        let header = Terminal::new((*self.header).clone());
        let body = Terminal::new((*self.body).clone());
        let on_changed = Terminal::new((*self.on_changed).clone());
        let label = Terminal::new((*self.label).clone());
        Self {
            state: self.state,
            header,
            body,
            on_changed,
            direction: self.direction,
            indicator_color: self.indicator_color,
            label,
        }
    }
}

impl Drop for Disclosure {
    fn drop(&mut self) {
        let header = self.header.withdraw();
        let body = self.body.withdraw();
        let callback = self.on_changed.withdraw();
        let label = self.label.withdraw();
        drop((header, body, callback, label));
    }
}

impl Disclosure {
    /// Create a disabled section with application-owned `state` and children.
    #[must_use]
    pub fn new(state: ExpansionState, header: impl IntoView, body: impl IntoView) -> Self {
        let mut header = Terminal::new(header);
        let mut body = Terminal::new(body);
        let header = Terminal::new(header.take_value().into_view().boxed());
        let body = Terminal::new(body.take_value().into_view().boxed());
        Self {
            state,
            header,
            body,
            on_changed: Terminal::new(None),
            direction: None,
            indicator_color: Color::BLACK,
            label: Terminal::new(None),
        }
    }

    /// Enable activation and report proposed state changes in their event.
    #[must_use]
    pub fn on_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ExpansionState) -> R + 'static,
        R: EventOutcome,
    {
        let callback = Terminal::new(callback);
        self.on_changed = Terminal::new(Some(value_callback(move |cx, state| callback(cx, state))));
        self
    }

    /// Override the ambient reading direction for both content and indicator.
    #[must_use]
    pub fn text_direction(mut self, direction: TextDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    /// Set the indicator's color (default black).
    #[must_use]
    pub fn indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = color;
        self
    }

    /// Name the header explicitly, replacing its descendants' accessible text.
    /// Body semantics remain separate and are never excluded by this override.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        let mut label = Terminal::new(label);
        self.label = Terminal::new(Some(label.take_value().into()));
        self
    }
}

/// Only focus presentation is local; expansion is always controlled.
pub struct DisclosureState {
    node: Terminal<Rc<FocusNode>>,
    focused: Terminal<Rc<Cell<bool>>>,
    rebuild: Terminal<Option<RebuildHandle>>,
    live: Terminal<Rc<RefCell<LiveDisclosure>>>,
}

impl std::fmt::Debug for DisclosureState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisclosureState")
            .field("node_id", &self.node.id())
            .field("focused", &self.focused.get())
            .finish_non_exhaustive()
    }
}

impl Drop for DisclosureState {
    fn drop(&mut self) {
        let (handler, writer) = {
            let mut live = self.live.borrow_mut();
            live.mounted = false;
            (live.handler.withdraw(), live.writer.withdraw())
        };
        let node = self.node.withdraw();
        let focused = self.focused.withdraw();
        let rebuild = self.rebuild.withdraw();
        let live = self.live.withdraw();
        drop((handler, writer, node, focused, rebuild, live));
    }
}

struct LiveDisclosure {
    mounted: bool,
    state: ExpansionState,
    handler: Terminal<Option<ValueCallback<ExpansionState>>>,
    writer: Terminal<Option<WriterSource>>,
}

impl Drop for LiveDisclosure {
    fn drop(&mut self) {
        self.mounted = false;
        let handler = self.handler.withdraw();
        let writer = self.writer.withdraw();
        drop((handler, writer));
    }
}

fn propose(
    live: &Rc<RefCell<LiveDisclosure>>,
    cx: &mut EventCx<'_>,
    target: Option<ExpansionState>,
) -> bool {
    let (handler, writer, target) = {
        let current = live.borrow();
        if !current.mounted {
            return false;
        }
        let target = target.unwrap_or_else(|| current.state.toggled());
        if target == current.state {
            return false;
        }
        let Some(handler) = current.handler.as_ref() else {
            return false;
        };
        let Some(writer) = current.writer.as_ref() else {
            return false;
        };
        (
            Terminal::new(Rc::clone(handler)),
            Terminal::new(writer.clone()),
            target,
        )
    };
    if writer.check_context(cx).is_err() {
        return false;
    }
    handler(cx, target);
    true
}

impl StatefulView for Disclosure {
    type State = DisclosureState;

    fn create_state(&self) -> Self::State {
        DisclosureState {
            node: Terminal::new(FocusNode::with_debug_label("Disclosure")),
            focused: Terminal::new(Rc::new(Cell::new(false))),
            rebuild: Terminal::new(None),
            live: Terminal::new(Rc::new(RefCell::new(LiveDisclosure {
                mounted: false,
                state: self.state,
                handler: Terminal::new((*self.on_changed).clone()),
                writer: Terminal::new(None),
            }))),
        }
    }
}

impl ViewState<Disclosure> for DisclosureState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rebuild = Terminal::new(Some(ctx.rebuild_handle()));
        let mut live = self.live.borrow_mut();
        live.writer = Terminal::new(Some(ctx.writer_source()));
        live.mounted = true;
    }

    fn did_update_view(&mut self, _old: &Disclosure, new: &Disclosure) {
        let incoming = Terminal::new((*new.on_changed).clone());
        let outgoing = {
            let mut live = self.live.borrow_mut();
            live.state = new.state;
            let outgoing = live.handler.withdraw();
            live.handler = incoming;
            outgoing
        };
        drop(outgoing);
    }

    fn dispose(&mut self) {
        let outgoing = {
            let mut live = self.live.borrow_mut();
            live.mounted = false;
            let handler = live.handler.withdraw();
            let writer = live.writer.withdraw();
            (handler, writer)
        };
        drop(outgoing);
    }

    fn build(&self, view: &Disclosure, ctx: &dyn BuildContext) -> impl IntoView {
        let direction = view
            .direction
            .unwrap_or_else(|| Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr));
        let snapshot = Terminal::new(view.clone());
        let node = Terminal::new(Rc::clone(&self.node));
        let focused = Terminal::new(Rc::clone(&self.focused));
        let live = Terminal::new(Rc::clone(&self.live));
        let rebuild = Terminal::new(
            self.rebuild
                .clone()
                .expect("BUG: Disclosure built before init_state"),
        );
        LayoutBuilder::new(move |_ctx, constraints| {
            let side = 18.0_f64
                .min(constraints.max_width)
                .min(constraints.max_height)
                .max(0.0);
            let header = Terminal::new((*snapshot.header).clone());
            let mut header = header;
            let header = Flexible::new(header.take_value()).boxed();
            let indicator = CustomPaint::new()
                .size(Size::new(side, side))
                .painter(Arc::new(Chevron {
                    state: snapshot.state,
                    direction,
                    color: snapshot.indicator_color,
                    focused: focused.get(),
                }))
                .boxed();
            let header = Row::new(GuardedChildren::new([header, indicator]))
                .main_axis_size(MainAxisSize::Min);
            let enabled = snapshot.on_changed.is_some();
            let header = if enabled {
                let live = Rc::clone(&live);
                let node = Terminal::new(Rc::clone(&node));
                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .child(header)
                    .on_tap(move |cx| {
                        let _ = node.request_focus();
                        // Focus notifications can unmount, disable or reconfigure us.
                        // Re-admit against the current owner after that user-code edge.
                        if node.is_attached() {
                            let _ = propose(&live, cx, None);
                        }
                    })
                    .boxed()
            } else {
                header.boxed()
            };
            let mut semantics = Semantics::new()
                .button(true)
                .enabled(enabled)
                .expanded(snapshot.state == ExpansionState::Expanded)
                .child(header);
            if let Some(label) = snapshot.label.as_ref() {
                semantics = semantics.label(label.clone()).exclude_semantics(true);
            }
            if enabled {
                let activate = Rc::clone(&live);
                let expand = Rc::clone(&live);
                let collapse = Rc::clone(&live);
                semantics = semantics
                    .on_tap(move |cx| {
                        let _ = propose(&activate, cx, None);
                    })
                    .on_expand(move |cx| {
                        let _ = propose(&expand, cx, Some(ExpansionState::Expanded));
                    })
                    .on_collapse(move |cx| {
                        let _ = propose(&collapse, cx, Some(ExpansionState::Collapsed));
                    });
            }
            let changed = Rc::clone(&focused);
            let rebuild = rebuild.clone();
            let header = Focus::new(semantics)
                .focus_node(Rc::clone(&node))
                .can_request_focus(enabled)
                .on_focus_change(move |_cx, next| {
                    if changed.replace(next) != next {
                        rebuild.schedule(RebuildReason::StateChange);
                    }
                });
            let mut children = GuardedChildren::default();
            children.push(
                // Annotated descendants otherwise publish separate nodes: the
                // header's name and its actual Focus/actions must share one node.
                Actions::new(MergeSemantics::new().child(header))
                    .action::<ActivateIntent>(DisclosureActivate {
                        live: Rc::clone(&live),
                    })
                    .action::<ButtonActivateIntent>(DisclosureActivate {
                        live: Rc::clone(&live),
                    })
                    .boxed(),
            );
            if snapshot.state == ExpansionState::Expanded {
                let mut body = Terminal::new((*snapshot.body).clone());
                children.push(Flexible::new(body.take_value()).boxed());
            }
            Directionality::new(
                direction,
                ClipRect::new().child(
                    Column::new(children)
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Start),
                ),
            )
        })
    }
}

struct DisclosureActivate {
    live: Rc<RefCell<LiveDisclosure>>,
}

impl<T: Intent> Action<T> for DisclosureActivate {
    fn is_enabled(&self, _intent: &T) -> bool {
        let live = self.live.borrow();
        live.mounted && live.handler.is_some()
    }

    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &T) -> ActionOutcome {
        if propose(&self.live, cx, None) {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

#[derive(Default)]
struct GuardedChildren(Vec<Terminal<BoxedView>>);

impl GuardedChildren {
    fn new(children: impl IntoIterator<Item = BoxedView>) -> Self {
        Self(children.into_iter().map(Terminal::new).collect())
    }

    fn push(&mut self, child: BoxedView) {
        self.0.push(Terminal::new(child));
    }
}

impl Clone for GuardedChildren {
    fn clone(&self) -> Self {
        Self(
            self.0
                .iter()
                .map(|child| Terminal::new((**child).clone()))
                .collect(),
        )
    }
}

impl ViewSeq for GuardedChildren {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn for_each<F: FnMut(usize, &dyn View)>(&self, mut f: F) {
        for (index, child) in self.0.iter().enumerate() {
            f(index, &**child);
        }
    }

    fn into_boxed_vec(self) -> Vec<BoxedView> {
        self.0
            .into_iter()
            .map(|mut child| child.take_value())
            .collect()
    }
}

#[derive(Debug)]
struct Chevron {
    state: ExpansionState,
    direction: TextDirection,
    color: Color,
    focused: bool,
}

impl CustomPainter for Chevron {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return;
        }
        let points = match (self.state, self.direction) {
            (ExpansionState::Expanded, _) => [(0.25, 0.35), (0.5, 0.65), (0.75, 0.35)],
            (ExpansionState::Collapsed, TextDirection::Ltr) => {
                [(0.35, 0.25), (0.65, 0.5), (0.35, 0.75)]
            }
            (ExpansionState::Collapsed, TextDirection::Rtl) => {
                [(0.65, 0.25), (0.35, 0.5), (0.65, 0.75)]
            }
        };
        let width = if self.focused { 3.0_f64 } else { 1.5_f64 };
        let paint = Paint::stroke(
            self.color,
            width.min(size.width * 0.2).min(size.height * 0.2),
        );
        let point = |(x, y)| Point::new(size.width * x, size.height * y);
        canvas.draw_line(point(points[0]), point(points[1]), &paint);
        canvas.draw_line(point(points[1]), point(points[2]), &paint);
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any().downcast_ref::<Self>().is_none_or(|old| {
            self.state != old.state
                || self.direction != old.direction
                || self.color != old.color
                || self.focused != old.focused
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
