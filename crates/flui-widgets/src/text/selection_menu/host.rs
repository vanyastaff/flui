//! The selection menu of one mounted field or area: its look, its policy and
//! its items as the last build resolved them, the popup it has open, and
//! whether its toolbar shows. An [`EditableText`](crate::EditableText) and a
//! [`SelectionArea`](crate::SelectionArea) each own one and give it their
//! standard commands; what follows is the same for both.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::Offset;
use flui_foundation::panic::FirstPanic;
use flui_painting::typography::TextDirection;
use flui_scheduler::{FrameTiming, PostFrameHandle};
use flui_view::prelude::*;

use super::{
    DefaultSelectionControls, MenuChoice, PopupOrigin, SelectionMenu, SelectionMenuPolicy,
    SelectionPopup,
};
use crate::interaction::actions::{
    Action, ActionChain, ActionOutcome, ErasedAction, ShowContextMenuIntent,
};
use crate::interaction::{GestureCommand, MenuCause, MenuItem, MenuPresenter, MenuRequest};
use crate::localization::Directionality;
use crate::semantics::Semantics;

/// The menu knobs of one field or area, which its `menu`, `no_menu`,
/// `menu_policy` and `menu_command` set.
#[derive(Clone, Default)]
pub(crate) struct MenuSettings {
    /// `Some(MenuChoice::Hide)` turns the menu off; `None` takes the nearest
    /// [`DefaultSelectionControls`]' look.
    look: Option<MenuChoice>,
    policy: Option<SelectionMenuPolicy>,
    commands: Vec<GestureCommand>,
}

impl MenuSettings {
    pub(crate) fn show(&mut self, menu: impl SelectionMenu) {
        self.look = Some(MenuChoice::Show(Rc::new(menu)));
    }

    pub(crate) fn hide(&mut self) {
        self.look = Some(MenuChoice::Hide);
    }

    pub(crate) fn set_policy(&mut self, policy: SelectionMenuPolicy) {
        self.policy = Some(policy);
    }

    pub(crate) fn add_command(&mut self, command: GestureCommand) {
        self.commands.push(command);
    }
}

/// A field's or area's menu as its last build resolved it.
#[derive(Default)]
pub(crate) struct MenuHost {
    look: RefCell<Option<Rc<dyn SelectionMenu>>>,
    /// The popup of `look`, which keeps the menu it has open.
    popup: RefCell<Option<Rc<dyn SelectionPopup>>>,
    /// Popups a rebuild retired (of a replaced look, or holding replaced
    /// items), closed after the frame, since a build may not change the
    /// routes.
    retired: RefCell<Vec<Rc<dyn SelectionPopup>>>,
    presenter: RefCell<Option<MenuPresenter>>,
    policy: Cell<Option<SelectionMenuPolicy>>,
    items: RefCell<Option<Rc<[MenuItem]>>>,
    warned: Cell<bool>,
    /// The reading direction at the owner, which its toolbar keeps though
    /// it is built in the overlay.
    direction: Cell<TextDirection>,
    /// The platform whose conventions the owner followed as of its last
    /// build, which picks the toolbar policy when none is given.
    platform: Cell<Option<flui_platform::TargetPlatform>>,
    /// Where a secondary press went down, and the source offset there, until
    /// its tap wins the arena (or the next press replaces it).
    secondary: Cell<Option<(Offset<f64>, Option<usize>)>>,
    /// Set while the toolbar shows.
    toolbar: Cell<bool>,
    /// The toolbar's subscriptions to its items' enablement.
    toolbar_watches: RefCell<Vec<flui_foundation::Subscription>>,
}

impl MenuHost {
    /// Resolves the menu in the owner's `build`: its look, its policy, and
    /// `standard` with the added commands as items performing through
    /// `chain`, the owner's actions. `owner` names the widget in the report
    /// of a missing look.
    pub(crate) fn configure(
        &self,
        ctx: &dyn BuildContext,
        settings: &MenuSettings,
        standard: &[GestureCommand],
        chain: Option<ActionChain>,
        origin: &PopupOrigin,
        owner: &'static str,
    ) {
        let controls = DefaultSelectionControls::controls_of(ctx);
        self.platform
            .set(Some(flui_view::environment::PlatformConvention::of(ctx)));
        self.direction
            .set(Directionality::maybe_of(ctx).unwrap_or_default());
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
                "{owner} shows no selection menu: no DefaultSelectionControls gives one"
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
        let items = standard
            .iter()
            .cloned()
            .chain(scope_commands)
            .chain(settings.commands.iter().cloned())
            .map(|command| MenuItem::new(command, chain.clone()))
            .collect::<Rc<[MenuItem]>>();
        // An open popup holds the items it opened with: when the commands or
        // their bindings change under the same look, it closes with the
        // retired ones rather than offer what the owner no longer does.
        let outgoing = self.items.replace(Some(Rc::clone(&items)));
        let changed = outgoing.is_some_and(|outgoing| {
            outgoing.len() != items.len()
                || outgoing
                    .iter()
                    .zip(items.iter())
                    .any(|(old, new)| !old.same_as(new))
        });
        if changed && same_look {
            let popup = self.popup.borrow().clone();
            self.retired.borrow_mut().extend(popup);
        }
    }

    /// After [`configure`](Self::configure): retired popups close after the
    /// frame through `post_frame`. Returns whether the toolbar shows, which
    /// the owner then shows again with the new items.
    pub(crate) fn configured(self: &Rc<Self>, post_frame: Option<&PostFrameHandle>) -> bool {
        if self.has_retired()
            && let Some(post_frame) = post_frame
        {
            let host = Rc::downgrade(self);
            let _ = post_frame.schedule(move |_timing: &FrameTiming| {
                if let Some(host) = host.upgrade() {
                    host.close_retired();
                }
            });
        }
        self.toolbar.get()
    }

    /// When a touch shows the toolbar.
    pub(crate) fn policy(&self) -> SelectionMenuPolicy {
        self.policy.get().unwrap_or_else(|| {
            SelectionMenuPolicy::for_platform(
                self.platform
                    .get()
                    .unwrap_or(flui_platform::TargetPlatform::Unknown),
            )
        })
    }

    /// What the menu looks like, when the owner shows one.
    pub(crate) fn look(&self) -> Option<Rc<dyn SelectionMenu>> {
        self.look.borrow().clone()
    }

    /// A secondary press went down at `position`, over source offset `at`;
    /// the popup opens if its tap wins the arena.
    pub(crate) fn secondary_down(&self, position: Offset<f64>, at: Option<usize>) {
        self.secondary.set(Some((position, at)));
    }

    /// The secondary press whose tap just won, once.
    pub(crate) fn take_secondary(&self) -> Option<(Offset<f64>, Option<usize>)> {
        self.secondary.take()
    }

    /// The reading direction at the owner.
    pub(crate) fn direction(&self) -> TextDirection {
        self.direction.get()
    }

    /// Every item, offered or not.
    pub(crate) fn items(&self) -> Rc<[MenuItem]> {
        self.items.borrow().clone().unwrap_or_else(|| Rc::from([]))
    }

    /// The items the owner can perform now.
    pub(crate) fn offered(&self) -> Vec<MenuItem> {
        self.items()
            .iter()
            .filter(|item| item.is_enabled())
            .cloned()
            .collect::<Vec<MenuItem>>()
    }

    /// Whether opening the popup would show anything.
    pub(crate) fn can_open(&self) -> bool {
        self.presenter.borrow().is_some() && self.items().iter().any(MenuItem::is_enabled)
    }

    /// Opens the popup with the items offered now, raised by `cause` at
    /// `position` in root coordinates. Returns whether it opened.
    pub(crate) fn open(
        &self,
        ctx: &mut EventContext<'_>,
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
        presenter(ctx, MenuRequest::new(items, cause, position));
        true
    }

    /// Closes the popup the owner has open, and the retired ones.
    pub(crate) fn close(&self) {
        self.close_retired();
        let popup = self.popup.borrow().clone();
        if let Some(popup) = popup {
            popup.close();
        }
    }

    /// Whether a rebuild retired a popup that waits to close.
    pub(crate) fn has_retired(&self) -> bool {
        !self.retired.borrow().is_empty()
    }

    /// Closes the popups a rebuild retired; each closes though an earlier
    /// one panicked, and the first panic resumes after.
    pub(crate) fn close_retired(&self) {
        let retired = std::mem::take(&mut *self.retired.borrow_mut());
        let mut failure = FirstPanic::new();
        for popup in retired {
            failure.contain(|| popup.close());
        }
        failure.resume();
    }

    /// Whether the toolbar shows.
    pub(crate) fn toolbar_shown(&self) -> bool {
        self.toolbar.get()
    }

    /// Marks the toolbar shown, when the owner has a look and it was not:
    /// `changed` runs when an item's enablement changes while it shows.
    /// Returns whether it now shows where it did not.
    pub(crate) fn start_toolbar(&self, changed: &Rc<dyn Fn()>) -> bool {
        if self.look().is_none() || self.toolbar.replace(true) {
            return false;
        }
        let watches = self
            .items()
            .iter()
            .filter_map(MenuItem::enablement_changes)
            .map(|changes| {
                let changed = Rc::clone(changed);
                changes.listen(Rc::new(move || changed()))
            })
            .collect::<Vec<flui_foundation::Subscription>>();
        *self.toolbar_watches.borrow_mut() = watches;
        true
    }

    /// Marks the toolbar gone. Returns whether it showed.
    pub(crate) fn stop_toolbar(&self) -> bool {
        drop(std::mem::take(&mut *self.toolbar_watches.borrow_mut()));
        self.toolbar.replace(false)
    }
}

fn same_menu(a: &Rc<dyn SelectionMenu>, b: &Rc<dyn SelectionMenu>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `semantics` with assistive technology's `ShowContextMenu` opening the
/// popup, while the owner shows one. `focus` runs first: paste and the
/// edits the menu offers act on the focused owner, and the popup gives the
/// focus back to it.
pub(crate) fn show_menu_action(
    semantics: Semantics,
    host: &Rc<MenuHost>,
    focus: impl Fn(&mut EventContext<'_>) + 'static,
) -> Semantics {
    if !host.can_open() {
        return semantics;
    }
    let host = Rc::downgrade(host);
    semantics.on_show_context_menu(move |ctx| {
        focus(ctx);
        if let Some(host) = host.upgrade() {
            host.open(ctx, MenuCause::Assistive, None);
        }
    })
}

/// The menu key (`ShowContextMenuIntent`) in a focused field or area opens
/// its popup; while it can't, the key goes to the answer it would hide, so
/// one with no menu leaves it to an enclosing `ContextMenuArea` or app
/// binding.
#[derive(Clone)]
pub(crate) struct ShowSelectionMenu {
    host: Weak<MenuHost>,
    /// The enclosing chain's answer to the intent.
    enclosing: Option<ErasedAction>,
}

impl ShowSelectionMenu {
    /// The owner's answer over `enclosing`, the chain the owner is in.
    pub(crate) fn new(host: &Rc<MenuHost>, enclosing: Option<&ActionChain>) -> Self {
        Self {
            host: Rc::downgrade(host),
            enclosing: enclosing
                .and_then(|chain| chain.get(std::any::TypeId::of::<ShowContextMenuIntent>()))
                .cloned(),
        }
    }

    fn owner_opens(&self) -> Option<Rc<MenuHost>> {
        self.host.upgrade().filter(|host| host.can_open())
    }
}

impl Action<ShowContextMenuIntent> for ShowSelectionMenu {
    fn is_enabled(&self, intent: &ShowContextMenuIntent) -> bool {
        self.owner_opens().is_some()
            || self
                .enclosing
                .as_ref()
                .is_some_and(|enclosing| enclosing.is_enabled(intent))
    }

    fn invoke(&self, ctx: &mut EventContext<'_>, intent: &ShowContextMenuIntent) -> ActionOutcome {
        if let Some(host) = self.owner_opens() {
            return if host.open(ctx, MenuCause::Key, None) {
                ActionOutcome::Performed
            } else {
                ActionOutcome::NotPerformed
            };
        }
        match &self.enclosing {
            Some(enclosing) if enclosing.is_enabled(intent) => enclosing.invoke(ctx, intent),
            _ => ActionOutcome::NotPerformed,
        }
    }
}
