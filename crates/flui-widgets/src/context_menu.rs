//! [`ContextMenuArea`] — a region whose commands open in a theme-free context
//! menu (ADR-0216 §2).
//!
//! The area mounts [`GestureAlternatives`] with an
//! [`Alternative::ContextMenu`] around its content, so every way to open the
//! menu is the alternatives': a secondary click, a touch or pen long press
//! composed as `Sequenced`, the platform's menu keys, assistive technology's
//! `ShowContextMenu` and the control focus reveals. The area is the
//! [`MenuSpec`]'s presenter: it shows the request through the crate's plain
//! popup menu (`popup_menu`), placed below the area when no pointer opened it.

use std::fmt;
use std::rc::Rc;

use flui_objects::SubtreeAnchor;
use flui_painting::typography::TextDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::PipelineCell;
use flui_view::BoxedView;
use flui_view::prelude::*;

use crate::anchored_box::AnchoredBox;
use crate::interaction::{
    Alternative, GestureAlternatives, GestureAlternativesError, GestureCommand, Listener, MenuSpec,
};
use crate::localization::Directionality;
use crate::navigator::NavigatorHandle;
use crate::popup_menu::{MenuSession, Presenter};

/// A region whose commands are reached through a context menu.
///
/// ```rust,ignore
/// ContextMenuArea::try_new(
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
/// its command's intent through the [`Actions`](crate::Actions) chain where the area is
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
    pub fn try_new(
        first: GestureCommand,
        more: impl IntoIterator<Item = GestureCommand>,
    ) -> Result<Self, GestureAlternativesError> {
        let menu = more
            .into_iter()
            .fold(MenuSpec::new(first, |_cx, _request| {}), MenuSpec::item);
        Ok(Self {
            alternatives: GestureAlternatives::try_new(Alternative::ContextMenu(menu))?,
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
