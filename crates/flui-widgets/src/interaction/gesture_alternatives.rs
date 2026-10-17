//! [`GestureAlternatives`] — every gesture of a behavioural widget reached
//! without the gesture: from assistive technology, from the keyboard and with
//! one pointer and no path (ADR-0216).
//!
//! A behavioural widget (`Dismissible`, swipe actions, reorder, a zoomable
//! viewer, a context-menu area) takes a `GestureAlternatives` in its
//! constructor. Its gesture performs one [`GestureCommand`], an [`Intent`]
//! with a label; the alternatives perform the same intent through the same
//! [`Actions`](super::actions::Actions) chain:
//!
//! | Path | What reaches the intent |
//! |---|---|
//! | gesture | the widget, through [`GestureAlternatives::handle`] |
//! | assistive technology | one custom action per command, labelled by it; `ShowContextMenu` for a menu |
//! | keyboard | the command's own keys, and the platform's menu keys for a menu, while focus is in the row |
//! | one pointer | [`Alternative::VisibleControl`] or [`Alternative::ContextMenu`] |
//!
//! Every command sits inside the [`Alternative`], so no command can exist
//! without its single-pointer path.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_interaction::{
    FocusManager, LongPressGestureRecognizer, LongPressStartDetails, PointerKindSet,
    TapGestureRecognizer, TapUpDetails,
};
use flui_platform_api::TargetPlatform;
use flui_view::prelude::*;

use super::actions::{
    Action, ActionChain, ActionChainProvider, ActionOutcome, Actions, Intent,
    ShowContextMenuIntent, resolve,
};
use super::focus::Focus;
use super::focus_ring::FocusRing;
use super::gesture_arena_scope::GestureArenaScope;
use super::raw_button::RawButton;
use super::raw_gesture_detector::{
    GestureRecognizerFactory, RawGestureDetector, RecognizerContext,
};
use super::shortcuts::{Shortcuts, SingleActivator, reserved_key};
use crate::localization::Directionality;
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
};
use crate::{
    Center, ColoredBox, ConstrainedBox, ExcludeSemantics, Flexible, LayoutBuilder, Padding,
    Positioned, Row, Semantics, SizedBox, Stack,
};
use flui_objects::{CrossAxisAlignment, MainAxisSize, StackFit};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::parent_data::FlexFit;

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
/// effect is an intent bound with [`Actions`](super::actions::Actions), so
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
    /// action chain at `ctx`.
    pub(crate) fn handle(&self, ctx: &dyn BuildContext) -> CommandHandle {
        CommandHandle {
            command: self.clone(),
            chain: ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone()),
        }
    }
    /// Whether the command performs an intent of type `I`.
    pub(crate) fn performs<I: Intent>(&self) -> bool {
        let intent: &dyn Any = &*self.intent;
        intent.is::<I>()
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

    fn warn_reserved_keys(&self, platform: TargetPlatform) {
        for activator in &self.keys {
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
/// The set is closed: a control always shown, or a context menu.
#[derive(Clone, Debug)]
pub enum Alternative {
    /// Controls always shown beside the content, each performing its
    /// command when pressed. The default choice.
    VisibleControl(ControlSpec),
    /// A context menu opened by a secondary click, a long press, the
    /// platform's menu key and assistive technology, one item per command.
    ContextMenu(MenuSpec),
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
#[derive(Clone)]
pub struct MenuSpec {
    items: Vec<GestureCommand>,
    presenter: MenuPresenter,
}

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
        }
    }

    /// Another item.
    #[must_use]
    pub fn item(mut self, command: GestureCommand) -> Self {
        self.items.push(command);
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
/// the [`Actions`](super::actions::Actions) chain where the alternatives are
/// mounted, so every path has one outcome.
#[derive(Clone, StatefulView)]
pub struct GestureAlternatives {
    alternative: Alternative,
    target: Option<Rc<str>>,
    child: Option<BoxedView>,
}

impl GestureAlternatives {
    /// The alternatives `alternative` describes.
    ///
    /// # Errors
    ///
    /// [`GestureAlternativesError::EmptyLabel`] for a command without a
    /// label, [`GestureAlternativesError::DuplicateLabel`] for two commands
    /// with one.
    pub fn new(alternative: Alternative) -> Result<Self, GestureAlternativesError> {
        let alternatives = Self {
            alternative,
            target: None,
            child: None,
        };
        let mut seen: Vec<&str> = Vec::new();
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

    /// Every command, the gesture's first.
    pub fn commands(&self) -> impl Iterator<Item = &GestureCommand> + '_ {
        let (controls, items): (&[(GestureCommand, BoxedView)], &[GestureCommand]) =
            match &self.alternative {
                Alternative::VisibleControl(spec) => (&spec.controls, &[]),
                Alternative::ContextMenu(menu) => (&[], &menu.items),
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
        GestureAlternativesState {
            opener: OpenerSlot::new(Rc::new(|_cx, _cause, _position| {})),
        }
    }
}

/// What a mounted [`GestureAlternatives`] keeps across rebuilds: the menu
/// opener its live pointer triggers read when they fire, replaced by each
/// build so a trigger always opens the latest items.
pub struct GestureAlternativesState {
    opener: OpenerSlot,
}

impl fmt::Debug for GestureAlternativesState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureAlternativesState")
            .finish_non_exhaustive()
    }
}

impl ViewState<GestureAlternatives> for GestureAlternativesState {
    fn build(&self, view: &GestureAlternatives, ctx: &dyn BuildContext) -> impl IntoView {
        view.build_alternatives(ctx, &self.opener)
    }
}

impl GestureAlternatives {
    fn build_alternatives(&self, ctx: &dyn BuildContext, opener_slot: &OpenerSlot) -> BoxedView {
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
            command.warn_reserved_keys(platform);
            for activator in command.keys(platform) {
                shortcuts_bindings.push((activator.clone(), Rc::clone(&command.intent)));
            }
            let (command, chain) = (command.clone(), chain.clone());
            semantics = semantics.custom_action(command.label().to_owned(), move |cx| {
                let _ = command.perform(cx, chain.as_ref());
            });
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
                    menu_control: Some((label, Rc::clone(&opener))),
                    min_target,
                };
                opener_slot.set(Rc::clone(&opener));
                let triggers = RawGestureDetector::new()
                    .recognizer(SecondaryClickTrigger(opener_slot.clone()))
                    .recognizer(LongPressTrigger(opener_slot.clone()))
                    .child(decorated)
                    .boxed();
                (triggers, Some(OpenMenu(opener)))
            }
        };

        // The row's `Focus` records the chain above it, so the menu's action
        // sits above the `Focus` for a key at the row to find it.
        let focus = Focus::new(row).debug_label("GestureAlternatives").boxed();
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

/// The theme-free "more" glyph the revealed menu control draws: three dots.
fn more_glyph() -> BoxedView {
    let dot = || {
        Padding::symmetric(1.5, 0.0)
            .child(SizedBox::new(3.0, 3.0).child(ColoredBox::new(Color::BLACK)))
            .boxed()
    };
    Row::new(vec![dot(), dot(), dot()])
        .main_axis_size(MainAxisSize::Min)
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
/// adapter does not route `ShowContextMenu` (ADR-0216 §2).
///
/// The content is laid out with the row's own constraints and stays the
/// first child, so revealing either keeps its elements and state; the
/// revealed control sits inside the row at its trailing edge.
#[derive(Clone, StatefulView)]
struct FocusDecorations {
    content: BoxedView,
    menu_control: Option<(String, MenuOpener)>,
    min_target: f64,
}

struct FocusDecorationsState {
    focus_manager: Option<Rc<FocusManager>>,
}

impl StatefulView for FocusDecorations {
    type State = FocusDecorationsState;

    fn create_state(&self) -> Self::State {
        FocusDecorationsState {
            focus_manager: None,
        }
    }
}

impl ViewState<FocusDecorations> for FocusDecorationsState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());
    }

    /// The row's `Focus` rebuilds this when focus enters or leaves the row
    /// and when the input modality changes while the row has focus.
    fn build(&self, view: &FocusDecorations, ctx: &dyn BuildContext) -> impl IntoView {
        let row = Focus::maybe_of(ctx);
        let visible_in_row = row.as_ref().is_some_and(|row| row.has_focus())
            && self
                .focus_manager
                .as_ref()
                .and_then(|manager| manager.primary_focus())
                .is_some_and(|primary| primary.has_visible_focus());
        let mut layers = vec![view.content.clone()];
        if row.as_ref().is_some_and(|row| row.has_visible_focus()) {
            layers.push(Positioned::fill(FocusRing::new(SizedBox::expand())).boxed());
        }
        if visible_in_row && let Some((label, opener)) = &view.menu_control {
            let opener = Rc::clone(opener);
            let control = control_button(label.clone(), more_glyph(), view.min_target, move |cx| {
                opener(cx, MenuCause::Control, None);
            });
            let trailing = Positioned::new(control).top(0.0).bottom(0.0);
            let trailing = match Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr) {
                TextDirection::Rtl => trailing.left(0.0),
                TextDirection::Ltr => trailing.right(0.0),
            };
            layers.push(trailing.boxed());
        }
        Stack::new(layers).fit(StackFit::Passthrough)
    }
}

impl fmt::Debug for FocusDecorations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusDecorations")
            .field(
                "menu_control",
                &self.menu_control.as_ref().map(|(label, _)| label),
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

/// The menu's long-press trigger: the hold of a `Sequenced` composition, so
/// a later successor (press, drag onto an item, release) composes with it.
struct LongPressTrigger(OpenerSlot);

impl GestureRecognizerFactory for LongPressTrigger {
    type Recognizer = LongPressGestureRecognizer;

    fn create(&self, cx: &RecognizerContext) -> Rc<Self::Recognizer> {
        let arena = match cx.arena().sequenced() {
            Ok(branch) => branch.hold(),
            Err(error) => {
                tracing::warn!(
                    target: "flui::a11y",
                    %error,
                    "the context menu's long press could not compose as Sequenced; it joins the arena directly"
                );
                cx.arena().clone()
            }
        };
        let opener = self.0.clone();
        let writer = cx.writer().clone();
        LongPressGestureRecognizer::builder(arena)
            .settings(cx.settings().clone())
            .allowed_devices(LONG_PRESS_DEVICES)
            .on_long_press_start(move |details: LongPressStartDetails| {
                let open = opener.get();
                writer.write(|cx| open(cx, MenuCause::LongPress, Some(details.global_position)));
            })
            .build()
    }

    fn update(&self, _recognizer: &Self::Recognizer) {}
}

/// The opener a trigger's callback reads when it fires.
#[derive(Clone)]
struct OpenerSlot(Rc<std::cell::RefCell<MenuOpener>>);

impl OpenerSlot {
    fn new(opener: MenuOpener) -> Self {
        Self(Rc::new(std::cell::RefCell::new(opener)))
    }

    /// The current opener, with the borrow released before it runs.
    fn get(&self) -> MenuOpener {
        Rc::clone(&self.0.borrow())
    }

    fn set(&self, opener: MenuOpener) {
        let outgoing = self.0.replace(opener);
        drop(outgoing);
    }
}
