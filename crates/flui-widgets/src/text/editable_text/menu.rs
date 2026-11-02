//! The selection menu of an editable text: a popup a secondary click, the
//! menu key or assistive technology opens, and a toolbar over the selection
//! a touch leaves, both offering the field's editing commands (cut, copy,
//! paste, select all) and the commands its [`DefaultSelectionControls`] and
//! [`EditableText::menu_command`] add. What they look like is the
//! [`SelectionMenu`] given to the field or to the nearest
//! [`DefaultSelectionControls`]; without one the field shows no menu, which
//! a debug build reports once.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::Offset;
use flui_view::prelude::*;

use flui_interaction::focus::FocusNode;
use flui_platform::text_store::OwnerCalls;

use super::{CompositionCommit, EditableText};
use crate::interaction::actions::{Action, ActionChain, ShowContextMenuIntent};
use crate::interaction::{GestureCommand, MenuCause, MenuItem, MenuPresenter, MenuRequest};
use crate::semantics::Semantics;
use crate::text::controller::TextEditingController;
use crate::text::{
    DefaultSelectionControls, MenuChoice, PopupOrigin, SelectionMenu, SelectionMenuPolicy,
    SelectionPopup, editing_commands,
};

/// The menu knobs of one [`EditableText`].
#[derive(Clone, Default)]
pub(crate) struct MenuSettings {
    /// `Some(MenuChoice::Hide)` turns the menu off; `None` takes the nearest
    /// [`DefaultSelectionControls`]' look.
    look: Option<MenuChoice>,
    policy: Option<SelectionMenuPolicy>,
    commands: Vec<GestureCommand>,
}

impl EditableText {
    /// Show the selection menu with `menu`, in place of the nearest
    /// [`DefaultSelectionControls`]' look.
    ///
    /// A secondary click, the menu key or assistive technology's
    /// `ShowContextMenu` opens it as a popup; a touch selection shows it as
    /// a toolbar over the selection, as [`Self::menu_policy`] says. Either
    /// offers cut, copy, paste and select all while the field can perform
    /// them, then the commands of the nearest [`DefaultSelectionControls`]
    /// and those added with [`Self::menu_command`].
    #[must_use]
    pub fn menu(mut self, menu: impl SelectionMenu) -> Self {
        self.menu.look = Some(MenuChoice::Show(Rc::new(menu)));
        self
    }

    /// Show no selection menu, though a [`DefaultSelectionControls`] above
    /// gives one.
    #[must_use]
    pub fn no_menu(mut self) -> Self {
        self.menu.look = Some(MenuChoice::Hide);
        self
    }

    /// When a touch shows the toolbar, in place of the nearest
    /// [`DefaultSelectionControls`]' policy or the platform's
    /// ([`SelectionMenuPolicy::default`]).
    #[must_use]
    pub fn menu_policy(mut self, policy: SelectionMenuPolicy) -> Self {
        self.menu.policy = Some(policy);
        self
    }

    /// Also offer `command` in the selection menu, after the editing
    /// commands and those of the nearest [`DefaultSelectionControls`]. It
    /// performs through the actions at the field, so an app's `Actions`
    /// above the field answers its intent.
    #[must_use]
    pub fn menu_command(mut self, command: GestureCommand) -> Self {
        self.menu.commands.push(command);
        self
    }
}

/// A field's menu as its last build resolved it.
#[derive(Default)]
pub(super) struct FieldMenu {
    look: RefCell<Option<Rc<dyn SelectionMenu>>>,
    /// The popup of `look`, which keeps the menu it has open.
    popup: RefCell<Option<Rc<dyn SelectionPopup>>>,
    /// Popups of looks a rebuild replaced, closed at the next event, since
    /// a build may not change the routes.
    retired: RefCell<Vec<Rc<dyn SelectionPopup>>>,
    presenter: RefCell<Option<MenuPresenter>>,
    policy: Cell<Option<SelectionMenuPolicy>>,
    items: RefCell<Option<Rc<[MenuItem]>>>,
    warned: Cell<bool>,
}

impl FieldMenu {
    /// Resolves the menu in the field's `build`: its look, its policy, and
    /// its items performing through `chain`, the field's actions.
    pub(super) fn configure(
        &self,
        ctx: &dyn BuildContext,
        settings: &MenuSettings,
        chain: Option<ActionChain>,
        origin: &PopupOrigin,
    ) {
        let controls = DefaultSelectionControls::controls_of(ctx);
        let decided = settings.look.is_some()
            || controls
                .as_ref()
                .is_some_and(|controls| controls.menu_decided());
        let look = match &settings.look {
            Some(choice) => choice.look(),
            None => controls.as_ref().and_then(|controls| controls.menu()),
        };
        if !decided && cfg!(debug_assertions) && !self.warned.replace(true) {
            tracing::warn!(
                "an EditableText shows no selection menu: no DefaultSelectionControls gives one"
            );
        }
        let same_look = match (self.look.borrow().as_ref(), look.as_ref()) {
            (Some(old), Some(new)) => same_menu(old, new),
            (None, None) => true,
            _ => false,
        };
        if !same_look {
            let popup = look.as_ref().map(|look| look.popup());
            let outgoing = self.popup.replace(popup);
            self.retired.borrow_mut().extend(outgoing);
            self.look.replace(look);
        }
        let presenter = self
            .popup
            .borrow()
            .as_ref()
            .map(|popup| popup.presenter(ctx, origin));
        self.presenter.replace(presenter);
        self.policy.set(
            settings
                .policy
                .or_else(|| controls.as_ref().and_then(|controls| controls.policy())),
        );
        let scope_commands = controls
            .as_ref()
            .map(|controls| controls.commands().to_vec())
            .unwrap_or_default();
        let items: Rc<[MenuItem]> = editing_commands(ctx)
            .into_iter()
            .chain(scope_commands)
            .chain(settings.commands.iter().cloned())
            .map(|command| MenuItem::new(command, chain.clone()))
            .collect();
        self.items.replace(Some(items));
    }

    /// When a touch shows the toolbar.
    pub(super) fn policy(&self) -> SelectionMenuPolicy {
        self.policy.get().unwrap_or_default()
    }

    /// What the menu looks like, when the field shows one.
    pub(super) fn look(&self) -> Option<Rc<dyn SelectionMenu>> {
        self.look.borrow().clone()
    }

    /// Every item, offered or not.
    pub(super) fn items(&self) -> Rc<[MenuItem]> {
        self.items.borrow().clone().unwrap_or_else(|| Rc::from([]))
    }

    /// The items the field can perform now.
    pub(super) fn offered(&self) -> Vec<MenuItem> {
        self.items()
            .iter()
            .filter(|item| item.is_enabled())
            .cloned()
            .collect()
    }

    /// Whether opening the popup would show anything.
    fn can_open(&self) -> bool {
        self.presenter.borrow().is_some() && self.items().iter().any(MenuItem::is_enabled)
    }

    /// Opens the popup with the items offered now, raised by `cause` at
    /// `position` in root coordinates. Returns whether it opened.
    pub(super) fn open(
        &self,
        cx: &mut EventCx<'_>,
        cause: MenuCause,
        position: Option<Offset<f64>>,
    ) -> bool {
        self.close_retired();
        let presenter = self.presenter.borrow().clone();
        let Some(presenter) = presenter else {
            return false;
        };
        let items: Rc<[MenuItem]> = self.offered().into();
        if items.is_empty() {
            return false;
        }
        presenter(cx, MenuRequest::new(items, cause, position));
        true
    }

    /// Closes the popup the field has open, and those of replaced looks.
    pub(super) fn close(&self) {
        self.close_retired();
        let popup = self.popup.borrow().clone();
        if let Some(popup) = popup {
            popup.close();
        }
    }

    fn close_retired(&self) {
        let retired = std::mem::take(&mut *self.retired.borrow_mut());
        for popup in retired {
            popup.close();
        }
    }
}

fn same_menu(a: &Rc<dyn SelectionMenu>, b: &Rc<dyn SelectionMenu>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `semantics` with assistive technology's `ShowContextMenu` opening the
/// popup, while the field shows one.
pub(super) fn show_menu_action(semantics: Semantics, menu: &Rc<FieldMenu>) -> Semantics {
    if !menu.can_open() {
        return semantics;
    }
    let menu = Rc::downgrade(menu);
    semantics.on_show_context_menu(move |cx| {
        if let Some(menu) = menu.upgrade() {
            menu.open(cx, MenuCause::Assistive, None);
        }
    })
}

/// The menu key (`ShowContextMenuIntent`) in a focused field opens its
/// popup; a field with no menu leaves the key to an enclosing answer.
#[derive(Clone)]
pub(super) struct ShowFieldMenu(pub(super) Weak<FieldMenu>);

impl Action<ShowContextMenuIntent> for ShowFieldMenu {
    fn is_enabled(&self, _intent: &ShowContextMenuIntent) -> bool {
        self.0.upgrade().is_some_and(|menu| menu.can_open())
    }

    fn invoke(
        &self,
        cx: &mut EventCx<'_>,
        _intent: &ShowContextMenuIntent,
    ) -> crate::interaction::actions::ActionOutcome {
        use crate::interaction::actions::ActionOutcome;
        match self.0.upgrade() {
            Some(menu) if menu.open(cx, MenuCause::Key, None) => ActionOutcome::Performed,
            _ => ActionOutcome::NotPerformed,
        }
    }
}

/// A secondary press at `position`, over source offset `at`: the field
/// takes the focus, the caret moves there unless the press is on the
/// selection, which the menu then acts on, and the popup opens. A
/// composition is committed first; each step runs though an earlier one's
/// owner code failed, and the first failure is resumed after the popup
/// opened.
pub(super) fn secondary_press(
    cx: &mut EventCx<'_>,
    menu: &FieldMenu,
    position: Offset<f64>,
    at: Option<usize>,
    field: (&Rc<RefCell<TextEditingController>>, &Rc<FocusNode>),
    commit: &CompositionCommit,
) {
    let (controller, focus_node) = field;
    let mut calls = OwnerCalls::new();
    commit.run(&mut calls);
    calls.run(|| {
        let _ = focus_node.request_focus();
    });
    let controller = controller.borrow().clone();
    let selection = controller.selection();
    if let Some(at) = at
        && (selection.is_empty() || !(selection.start..=selection.end).contains(&at))
    {
        calls.run(|| controller.set_caret_byte_offset(at));
    }
    calls.run(|| {
        menu.open(cx, MenuCause::SecondaryClick, Some(position));
    });
    calls.resume();
}
