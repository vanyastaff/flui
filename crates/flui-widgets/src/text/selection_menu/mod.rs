//! The controls of a selection beyond its highlight: the touch handles, the
//! caret handle and the menu of commands, set for a subtree by
//! [`DefaultSelectionControls`].
//!
//! A selection's commands (cut, copy, paste, select all and an app's own)
//! are [`MenuItem`]s performed through the [`Actions`](crate::Actions) chain
//! where the field or area is mounted, so a menu offers exactly what the
//! keys and assistive technology reach. One [`SelectionMenu`] shows them: a
//! touch shows its floating toolbar over the selection, a secondary click,
//! the menu key or assistive technology opens its popup.
//! [`SelectionMenuPolicy`] says which touches show the toolbar.

mod host;
mod magnifier;
mod toolbar;
mod toolbar_items;

pub(crate) use self::host::{MenuHost, MenuSettings, ShowSelectionMenu, show_menu_action};
use self::magnifier::MagnifierChoice;
pub(crate) use self::magnifier::magnifier;
pub use self::magnifier::{MagnifierInfo, PlainMagnifier, SelectionMagnifier};
pub(crate) use self::toolbar::{ToolbarAnchors, toolbar};
pub use self::toolbar_items::{ToolbarItems, ToolbarItemsController, ToolbarItemsState};

use std::fmt;
use std::rc::Rc;

use flui_objects::SubtreeAnchor;
use flui_platform::TargetPlatform;
use flui_rendering::pipeline::PipelineCell;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, View, ViewExt, impl_inherited_view};

use crate::interaction::{
    CopySelectionTextIntent, GestureCommand, MenuItem, MenuPresenter, PasteTextIntent,
    SelectAllTextIntent,
};
use crate::localization::{BoxedWidgetsLocalizations, WidgetsLocalizations};
use crate::text::editable_text::CaretHandle;
use crate::text::selection_area::SelectionHandles;

/// How a selection's menu looks: the floating toolbar a touch shows and
/// the popup a secondary click opens.
///
/// FLUI decides what the menu holds and when it shows; the look lays the
/// items out. [`WidgetsApp`](crate::WidgetsApp) installs a plain one; a design
/// package sets its own on a [`DefaultSelectionControls`].
pub trait SelectionMenu: 'static {
    /// The toolbar a touch shows over a selection, offering `items`.
    ///
    /// FLUI places it above the selection, or below it where there is no
    /// room, never gives it the focus, and hides it with the selection's
    /// handles. Choosing an item is [`MenuItem::perform`]; FLUI hides the
    /// toolbar after the frame, however the item was chosen.
    fn toolbar(&self, items: &[MenuItem], ctx: &dyn BuildContext) -> BoxedView;

    /// A popup for one mounted field or area, which keeps it while this look
    /// is in use there.
    fn popup(&self) -> Rc<dyn SelectionPopup>;
}

/// The popup menu of one mounted field or area, made by its
/// [`SelectionMenu`].
pub trait SelectionPopup {
    /// What shows a request of the field or area built at `ctx`. Called in
    /// each of its builds; the last presenter is the one used.
    fn presenter(&self, ctx: &dyn BuildContext, origin: &PopupOrigin) -> MenuPresenter;

    /// Close what this popup shows, if anything.
    fn close(&self);
}

/// What a [`SelectionPopup`] places itself by when no pointer opened it:
/// the bounds of the field or area.
#[derive(Clone)]
pub struct PopupOrigin {
    anchor: SubtreeAnchor,
    owner: Option<PipelineCell>,
}

impl PopupOrigin {
    pub(crate) fn new(anchor: SubtreeAnchor, owner: Option<PipelineCell>) -> Self {
        Self { anchor, owner }
    }

    /// The field's or area's content.
    #[must_use]
    pub fn anchor(&self) -> &SubtreeAnchor {
        &self.anchor
    }

    /// The pipeline that lays the content out.
    #[must_use]
    pub fn owner(&self) -> Option<&PipelineCell> {
        self.owner.as_ref()
    }
}

impl fmt::Debug for PopupOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PopupOrigin").finish_non_exhaustive()
    }
}

/// Which touches show a selection's toolbar. A secondary click, the menu
/// key and assistive technology open the popup on every platform.
///
/// [`for_platform`](Self::for_platform) follows each platform's own text:
///
/// | Touch | Android, Linux, Fuchsia | iOS | Windows | macOS |
/// |---|---|---|---|---|
/// | a long press selects a word | toolbar | toolbar | toolbar | toolbar |
/// | a tap on the caret handle | toolbar | toolbar | toolbar | no |
/// | a handle drag ends | toolbar | toolbar | toolbar | no |
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::SelectionMenuPolicy;
/// # use flui_platform::TargetPlatform;
/// let mac = SelectionMenuPolicy::for_platform(TargetPlatform::MacOS);
/// assert!(mac.after_long_press());
/// assert!(!mac.after_handle_drag());
/// assert!(!SelectionMenuPolicy::NEVER.after_long_press());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionMenuPolicy {
    after_long_press: bool,
    on_caret_handle_tap: bool,
    after_handle_drag: bool,
}

impl SelectionMenuPolicy {
    /// No touch shows the toolbar.
    pub const NEVER: Self = Self {
        after_long_press: false,
        on_caret_handle_tap: false,
        after_handle_drag: false,
    };

    /// Every touch the table names shows the toolbar.
    pub const ALWAYS: Self = Self {
        after_long_press: true,
        on_caret_handle_tap: true,
        after_handle_drag: true,
    };

    /// The toolbar `platform`'s own text shows.
    #[must_use]
    pub const fn for_platform(platform: TargetPlatform) -> Self {
        match platform {
            TargetPlatform::MacOS => Self::NEVER.with_after_long_press(true),
            _ => Self::ALWAYS,
        }
    }

    /// Whether a long press that selects a word shows the toolbar.
    #[must_use]
    pub const fn with_after_long_press(mut self, shows: bool) -> Self {
        self.after_long_press = shows;
        self
    }

    /// Whether a tap on the caret handle shows the toolbar.
    #[must_use]
    pub const fn with_on_caret_handle_tap(mut self, shows: bool) -> Self {
        self.on_caret_handle_tap = shows;
        self
    }

    /// Whether the end of a handle drag shows the toolbar again.
    #[must_use]
    pub const fn with_after_handle_drag(mut self, shows: bool) -> Self {
        self.after_handle_drag = shows;
        self
    }

    /// Whether a long press that selects a word shows the toolbar.
    #[must_use]
    pub const fn after_long_press(&self) -> bool {
        self.after_long_press
    }

    /// Whether a tap on the caret handle shows the toolbar.
    #[must_use]
    pub const fn on_caret_handle_tap(&self) -> bool {
        self.on_caret_handle_tap
    }

    /// Whether the end of a handle drag shows the toolbar again.
    #[must_use]
    pub const fn after_handle_drag(&self) -> bool {
        self.after_handle_drag
    }
}

impl Default for SelectionMenuPolicy {
    /// The toolbar of the platform the app is built for.
    fn default() -> Self {
        Self::for_platform(TargetPlatform::current())
    }
}

/// Sets the selection controls of the selection areas and editable texts
/// beneath it that set none of their own: their touch handles, when a tap
/// shows a caret handle, their menu's look and policy, and the app's own
/// commands the menu offers after the standard ones.
///
/// Each control left unset here is the one the nearest enclosing
/// `DefaultSelectionControls` sets; the commands of both are offered, the
/// enclosing scope's first.
///
/// # Examples
///
/// ```rust,ignore
/// DefaultSelectionControls::new(content)
///     .handles(DotHandles::new(brand))
///     .command(GestureCommand::new("Translate", TranslateIntent))
/// ```
#[derive(Clone)]
pub struct DefaultSelectionControls {
    own: SelectionControls,
    child: BoxedView,
}

/// What a [`DefaultSelectionControls`] sets; `None` leaves a control to the
/// enclosing scope.
#[derive(Clone, Default)]
pub(crate) struct SelectionControls {
    handles: Option<Rc<dyn SelectionHandles>>,
    caret_handle: Option<CaretHandle>,
    /// `Some(MenuChoice::Hide)` turns the menu off.
    menu: Option<MenuChoice>,
    policy: Option<SelectionMenuPolicy>,
    /// `Some(MagnifierChoice::Hide)` turns the magnifier off.
    magnifier: Option<MagnifierChoice>,
    commands: Rc<[GestureCommand]>,
}

impl SelectionControls {
    /// These controls, each unset one taken from `outer`; `outer`'s
    /// commands come first.
    fn over(&self, outer: &Self) -> Self {
        Self {
            handles: self.handles.clone().or_else(|| outer.handles.clone()),
            caret_handle: self.caret_handle.or(outer.caret_handle),
            menu: self.menu.clone().or_else(|| outer.menu.clone()),
            policy: self.policy.or(outer.policy),
            magnifier: self.magnifier.clone().or_else(|| outer.magnifier.clone()),
            commands: outer
                .commands
                .iter()
                .chain(self.commands.iter())
                .cloned()
                .collect::<Rc<[GestureCommand]>>(),
        }
    }

    pub(crate) fn handles(&self) -> Option<Rc<dyn SelectionHandles>> {
        self.handles.clone()
    }

    pub(crate) fn caret_handle(&self) -> Option<CaretHandle> {
        self.caret_handle
    }

    /// The menu's look, unless a scope turned the menu off or none set one.
    pub(crate) fn menu(&self) -> Option<Rc<dyn SelectionMenu>> {
        match &self.menu {
            Some(MenuChoice::Show(menu)) => Some(Rc::clone(menu)),
            Some(MenuChoice::Hide) | None => None,
        }
    }

    /// Whether some scope set a look or turned the menu off.
    pub(crate) fn menu_decided(&self) -> bool {
        self.menu.is_some()
    }

    pub(crate) fn policy(&self) -> Option<SelectionMenuPolicy> {
        self.policy
    }

    /// The magnifier of a handle drag: the one a scope set, none where a
    /// scope turned it off, else the platform's ([`PlainMagnifier::for_platform`]).
    pub(crate) fn magnifier(controls: Option<&Self>) -> Option<Rc<dyn SelectionMagnifier>> {
        match controls.and_then(|controls| controls.magnifier.as_ref()) {
            Some(MagnifierChoice::Show(magnifier)) => Some(Rc::clone(magnifier)),
            Some(MagnifierChoice::Hide) => None,
            None => PlainMagnifier::for_platform(TargetPlatform::current())
                .map(|plain| Rc::new(plain) as Rc<dyn SelectionMagnifier>),
        }
    }

    pub(crate) fn commands(&self) -> &[GestureCommand] {
        &self.commands
    }
}

impl fmt::Debug for DefaultSelectionControls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultSelectionControls")
            .field("caret_handle", &self.own.caret_handle)
            .field("policy", &self.own.policy)
            .field(
                "commands",
                &self
                    .own
                    .commands
                    .iter()
                    .map(GestureCommand::label)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl DefaultSelectionControls {
    /// A scope over `child` that sets nothing yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            own: SelectionControls::default(),
            child: child.into_view().boxed(),
        }
    }

    /// The touch handles of the areas and fields beneath.
    #[must_use]
    pub fn handles(mut self, handles: impl SelectionHandles) -> Self {
        self.own.handles = Some(Rc::new(handles));
        self
    }

    /// When a tap in an editable text beneath shows a caret handle, in
    /// place of the platform's ([`CaretHandle::default`]).
    #[must_use]
    pub fn caret_handle(mut self, caret_handle: CaretHandle) -> Self {
        self.own.caret_handle = Some(caret_handle);
        self
    }

    /// The look of the selection menu beneath.
    #[must_use]
    pub fn menu(mut self, menu: impl SelectionMenu) -> Self {
        self.own.menu = Some(MenuChoice::Show(Rc::new(menu)));
        self
    }

    /// No selection menu beneath: neither a toolbar nor a popup.
    #[must_use]
    pub fn no_menu(mut self) -> Self {
        self.own.menu = Some(MenuChoice::Hide);
        self
    }

    /// Which touches show the toolbar beneath, in place of the platform's
    /// ([`SelectionMenuPolicy::default`]).
    #[must_use]
    pub fn menu_policy(mut self, policy: SelectionMenuPolicy) -> Self {
        self.own.policy = Some(policy);
        self
    }

    /// The magnifier a handle drag beneath shows, in place of the
    /// platform's ([`PlainMagnifier::for_platform`]).
    #[must_use]
    pub fn magnifier(mut self, magnifier: impl SelectionMagnifier) -> Self {
        self.own.magnifier = Some(MagnifierChoice::Show(Rc::new(magnifier)));
        self
    }

    /// No magnifier beneath, though the platform shows one.
    #[must_use]
    pub fn no_magnifier(mut self) -> Self {
        self.own.magnifier = Some(MagnifierChoice::Hide);
        self
    }

    /// Also offer `command` in the menu, after the standard commands
    /// ("Translate", "Share"). Its intent is performed through the
    /// [`Actions`](crate::Actions) chain where the field or area is mounted.
    #[must_use]
    pub fn command(mut self, command: GestureCommand) -> Self {
        self.own.commands = self
            .own
            .commands
            .iter()
            .cloned()
            .chain(std::iter::once(command))
            .collect::<Rc<[GestureCommand]>>();
        self
    }

    /// The handles the nearest scope above `ctx` sets, registering a
    /// dependency on it.
    pub fn handles_of(ctx: &dyn BuildContext) -> Option<Rc<dyn SelectionHandles>> {
        Self::controls_of(ctx).and_then(|controls| controls.handles())
    }

    /// The caret handle the nearest scope above `ctx` sets, registering a
    /// dependency on it.
    pub fn caret_handle_of(ctx: &dyn BuildContext) -> Option<CaretHandle> {
        Self::controls_of(ctx).and_then(|controls| controls.caret_handle())
    }

    /// The magnifier a handle drag under `ctx` shows, registering a
    /// dependency on the scopes above.
    pub(crate) fn magnifier_of(ctx: &dyn BuildContext) -> Option<Rc<dyn SelectionMagnifier>> {
        SelectionControls::magnifier(Self::controls_of(ctx).as_deref())
    }

    /// Everything the scopes above `ctx` set, registering a dependency.
    pub(crate) fn controls_of(ctx: &dyn BuildContext) -> Option<Rc<SelectionControls>> {
        ctx.depend_on::<SelectionControlsScope, _>(|scope| Rc::clone(&scope.controls))
    }
}

impl View for DefaultSelectionControls {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for DefaultSelectionControls {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let controls = match Self::controls_of(ctx) {
            Some(outer) => self.own.over(&outer),
            None => self.own.clone(),
        };
        SelectionControlsScope {
            controls: Rc::new(controls),
            child: self.child.clone(),
        }
    }
}

/// The merged controls a [`DefaultSelectionControls`] provides.
#[derive(Clone)]
struct SelectionControlsScope {
    controls: Rc<SelectionControls>,
    child: BoxedView,
}

impl fmt::Debug for SelectionControlsScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionControlsScope")
            .finish_non_exhaustive()
    }
}

impl InheritedView for SelectionControlsScope {
    type Data = Rc<SelectionControls>;

    fn data(&self) -> &Self::Data {
        &self.controls
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.controls, &old.controls)
    }
}

impl_inherited_view!(SelectionControlsScope);

/// The standard commands of an editable text, labelled in the locale at
/// `ctx`: cut, copy, paste and select all.
pub(crate) fn editing_commands(ctx: &dyn BuildContext) -> [GestureCommand; 4] {
    let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
    let label = |pick: fn(&dyn WidgetsLocalizations) -> &'static str, fallback: &'static str| {
        localizations.as_deref().map_or(fallback, |l| pick(&**l))
    };
    [
        GestureCommand::new(
            label(WidgetsLocalizations::cut_button_label, "Cut"),
            CopySelectionTextIntent::Cut,
        ),
        GestureCommand::new(
            label(WidgetsLocalizations::copy_button_label, "Copy"),
            CopySelectionTextIntent::Copy,
        ),
        GestureCommand::new(
            label(WidgetsLocalizations::paste_button_label, "Paste"),
            PasteTextIntent,
        ),
        GestureCommand::new(
            label(WidgetsLocalizations::select_all_button_label, "Select all"),
            SelectAllTextIntent,
        ),
    ]
}

/// The standard commands of a selection of static text, labelled in the
/// locale at `ctx`: copy and select all. An enclosing answer to cut or
/// paste has nothing to act on there, so they are not offered.
pub(crate) fn reading_commands(ctx: &dyn BuildContext) -> [GestureCommand; 2] {
    let [_cut, copy, _paste, select_all] = editing_commands(ctx);
    [copy, select_all]
}

/// A menu set for a field or a subtree: shown with a look, or turned off.
#[derive(Clone)]
pub(crate) enum MenuChoice {
    Show(Rc<dyn SelectionMenu>),
    Hide,
}

impl MenuChoice {
    /// The look, unless the menu is turned off.
    pub(crate) fn look(&self) -> Option<Rc<dyn SelectionMenu>> {
        match self {
            Self::Show(menu) => Some(Rc::clone(menu)),
            Self::Hide => None,
        }
    }
}
