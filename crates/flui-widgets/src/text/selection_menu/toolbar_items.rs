//! [`ToolbarItems`]: a toolbar's commands in one row, the trailing ones that
//! don't fit behind an overflow button, and [`ToolbarItemsController`], which
//! holds whether the button shows them and which they are.

use std::cell::Cell;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_objects::{RenderToolbarItems, ToolbarItemsSplit};
use flui_painting::typography::TextDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, CreationSite, RebuildHandle, RebuildReason, View};

use crate::interaction::{ExcludeFocus, GestureDetector};
use crate::localization::{BoxedWidgetsLocalizations, Directionality};
use crate::{ExcludeSemantics, Padding, Semantics, SemanticsRole, Text};

/// Whether a [`ToolbarItems`] shows its left-over items in place of its row,
/// and which items its last layout left over.
///
/// A toolbar holds one of its own unless it is given one with
/// [`ToolbarItems::controller`]; given one, the app or design package
/// decides when it expands, and can offer the left-over items its own way,
/// in a popup of its own, with [`hidden`](Self::hidden). Every clone shares
/// one state and one notification channel on the owning UI thread; a change
/// notifies once, after it is made. Change it in an event or after a frame,
/// not in a build.
#[derive(Clone, Default)]
pub struct ToolbarItemsController {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    expanded: Cell<bool>,
    split: ToolbarItemsSplit,
    notifier: ChangeNotifier,
}

impl ToolbarItemsController {
    /// A controller showing the row.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the left-over items show in place of the row.
    #[must_use]
    pub fn is_expanded(&self) -> bool {
        self.inner.expanded.get()
    }

    /// Shows the left-over items in place of the row, or the row again, and
    /// notifies when that changes. While every item fits there is nothing
    /// to show and the toolbar keeps its row.
    pub fn set_expanded(&self, expanded: bool) {
        if self.inner.expanded.replace(expanded) != expanded {
            self.inner.notifier.notify_listeners();
        }
    }

    /// Flips [`is_expanded`](Self::is_expanded).
    pub fn toggle(&self) {
        self.set_expanded(!self.is_expanded());
    }

    /// The items the toolbar's last layout left out of its row, by their
    /// index in [`ToolbarItems::new`]: empty before it is laid out and
    /// while every item fits.
    #[must_use]
    pub fn hidden(&self) -> Range<usize> {
        self.inner.split.hidden()
    }

    /// Whether `self` and `other` share one state.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for ToolbarItemsController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarItemsController")
            .field("expanded", &self.is_expanded())
            .field("hidden", &self.hidden())
            .finish_non_exhaustive()
    }
}

impl Listenable for ToolbarItemsController {
    fn listen(&self, listener: ListenerCallback) -> flui_foundation::Subscription {
        self.inner.notifier.listen(listener)
    }
    fn listen_with_recovery(
        &self,
        relay: flui_foundation::ListenerObserver,
    ) -> flui_foundation::Subscription {
        self.inner.notifier.listen_with_recovery(relay)
    }
    fn add_observer(&self, observer: flui_foundation::ListenerObserver) -> ListenerId {
        self.inner.notifier.add_observer(observer)
    }

    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.inner.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.inner.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.inner.notifier.remove_all_listeners();
    }
}

/// Builds a toolbar's overflow button from its controller: it reads
/// whether the toolbar is expanded and, when chosen, expands it, takes it
/// back to its row or opens a popup of the hidden items.
type ButtonBuilder = Rc<dyn Fn(&ToolbarItemsController, &dyn BuildContext) -> BoxedView>;

/// A toolbar's commands in one row, as many leading ones as fit, and an
/// overflow button when some are left over. The button shows the left-over
/// items in a column in place of the row (more columns beside it, or runs
/// across, in a small room), then takes it back.
///
/// The items and the button are laid out at their own width in the room
/// the toolbar is given; an item left out is not painted, not hit and not
/// seen by assistive technology. Nothing in a toolbar takes the focus, so a
/// field keeps it and its input session. A design package builds its look's
/// toolbar on it, with its own [`overflow_button`](Self::overflow_button),
/// and can keep the column for itself: a [`controller`](Self::controller) it
/// holds names the [`hidden`](ToolbarItemsController::hidden) items for a
/// popup of its own. Without a controller the toolbar holds its own, and
/// goes back to its row when its number of items changes.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::{Text, ToolbarItems};
/// # use flui_view::ViewExt;
/// let toolbar = ToolbarItems::new(vec![Text::new("Copy").boxed(), Text::new("Share").boxed()]);
/// # let _ = toolbar;
/// ```
#[derive(Clone)]
pub struct ToolbarItems {
    items: Vec<BoxedView>,
    button: Option<ButtonBuilder>,
    controller: Option<ToolbarItemsController>,
    site: CreationSite,
}

impl ToolbarItems {
    /// A toolbar of `items`, in reading order.
    #[must_use]
    #[track_caller]
    pub fn new(items: Vec<BoxedView>) -> Self {
        Self {
            items,
            button: None,
            controller: None,
            site: CreationSite::here(),
        }
    }

    /// Builds the overflow button with `build`, in place of a plain one
    /// labelled [`more_button_label`](crate::WidgetsLocalizations::more_button_label)
    /// in the row and
    /// [`back_button_label`](crate::WidgetsLocalizations::back_button_label) over
    /// the column. It is built again when the toolbar expands or goes back;
    /// the button decides what choosing it does.
    #[must_use]
    pub fn overflow_button(
        mut self,
        build: impl Fn(&ToolbarItemsController, &dyn BuildContext) -> BoxedView + 'static,
    ) -> Self {
        self.button = Some(Rc::new(build));
        self
    }

    /// Expands and reports through `controller` in place of the toolbar's
    /// own.
    #[must_use]
    pub fn controller(mut self, controller: ToolbarItemsController) -> Self {
        self.controller = Some(controller);
        self
    }
}

impl fmt::Debug for ToolbarItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarItems")
            .field("items", &self.items.len())
            .field("button", &self.button.is_some())
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

impl View for ToolbarItems {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }

    fn creation_site(&self) -> CreationSite {
        self.site
    }
}

impl StatefulView for ToolbarItems {
    type State = ToolbarItemsState;

    fn create_state(&self) -> Self::State {
        // `init_state` is not handed the view, so the controller it listens
        // to is copied here.
        ToolbarItemsState {
            own: ToolbarItemsController::new(),
            given: self.controller.clone(),
            listening: None,
            handle: None,
        }
    }
}

/// The state of a [`ToolbarItems`]: its own controller, the one it was
/// given, and the subscription to the one in use.
pub struct ToolbarItemsState {
    own: ToolbarItemsController,
    given: Option<ToolbarItemsController>,
    listening: Option<(ToolbarItemsController, ListenerId)>,
    handle: Option<RebuildHandle>,
}

impl fmt::Debug for ToolbarItemsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarItemsState")
            .field("controller", self.in_use())
            .field("given", &self.given.is_some())
            .finish_non_exhaustive()
    }
}

impl ToolbarItemsState {
    fn in_use(&self) -> &ToolbarItemsController {
        self.given.as_ref().unwrap_or(&self.own)
    }

    /// Rebuilds through `handle` on a change of the controller in use,
    /// leaving the one listened to before.
    fn listen(&mut self, handle: RebuildHandle) {
        let controller = self.in_use().clone();
        self.unlisten();
        let id = controller.add_listener(Rc::new(move || {
            handle.schedule(RebuildReason::StateChange);
        }));
        self.listening = Some((controller, id));
    }

    fn unlisten(&mut self) {
        if let Some((controller, id)) = self.listening.take() {
            controller.remove_listener(id);
        }
    }
}

impl ViewState<ToolbarItems> for ToolbarItemsState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let handle = ctx.rebuild_handle();
        self.handle = Some(handle.clone());
        self.listen(handle);
    }

    fn build(&self, view: &ToolbarItems, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.in_use();
        let button = match &view.button {
            Some(build) => build(controller, ctx),
            None => plain_button(controller, ctx),
        };
        let mut children = view.items.clone();
        children.push(button);
        // A toolbar never takes the focus, so a field keeps it and its input
        // session, and no hidden item can hold it.
        ExcludeFocus::new(ToolbarRow {
            expanded: controller.is_expanded(),
            direction: Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr),
            split: controller.inner.split.clone(),
            children,
        })
    }

    fn did_update_view(&mut self, old_view: &ToolbarItems, new_view: &ToolbarItems) {
        if new_view.controller.is_none() && old_view.items.len() != new_view.items.len() {
            // Back to the row, quietly: this build shows it.
            self.own.inner.expanded.set(false);
        }
        let same = match (&self.given, &new_view.controller) {
            (Some(old), Some(new)) => old.is_same(new),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.given.clone_from(&new_view.controller);
            if let Some(handle) = self.handle.clone() {
                self.listen(handle);
            }
        }
    }

    fn dispose(&mut self) {
        self.unlisten();
    }
}

/// The plain overflow button: "â‹¯" in the row and "â€¹" over the column,
/// labelled for assistive technology, flipping the toolbar when chosen. It
/// never takes the focus.
fn plain_button(controller: &ToolbarItemsController, ctx: &dyn BuildContext) -> BoxedView {
    let strings = BoxedWidgetsLocalizations::of(ctx);
    let (glyph, label) = if controller.is_expanded() {
        ("\u{2039}", strings.back_button_label())
    } else {
        ("\u{22EF}", strings.more_button_label())
    };
    let controller = controller.clone();
    Semantics::new()
        .container(true)
        .role(SemanticsRole::Button)
        .label(label)
        .child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move |_cx| controller.toggle())
                .child(
                    Padding::symmetric(12.0, 8.0)
                        .child(ExcludeSemantics::new().child(Text::new(glyph))),
                ),
        )
        .boxed()
}

/// The render view of a [`ToolbarItems`]: its items, then its button.
#[derive(Clone)]
struct ToolbarRow {
    expanded: bool,
    direction: TextDirection,
    split: ToolbarItemsSplit,
    children: Vec<BoxedView>,
}

impl fmt::Debug for ToolbarRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarRow")
            .field("expanded", &self.expanded)
            .field("direction", &self.direction)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl View for ToolbarRow {
    fn create_element(&self) -> ElementKind {
        ElementKind::render_variable(self)
    }
}

impl flui_view::RenderView for ToolbarRow {
    type Protocol = BoxProtocol;
    type RenderObject = RenderToolbarItems;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderToolbarItems::new(self.split.clone())
            .with_expanded(self.expanded)
            .with_text_direction(self.direction)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_split(self.split.clone());
        impact |= render_object.set_expanded(self.expanded);
        impact |= render_object.set_text_direction(self.direction);
        impact
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}
