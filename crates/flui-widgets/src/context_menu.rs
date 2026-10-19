//! [`ContextMenuArea`] — a region whose commands open in a theme-free context
//! menu (ADR-0216 §2).
//!
//! The area mounts [`GestureAlternatives`] with an
//! [`Alternative::ContextMenu`] around its content, so every way to open the
//! menu is the alternatives': a secondary click, a touch or pen long press
//! composed as `Sequenced`, the platform's menu keys, assistive technology's
//! `ShowContextMenu` and the control focus reveals. The area is the
//! [`MenuSpec`]'s presenter: it shows the request as a popup route on the
//! nearest [`Navigator`](crate::Navigator), which gives the menu its focus
//! trap, the focus restored on close, Escape and a tap outside as dismissal,
//! and the page below hidden from assistive technology while it is open.
//!
//! The menu is plain: a white panel of rows, one per command, each labelled
//! by its command. The design packages style it.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_interaction::PointerKind;
use flui_interaction::focus::FocusNode;
use flui_objects::{MainAxisSize, SubtreeAnchor};
use flui_painting::AlignmentDirectional;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform_api::keyboard::NamedKey;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::delegates::SingleChildLayoutDelegate;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::PipelineCell;
use flui_view::BoxedView;
use flui_view::prelude::*;

use crate::anchored_box::AnchoredBox;
use crate::interaction::{
    Action, ActionOutcome, Actions, ActivateIntent, ActivationPressIntent, Alternative,
    ButtonActivateIntent, Focus, FocusRing, GestureAlternatives, GestureAlternativesError,
    GestureArenaScope, GestureCommand, GestureDetector, Intent, Listener, MenuItem, MenuRequest,
    MenuSpec, Shortcuts, SingleActivator,
};
use crate::localization::Directionality;
use crate::navigator::{NavigatorHandle, PopupRoute, RouteId};
use crate::{
    Align, ColoredBox, Column, ConstrainedBox, CustomSingleChildLayout, ExcludeSemantics,
    IntrinsicWidth, Padding, Semantics, SemanticsRole, Text,
};

/// A region whose commands are reached through a context menu.
///
/// ```rust,ignore
/// ContextMenuArea::new(
///     GestureCommand::new("Delete Alice", Delete("Alice")),
///     [GestureCommand::new("Archive Alice", Archive("Alice"))],
/// )?
/// .target("Alice")
/// .child(message_row)
/// ```
///
/// The menu opens on a secondary click, on a touch or pen long press, on the
/// platform's menu keys, on assistive technology's `ShowContextMenu` and from
/// the control keyboard focus reveals in the area; each command is also the
/// area's own custom action. Choosing an item closes the menu and performs
/// its command's intent through the [`Actions`] chain where the area is
/// mounted.
///
/// The menu sits at the pointer that opened it, or below the area when a key
/// or assistive technology did, starting at the reading direction's leading
/// edge, and flips to the other side of that point where the window has no
/// room for it. While it is open the arrow keys move between its items
/// (wrapping), Home and End go to the first and the last, Tab stays inside,
/// and Escape or a tap outside closes it and returns the focus to where it
/// was. Unmounting the area closes its menu.
///
/// The menu is a popup route, so the area needs a
/// [`Navigator`](crate::Navigator) above it, as `WidgetsApp` provides.
#[derive(Clone, StatefulView)]
pub struct ContextMenuArea {
    /// The checked commands as a menu; the mounted area presents it.
    alternatives: GestureAlternatives,
    child: Option<BoxedView>,
}

impl ContextMenuArea {
    /// An area whose menu shows `first`, then `more`.
    ///
    /// # Errors
    ///
    /// [`GestureAlternativesError::EmptyLabel`] for a command without a
    /// label, [`GestureAlternativesError::DuplicateLabel`] for two commands
    /// with one.
    pub fn new(
        first: GestureCommand,
        more: impl IntoIterator<Item = GestureCommand>,
    ) -> Result<Self, GestureAlternativesError> {
        let menu = more
            .into_iter()
            .fold(MenuSpec::new(first, |_cx, _request| {}), MenuSpec::item);
        Ok(Self {
            alternatives: GestureAlternatives::new(Alternative::ContextMenu(menu))?,
            child: None,
        })
    }

    /// Name what the commands act on ("Alice"), for the labels the area
    /// makes itself, such as the control focus reveals.
    #[must_use]
    pub fn target(mut self, name: impl Into<String>) -> Self {
        self.alternatives = self.alternatives.target(name);
        self
    }

    /// The area's content.
    #[must_use]
    pub fn child(mut self, content: impl IntoView) -> Self {
        self.child = Some(content.into_view().boxed());
        self
    }

    /// The area's commands as a context menu; the mounted area presents it.
    pub(crate) fn alternatives(&self) -> &GestureAlternatives {
        &self.alternatives
    }
}

impl fmt::Debug for ContextMenuArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextMenuArea")
            .field(
                "items",
                &self
                    .alternatives
                    .commands()
                    .map(GestureCommand::label)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl StatefulView for ContextMenuArea {
    type State = ContextMenuAreaState;

    fn create_state(&self) -> Self::State {
        ContextMenuAreaState {
            anchor: SubtreeAnchor::new(),
            session: Rc::new(MenuSession::default()),
            owner: None,
        }
    }
}

/// What a mounted [`ContextMenuArea`] keeps: where its content is, and the
/// menu it has open.
pub struct ContextMenuAreaState {
    anchor: SubtreeAnchor,
    session: Rc<MenuSession>,
    owner: Option<PipelineCell>,
}

impl fmt::Debug for ContextMenuAreaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextMenuAreaState")
            .field("open", &self.session.is_open())
            .finish_non_exhaustive()
    }
}

impl ViewState<ContextMenuArea> for ContextMenuAreaState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.owner = ctx.pipeline_owner();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.owner = ctx.pipeline_owner();
    }

    /// The anchor of an open menu is gone, so the menu closes with it.
    fn dispose(&mut self) {
        self.session.close_current();
    }

    fn build(&self, view: &ContextMenuArea, ctx: &dyn BuildContext) -> impl IntoView {
        let presenter = Presenter {
            session: Rc::clone(&self.session),
            navigator: NavigatorHandle::maybe_of(ctx),
            anchor: self.anchor.clone(),
            owner: self.owner.clone(),
            direction: Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr),
        };
        let content = view
            .child
            .clone()
            .unwrap_or_else(|| crate::SizedBox::shrink().boxed());
        // The whole area is a trigger, not just what its content paints.
        let content = Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(AnchoredBox::new(self.anchor.clone(), content));
        // A long press that hands its contact to a drag closes the menu it
        // opened.
        let session = Rc::clone(&self.session);
        view.alternatives
            .clone()
            .present_menu_with(
                Rc::new(move |_cx, request| presenter.present(request)),
                move |_cx| session.close_current(),
            )
            .child(content)
    }
}

// ============================================================================
// Opening and closing
// ============================================================================

/// The menu an area has open, shared by the area and the menu's page.
#[derive(Default)]
struct MenuSession {
    open: RefCell<Option<Rc<OpenMenu>>>,
}

/// One shown menu: the navigator it is a route of, and that route.
struct OpenMenu {
    navigator: NavigatorHandle,
    route: Cell<Option<RouteId>>,
}

impl MenuSession {
    fn is_open(&self) -> bool {
        self.open.borrow().is_some()
    }

    fn is_current(&self, menu: &Rc<OpenMenu>) -> bool {
        self.open
            .borrow()
            .as_ref()
            .is_some_and(|open| Rc::ptr_eq(open, menu))
    }

    /// Close whatever menu is open, removing its route.
    fn close_current(&self) {
        let open = self.open.take();
        if let Some(open) = open
            && let Some(route) = open.route.get()
        {
            let _ = open.navigator.remove_route(route);
        }
    }

    /// Close `menu` if it is the one open.
    fn close(&self, menu: &Rc<OpenMenu>) {
        if self.is_current(menu) {
            self.close_current();
        }
    }

    /// `menu`'s route is gone (dismissed, or removed with its navigator).
    fn closed(&self, menu: &Rc<OpenMenu>) {
        if self.is_current(menu) {
            let outgoing = self.open.take();
            drop(outgoing);
        }
    }
}

/// Shows an area's menu requests.
struct Presenter {
    session: Rc<MenuSession>,
    navigator: Option<NavigatorHandle>,
    anchor: SubtreeAnchor,
    owner: Option<PipelineCell>,
    direction: TextDirection,
}

impl Presenter {
    fn present(&self, request: MenuRequest) {
        let Some(navigator) = self.navigator.clone() else {
            tracing::warn!(
                target: "flui::a11y",
                "a ContextMenuArea has no Navigator above it, so its menu cannot open"
            );
            return;
        };
        self.session.close_current();
        let placement = MenuPlacement {
            anchor: self.anchor_in_overlay(&navigator, request.position()),
            direction: self.direction,
        };
        let menu = Rc::new(OpenMenu {
            navigator: navigator.clone(),
            route: Cell::new(None),
        });
        let outgoing = self.session.open.replace(Some(Rc::clone(&menu)));
        drop(outgoing);
        let panel = MenuPanel {
            items: request.items().into(),
            placement,
            session: Rc::clone(&self.session),
            menu: Rc::clone(&menu),
        };
        let route =
            PopupRoute::<()>::new(move |_ctx, _animation, _secondary| panel.clone().boxed())
                .barrier_dismissible(true)
                .transition_duration(Duration::ZERO)
                .reverse_transition_duration(Duration::ZERO)
                .named("ContextMenuArea");
        match navigator.push_identified(route) {
            Some(route) => menu.route.set(Some(route)),
            None => self.session.closed(&menu),
        }
    }

    /// Where the menu hangs from, in the navigator overlay's space: the
    /// pointer that opened it, or else the area's bounds.
    fn anchor_in_overlay(&self, navigator: &NavigatorHandle, pointer: Option<Offset<f64>>) -> Rect {
        let overlay = navigator.overlay().render_id();
        let owner = self.owner.as_ref();
        let in_root = match pointer {
            Some(at) => Some(Rect::from_ltwh(at.dx, at.dy, 0.0, 0.0)),
            None => owner.and_then(|owner| {
                let area = self.anchor.get()?;
                owner
                    .try_with_layout(|owner| {
                        let size = owner.box_size(area)?;
                        let root = owner.root_id()?;
                        let transform = owner.transform_to(area, root)?;
                        Some(transform.transform_rect(&Rect::from_ltwh(
                            0.0,
                            0.0,
                            size.width,
                            size.height,
                        )))
                    })
                    .flatten()
            }),
        };
        let Some(in_root) = in_root else {
            return Rect::from_ltwh(0.0, 0.0, 0.0, 0.0);
        };
        let in_overlay = owner.zip(overlay).and_then(|(owner, overlay)| {
            owner
                .try_with_layout(|owner| {
                    let corner =
                        |x: f64, y: f64| owner.global_to_local(overlay, Point::new(x, y), None);
                    let top_left = corner(in_root.left(), in_root.top())?;
                    let bottom_right = corner(in_root.right(), in_root.bottom())?;
                    Some(Rect::from_points(top_left, bottom_right))
                })
                .flatten()
        });
        in_overlay.unwrap_or(in_root)
    }
}

// ============================================================================
// The menu
// ============================================================================

/// Where the menu goes inside the overlay: hanging from `anchor` towards the
/// reading direction's trailing side and down, flipped across the anchor on
/// an axis without room, then kept inside the overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MenuPlacement {
    anchor: Rect,
    direction: TextDirection,
}

impl MenuPlacement {
    fn position(&self, size: Size, menu: Size) -> Offset<f64> {
        let (anchor, width, height) = (self.anchor, menu.width, menu.height);
        let x = match self.direction {
            TextDirection::Ltr if anchor.left() + width <= size.width => anchor.left(),
            TextDirection::Ltr => anchor.right() - width,
            TextDirection::Rtl if anchor.right() - width >= 0.0 => anchor.right() - width,
            TextDirection::Rtl => anchor.left(),
        };
        let y = if anchor.bottom() + height <= size.height {
            anchor.bottom()
        } else {
            anchor.top() - height
        };
        let inside = |at: f64, extent: f64, room: f64| {
            let at = at.min(room - extent).max(0.0);
            if at.is_finite() { at } else { 0.0 }
        };
        Offset::new(inside(x, width, size.width), inside(y, height, size.height))
    }
}

impl SingleChildLayoutDelegate for MenuPlacement {
    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.loosen()
    }

    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
        self.position(size, child_size)
    }

    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        old_delegate.as_any().downcast_ref::<Self>() != Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The menu's page: the panel of items, placed.
#[derive(Clone, StatefulView)]
struct MenuPanel {
    items: Rc<[MenuItem]>,
    placement: MenuPlacement,
    session: Rc<MenuSession>,
    menu: Rc<OpenMenu>,
}

struct MenuPanelState {
    nodes: Rc<[Rc<FocusNode>]>,
    session: Rc<MenuSession>,
    menu: Rc<OpenMenu>,
}

impl StatefulView for MenuPanel {
    type State = MenuPanelState;

    fn create_state(&self) -> Self::State {
        MenuPanelState {
            nodes: self
                .items
                .iter()
                .map(|_| FocusNode::with_debug_label("ContextMenuArea item"))
                .collect(),
            session: Rc::clone(&self.session),
            menu: Rc::clone(&self.menu),
        }
    }
}

impl ViewState<MenuPanel> for MenuPanelState {
    /// The page goes when its route does, however it was closed.
    fn dispose(&mut self) {
        self.session.closed(&self.menu);
    }

    fn build(&self, view: &MenuPanel, ctx: &dyn BuildContext) -> impl IntoView {
        let min_target =
            GestureArenaScope::settings_or_default_of(ctx).min_target_for(PointerKind::Touch);
        let direction = view.placement.direction;
        let rows: Vec<BoxedView> = view
            .items
            .iter()
            .zip(self.nodes.iter())
            .map(|(item, node)| {
                let choose = Choose(Rc::new({
                    let (item, session, menu) = (
                        item.clone(),
                        Rc::clone(&view.session),
                        Rc::clone(&view.menu),
                    );
                    // The menu closes before the command runs, so a command
                    // that fails leaves no menu open behind it.
                    move |cx: &mut EventCx<'_>| {
                        session.close(&menu);
                        let _ = item.perform(cx);
                    }
                }));
                menu_row(item.label(), Rc::clone(node), direction, min_target, choose)
            })
            .collect();
        let mut keys = Shortcuts::new(
            Actions::new(
                ColoredBox::new(Color::WHITE).child(
                    Padding::symmetric(0.0, 4.0).child(
                        IntrinsicWidth::new()
                            .child(Column::new(rows).main_axis_size(MainAxisSize::Min)),
                    ),
                ),
            )
            .action(MoveFocus(Rc::clone(&self.nodes))),
        );
        for (key, step) in [
            (NamedKey::ArrowDown, MenuStep::Next),
            (NamedKey::ArrowUp, MenuStep::Previous),
            (NamedKey::Home, MenuStep::First),
            (NamedKey::End, MenuStep::Last),
        ] {
            keys = keys.shortcut(SingleActivator::named(key), step);
        }
        CustomSingleChildLayout::new(Arc::new(view.placement)).child(
            Semantics::new()
                .container(true)
                .role(SemanticsRole::Menu)
                .child(keys),
        )
    }
}

/// One item: a focusable row labelled by its command, chosen by a tap, the
/// activation keys or assistive technology's click. The menu is a route of
/// the navigator's overlay, outside the area's subtree, so its label reads in
/// the area's `direction`, not the overlay's.
fn menu_row(
    label: &str,
    node: Rc<FocusNode>,
    direction: TextDirection,
    min_target: f64,
    choose: Choose,
) -> BoxedView {
    let side = if min_target.is_finite() {
        min_target.max(0.0)
    } else {
        0.0
    };
    let content = ConstrainedBox::new(BoxConstraints::new(0.0, f64::INFINITY, side, f64::INFINITY))
        .child(Padding::symmetric(12.0, 8.0).child(
            Align::new(AlignmentDirectional::CENTER_START.resolve(direction.is_ltr())).child(
                ExcludeSemantics::new().child(Text::new(label.to_owned()).direction(direction)),
            ),
        ));
    let focus = Actions::new(
        Focus::new(FocusRing::new(content))
            .focus_node(node)
            .debug_label("ContextMenuArea item"),
    )
    .action::<ActivateIntent>(choose.clone())
    .action::<ButtonActivateIntent>(choose.clone())
    .action::<ActivationPressIntent>(choose.clone());
    let tap = choose.0;
    Semantics::new()
        .container(true)
        .role(SemanticsRole::MenuItem)
        .label(label.to_owned())
        .child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move |cx| tap(cx))
                .child(focus),
        )
        .boxed()
}

/// Chooses one item: closes the menu, then performs the item's command.
#[derive(Clone)]
struct Choose(Rc<dyn Fn(&mut EventCx<'_>)>);

impl Action<ActivateIntent> for Choose {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        (self.0)(cx);
        ActionOutcome::Performed
    }
}

impl Action<ButtonActivateIntent> for Choose {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ButtonActivateIntent) -> ActionOutcome {
        (self.0)(cx);
        ActionOutcome::Performed
    }
}

/// Space's key-down on an item: consumed, so its release chooses it.
impl Action<ActivationPressIntent> for Choose {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivationPressIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

/// A move of the focus between a menu's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuStep {
    Next,
    Previous,
    First,
    Last,
}

impl Intent for MenuStep {}

/// Moves the focus between the items, wrapping at either end.
struct MoveFocus(Rc<[Rc<FocusNode>]>);

impl Action<MenuStep> for MoveFocus {
    fn invoke(&self, _cx: &mut EventCx<'_>, step: &MenuStep) -> ActionOutcome {
        let count = self.0.len();
        if count == 0 {
            return ActionOutcome::NotPerformed;
        }
        let focused = self.0.iter().position(|node| node.has_focus());
        let target = match (step, focused) {
            (MenuStep::First, _) | (MenuStep::Next, None) => 0,
            (MenuStep::Last, _) | (MenuStep::Previous, None) => count - 1,
            (MenuStep::Next, Some(index)) => (index + 1) % count,
            (MenuStep::Previous, Some(index)) => (index + count - 1) % count,
        };
        let _ = self.0[target].request_focus();
        ActionOutcome::Performed
    }
}

impl fmt::Debug for MenuPanel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuPanel")
            .field("items", &self.items)
            .field("placement", &self.placement)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for MenuPanelState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuPanelState")
            .field("items", &self.nodes.len())
            .finish_non_exhaustive()
    }
}
