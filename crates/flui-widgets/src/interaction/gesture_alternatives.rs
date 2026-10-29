//! [`GestureAlternatives`] — every gesture of a behavioural widget reached
//! without the gesture: from assistive technology, from the keyboard and with
//! one pointer and no path (ADR-0216).
//!
//! A behavioural widget (`Dismissible`, swipe actions, reorder, a zoomable
//! viewer, a context-menu area) takes a `GestureAlternatives` in its
//! constructor. Its gesture performs one [`GestureCommand`], an [`Intent`]
//! with a label; the alternatives perform the same intent through the same
//! [`Actions`] chain:
//!
//! | Path | What reaches the intent |
//! |---|---|
//! | gesture | the widget, through [`GestureAlternatives::handle`] |
//! | assistive technology | one custom action per command, labelled by it; `ShowContextMenu` for a menu |
//! | keyboard | the command's own keys, and the platform's menu keys for a menu, while focus is in the row |
//! | one pointer | [`Alternative::VisibleControl`], [`Alternative::ContextMenu`] or [`Alternative::Companion`] |
//!
//! Every command sits inside the [`Alternative`], so no command can exist
//! without its single-pointer path.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use flui_foundation::ChangeNotifier;
use flui_foundation::geometry::Offset;
use flui_interaction::gesture::SequencedBranch;
use flui_interaction::{
    FocusManager, GestureArena, GestureRecognizer, GestureSettingsProvider,
    LongPressGestureRecognizer, LongPressMoveUpdateDetails, LongPressStartDetails, PointerEvent,
    PointerEventExt, PointerId, PointerKindSet, TapGestureRecognizer, TapUpDetails,
};
use flui_platform::TargetPlatform;
use flui_view::prelude::*;
use flui_view::{InheritedView, RebuildHandle, RebuildReason, WriterSource, impl_inherited_view};
use smallvec::SmallVec;

use super::actions::{
    Action, ActionChain, ActionChainProvider, ActionOutcome, Actions, Intent,
    ShowContextMenuIntent, resolve,
};
use super::companion::{CompanionGap, CompanionLink, LinkedCompanions};
use super::focus::Focus;
use super::focus_ring::FocusRing;
use super::gesture_arena_scope::{GestureArenaScope, GesturePolicy};
use super::listener::Listener;
use super::raw_button::RawButton;
use super::raw_gesture_detector::{
    GestureRecognizerFactory, RawGestureDetector, RecognizerContext,
};
use super::recognizer_attachment::RecognizerAttachment;
use super::shortcuts::{Shortcuts, SingleActivator, reserved_key};
use crate::localization::Directionality;
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
};
use crate::{
    Center, ColoredBox, ConstrainedBox, ExcludeSemantics, Flexible, IgnorePointer, LayoutBuilder,
    Padding, Positioned, Row, Semantics, SizedBox, Stack,
};
use flui_objects::{CrossAxisAlignment, MainAxisSize, StackFit};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::parent_data::FlexFit;
use flui_rendering::semantics::SemanticsAnchor;

// ============================================================================
// Commands
// ============================================================================

/// One operation a behavioural widget's gesture performs: an [`Intent`], the
/// label every alternative path shows for it and the keys that perform it.
///
/// The label names the operation's target ("Delete Alice"), because within
/// one row a label names one action. It is the custom action's name for
/// assistive technology, the visible control's and the menu item's label. A
/// [`ControlSpec`] or [`MenuSpec`] holds the command, so the command always
/// has a single-pointer path.
///
/// Passing a closure instead of an intent does not compile: the gesture's
/// effect is an intent bound with [`Actions`], so
/// every path performs the same one.
#[derive(Clone)]
pub struct GestureCommand {
    label: Rc<str>,
    intent: Rc<dyn Intent>,
    intent_name: &'static str,
    keys: Vec<SingleActivator>,
}

impl GestureCommand {
    /// A command performing `intent`, shown as `label`, with no keys.
    #[must_use]
    pub fn new<I: Intent>(label: impl Into<String>, intent: I) -> Self {
        Self {
            label: Rc::from(label.into()),
            intent: Rc::new(intent),
            intent_name: std::any::type_name::<I>(),
            keys: Vec::new(),
        }
    }

    /// Also perform the command on `activator` while focus is in the row.
    ///
    /// A key that already means something on the running platform — Tab,
    /// Enter, Space or Escape, going back, closing the window or tab, or
    /// typing and editing in a text field — is not bound, with a warning:
    /// Escape never performs a command (ADR-0216 §1).
    #[must_use]
    pub fn key(mut self, activator: SingleActivator) -> Self {
        self.keys.push(activator);
        self
    }

    /// The label every alternative path shows.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The keys bound to the command on `platform`: its own, less any the
    /// platform reserves.
    pub fn keys(&self, platform: TargetPlatform) -> impl Iterator<Item = &SingleActivator> + '_ {
        self.keys
            .iter()
            .filter(move |activator| reserved_key(platform, activator).is_none())
    }

    /// What performs this command from a gesture, resolved through the
    /// action chain at `ctx`: how a widget whose gestures perform several
    /// commands (a swipe row's leading and trailing full swipes, its tray
    /// buttons) reaches each one, where [`GestureAlternatives::handle`]
    /// reaches only the first.
    ///
    /// The handle holds the chain as it is at `ctx` and `ctx` depends on it,
    /// so take it in the `build` of the widget that mounts the alternatives,
    /// at their position, and replace the one a gesture callback reads on
    /// every build.
    #[must_use]
    pub fn handle(&self, ctx: &dyn BuildContext) -> CommandHandle {
        CommandHandle {
            command: self.clone(),
            chain: ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone()),
        }
    }

    /// Whether the command performs an intent of type `I`: how a widget that
    /// answers one intent itself checks the commands it was given.
    #[must_use]
    pub fn performs<I: Intent>(&self) -> bool {
        let intent: &dyn Any = &*self.intent;
        intent.is::<I>()
    }

    fn intent_any(&self) -> &dyn Any {
        &*self.intent
    }

    /// Whether assistive technology is offered this command as a custom
    /// action: unless the action bound to its intent in `chain` is disabled.
    /// An intent with no action bound stays offered, so the missing binding
    /// is reported when the command is performed rather than hidden.
    fn is_offered_in(&self, chain: Option<&ActionChain>) -> bool {
        let intent = self.intent_any();
        chain
            .and_then(|chain| chain.get(&intent.type_id()))
            .is_none_or(|action| action.is_enabled(intent))
    }

    /// The signal that this command's offer may have changed, from the action
    /// bound to its intent in `chain`.
    fn offer_changes_in(&self, chain: Option<&ActionChain>) -> Option<ChangeNotifier> {
        chain
            .and_then(|chain| chain.get(&self.intent_any().type_id()))
            .and_then(|action| action.enablement_changes().cloned())
    }

    /// Perform the intent through `chain`.
    fn perform(&self, cx: &mut EventCx<'_>, chain: Option<&ActionChain>) -> ActionOutcome {
        let intent: &dyn Any = &*self.intent;
        let Some(action) = chain.and_then(|chain| resolve(chain, intent)) else {
            if chain.is_none_or(|chain| !chain.contains_key(&intent.type_id())) {
                tracing::warn!(
                    target: "flui::a11y",
                    intent = self.intent_name,
                    command = &*self.label,
                    "a gesture command's intent has no action above its GestureAlternatives; \
                     bind it with Actions"
                );
            }
            return ActionOutcome::NotPerformed;
        };
        action.invoke(cx, intent)
    }

    /// Report this command's keys reserved on `platform`, except those
    /// `already` reported.
    fn warn_reserved_keys(&self, platform: TargetPlatform, already: &[&SingleActivator]) {
        for activator in &self.keys {
            if already.contains(&activator) {
                continue;
            }
            if let Some(reserved) = reserved_key(platform, activator) {
                tracing::warn!(
                    target: "flui::a11y",
                    command = &*self.label,
                    ?activator,
                    ?reserved,
                    "a gesture command's key is reserved on this platform and is not bound"
                );
            }
        }
    }
}

impl fmt::Debug for GestureCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureCommand")
            .field("label", &self.label)
            .field("intent", &self.intent_name)
            .field("keys", &self.keys)
            .finish()
    }
}

/// Performs a gesture's command from the gesture itself, through the same
/// action chain its alternatives use.
///
/// Built by [`GestureAlternatives::handle`] in the behavioural widget's
/// `build`, and called from its gesture callback.
#[derive(Clone)]
pub struct CommandHandle {
    command: GestureCommand,
    chain: Option<ActionChain>,
}

impl CommandHandle {
    /// Perform the command: what the gesture does when it completes.
    pub fn perform(&self, cx: &mut EventCx<'_>) -> ActionOutcome {
        self.command.perform(cx, self.chain.as_ref())
    }
}

impl fmt::Debug for CommandHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandHandle")
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// Single-pointer alternatives
// ============================================================================

/// How a behavioural widget's commands are reached with one pointer and no
/// path or second contact (WCAG 2.2, 2.5.1 and 2.5.7).
///
/// The set is closed: a control always shown, a context menu, or controls a
/// companion widget already draws.
#[derive(Clone, Debug)]
pub enum Alternative {
    /// Controls always shown beside the content, each performing its
    /// command when pressed. The default choice.
    VisibleControl(ControlSpec),
    /// A context menu opened by a secondary click, a long press, the
    /// platform's menu key and assistive technology, one item per command.
    ContextMenu(MenuSpec),
    /// Controls drawn by another widget that is mounted beside the content
    /// and registered on a [`CompanionLink`]: a tab bar for a pager.
    Companion(CompanionSpec),
}

/// Where controls sit beside the content, in reading order: mirrored in a
/// right-to-left locale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ControlPlacement {
    /// Before the content: the left in a left-to-right locale.
    Leading,
    /// After the content: the right in a left-to-right locale.
    #[default]
    Trailing,
}

/// The controls of [`Alternative::VisibleControl`]: one per command, the
/// first performing the widget's gesture command.
///
/// Each control is a focusable button labelled by its command, activated on
/// release (a press that moves off cancels) and hit across at least the
/// gesture settings' minimum target, reachable without hover.
#[derive(Clone)]
pub struct ControlSpec {
    controls: Vec<(GestureCommand, BoxedView)>,
    placement: ControlPlacement,
}

impl ControlSpec {
    /// A control showing `content` that performs `command`, the gesture's.
    #[must_use]
    pub fn new(command: GestureCommand, content: impl IntoView) -> Self {
        Self {
            controls: vec![(command, content.into_view().boxed())],
            placement: ControlPlacement::default(),
        }
    }

    /// Another control, for a further command the widget's gestures perform.
    #[must_use]
    pub fn control(mut self, command: GestureCommand, content: impl IntoView) -> Self {
        self.controls.push((command, content.into_view().boxed()));
        self
    }

    /// Where the controls sit; [`ControlPlacement::Trailing`] by default.
    #[must_use]
    pub fn placement(mut self, placement: ControlPlacement) -> Self {
        self.placement = placement;
        self
    }
}

impl fmt::Debug for ControlSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlSpec")
            .field(
                "commands",
                &self
                    .controls
                    .iter()
                    .map(|(command, _)| command.label())
                    .collect::<Vec<_>>(),
            )
            .field("placement", &self.placement)
            .finish()
    }
}

/// Shows a context menu: called with each [`MenuRequest`], it presents the
/// request's items until one is chosen or the menu is dismissed.
pub type MenuPresenter = Rc<dyn Fn(&mut EventCx<'_>, MenuRequest)>;

/// The items of [`Alternative::ContextMenu`], the first performing the
/// widget's gesture command, and the presenter that shows them.
///
/// The menu opens on a secondary click, on a long press that joins the arena
/// as the hold of a `Sequenced` composition (ADR-0208) after the gesture
/// settings' long-press timeout, which the operating system's accessibility
/// delay raises, on the platform's menu key, on assistive technology's
/// `ShowContextMenu`, and from a control revealed while keyboard or
/// assistive-technology focus is in the row. Each item is also its own
/// custom action. None of these can be turned off.
///
/// The long press is the hold the long-press drags beneath the row succeed:
/// a finger that opened the menu and then moves past the pan slop hands its
/// contact to the drag under it, and the menu closes through
/// [`dismiss`](Self::dismiss). A finger that moves with no such drag beneath
/// leaves the menu open.
#[derive(Clone)]
pub struct MenuSpec {
    items: Vec<GestureCommand>,
    presenter: MenuPresenter,
    dismiss: Option<MenuDismiss>,
}

/// Closes the menu a [`MenuPresenter`] shows.
type MenuDismiss = Rc<dyn Fn(&mut EventCx<'_>)>;

impl MenuSpec {
    /// A menu whose first item is `first`, the gesture's command, shown by
    /// `presenter`.
    #[must_use]
    pub fn new(
        first: GestureCommand,
        presenter: impl Fn(&mut EventCx<'_>, MenuRequest) + 'static,
    ) -> Self {
        Self {
            items: vec![first],
            presenter: Rc::new(presenter),
            dismiss: None,
        }
    }

    /// Another item.
    #[must_use]
    pub fn item(mut self, command: GestureCommand) -> Self {
        self.items.push(command);
        self
    }

    /// Close the menu the presenter shows when the long press that opened it
    /// hands its contact to a drag. Without it the menu stays open behind
    /// the drag.
    #[must_use]
    pub fn dismiss(mut self, dismiss: impl Fn(&mut EventCx<'_>) + 'static) -> Self {
        self.dismiss = Some(Rc::new(dismiss));
        self
    }
}

impl fmt::Debug for MenuSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuSpec")
            .field(
                "items",
                &self
                    .items
                    .iter()
                    .map(GestureCommand::label)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

/// The commands of [`Alternative::Companion`], and the [`CompanionLink`] the
/// widget that draws their controls registers on.
///
/// The alternatives draw no control: each command is performed by the node a
/// registered companion publishes for it. With no companion registered the
/// alternatives are unfulfilled, which is a `flui::a11y` warning once the frame
/// that mounted them has run and a failure of
/// `flui_testing::assert_gesture_alternatives`.
///
/// Every command performs an intent the link carries; the first is the
/// widget's gesture command.
#[derive(Clone)]
pub struct CompanionSpec {
    commands: Vec<GestureCommand>,
    link: Rc<dyn LinkedCompanions>,
}

impl CompanionSpec {
    /// Alternatives whose first command is `first`, the gesture's, performed
    /// by the companions registered on `link`.
    #[must_use]
    pub fn new<I: Intent>(first: GestureCommand, link: &CompanionLink<I>) -> Self {
        Self {
            commands: vec![first],
            link: link.linked(),
        }
    }

    /// Another command, for a further operation the widget's gestures
    /// perform.
    #[must_use]
    pub fn command(mut self, command: GestureCommand) -> Self {
        self.commands.push(command);
        self
    }

    /// The node of a registered companion that performs `command` now.
    ///
    /// # Errors
    ///
    /// [`CompanionGap::Unregistered`] when no companion is registered,
    /// [`CompanionGap::Unavailable`] when none has a node for the command.
    pub fn node_for(&self, command: &GestureCommand) -> Result<SemanticsAnchor, CompanionGap> {
        self.link.node_for(command.intent_any())
    }
}

impl fmt::Debug for CompanionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompanionSpec")
            .field(
                "commands",
                &self
                    .commands
                    .iter()
                    .map(GestureCommand::label)
                    .collect::<Vec<_>>(),
            )
            .field("fulfilled", &self.link.is_fulfilled())
            .finish()
    }
}

// ============================================================================
// Commands whose intent the widget fixes
// ============================================================================

/// How one command of a widget that fixes its own intents appears on every
/// path: its label, its keys and what its visible control shows.
///
/// A [`Draggable`](crate::Draggable) cuts and copies, a
/// [`DropTarget`](crate::DropTarget) pastes: they take the appearance of each
/// operation and pair it with the intent their gesture performs, so no
/// alternative of theirs can perform another operation.
#[derive(Clone)]
pub struct CommandAppearance {
    label: String,
    keys: Vec<SingleActivator>,
    content: BoxedView,
}

impl CommandAppearance {
    /// A command shown as `label`, its visible control showing `content`.
    #[must_use]
    pub fn new(label: impl Into<String>, content: impl IntoView) -> Self {
        Self {
            label: label.into(),
            keys: Vec::new(),
            content: content.into_view().boxed(),
        }
    }

    /// Also perform the command on `activator` while focus is in the row, as
    /// [`GestureCommand::key`] does.
    #[must_use]
    pub fn key(mut self, activator: SingleActivator) -> Self {
        self.keys.push(activator);
        self
    }

    /// The command performing `intent`, and its control's content.
    pub(crate) fn performing<I: Intent>(&self, intent: I) -> (GestureCommand, BoxedView) {
        let command = self.keys.iter().cloned().fold(
            GestureCommand::new(self.label.clone(), intent),
            GestureCommand::key,
        );
        (command, self.content.clone())
    }
}

impl fmt::Debug for CommandAppearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandAppearance")
            .field("label", &self.label)
            .field("keys", &self.keys)
            .finish_non_exhaustive()
    }
}

/// How a widget that fixes its own intents reaches its commands with one
/// pointer: an [`Alternative`] whose commands the widget supplies.
#[derive(Clone)]
#[non_exhaustive]
pub enum AlternativeKind {
    /// Each command as a control always shown beside the content.
    VisibleControl(ControlPlacement),
    /// A context menu, one item per command, shown by the presenter.
    ContextMenu(MenuPresenter),
}

impl Default for AlternativeKind {
    /// Controls after the content.
    fn default() -> Self {
        Self::VisibleControl(ControlPlacement::default())
    }
}

impl AlternativeKind {
    /// A context menu shown by `presenter`.
    #[must_use]
    pub fn context_menu(presenter: impl Fn(&mut EventCx<'_>, MenuRequest) + 'static) -> Self {
        Self::ContextMenu(Rc::new(presenter))
    }
}

impl fmt::Debug for AlternativeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VisibleControl(placement) => {
                f.debug_tuple("VisibleControl").field(placement).finish()
            }
            Self::ContextMenu(_) => f.write_str("ContextMenu"),
        }
    }
}

impl GestureAlternatives {
    /// The alternatives of `commands`, the first the gesture's, reached the
    /// way `kind` says.
    pub(crate) fn of_kind(
        kind: AlternativeKind,
        first: (GestureCommand, BoxedView),
        more: impl IntoIterator<Item = (GestureCommand, BoxedView)>,
    ) -> Result<Self, GestureAlternativesError> {
        let alternative = match kind {
            AlternativeKind::VisibleControl(placement) => {
                let spec = more.into_iter().fold(
                    ControlSpec::new(first.0, first.1),
                    |spec, (command, content)| spec.control(command, content),
                );
                Alternative::VisibleControl(spec.placement(placement))
            }
            AlternativeKind::ContextMenu(presenter) => {
                let menu = MenuSpec {
                    items: vec![first.0],
                    presenter,
                    dismiss: None,
                };
                Alternative::ContextMenu(
                    more.into_iter()
                        .fold(menu, |menu, (command, _)| menu.item(command)),
                )
            }
        };
        Self::new(alternative)
    }
}

/// What opened a context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MenuCause {
    /// A secondary click on the row.
    SecondaryClick,
    /// A long press on the row.
    LongPress,
    /// The platform's menu key.
    Key,
    /// Assistive technology's `ShowContextMenu`.
    Assistive,
    /// The control revealed while focus is in the row.
    Control,
}

/// One request to show a context menu, handed to its [`MenuPresenter`].
#[derive(Clone)]
pub struct MenuRequest {
    items: Rc<[MenuItem]>,
    cause: MenuCause,
    position: Option<Offset<f64>>,
}

impl MenuRequest {
    /// The items to show, in order.
    #[must_use]
    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    /// What opened the menu.
    #[must_use]
    pub fn cause(&self) -> MenuCause {
        self.cause
    }

    /// Where a pointer opened the menu, in root coordinates; `None` when a
    /// key, assistive technology or the revealed control did.
    #[must_use]
    pub fn position(&self) -> Option<Offset<f64>> {
        self.position
    }
}

impl fmt::Debug for MenuRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuRequest")
            .field("items", &self.items)
            .field("cause", &self.cause)
            .field("position", &self.position)
            .finish()
    }
}

/// One item of a [`MenuRequest`].
#[derive(Clone)]
pub struct MenuItem {
    command: GestureCommand,
    chain: Option<ActionChain>,
}

impl MenuItem {
    /// The item's label.
    #[must_use]
    pub fn label(&self) -> &str {
        self.command.label()
    }

    /// Perform the item's command, as choosing it does.
    pub fn perform(&self, cx: &mut EventCx<'_>) -> ActionOutcome {
        self.command.perform(cx, self.chain.as_ref())
    }
}

impl fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MenuItem").field(&self.label()).finish()
    }
}

// ============================================================================
// GestureAlternatives
// ============================================================================

/// Why a [`GestureAlternatives`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GestureAlternativesError {
    /// A command has an empty label, which no path can show.
    #[error("a gesture command has an empty label")]
    EmptyLabel,
    /// Two commands share a label: assistive technology would offer one
    /// action for both.
    #[error("two gesture commands are labelled {label:?}")]
    DuplicateLabel {
        /// The shared label.
        label: String,
    },
    /// Two commands have keys one key press matches: `ctrl+d` and `ctrl+D`,
    /// or Shift ignored beside Shift required, so pressing it could perform
    /// either.
    #[error("two gesture commands share the key {key:?}")]
    DuplicateKey {
        /// The later command's key that a press of an earlier one matches.
        key: SingleActivator,
    },
    /// A companion command performs an intent of a type its
    /// [`CompanionLink`] does not carry, so no companion could ever publish
    /// a node for it.
    #[error("the gesture command {label:?} performs an intent its companion link does not carry")]
    ForeignIntent {
        /// The command's label.
        label: String,
    },
}

/// The alternatives of a behavioural widget's gestures: a required argument
/// of its constructor (ADR-0216).
///
/// Mounted around the widget's content (with [`child`](Self::child)), it
/// makes the content a row in focus traversal and publishes:
///
/// - for assistive technology, one custom action per command on the row's
///   node, and `ShowContextMenu` for a menu;
/// - for the keyboard, each command's keys while focus is in the row, and the
///   platform's menu keys for a menu;
/// - for one pointer, the [`Alternative`]'s controls or menu.
///
/// The widget's gesture performs the first command through
/// [`handle`](Self::handle). Every path resolves the command's intent through
/// the [`Actions`] chain where the alternatives are
/// mounted, so every path has one outcome.
#[derive(Clone, StatefulView)]
pub struct GestureAlternatives {
    alternative: Alternative,
    target: Option<Rc<str>>,
    child: Option<BoxedView>,
    /// The hold a menu's long press joins instead of composing its own: the
    /// widget's own long-press drag succeeds it.
    hold: Option<Rc<SequencedBranch>>,
}

impl GestureAlternatives {
    /// The alternatives `alternative` describes.
    ///
    /// # Errors
    ///
    /// [`GestureAlternativesError::EmptyLabel`] for a command without a
    /// label, [`GestureAlternativesError::DuplicateLabel`] for two commands
    /// with one, [`GestureAlternativesError::DuplicateKey`] for two commands
    /// with one key, [`GestureAlternativesError::ForeignIntent`] for a
    /// companion command whose intent its link does not carry.
    pub fn new(alternative: Alternative) -> Result<Self, GestureAlternativesError> {
        let alternatives = Self {
            alternative,
            target: None,
            child: None,
            hold: None,
        };
        let mut seen: Vec<&str> = Vec::new();
        let mut keys: Vec<&SingleActivator> = Vec::new();
        for command in alternatives.commands() {
            if command.label().is_empty() {
                return Err(GestureAlternativesError::EmptyLabel);
            }
            if seen.contains(&command.label()) {
                return Err(GestureAlternativesError::DuplicateLabel {
                    label: command.label().to_owned(),
                });
            }
            seen.push(command.label());
            let earlier = keys.len();
            for key in &command.keys {
                if keys[..earlier].iter().any(|earlier| earlier.overlaps(key)) {
                    return Err(GestureAlternativesError::DuplicateKey { key: key.clone() });
                }
                keys.push(key);
            }
            if let Alternative::Companion(spec) = &alternatives.alternative
                && !spec.link.accepts(command.intent_any())
            {
                return Err(GestureAlternativesError::ForeignIntent {
                    label: command.label().to_owned(),
                });
            }
        }
        Ok(alternatives)
    }

    /// Name what the commands act on ("Alice"): the localized labels the
    /// alternatives make themselves, such as the menu control's, take it as
    /// a parameter.
    #[must_use]
    pub fn target(mut self, name: impl Into<String>) -> Self {
        self.target = Some(Rc::from(name.into()));
        self
    }

    /// Mount the alternatives around `content`, the widget's own subtree.
    #[must_use]
    pub fn child(mut self, content: impl IntoView) -> Self {
        self.child = Some(content.into_view().boxed());
        self
    }

    /// Open the menu's long press as `hold`, whose successor the mounting
    /// widget's own long-press drag is, instead of composing a hold of its
    /// own. Ignored by visible controls.
    pub(crate) fn with_hold(mut self, hold: Option<Rc<SequencedBranch>>) -> Self {
        self.hold = hold;
        self
    }

    /// Show the menu through `presenter` and close it through `dismiss`,
    /// keeping the commands already checked. Ignored by visible controls and
    /// companions.
    pub(crate) fn present_menu_with(
        mut self,
        presenter: MenuPresenter,
        dismiss: impl Fn(&mut EventCx<'_>) + 'static,
    ) -> Self {
        if let Alternative::ContextMenu(spec) = &mut self.alternative {
            spec.presenter = presenter;
            spec.dismiss = Some(Rc::new(dismiss));
        }
        self
    }

    /// Whether one pointer reaches the commands through a context menu.
    pub(crate) fn is_menu(&self) -> bool {
        matches!(self.alternative, Alternative::ContextMenu(_))
    }

    /// Every command, the gesture's first.
    pub fn commands(&self) -> impl Iterator<Item = &GestureCommand> + '_ {
        let (controls, items): (&[(GestureCommand, BoxedView)], &[GestureCommand]) =
            match &self.alternative {
                Alternative::VisibleControl(spec) => (&spec.controls, &[]),
                Alternative::ContextMenu(menu) => (&[], &menu.items),
                Alternative::Companion(spec) => (&[], &spec.commands),
            };
        controls.iter().map(|(command, _)| command).chain(items)
    }

    /// The single-pointer alternative.
    #[must_use]
    pub fn alternative(&self) -> &Alternative {
        &self.alternative
    }

    /// What the widget's gesture calls to perform its command, resolved
    /// through the action chain at `ctx`. Call it from the `build` of the
    /// widget that mounts these alternatives, so the gesture and the
    /// alternatives resolve at one position.
    #[must_use]
    pub fn handle(&self, ctx: &dyn BuildContext) -> CommandHandle {
        self.commands()
            .next()
            .expect("BUG: an alternative holds at least one command")
            .handle(ctx)
    }
}

impl fmt::Debug for GestureAlternatives {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureAlternatives")
            .field("alternative", &self.alternative)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

/// Opens the menu with the cause and pointer position.
type MenuOpener = Rc<dyn Fn(&mut EventCx<'_>, MenuCause, Option<Offset<f64>>)>;

impl StatefulView for GestureAlternatives {
    type State = GestureAlternativesState;

    fn create_state(&self) -> Self::State {
        let platform = TargetPlatform::current();
        for command in self.commands() {
            command.warn_reserved_keys(platform, &[]);
        }
        GestureAlternativesState {
            opener: OpenerSlot::new(Rc::new(|_cx, _cause, _position| {})),
            menu: self.is_menu(),
            given_hold: self.hold.clone(),
            policy: None,
            hold: None,
            hold_attachment: Rc::new(RecognizerAttachment::default()),
            pressed: Rc::default(),
            companion: match &self.alternative {
                Alternative::Companion(spec) => Some(CompanionWatch {
                    link: Rc::downgrade(&spec.link),
                    commands: spec
                        .commands
                        .iter()
                        .map(|command| command.label().to_owned())
                        .collect(),
                }),
                _ => None,
            },
            mounted: Rc::new(Cell::new(true)),
            control_revealed: Rc::new(Cell::new(false)),
        }
    }
}

/// What a companion's alternatives need to report, after the frame they
/// mounted in, that no companion registered.
struct CompanionWatch {
    link: Weak<dyn LinkedCompanions>,
    commands: Vec<String>,
}

/// What a mounted [`GestureAlternatives`] keeps across rebuilds: the menu
/// opener its live pointer triggers read when they fire, replaced by each
/// build so a trigger always opens the latest items, and a menu's long press.
pub struct GestureAlternativesState {
    opener: OpenerSlot,
    /// Whether the alternative is a menu, and the hold the view hands its
    /// long press.
    menu: bool,
    given_hold: Option<Rc<SequencedBranch>>,
    /// The arena, settings and writer the long press is built against.
    policy: Option<(GesturePolicy, WriterSource)>,
    hold: Option<MenuHold>,
    hold_attachment: Rc<RecognizerAttachment<LongPressGestureRecognizer>>,
    /// The contacts down on the row, one of which the long press holds.
    pressed: Rc<RefCell<SmallVec<[PointerId; 2]>>>,
    /// A companion alternative's link, until its check has been scheduled.
    companion: Option<CompanionWatch>,
    /// Cleared on dispose, so a check queued by the mount frame stays silent
    /// for alternatives that have gone.
    mounted: Rc<Cell<bool>>,
    /// Whether a menu row's revealed control is shown: set when visible focus
    /// is in the row, cleared the moment focus leaves it (the row's
    /// `on_focus_change`), so focus that leaves and comes back by pointer
    /// within one frame does not find it still set.
    control_revealed: Rc<Cell<bool>>,
}

/// A menu's long press: a hold whose successors are the long-press drags
/// beneath the row, and the recognizer built as that hold.
struct MenuHold {
    branch: Option<Rc<SequencedBranch>>,
    recognizer: Rc<LongPressGestureRecognizer>,
    /// What the hold was built for: the arena handle and settings, and the
    /// hold the view handed it.
    built_for: (GesturePolicy, Option<Rc<SequencedBranch>>),
}

impl fmt::Debug for GestureAlternativesState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureAlternativesState")
            .field("menu", &self.menu)
            .finish_non_exhaustive()
    }
}

impl GestureAlternativesState {
    /// Acquire the arena, the policy and the writer, in a lifecycle hook.
    /// Only a menu's long press uses them. A companion row mounts no gesture
    /// detector, so it works outside a gesture scope and acquires none there;
    /// a menu cannot be outside one without a long press, since its
    /// secondary-click detector panics outside a scope first.
    fn acquire(&mut self, ctx: &dyn LifecycleContext) {
        self.policy = GesturePolicy::maybe_of(ctx).map(|policy| (policy, ctx.writer_source()));
        self.rebuild_hold();
    }

    /// Build the menu's long press for the current arena, policy and hold,
    /// retiring the one it replaces; drop it when the alternative is no
    /// longer a menu.
    fn rebuild_hold(&mut self) {
        let Some((policy, writer)) = self.policy.clone().filter(|_| self.menu) else {
            self.hold_attachment.clear();
            if let Some(outgoing) = self.hold.take() {
                let _ = outgoing.recognizer.cancel();
            }
            return;
        };
        let current = self.hold.as_ref().is_some_and(|hold| {
            let (built_policy, built_hold) = &hold.built_for;
            *built_policy == policy && same_hold(built_hold.as_ref(), self.given_hold.as_ref())
        });
        if current {
            return;
        }
        let GesturePolicy { arena, settings } = &policy;
        let branch = match &self.given_hold {
            Some(branch) => Some(Rc::clone(branch)),
            None => match arena.sequenced() {
                Ok(branch) => Some(Rc::new(branch)),
                Err(error) => {
                    tracing::warn!(
                        target: "flui::a11y",
                        %error,
                        "the context menu's long press could not compose as Sequenced; it joins the arena directly"
                    );
                    None
                }
            },
        };
        let recognizer = menu_long_press(
            branch.as_ref(),
            arena,
            settings,
            &writer,
            &self.opener,
            &self.pressed,
        );
        self.hold_attachment.attach(&recognizer);
        let outgoing = self.hold.replace(MenuHold {
            branch,
            recognizer,
            built_for: (policy, self.given_hold.clone()),
        });
        if let Some(outgoing) = outgoing {
            let _ = outgoing.recognizer.cancel();
        }
    }
}

fn same_hold(a: Option<&Rc<SequencedBranch>>, b: Option<&Rc<SequencedBranch>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// The menu's long press, built as the hold of `branch` (or, when the
/// composition was refused, as a plain member of `arena`): it opens the menu,
/// and once the contact moves past the pan slop it hands the contact to the
/// drag beneath and closes the menu.
fn menu_long_press(
    branch: Option<&Rc<SequencedBranch>>,
    arena: &GestureArena,
    settings: &GestureSettingsProvider,
    writer: &WriterSource,
    menu: &OpenerSlot,
    pressed: &Rc<RefCell<SmallVec<[PointerId; 2]>>>,
) -> Rc<LongPressGestureRecognizer> {
    let hold_arena = branch.map_or_else(|| arena.clone(), |branch| branch.hold());
    let (opener, open_writer) = (menu.clone(), writer.clone());
    let mut builder = LongPressGestureRecognizer::builder(hold_arena)
        .settings(settings.clone())
        .allowed_devices(LONG_PRESS_DEVICES)
        .on_long_press_start(move |details: LongPressStartDetails| {
            let open = opener.get();
            open_writer.write(|cx| open(cx, MenuCause::LongPress, Some(details.global_position)));
        });
    if let Some(branch) = branch {
        let (branch, settings, writer) = (Rc::clone(branch), settings.clone(), writer.clone());
        let (menu, pressed) = (menu.clone(), Rc::clone(pressed));
        builder = builder.on_long_press_move_update(move |details: LongPressMoveUpdateDetails| {
            let moved = settings
                .snapshot_for(details.kind)
                .exceeds_pan_slop_for(details.kind, details.offset_from_origin);
            if !moved {
                return;
            }
            let contacts: SmallVec<[PointerId; 2]> = pressed.borrow().iter().copied().collect();
            if contacts.into_iter().any(|pointer| branch.hand_off(pointer))
                && let Some(dismiss) = menu.dismiss()
            {
                writer.write(|cx| dismiss(cx));
            }
        });
    }
    builder.build()
}

impl ViewState<GestureAlternatives> for GestureAlternativesState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
        self.watch_companion(ctx);
    }

    /// A new arena, policy or writer replaces the menu's long press.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    /// A view that turns the alternative into a menu, out of one, or hands
    /// the long press another hold replaces it.
    fn did_update_view(&mut self, old_view: &GestureAlternatives, new_view: &GestureAlternatives) {
        // A rebuild reports only the reserved keys it brings: those the
        // command already had were reported when it got them.
        let platform = TargetPlatform::current();
        for command in new_view.commands() {
            let already: Vec<&SingleActivator> = old_view
                .commands()
                .filter(|old| old.label == command.label)
                .flat_map(|old| &old.keys)
                .collect();
            command.warn_reserved_keys(platform, &already);
        }
        self.menu = new_view.is_menu();
        self.given_hold.clone_from(&new_view.hold);
        self.rebuild_hold();
    }

    fn dispose(&mut self) {
        self.mounted.set(false);
        self.hold_attachment.clear();
        if let Some(hold) = self.hold.take() {
            let _ = hold.recognizer.cancel();
        }
    }

    fn build(&self, view: &GestureAlternatives, ctx: &dyn BuildContext) -> impl IntoView {
        view.build_alternatives(ctx, self)
    }
}

impl GestureAlternativesState {
    /// Report, once the frame this mounted in has run, that no companion
    /// registered. A companion is usually mounted in the same frame, in
    /// either order with these alternatives, so the question is asked after
    /// it rather than during mount. A context with no post-frame lane has no
    /// such moment, and asks nothing.
    fn watch_companion(&mut self, ctx: &dyn LifecycleContext) {
        let (Some(watch), Some(frame)) = (self.companion.take(), ctx.post_frame_handle()) else {
            return;
        };
        let mounted = Rc::clone(&self.mounted);
        let scheduled = frame.schedule(move |_timing| {
            let unfulfilled = watch
                .link
                .upgrade()
                .is_some_and(|link| !link.is_fulfilled());
            if mounted.get() && unfulfilled {
                tracing::warn!(
                    target: "flui::a11y",
                    commands = ?watch.commands,
                    "a gesture's alternatives rely on a companion, and none is registered on \
                     their CompanionLink; its commands have no single-pointer path"
                );
            }
        });
        if let Err(error) = scheduled {
            tracing::debug!(?error, "the companion check found its lane gone");
        }
    }
}

impl GestureAlternatives {
    fn build_alternatives(
        &self,
        ctx: &dyn BuildContext,
        state: &GestureAlternativesState,
    ) -> BoxedView {
        let opener_slot = &state.opener;
        let chain = ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone());
        let platform = TargetPlatform::current();
        let content = self
            .child
            .clone()
            .unwrap_or_else(|| SizedBox::shrink().boxed());

        // A control is drawn at least as large as the minimum target of the
        // coarsest pointer, so no edge or neighbour can clip its hit area.
        let min_target = GestureArenaScope::settings_or_default_of(ctx)
            .min_target_for(flui_interaction::PointerKind::Touch);
        let mut semantics = Semantics::new().container(true);
        let mut shortcuts_bindings = Vec::new();
        for command in self.commands() {
            for activator in command.keys(platform) {
                shortcuts_bindings.push((activator.clone(), Rc::clone(&command.intent)));
            }
            if let Some(changes) = command.offer_changes_in(chain.as_ref()) {
                semantics = semantics.refresh_on(changes);
            }
            let (gate, command, chain) = (command.clone(), command.clone(), chain.clone());
            let gate_chain = chain.clone();
            semantics = semantics.custom_action_when(
                command.label().to_owned(),
                move || gate.is_offered_in(gate_chain.as_ref()),
                move |cx| {
                    let _ = command.perform(cx, chain.as_ref());
                },
            );
        }

        let (row, open_menu) = match &self.alternative {
            Alternative::VisibleControl(spec) => {
                let controls: Vec<BoxedView> = spec
                    .controls
                    .iter()
                    .map(|(command, content)| {
                        let (command, chain) = (command.clone(), chain.clone());
                        control_button(
                            command.label().to_owned(),
                            content.clone(),
                            min_target,
                            move |cx| {
                                let _ = command.perform(cx, chain.as_ref());
                            },
                        )
                    })
                    .collect();
                let content = FocusDecorations {
                    content,
                    menu_control: None,
                    min_target,
                }
                .boxed();
                let placement = spec.placement;
                // The content keeps the kind of constraint the row
                // receives, less the controls: a tight row fills, a loose one
                // takes the content's own width.
                let row = LayoutBuilder::new(move |_ctx, constraints| {
                    let tight = constraints.min_width == constraints.max_width;
                    let tight_height = constraints.min_height == constraints.max_height;
                    let content = Flexible::new(content.clone())
                        .fit(if tight {
                            FlexFit::Tight
                        } else {
                            FlexFit::Loose
                        })
                        .boxed();
                    let controls = controls.iter().cloned();
                    let children: Vec<BoxedView> = match placement {
                        ControlPlacement::Leading => controls.chain([content]).collect(),
                        ControlPlacement::Trailing => {
                            [content].into_iter().chain(controls).collect()
                        }
                    };
                    Row::new(children)
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(if tight_height {
                            CrossAxisAlignment::Stretch
                        } else {
                            CrossAxisAlignment::Center
                        })
                })
                .boxed();
                (row, None)
            }
            // The controls are the companion's: the row is its content under
            // the focus decorations, and its commands reach assistive
            // technology and the keyboard as custom actions and keys.
            Alternative::Companion(_) => {
                let content = FocusDecorations {
                    content,
                    menu_control: None,
                    min_target,
                }
                .boxed();
                (content, None)
            }
            Alternative::ContextMenu(menu) => {
                let items: Rc<[MenuItem]> = menu
                    .items
                    .iter()
                    .map(|command| MenuItem {
                        command: command.clone(),
                        chain: chain.clone(),
                    })
                    .collect();
                let presenter = Rc::clone(&menu.presenter);
                let opener: MenuOpener = Rc::new(move |cx, cause, position| {
                    presenter(
                        cx,
                        MenuRequest {
                            items: Rc::clone(&items),
                            cause,
                            position,
                        },
                    );
                });
                let assistive = Rc::clone(&opener);
                semantics = semantics.on_show_context_menu(move |cx| {
                    assistive(cx, MenuCause::Assistive, None);
                });
                let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
                let label = localizations.as_deref().map_or_else(
                    || {
                        DefaultWidgetsLocalizations
                            .context_menu_button_label(self.target.as_deref())
                    },
                    |localizations| localizations.context_menu_button_label(self.target.as_deref()),
                );
                let decorated = FocusDecorations {
                    content,
                    menu_control: Some(MenuControl {
                        label,
                        opener: Rc::clone(&opener),
                        revealed: Rc::clone(&state.control_revealed),
                    }),
                    min_target,
                };
                opener_slot.set(Rc::clone(&opener), menu.dismiss.clone());
                let triggers = RawGestureDetector::new()
                    .recognizer(SecondaryClickTrigger(opener_slot.clone()))
                    .child(MenuHoldScope {
                        hold: state.hold.as_ref().and_then(|hold| hold.branch.clone()),
                        child: decorated.boxed(),
                    });
                (state.long_press(triggers), Some(OpenMenu(opener)))
            }
        };

        // The row's `Focus` records the chain above it, so the menu's action
        // sits above the `Focus` for a key at the row to find it.
        let revealed = Rc::clone(&state.control_revealed);
        let focus = Focus::new(row)
            .debug_label("GestureAlternatives")
            .on_focus_change(move |_cx, in_row| {
                if !in_row {
                    revealed.set(false);
                }
            })
            .boxed();
        let focus = match open_menu {
            Some(open_menu) => Actions::new(focus).action(open_menu).boxed(),
            None => focus,
        };
        let mut shortcuts = Shortcuts::new(focus);
        for (activator, intent) in shortcuts_bindings {
            shortcuts = shortcuts.shared_shortcut(activator, intent);
        }
        semantics.child(shortcuts).boxed()
    }
}

/// A focusable button showing `content`, labelled `label` alone (what the
/// content draws is not read out), drawn at least `min_target` square with
/// the content centred in it, so neither a window edge nor a neighbour can
/// clip the target (WCAG 2.2, 2.5.8).
fn control_button(
    label: String,
    content: BoxedView,
    min_target: f64,
    on_press: impl Fn(&mut EventCx<'_>) + 'static,
) -> BoxedView {
    let side = if min_target.is_finite() {
        min_target.max(0.0)
    } else {
        0.0
    };
    ConstrainedBox::new(BoxConstraints::new(
        side,
        f64::INFINITY,
        side,
        f64::INFINITY,
    ))
    .child(
        RawButton::new(
            Semantics::new()
                .label(label)
                .child(ExcludeSemantics::new().child(Center::new().child(content))),
        )
        .on_press(on_press),
    )
    .boxed()
}

/// The theme-free glyph's surface and dots: the control paints both, as the
/// theme-free menu does, so whatever the row draws behind it cannot hide it.
const GLYPH_SURFACE: Color = Color::WHITE;
const GLYPH_INK: Color = Color::BLACK;

/// The theme-free "more" glyph the revealed menu control draws: three dots
/// on a surface of their own.
fn more_glyph() -> BoxedView {
    let dot = || {
        Padding::symmetric(1.5, 0.0)
            .child(SizedBox::new(3.0, 3.0).child(ColoredBox::new(GLYPH_INK)))
            .boxed()
    };
    ColoredBox::new(GLYPH_SURFACE)
        .child(
            Padding::all(4.0)
                .child(Row::new(vec![dot(), dot(), dot()]).main_axis_size(MainAxisSize::Min)),
        )
        .boxed()
}

/// Answers [`ShowContextMenuIntent`] for the focused row.
struct OpenMenu(MenuOpener);

impl Action<ShowContextMenuIntent> for OpenMenu {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ShowContextMenuIntent) -> ActionOutcome {
        (self.0)(cx, MenuCause::Key, None);
        ActionOutcome::Performed
    }
}

/// The row's content, with what keyboard or assistive-technology focus in
/// the row reveals over it: the focus ring while the row itself has focus,
/// and, for a menu, the control that opens it — the keyboard's and assistive
/// technology's way to the menu where the platform has no menu key or the
/// adapter does not route `ShowContextMenu` (ADR-0216 §2). Once revealed, the
/// control stays until focus leaves the row or a pointer presses the row off
/// it, so a pointer may press it too.
///
/// The content is laid out with the row's own constraints and stays the
/// first child, so revealing either keeps its elements and state; the
/// revealed control sits inside the row at its trailing edge.
#[derive(Clone, StatefulView)]
struct FocusDecorations {
    content: BoxedView,
    menu_control: Option<MenuControl>,
    min_target: f64,
}

/// A menu row's revealed control: its label, what it opens, and whether it
/// is shown.
#[derive(Clone)]
struct MenuControl {
    label: String,
    opener: MenuOpener,
    /// Set from when visible focus is in the row until focus leaves it,
    /// which the row's `on_focus_change` clears as it happens, or a pointer
    /// presses the row anywhere but on the control. A pointer press ends the
    /// visible focus before it reaches anything, so a control that hid with
    /// the focus ring would leave the pointer that pressed it nothing to
    /// release on; one that stayed after a press elsewhere would cover the
    /// row's trailing edge, where the row's own gesture could not start.
    revealed: Rc<Cell<bool>>,
}

struct FocusDecorationsState {
    focus_manager: Option<Rc<FocusManager>>,
    rebuild: Option<RebuildHandle>,
    /// Set by a press on the revealed control, read by the row's listener
    /// the same press reaches next.
    control_pressed: Rc<Cell<bool>>,
}

impl StatefulView for FocusDecorations {
    type State = FocusDecorationsState;

    fn create_state(&self) -> Self::State {
        FocusDecorationsState {
            focus_manager: None,
            rebuild: None,
            control_pressed: Rc::new(Cell::new(false)),
        }
    }
}

impl ViewState<FocusDecorations> for FocusDecorationsState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());
    }

    /// The row's `Focus` rebuilds this when focus enters or leaves the row
    /// and when the input modality changes while the row has focus.
    fn build(&self, view: &FocusDecorations, ctx: &dyn BuildContext) -> impl IntoView {
        let row = Focus::maybe_of(ctx);
        let focus_in_row = row.as_ref().is_some_and(|row| row.has_focus());
        let visible_in_row = focus_in_row
            && self
                .focus_manager
                .as_ref()
                .and_then(|manager| manager.primary_focus())
                .is_some_and(|primary| primary.has_visible_focus());
        // Every layer keeps its place, so the control is never rebuilt under
        // the press that ends the visible focus: the ring's slot is always
        // there, behind the control, holding the ring while the row has
        // visible focus, and takes no pointer input, so a press lands on
        // what it covers.
        let ring = if row.as_ref().is_some_and(|row| row.has_visible_focus()) {
            FocusRing::new(SizedBox::expand()).boxed()
        } else {
            SizedBox::shrink().boxed()
        };
        let ring = IgnorePointer::new().child(ring);
        let mut layers = vec![view.content.clone(), Positioned::fill(ring).boxed()];
        let revealed = view.menu_control.as_ref().is_some_and(|control| {
            let revealed = focus_in_row && (visible_in_row || control.revealed.get());
            control.revealed.set(revealed);
            revealed
        });
        if revealed && let Some(MenuControl { label, opener, .. }) = &view.menu_control {
            let opener = Rc::clone(opener);
            let control = control_button(label.clone(), more_glyph(), view.min_target, move |cx| {
                opener(cx, MenuCause::Control, None);
            });
            let pressed = Rc::clone(&self.control_pressed);
            let control = Listener::new()
                .on_pointer_down(move |_cx, _dispatch| pressed.set(true))
                .child(control);
            let trailing = Positioned::new(control).top(0.0).bottom(0.0);
            let trailing = match Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr) {
                TextDirection::Rtl => trailing.left(0.0),
                TextDirection::Ltr => trailing.right(0.0),
            };
            layers.push(trailing.boxed());
        }
        let stack = Stack::new(layers).fit(StackFit::Passthrough);
        let Some(MenuControl { revealed, .. }) = &view.menu_control else {
            return stack.boxed();
        };
        // A press anywhere in the row but on the control hides it: the
        // control's own listener has heard the press first if it was on it.
        let (revealed, pressed) = (Rc::clone(revealed), Rc::clone(&self.control_pressed));
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: focus decorations built before init_state captured their rebuild handle");
        Listener::new()
            .on_pointer_down(move |_cx, _dispatch| {
                if !pressed.replace(false) && revealed.replace(false) {
                    rebuild.schedule(RebuildReason::StateChange);
                }
            })
            .child(stack)
            .boxed()
    }
}

impl fmt::Debug for FocusDecorations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusDecorations")
            .field(
                "menu_control",
                &self.menu_control.as_ref().map(|control| &control.label),
            )
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for FocusDecorationsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusDecorationsState")
            .finish_non_exhaustive()
    }
}

/// The menu's secondary-click trigger. The opener is read from the
/// alternatives' slot when the click lands, so a rebuild needs no `update`.
struct SecondaryClickTrigger(OpenerSlot);

impl GestureRecognizerFactory for SecondaryClickTrigger {
    type Recognizer = TapGestureRecognizer;

    fn create(&self, cx: &RecognizerContext) -> Rc<Self::Recognizer> {
        let opener = self.0.clone();
        let writer = cx.writer().clone();
        TapGestureRecognizer::builder(cx.arena().clone())
            .settings(cx.settings().clone())
            .on_secondary_tap(move |details: TapUpDetails| {
                let open = opener.get();
                writer.write(|cx| {
                    open(cx, MenuCause::SecondaryClick, Some(details.global_position));
                });
            })
            .build()
    }

    fn update(&self, _recognizer: &Self::Recognizer) {}
}

/// The pointers a menu's long press admits: a finger and a pen tip. A mouse
/// and a trackpad open the menu by secondary click, and a dwell or switch
/// device by clicking the control focus reveals, so neither holds.
const LONG_PRESS_DEVICES: PointerKindSet = PointerKindSet::TOUCH.union(PointerKindSet::PEN);

impl GestureAlternativesState {
    /// `child` under the menu's long press, which this listener feeds and
    /// whose contacts it records for the hand-off.
    fn long_press(&self, child: impl IntoView) -> BoxedView {
        let (pressed, lifted, cancelled) = (
            Rc::downgrade(&self.pressed),
            Rc::downgrade(&self.pressed),
            Rc::downgrade(&self.pressed),
        );
        Listener::new()
            .on_pointer_down(move |_cx, dispatch| {
                if let (Some(pressed), PointerEvent::Down(press)) =
                    (pressed.upgrade(), dispatch.local)
                {
                    let mut contacts = pressed.borrow_mut();
                    contacts.retain(|id| *id != press.pointer.id);
                    contacts.push(press.pointer.id);
                }
            })
            .on_pointer_up(move |_cx, dispatch| forget_contact(&lifted, dispatch.local))
            .on_pointer_cancel(move |_cx, dispatch| forget_contact(&cancelled, dispatch.local))
            .recognizer(&self.hold_attachment)
            .child(child)
            .boxed()
    }
}

/// A contact on the row ended: the long press holds it no more.
fn forget_contact(pressed: &Weak<RefCell<SmallVec<[PointerId; 2]>>>, event: &PointerEvent) {
    if let (Some(pressed), Some(pointer)) = (pressed.upgrade(), event.pointer_id()) {
        pressed.borrow_mut().retain(|id| *id != pointer);
    }
}

/// The hold of the nearest enclosing context menu's long press, which a
/// long-press drag beneath it succeeds instead of holding the contact itself
/// (ADR-0208 §3): one hold per contact opens the menu, then hands the contact
/// to the drag.
#[derive(Clone)]
pub(crate) struct MenuHoldScope {
    hold: Option<Rc<SequencedBranch>>,
    child: BoxedView,
}

impl MenuHoldScope {
    /// The hold a long-press drag at `ctx` succeeds, if a context menu
    /// encloses it.
    pub(crate) fn of(ctx: &dyn BuildContext) -> Option<Rc<SequencedBranch>> {
        ctx.depend_on::<Self, _>(|scope| scope.hold.clone())
            .flatten()
    }
}

impl InheritedView for MenuHoldScope {
    type Data = Option<Rc<SequencedBranch>>;
    fn data(&self) -> &Self::Data {
        &self.hold
    }
    fn child(&self) -> &dyn View {
        &self.child
    }
    fn update_should_notify(&self, old: &Self) -> bool {
        !same_hold(self.hold.as_ref(), old.hold.as_ref())
    }
}

impl_inherited_view!(MenuHoldScope);

/// What a trigger's callback reads when it fires: the opener, and how the
/// presenter closes the menu.
#[derive(Clone)]
struct OpenerSlot(Rc<RefCell<(MenuOpener, Option<MenuDismiss>)>>);

impl OpenerSlot {
    fn new(opener: MenuOpener) -> Self {
        Self(Rc::new(RefCell::new((opener, None))))
    }

    /// The current opener, with the borrow released before it runs.
    fn get(&self) -> MenuOpener {
        Rc::clone(&self.0.borrow().0)
    }

    /// How the presenter closes the menu, with the borrow released before it
    /// runs.
    fn dismiss(&self) -> Option<MenuDismiss> {
        self.0.borrow().1.clone()
    }

    fn set(&self, opener: MenuOpener, dismiss: Option<MenuDismiss>) {
        let outgoing = self.0.replace((opener, dismiss));
        drop(outgoing);
    }
}
