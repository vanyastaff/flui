//! The selection menu of a [`SelectionArea`]: a popup a secondary click,
//! the menu key or assistive technology opens, and a toolbar over the
//! selection a touch leaves, both offering copy and select all while the
//! area can perform them, then the commands its
//! [`DefaultSelectionControls`] and [`SelectionArea::menu_command`] add.
//! They look as the [`SelectionMenu`] given to the area or to the nearest
//! [`DefaultSelectionControls`] says; without one the area shows no menu,
//! which a debug build reports once.

use std::any::TypeId;
use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_foundation::panic::FirstPanic;
use flui_rendering::semantics::DocumentSelection;
use flui_view::BoxedView;
use flui_view::prelude::*;

use super::input::{AreaAction, AreaDismiss};
use super::{HandleSide, SelectionArea, SelectionInput, SelectionModel};
use crate::interaction::actions::{
    ActionChain, CopySelectionTextIntent, DismissIntent, SelectAllTextIntent,
    ShowContextMenuIntent, erased_action, layered_chain,
};
use crate::interaction::{GestureCommand, MenuCause};
#[cfg(doc)]
use crate::text::DefaultSelectionControls;
use crate::text::{
    MenuSettings, PopupOrigin, SelectionMenu, SelectionMenuPolicy, ShowSelectionMenu,
    ToolbarAnchors, reading_commands,
};

impl SelectionArea {
    /// Show the selection menu with `menu`, in place of the nearest
    /// [`DefaultSelectionControls`]' look.
    ///
    /// A secondary click, the menu key or assistive technology's
    /// `ShowContextMenu` opens it as a popup; a touch selection shows it as
    /// a toolbar over the selection, as [`Self::menu_policy`] says. Either
    /// offers copy while something is selected and select all, then the
    /// commands of the nearest [`DefaultSelectionControls`] and those added
    /// with [`Self::menu_command`].
    #[must_use]
    pub fn menu(mut self, menu: impl SelectionMenu) -> Self {
        self.menu.show(menu);
        self
    }

    /// Show no selection menu, though a [`DefaultSelectionControls`] above
    /// gives one.
    #[must_use]
    pub fn no_menu(mut self) -> Self {
        self.menu.hide();
        self
    }

    /// When a touch shows the toolbar, in place of the nearest
    /// [`DefaultSelectionControls`]' policy or the platform's
    /// ([`SelectionMenuPolicy::for_platform`] of the [`PlatformConvention`](flui_view::environment::PlatformConvention)). An area has no caret, so only
    /// the long press and the end of a handle drag count.
    #[must_use]
    pub fn menu_policy(mut self, policy: SelectionMenuPolicy) -> Self {
        self.menu.set_policy(policy);
        self
    }

    /// Also offer `command` in the selection menu, after copy, select all
    /// and the commands of the nearest [`DefaultSelectionControls`]. It
    /// performs through the actions at the area, so an app's `Actions`
    /// above the area answers its intent.
    #[must_use]
    pub fn menu_command(mut self, command: GestureCommand) -> Self {
        self.menu.add_command(command);
        self
    }
}

/// The area's actions as its last build layered them over the enclosing
/// chain.
#[derive(Default)]
pub(super) struct AreaChain {
    held: RefCell<Option<HeldChain>>,
}

struct HeldChain {
    enclosing: Option<ActionChain>,
    chain: ActionChain,
}

impl SelectionModel {
    /// The chain at the area: select all, copy, the menu key and dismiss,
    /// over `enclosing`. It is kept while `enclosing` is the same, so the
    /// menu's items keep their bindings and an open popup stays across the
    /// area's rebuilds, a change of the selection included.
    pub(super) fn action_chain(self: &Rc<Self>, enclosing: Option<ActionChain>) -> ActionChain {
        if let Some(held) = self.chain.held.borrow().as_ref() {
            let same_enclosing = match (&held.enclosing, &enclosing) {
                (Some(old), Some(new)) => old.is(new),
                (None, None) => true,
                _ => false,
            };
            if same_enclosing {
                return held.chain.clone();
            }
        }
        let area = || AreaAction(Rc::downgrade(self));
        let enclosing_answer = |intent: TypeId| {
            enclosing
                .as_ref()
                .and_then(|chain| chain.get(intent))
                .cloned()
        };
        let own = vec![
            erased_action::<SelectAllTextIntent>(area()),
            erased_action::<CopySelectionTextIntent>(area()),
            erased_action::<ShowContextMenuIntent>(ShowSelectionMenu::new(
                &self.handles.menu,
                enclosing.as_ref(),
            )),
            erased_action::<DismissIntent>(AreaDismiss {
                model: Rc::downgrade(self),
                enclosing: enclosing_answer(TypeId::of::<DismissIntent>()),
            }),
        ];
        let chain = layered_chain(enclosing.clone(), Rc::new(own));
        *self.chain.held.borrow_mut() = Some(HeldChain {
            enclosing,
            chain: chain.clone(),
        });
        chain
    }

    /// Resolves the menu in the area's build, its items performing through
    /// `chain`: a retired popup closes after the frame, and a shown toolbar
    /// shows the new items, or goes with the look.
    pub(super) fn configure_menu(
        self: &Rc<Self>,
        ctx: &dyn BuildContext,
        settings: &MenuSettings,
        chain: ActionChain,
    ) {
        let menu = &self.handles.menu;
        let origin = PopupOrigin::new(self.anchor.clone(), self.pipeline.borrow().clone());
        menu.configure(
            ctx,
            settings,
            &reading_commands(ctx),
            Some(chain),
            &origin,
            "a SelectionArea",
        );
        let post_frame = self.post_frame.borrow().clone();
        if menu.configured(post_frame.as_ref()) {
            let mut failure = FirstPanic::new();
            // Taken away first, so a menu turned off leaves no toolbar.
            self.hide_toolbar(&mut failure);
            self.show_toolbar(&mut failure);
            failure.resume();
        }
    }

    /// Shows the menu's toolbar over shown handles, when the area has a
    /// menu; it follows its items' enablement while it shows. A wake that
    /// fails is held in `failure`.
    pub(super) fn show_toolbar(self: &Rc<Self>, failure: &mut FirstPanic) {
        if !self.handles.shown.get() {
            return;
        }
        let model = Rc::downgrade(self);
        let changed: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(model) = model.upgrade() {
                let mut failure = FirstPanic::new();
                model.handles.refresh(&mut failure);
                failure.resume();
            }
        });
        if self.handles.menu.start_toolbar(&changed) {
            self.handles.refresh(failure);
        }
    }

    /// Takes the toolbar away, leaving the handles. A wake that fails is
    /// held in `failure`.
    pub(super) fn hide_toolbar(&self, failure: &mut FirstPanic) {
        if self.handles.menu.stop_toolbar() {
            self.handles.refresh(failure);
        }
    }

    /// The long-pressing touch lifted: the toolbar shows over the word it
    /// selected, as the menu's policy says.
    pub(super) fn long_press_released(self: &Rc<Self>) {
        if self.handles.menu.policy().after_long_press() {
            let mut failure = FirstPanic::new();
            self.show_toolbar(&mut failure);
            failure.resume();
        }
    }

    /// A handle was let go: the toolbar shows again, as the menu's policy
    /// says.
    pub(super) fn handle_released(self: &Rc<Self>) {
        if self.handles.menu.policy().after_handle_drag() {
            let mut failure = FirstPanic::new();
            self.show_toolbar(&mut failure);
            failure.resume();
        }
    }

    /// A secondary tap at `global` won the arena: the area takes the focus
    /// and the popup opens. A press on the selection keeps it, so the menu
    /// acts on it; elsewhere the word there is selected on Apple platforms,
    /// and the selection is cleared on the others, as their own text does.
    /// The popup opens though a listener of that change panics; the first
    /// failure resumes after.
    pub(super) fn secondary_tapped(self: &Rc<Self>, ctx: &mut EventCx<'_>, global: Offset<f64>) {
        let mut failure = FirstPanic::new();
        self.hide_handles(&mut failure);
        if !self.focus.has_primary_focus() {
            failure.contain(|| {
                let _outcome = self.focus.request_focus();
            });
        }
        let ordered = self.ordered();
        let hit = self.position_at(&ordered, global);
        let selection = self.selection.borrow().clone();
        let on_selection = match (&hit, &selection) {
            (Some(hit), Some(selection)) => {
                let (start, end) = Self::ordered_ends(&ordered, selection);
                Self::compare(&ordered, start, &hit.at).is_le()
                    && Self::compare(&ordered, &hit.at, end).is_le()
            }
            _ => false,
        };
        if !on_selection {
            let replacement = if self.platform.get().is_apple() {
                hit.map(|hit| {
                    let (start, end) = self.word_at(&ordered, &hit.at);
                    DocumentSelection::new(start, end)
                })
            } else {
                None
            };
            failure.contain(|| self.select(ctx, replacement.as_ref(), SelectionInput::Pointer));
        }
        failure.contain(|| {
            self.handles
                .menu
                .open(ctx, MenuCause::SecondaryClick, Some(global));
        });
        failure.resume();
    }
}

/// The menu's toolbar over the selection's start, or under its end where
/// there is no room above: see [`crate::text::toolbar`]. Choosing an item
/// takes it away.
pub(super) fn toolbar(model: &Rc<SelectionModel>) -> Option<BoxedView> {
    let start = model.end_line(HandleSide::Start)?;
    let end = model.end_line(HandleSide::End)?;
    let overlay = model.handles.overlay.borrow().clone()?;
    let pipeline = model.pipeline.borrow().clone()?;
    let anchors = ToolbarAnchors::in_overlay(&overlay, &pipeline, start, end)?;
    let post_frame = model.post_frame.borrow().clone();
    let weak = Rc::downgrade(model);
    let hide: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(model) = weak.upgrade() {
            let mut failure = FirstPanic::new();
            model.hide_toolbar(&mut failure);
            failure.resume();
        }
    });
    crate::text::toolbar(
        &model.handles.menu,
        anchors,
        model.handles.link(HandleSide::Start),
        post_frame,
        hide,
    )
}
