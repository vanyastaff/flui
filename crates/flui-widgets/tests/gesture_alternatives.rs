//! Every gesture of a behavioural widget reaches its intent without the
//! gesture (ADR-0216): the input-path matrix rows of `Dismissible` and
//! `SwipeActions`, the
//! menu's pointer triggers on every platform preset, and the rules a type
//! cannot state — labels name one action, reserved keys stay unbound, the
//! controls mirror in a right-to-left locale and the localized labels take
//! their parameters.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Axis, Offset};
use flui_interaction::GestureProfiles;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform_api::TargetPlatform;
use flui_platform_api::keyboard::NamedKey;
use flui_testing::a11y::{Action, Role};
use flui_testing::replay::{MouseScript, PointerScript, TouchScript};
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};
use flui_view::prelude::*;
use flui_view::{BoxedView, StateHandle};
use flui_widgets::{
    Actions, Alternative, CallbackAction, ColoredBox, Column, CompanionGap, CompanionLink,
    CompanionRegistration, CompanionSpec, ControlPlacement, ControlSpec, Directionality,
    DismissDirection, Dismissible, Expanded, GestureAlternatives, GestureAlternativesError,
    GestureCommand, Intent, MenuRequest, MenuSpec, PageController, PageIntent, PageView, RawButton,
    Row, Semantics, SemanticsAnchor, SingleActivator, SizedBox, SwipeAction, SwipeActions,
    SwipeAlternative, SwipeEdge, Text, ViewportFraction,
};

use crate::common::{lay_out, tight};

/// Delete the named message.
#[derive(Clone, Copy, Debug)]
struct Delete(&'static str);
impl Intent for Delete {}

/// Archive the named message.
#[derive(Clone, Copy, Debug)]
struct Archive(&'static str);
impl Intent for Archive {}

/// What the commands change: the messages left, and the archived ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Mailbox {
    inbox: Vec<&'static str>,
    archived: Vec<&'static str>,
}

/// Which single-pointer alternative the rows offer.
#[derive(Clone, Copy, Debug)]
enum Rows {
    /// A Delete control on each row.
    Controls,
    /// A context menu with Delete and Archive on each row.
    Menu,
}

/// The inbox: one `Dismissible` row per message, the mailbox's actions above
/// them, and the menu a row opened shown below them.
#[derive(Clone, StatefulView)]
struct Inbox {
    rows: Rows,
    mailbox: StateHandle<Mailbox>,
    menu: StateHandle<Option<MenuRequest>>,
}

struct InboxState {
    mailbox: StateHandle<Mailbox>,
    menu: StateHandle<Option<MenuRequest>>,
}

impl StatefulView for Inbox {
    type State = InboxState;

    fn create_state(&self) -> Self::State {
        InboxState {
            mailbox: self.mailbox.clone(),
            menu: self.menu.clone(),
        }
    }
}

impl ViewState<Inbox> for InboxState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.mailbox.bind(ctx);
        self.menu.bind(ctx);
    }

    fn build(&self, view: &Inbox, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut children: Vec<BoxedView> = view
            .mailbox
            .with(|mailbox| mailbox.inbox.clone())
            .into_iter()
            .map(|name| {
                Dismissible::new(
                    SizedBox::new(300.0, 50.0)
                        .child(ColoredBox::new(Color::rgb(10, 20, 30)).child(Text::new(name))),
                    row_alternatives(view.rows, name, &view.menu),
                )
                .direction(DismissDirection::Horizontal)
                .boxed()
            })
            .collect();
        if let Some(request) = view.menu.with(Clone::clone) {
            children.extend(menu_buttons(&request, &view.menu));
        }
        mailbox_actions(&view.mailbox, Column::new(children).boxed())
    }
}

impl std::fmt::Debug for Inbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for InboxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InboxState")
    }
}

fn row_alternatives(
    rows: Rows,
    name: &'static str,
    menu: &StateHandle<Option<MenuRequest>>,
) -> GestureAlternatives {
    let delete = GestureCommand::new(format!("Delete {name}"), Delete(name))
        .key(SingleActivator::named(NamedKey::F2));
    let alternative = match rows {
        Rows::Controls => Alternative::VisibleControl(ControlSpec::new(
            delete,
            SizedBox::new(16.0, 16.0).child(Text::new("x")),
        )),
        Rows::Menu => {
            let menu = menu.clone();
            Alternative::ContextMenu(
                MenuSpec::new(delete, move |_cx, request| {
                    menu.update(|open| *open = Some(request));
                })
                .item(GestureCommand::new(
                    format!("Archive {name}"),
                    Archive(name),
                )),
            )
        }
    };
    GestureAlternatives::new(alternative)
        .expect("each row's labels name its message")
        .target(name)
}

/// A fresh inbox with Alice and Bob, the first row's alternatives and a
/// reading of the mailbox.
fn inbox(rows: Rows) -> AlternativesSubject<Mailbox> {
    let mailbox = StateHandle::new(Mailbox {
        inbox: vec!["Alice", "Bob"],
        archived: Vec::new(),
    });
    let menu = StateHandle::new(None);
    let alternatives = row_alternatives(rows, "Alice", &menu);
    let reading = mailbox.clone();
    AlternativesSubject::new(
        Inbox {
            rows,
            mailbox,
            menu,
        },
        &alternatives,
        move || reading.with(Clone::clone),
    )
}

/// Alice's row dragged out to the left.
fn swipe_alice_away() -> PointerScript {
    PointerScript::swipe(Offset::new(300.0, 25.0), Offset::new(20.0, 25.0))
}

/// Matrix row: `Dismissible` with a visible Delete control. Its swipe, the
/// custom action, F2 in the row, and the control pressed with a mouse, a
/// finger and a dwell pointer each delete Alice; Escape deletes nothing.
pub(crate) fn dismissible_with_a_visible_control() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "Dismissible / visible control",
        tight(400.0, 300.0),
        swipe_alice_away(),
        || inbox(Rows::Controls),
    ));
}

/// Matrix row: `Dismissible` with a context menu. Delete and Archive are
/// reached by a secondary click, a long press, the menu keys, assistive
/// technology's `ShowContextMenu` and the control focus reveals, and each
/// item is its own custom action.
pub(crate) fn dismissible_with_a_context_menu() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "Dismissible / context menu",
        tight(400.0, 300.0),
        swipe_alice_away(),
        || inbox(Rows::Menu),
    ));
}

/// Which single-pointer alternative the swipe rows offer.
#[derive(Clone, Copy, Debug)]
enum SwipeRows {
    Controls,
    PresentedMenu,
    PlainMenu,
}

/// Alice's and Bob's messages as `SwipeActions` rows: Delete then Archive at
/// the trailing edge, a full swipe deleting.
#[derive(Clone, StatefulView)]
struct SwipeInbox {
    rows: SwipeRows,
    mailbox: StateHandle<Mailbox>,
    menu: StateHandle<Option<MenuRequest>>,
}

struct SwipeInboxState {
    mailbox: StateHandle<Mailbox>,
    menu: StateHandle<Option<MenuRequest>>,
}

impl StatefulView for SwipeInbox {
    type State = SwipeInboxState;

    fn create_state(&self) -> Self::State {
        SwipeInboxState {
            mailbox: self.mailbox.clone(),
            menu: self.menu.clone(),
        }
    }
}

impl ViewState<SwipeInbox> for SwipeInboxState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.mailbox.bind(ctx);
        self.menu.bind(ctx);
    }

    fn build(&self, view: &SwipeInbox, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut children: Vec<BoxedView> = view
            .mailbox
            .with(|mailbox| mailbox.inbox.clone())
            .into_iter()
            .map(|name| swipe_row(view.rows, name, &view.menu).boxed())
            .collect();
        if let Some(request) = view.menu.with(Clone::clone) {
            children.extend(menu_buttons(&request, &view.menu));
        }
        mailbox_actions(&view.mailbox, Column::new(children).boxed())
    }
}

impl std::fmt::Debug for SwipeInbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwipeInbox")
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SwipeInboxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SwipeInboxState")
    }
}

/// The buttons a presented menu shows for `request`'s items.
fn menu_buttons(request: &MenuRequest, menu: &StateHandle<Option<MenuRequest>>) -> Vec<BoxedView> {
    request
        .items()
        .iter()
        .map(|item| {
            let (item, menu) = (item.clone(), menu.clone());
            RawButton::new(
                Semantics::new()
                    .label(item.label().to_owned())
                    .child(SizedBox::new(120.0, 30.0)),
            )
            .on_press(move |cx| {
                menu.update(|open| *open = None);
                let _ = item.perform(cx);
            })
            .boxed()
        })
        .collect()
}

/// `child` under the mailbox's Delete and Archive actions.
fn mailbox_actions(mailbox: &StateHandle<Mailbox>, child: BoxedView) -> BoxedView {
    let (delete, archive) = (mailbox.clone(), mailbox.clone());
    Actions::new(child)
        .action(CallbackAction::new(move |_cx, intent: &Delete| {
            delete.update(|mailbox| mailbox.inbox.retain(|name| *name != intent.0));
        }))
        .action(CallbackAction::new(move |_cx, intent: &Archive| {
            archive.update(|mailbox| {
                mailbox.inbox.retain(|name| *name != intent.0);
                mailbox.archived.push(intent.0);
            });
        }))
        .boxed()
}

fn swipe_row(
    rows: SwipeRows,
    name: &'static str,
    menu: &StateHandle<Option<MenuRequest>>,
) -> SwipeActions {
    let alternative = match rows {
        SwipeRows::Controls => SwipeAlternative::VisibleControls(ControlPlacement::Trailing),
        SwipeRows::PlainMenu => SwipeAlternative::ContextMenu,
        SwipeRows::PresentedMenu => {
            let menu = menu.clone();
            SwipeAlternative::PresentedMenu(std::rc::Rc::new(move |_cx, request| {
                menu.update(|open| *open = Some(request));
            }))
        }
    };
    SwipeActions::new(
        SizedBox::new(300.0, 50.0)
            .child(ColoredBox::new(Color::rgb(10, 20, 30)).child(Text::new(name))),
        [],
        [
            SwipeAction::new(
                GestureCommand::new(format!("Delete {name}"), Delete(name))
                    .key(SingleActivator::named(NamedKey::F2)),
                Text::new("Delete"),
            ),
            SwipeAction::new(
                GestureCommand::new(format!("Archive {name}"), Archive(name)),
                Text::new("Archive"),
            ),
        ],
        alternative,
    )
    .expect("each row's labels name its message")
    .full_swipe(SwipeEdge::Trailing)
    .target(name)
}

/// A fresh swipe inbox with Alice and Bob, the first row's alternatives and
/// a reading of the mailbox. The plain menu is a popup route, so its inbox is
/// a navigator's page.
fn swipe_inbox(rows: SwipeRows) -> AlternativesSubject<Mailbox> {
    let mailbox = StateHandle::new(Mailbox {
        inbox: vec!["Alice", "Bob"],
        archived: Vec::new(),
    });
    let menu = StateHandle::new(None);
    let alternatives = swipe_row(rows, "Alice", &menu).alternatives().clone();
    let reading = mailbox.clone();
    let inbox = SwipeInbox {
        rows,
        mailbox,
        menu,
    };
    let view = match rows {
        SwipeRows::PlainMenu => {
            let navigator = flui_widgets::NavigatorHandle::new();
            navigator.seed_initial(flui_widgets::PageRoute::<()>::new(move |_, _, _| {
                inbox.clone().boxed()
            }));
            flui_widgets::Navigator::new(navigator).boxed()
        }
        SwipeRows::Controls | SwipeRows::PresentedMenu => inbox.boxed(),
    };
    AlternativesSubject::new(view, &alternatives, move || reading.with(Clone::clone))
}

/// Alice's row swiped all the way toward its reading start: its full swipe.
fn swipe_alice_fully() -> PointerScript {
    PointerScript::swipe(Offset::new(280.0, 25.0), Offset::new(0.0, 25.0))
}

/// Matrix row: `SwipeActions` with visible controls. The full swipe, each
/// action's custom action, F2 in the row and each control pressed with a
/// mouse, a finger and a dwell pointer reach the same mailbox.
pub(crate) fn swipe_actions_with_visible_controls() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "SwipeActions / visible controls",
        tight(400.0, 300.0),
        swipe_alice_fully(),
        || swipe_inbox(SwipeRows::Controls),
    ));
}

/// Matrix row: `SwipeActions` with a design package's menu.
pub(crate) fn swipe_actions_with_a_presented_menu() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "SwipeActions / presented menu",
        tight(400.0, 300.0),
        swipe_alice_fully(),
        || swipe_inbox(SwipeRows::PresentedMenu),
    ));
}

/// Matrix row: `SwipeActions` with the theme-free menu of
/// `ContextMenuArea`, which a swipe row composes from the layer above the
/// menu.
pub(crate) fn swipe_actions_with_the_plain_context_menu() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "SwipeActions / plain context menu",
        tight(400.0, 300.0),
        swipe_alice_fully(),
        || swipe_inbox(SwipeRows::PlainMenu),
    ));
}

/// Matrix row: `InteractiveViewer` with zoom controls. Its double tap
/// performs "Zoom in"; each command is reached by assistive technology, by
/// its key, and by its control with a mouse, a finger and a virtual pointer,
/// with the same scale as a result.
pub(crate) fn interactive_viewer_with_zoom_controls() {
    use flui_widgets::{InteractiveViewer, TransformationController, ZoomIntent};

    let mount = || {
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Zoom in", ZoomIntent::In)
                    .key(SingleActivator::character("i").control()),
                Text::new("+"),
            )
            .control(
                GestureCommand::new("Zoom out", ZoomIntent::Out)
                    .key(SingleActivator::character("o").control()),
                Text::new("-"),
            ),
        ))
        .expect("two distinct labels");
        let transform = TransformationController::new();
        // Zoomed in one step, so both commands change the scale.
        transform.set_value(flui_foundation::geometry::Matrix4::scaling(2.0, 2.0, 1.0));
        let model = transform.clone();
        let view = InteractiveViewer::new(alternatives.clone())
            .controller(transform)
            .min_scale(1.0)
            .max_scale(8.0)
            .child(ColoredBox::new(Color::rgb(10, 20, 30)));
        AlternativesSubject::new(view, &alternatives, move || {
            let m = model.value().to_col_major_array();
            // The scale, in thousandths: every path zooms about some point,
            // and the scale is what the command changes.
            (m[0].hypot(m[1]) * 1000.0).round() as i64
        })
    };
    let double_tap = TouchScript::new("double tap")
        .down(1, Offset::new(100.0, 150.0))
        .wait(Duration::from_millis(30))
        .up(1)
        .wait(Duration::from_millis(60))
        .down(2, Offset::new(100.0, 150.0))
        .wait(Duration::from_millis(30))
        .up(2)
        .build()
        .expect("two whole taps");
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "InteractiveViewer / zoom controls",
        tight(400.0, 300.0),
        double_tap,
        mount,
    ));
}

/// Matrix row: `InteractiveViewer` with pan controls. A drag of one step
/// performs "Pan right"; each direction is reached by assistive technology,
/// by its key and by its control with each pointer, to the same position.
pub(crate) fn interactive_viewer_with_pan_controls() {
    use flui_widgets::{InteractiveViewer, PanIntent, TransformationController};

    let mount = || {
        let command = |label: &str, intent: PanIntent, key: &str| {
            GestureCommand::new(label, intent).key(SingleActivator::character(key).control())
        };
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(command("Pan right", PanIntent::Right, "l"), Text::new(">"))
                .control(command("Pan left", PanIntent::Left, "h"), Text::new("<"))
                .control(command("Pan down", PanIntent::Down, "j"), Text::new("v"))
                .control(command("Pan up", PanIntent::Up, "k"), Text::new("^")),
        ))
        .expect("four distinct labels");
        let transform = TransformationController::new();
        let model = transform.clone();
        let view = InteractiveViewer::new(alternatives.clone())
            .controller(transform)
            .double_tap_zoom(false)
            .boundary_margin(flui_foundation::geometry::EdgeInsets::all(200.0))
            .child(ColoredBox::new(Color::rgb(10, 20, 30)));
        AlternativesSubject::new(view, &alternatives, move || {
            let (x, y, _) = model.value().translation_component();
            ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
        })
    };
    // A slow drag of exactly one step (53 logical pixels) to the left shows
    // what lies to the right.
    let mut drag = MouseScript::new("one step").press(Offset::new(120.0, 150.0));
    for step in 1..=10 {
        drag = drag
            .wait(Duration::from_millis(16))
            .move_to(Offset::new(120.0 - 5.3 * f64::from(step), 150.0));
    }
    let drag = drag
        .wait(Duration::from_millis(300))
        .release()
        .build()
        .expect("one whole press");
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "InteractiveViewer / pan controls",
        tight(500.0, 300.0),
        drag,
        mount,
    ));
}

/// Matrix row: `PageView` with page controls. A swipe toward the reading
/// start turns back a page, as "Previous page" does; each command is reached
/// by assistive technology, by its key and by its control with each pointer,
/// to the same page.
pub(crate) fn page_view_with_page_controls() {
    let mount = || {
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Previous page", PageIntent::Previous)
                    .key(SingleActivator::character("[").control()),
                Text::new("<"),
            )
            .control(
                GestureCommand::new("Next page", PageIntent::Next)
                    .key(SingleActivator::character("]").control()),
                Text::new(">"),
            ),
        ))
        .expect("two distinct labels");
        // On the middle page, so both commands turn one.
        let controller = PageController::with_params(1, ViewportFraction::FULL);
        let model = controller.clone();
        let pages: Vec<BoxedView> = (0..3_u8)
            .map(|index| ColoredBox::new(Color::rgb(40 * index, 20, 30)).boxed())
            .collect();
        let view = PageView::new(pages, alternatives.clone()).controller(controller);
        AlternativesSubject::new(view, &alternatives, move || {
            model.page().map(|page| (page * 1000.0).round() as i64)
        })
    };
    let mut swipe = MouseScript::new("swipe back").press(Offset::new(60.0, 150.0));
    for step in 1..=10 {
        swipe = swipe
            .wait(Duration::from_millis(16))
            .move_to(Offset::new(60.0 + 30.0 * f64::from(step), 150.0));
    }
    let swipe = swipe
        .wait(Duration::from_millis(300))
        .release()
        .build()
        .expect("one whole press");
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "PageView / page controls",
        tight(500.0, 300.0),
        swipe,
        mount,
    ));
}

/// Matrix row: an `AnchoredDrag` sheet with expand and collapse controls. A
/// drag up from its middle anchor expands it, as "Expand sheet" does; each
/// command is reached by assistive technology, by its key and by its control
/// with each pointer, to the same anchor.
pub(crate) fn anchored_drag_with_anchor_controls() {
    use flui_widgets::{AnchorIntent, AnchoredDrag, AnchoredDragController, DraggableAnchors};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Stop {
        Full,
        Half,
        Peek,
    }

    let mount = || {
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Expand sheet", AnchorIntent::Previous)
                    .key(SingleActivator::character("[").control()),
                Text::new("^"),
            )
            .control(
                GestureCommand::new("Collapse sheet", AnchorIntent::Next)
                    .key(SingleActivator::character("]").control()),
                Text::new("v"),
            ),
        ))
        .expect("two distinct labels");
        let anchors = DraggableAnchors::builder()
            .at(Stop::Full, 0.0)
            .at(Stop::Half, 100.0)
            .at(Stop::Peek, 200.0)
            .build()
            .expect("three distinct anchors");
        // At the middle anchor, so both commands move it.
        let controller = AnchoredDragController::new(anchors, Stop::Half).expect("a known anchor");
        let model = controller.clone();
        let view = AnchoredDrag::new(
            controller,
            Axis::Vertical,
            ColoredBox::new(Color::rgb(10, 20, 30)),
            alternatives.clone(),
        )
        .semantics("Sheet", |stop: Stop| format!("{stop:?}"));
        AlternativesSubject::new(view, &alternatives, move || model.current())
    };
    let mut drag = MouseScript::new("drag up").press(Offset::new(120.0, 200.0));
    for step in 1..=10 {
        drag = drag
            .wait(Duration::from_millis(16))
            .move_to(Offset::new(120.0, 200.0 - 10.0 * f64::from(step)));
    }
    let drag = drag
        .wait(Duration::from_millis(300))
        .release()
        .build()
        .expect("one whole press");
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "AnchoredDrag / anchor controls",
        tight(400.0, 400.0),
        drag,
        mount,
    ));
}

/// Every platform's preset opens the menu with its own pointer trigger: a
/// secondary click on the desktop, a long press under the preset's timeout
/// on touch platforms.
pub(crate) fn every_preset_opens_the_menu_by_its_pointer_trigger() {
    for platform in [
        TargetPlatform::Android,
        TargetPlatform::iOS,
        TargetPlatform::Fuchsia,
        TargetPlatform::Linux,
        TargetPlatform::MacOS,
        TargetPlatform::Windows,
    ] {
        let subject_menu = StateHandle::new(None);
        let mailbox = StateHandle::new(Mailbox {
            inbox: vec!["Alice"],
            archived: Vec::new(),
        });
        let view = crate::scroll::FlingProfile {
            provider: flui_interaction::gesture::GestureSettingsSource::new(
                GestureProfiles::preset(platform),
            )
            .provider(),
            child: Inbox {
                rows: Rows::Menu,
                mailbox,
                menu: subject_menu.clone(),
            }
            .boxed(),
        };
        let mut laid = lay_out(view, tight(400.0, 300.0));
        let row = Offset::new(200.0, 25.0);
        let desktop = matches!(
            platform,
            TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Windows
        );
        if desktop {
            // A mouse has the secondary click: holding its primary button
            // opens nothing, so a mouse drag of the row meets no long press.
            let hold = MouseScript::new("mouse hold")
                .press(row)
                .wait(Duration::from_secs(1))
                .release()
                .build()
                .expect("the hold releases its button");
            laid.replay(&hold);
            laid.pump_for(Duration::from_millis(16));
            assert!(
                subject_menu.with(Option::is_none),
                "{platform:?}: a mouse hold opens no menu"
            );
        }
        let trigger = if desktop {
            MouseScript::new("secondary click")
                .press_button(row, flui_platform_api::pointer::PointerButton::SECONDARY)
                .wait(Duration::from_millis(16))
                .release()
                .build()
        } else {
            let hold = GestureProfiles::preset(platform)
                .for_kind(flui_interaction::PointerKind::Touch)
                .long_press_timeout()
                + Duration::from_millis(50);
            TouchScript::new("long press")
                .down(1, row)
                .wait(hold)
                .up(1)
                .build()
        }
        .expect("the trigger ends its contact");
        laid.replay(&trigger);
        laid.pump_for(Duration::from_millis(16));
        let opened = subject_menu.with(|menu| {
            menu.as_ref().map(|request: &MenuRequest| {
                request
                    .items()
                    .iter()
                    .map(|item| item.label().to_owned())
                    .collect::<Vec<_>>()
            })
        });
        assert_eq!(
            opened,
            Some(vec!["Delete Alice".to_owned(), "Archive Alice".to_owned()]),
            "{platform:?}: its pointer trigger opens Alice's menu"
        );
    }
}

/// Within one row a label names one action, so two commands may not share
/// one, and an empty label names none. A key performs one command too.
pub(crate) fn labels_name_one_action_each() {
    let keyed = |label: &str| {
        GestureCommand::new(label.to_owned(), Delete("Alice"))
            .key(SingleActivator::character("d").control())
    };
    let ctrl_d = SingleActivator::character("d").control();
    // A later key one press of `ctrl_d` also matches.
    let overlapping = [
        ("the same key", ctrl_d.clone()),
        ("the other case", SingleActivator::character("D").control()),
        (
            "Shift ignored",
            SingleActivator::character_ignoring_shift("d").control(),
        ),
        ("repeats refused", ctrl_d.allow_repeats(false)),
    ];
    for (label, key) in overlapping {
        let shared_key = GestureAlternatives::new(Alternative::ContextMenu(
            MenuSpec::new(keyed("Delete"), |_cx, _request| {})
                .item(GestureCommand::new("Archive", Delete("Alice")).key(key.clone())),
        ));
        assert_eq!(
            shared_key.err(),
            Some(GestureAlternativesError::DuplicateKey { key }),
            "{label}"
        );
    }
    let distinct = GestureAlternatives::new(Alternative::ContextMenu(
        MenuSpec::new(keyed("Delete"), |_cx, _request| {}).item(
            GestureCommand::new("Archive", Delete("Alice"))
                .key(SingleActivator::character("d").control().shift()),
        ),
    ));
    assert!(distinct.is_ok(), "Ctrl+Shift+D is another key press");
    let repeated_on_one = GestureAlternatives::new(Alternative::ContextMenu(MenuSpec::new(
        keyed("Delete").key(SingleActivator::character("d").control()),
        |_cx, _request| {},
    )));
    assert!(
        repeated_on_one.is_ok(),
        "a key one command lists twice performs only it"
    );

    let control = |label: &str| GestureCommand::new(label.to_owned(), Delete("Alice"));
    let duplicate = GestureAlternatives::new(Alternative::VisibleControl(
        ControlSpec::new(control("Delete"), SizedBox::shrink())
            .control(control("Delete"), SizedBox::shrink()),
    ));
    assert_eq!(
        duplicate.err(),
        Some(GestureAlternativesError::DuplicateLabel {
            label: "Delete".to_owned()
        })
    );
    let menu = GestureAlternatives::new(Alternative::ContextMenu(
        MenuSpec::new(control("Delete"), |_cx, _request| {}).item(control("Delete")),
    ));
    assert!(matches!(
        menu,
        Err(GestureAlternativesError::DuplicateLabel { .. })
    ));
    let empty = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        control(""),
        SizedBox::shrink(),
    )));
    assert_eq!(empty.err(), Some(GestureAlternativesError::EmptyLabel));
}

/// A command's Escape, Tab or typing key is not bound: Escape in the row
/// still deletes nothing, while the command's other key still works.
pub(crate) fn reserved_keys_are_not_bound() {
    let mailbox = StateHandle::new(Mailbox {
        inbox: vec!["Alice"],
        archived: Vec::new(),
    });
    let command = GestureCommand::new("Delete Alice", Delete("Alice"))
        .key(SingleActivator::named(NamedKey::Escape))
        .key(SingleActivator::character("x"))
        .key(SingleActivator::named(NamedKey::F2));
    let bound: Vec<_> = command.keys(TargetPlatform::current()).cloned().collect();
    assert_eq!(
        bound,
        [SingleActivator::named(NamedKey::F2)],
        "only the unreserved key is bound"
    );
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        command,
        SizedBox::new(16.0, 16.0),
    )))
    .expect("one labelled command");
    let reading = mailbox;
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "Dismissible / a command with reserved keys",
        tight(400.0, 300.0),
        swipe_alice_away(),
        move || {
            let mailbox = StateHandle::new(reading.with(Clone::clone));
            let read = mailbox.clone();
            AlternativesSubject::new(
                single_row(mailbox, alternatives.clone()),
                &alternatives,
                move || read.with(Clone::clone),
            )
        },
    ));
}

/// Alice's row alone, with `alternatives`, under the mailbox's actions.
fn single_row(mailbox: StateHandle<Mailbox>, alternatives: GestureAlternatives) -> impl View {
    let delete = mailbox.clone();
    MailboxRoot {
        mailbox,
        child: Actions::new(
            Dismissible::new(
                SizedBox::new(300.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
                alternatives,
            )
            .boxed(),
        )
        .action(CallbackAction::new(move |_cx, intent: &Delete| {
            delete.update(|mailbox| mailbox.inbox.retain(|name| *name != intent.0));
        }))
        .boxed(),
    }
}

/// Rebuilds `child` when `mailbox` changes, so a deleted row goes away.
#[derive(Clone, StatefulView)]
struct MailboxRoot {
    mailbox: StateHandle<Mailbox>,
    child: BoxedView,
}

struct MailboxRootState {
    mailbox: StateHandle<Mailbox>,
}

impl StatefulView for MailboxRoot {
    type State = MailboxRootState;

    fn create_state(&self) -> Self::State {
        MailboxRootState {
            mailbox: self.mailbox.clone(),
        }
    }
}

impl ViewState<MailboxRoot> for MailboxRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.mailbox.bind(ctx);
    }

    fn build(&self, view: &MailboxRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        if view.mailbox.with(|mailbox| mailbox.inbox.is_empty()) {
            SizedBox::shrink().boxed()
        } else {
            view.child.clone()
        }
    }
}

impl std::fmt::Debug for MailboxRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailboxRoot")
    }
}

impl std::fmt::Debug for MailboxRootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailboxRootState")
    }
}

/// A trailing control sits after the content in reading order: right of it
/// left-to-right, left of it right-to-left; a leading one the other way.
pub(crate) fn controls_follow_the_reading_direction() {
    for (direction, placement, control_first) in [
        (TextDirection::Ltr, ControlPlacement::Trailing, false),
        (TextDirection::Rtl, ControlPlacement::Trailing, true),
        (TextDirection::Ltr, ControlPlacement::Leading, true),
        (TextDirection::Rtl, ControlPlacement::Leading, false),
    ] {
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Delete Alice", Delete("Alice")),
                SizedBox::new(16.0, 16.0),
            )
            .placement(placement),
        ))
        .expect("one labelled command");
        let mut laid = lay_out(
            Directionality::new(
                direction,
                alternatives.child(
                    Semantics::new()
                        .container(true)
                        .label("Alice")
                        .child(SizedBox::new(300.0, 50.0)),
                ),
            ),
            tight(400.0, 300.0),
        );
        laid.enable_semantics();
        laid.pump();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let control = tree
            .find_by_role_and_label(Role::Button, "Delete Alice")
            .expect("the control is published");
        let content = tree
            .find_by_label("Alice")
            .expect("the content is published");
        let control_x = tree.window_bounds(&control).expect("control bounds").x0;
        let content_x = tree.window_bounds(&content).expect("content bounds").x0;
        assert_eq!(
            control_x < content_x,
            control_first,
            "{direction:?} / {placement:?}: control at {control_x}, content at {content_x}"
        );
    }
}

/// The control focus reveals names the row's target ("Actions for Alice"),
/// and a generic label without one; it offers a click and appears only
/// with keyboard or assistive-technology focus in the row.
pub(crate) fn the_revealed_menu_control_names_its_target() {
    let rows = [
        (Some("Alice"), "Actions for Alice", false),
        (None, "More actions", false),
        (Some("Alice"), "Actions for Alice", true),
    ];
    for (target, label, localized) in rows {
        let alternatives = GestureAlternatives::new(Alternative::ContextMenu(MenuSpec::new(
            GestureCommand::new("Delete Alice", Delete("Alice")),
            |_cx, _request| {},
        )))
        .expect("one labelled command");
        let alternatives = match target {
            Some(target) => alternatives.target(target),
            None => alternatives,
        };
        let row = alternatives.child(SizedBox::new(300.0, 50.0));
        // With a `Localizations` ancestor the label comes from its widgets
        // resource; without one, from the built-in English strings.
        let root = if localized {
            flui_widgets::Localizations::new(
                flui_platform_api::Locale::new("en", None::<&str>).expect("valid locale"),
                vec![flui_widgets::BoxedLocalizationsDelegate::new(
                    flui_widgets::DefaultWidgetsLocalizationsDelegate,
                )],
                row,
            )
            .boxed()
        } else {
            row.boxed()
        };
        let mut laid = lay_out(root, tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        let hidden = laid.a11y_tree().expect("semantics enabled");
        assert!(
            hidden.find_by_role_and_label(Role::Button, label).is_err(),
            "{label}: hidden until focus is in the row"
        );
        let manager = laid.focus_manager();
        let tab = flui_platform_api::keyboard::KeyEvent::new(
            flui_platform_api::keyboard::KeyState::Down,
            flui_platform_api::keyboard::Key::Named(NamedKey::Tab),
            flui_platform_api::keyboard::Code::Unidentified,
            flui_platform_api::EventTime::from_nanos(0),
        );
        let _ = manager.dispatch_key_event(&tab);
        laid.pump();
        let shown = laid.a11y_tree().expect("semantics enabled");
        let control = shown
            .find_by_role_and_label(Role::Button, label)
            .unwrap_or_else(|error| panic!("{label}: revealed by keyboard focus: {error}"));
        assert!(
            control.supports_action(Action::Click),
            "{label}: it offers a click"
        );
    }
}

/// The revealed menu control paints its glyph on a surface of its own, so
/// the row behind it cannot hide it: under an app's white text (a dark row),
/// black dots on the glyph's white, not dots in a colour the row chose.
pub(crate) fn the_revealed_menu_control_draws_its_glyph_on_its_own_surface() {
    use flui_painting::display_list::DrawOp;
    use flui_painting::typography::TextStyle;
    use flui_widgets::DefaultTextStyle;

    let alternatives = GestureAlternatives::new(Alternative::ContextMenu(MenuSpec::new(
        GestureCommand::new("Delete Alice", Delete("Alice")),
        |_cx, _request| {},
    )))
    .expect("one labelled command");
    let mut laid = lay_out(
        DefaultTextStyle::new(
            TextStyle::default().with_color(Color::WHITE),
            alternatives.child(SizedBox::new(300.0, 50.0)),
        ),
        tight(400.0, 300.0),
    );
    laid.pump();
    let tab = flui_platform_api::keyboard::KeyEvent::new(
        flui_platform_api::keyboard::KeyState::Down,
        flui_platform_api::keyboard::Key::Named(NamedKey::Tab),
        flui_platform_api::keyboard::Code::Unidentified,
        flui_platform_api::EventTime::from_nanos(0),
    );
    let _ = laid.focus_manager().dispatch_key_event(&tab);
    laid.pump();
    // Each filled rectangle, in paint order: its size and colour.
    let rects: Vec<((f64, f64), Color)> = laid
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { rect, paint } => Some(((rect.width(), rect.height()), paint.color)),
            _ => None,
        })
        .collect();
    let first_dot = rects
        .iter()
        .position(|(size, _)| *size == (3.0, 3.0))
        .expect("the glyph's dots are painted");
    let dots: Vec<Color> = rects[first_dot..]
        .iter()
        .filter(|(size, _)| *size == (3.0, 3.0))
        .map(|(_, colour)| *colour)
        .collect();
    assert_eq!(dots, [Color::BLACK; 3], "the glyph's three dots");
    // Three 3 px dots 1.5 px apart on each side, inset 4 px.
    assert!(
        rects[..first_dot].contains(&((26.0, 11.0), Color::WHITE)),
        "the dots sit on the glyph's own white surface: {rects:?}"
    );
}

/// A command whose intent nothing answers, and a command key the platform
/// reserves, each leave a `flui::a11y` warning naming the command: a gesture
/// that silently does nothing is the defect the diagnostic exists for. The
/// reserved key is reported once, not again on each rebuild.
pub(crate) fn an_unanswered_intent_and_a_reserved_key_are_reported() {
    let row = || {
        GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
            GestureCommand::new("Delete Alice", Delete("Alice"))
                .key(SingleActivator::named(NamedKey::Escape)),
            SizedBox::new(16.0, 16.0),
        )))
        .expect("one labelled command")
        .child(SizedBox::new(300.0, 50.0))
    };
    let ((), log) = flui_testing::log_capture::capture(|| {
        let mut laid = lay_out(row(), tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let control = tree
            .find_by_role_and_label(Role::Button, "Delete Alice")
            .expect("the control is published")
            .id();
        laid.invoke_semantics_action(flui_testing::a11y::ActionRequest {
            action: Action::Click,
            target_tree: flui_testing::a11y::TreeId::ROOT,
            target_node: control,
            data: None,
        })
        .expect("the control takes a click");
        laid.pump();
        laid.pump_widget(row());
        laid.pump_widget(row());
    });
    let warned = |text: &str| {
        log.records()
            .iter()
            .filter(|record| {
                record.target == "flui::a11y"
                    && record.message.contains(text)
                    && record.field("command") == Some("Delete Alice")
            })
            .count()
    };
    assert!(
        warned("has no action") > 0,
        "the unanswered intent is reported:\n{log}"
    );
    assert_eq!(
        warned("reserved"),
        1,
        "the reserved key is reported once:\n{log}"
    );
}

// ============================================================================
// Companions
// ============================================================================

/// Previous and Next buttons beside a pager, outside it: the companion of its
/// page commands. Registered while mounted.
#[derive(Clone, StatefulView)]
struct PageButtons {
    link: CompanionLink<PageIntent>,
    pager: PageController,
}

struct PageButtonsState {
    previous: SemanticsAnchor,
    next: SemanticsAnchor,
    _registration: CompanionRegistration,
}

impl StatefulView for PageButtons {
    type State = PageButtonsState;

    fn create_state(&self) -> Self::State {
        let (previous, next) = (SemanticsAnchor::new(), SemanticsAnchor::new());
        let (named_previous, named_next) = (previous.clone(), next.clone());
        PageButtonsState {
            _registration: self.link.register(move |intent| {
                Some(match intent {
                    PageIntent::Previous => named_previous.clone(),
                    PageIntent::Next => named_next.clone(),
                })
            }),
            previous,
            next,
        }
    }
}

impl ViewState<PageButtons> for PageButtonsState {
    fn build(&self, view: &PageButtons, _ctx: &dyn BuildContext) -> impl IntoView {
        let button = |anchor: &SemanticsAnchor, label: &str, forward: bool| {
            let pager = view.pager.clone();
            RawButton::new(
                Semantics::new()
                    .label(label.to_owned())
                    .anchor(anchor.clone())
                    .child(SizedBox::new(48.0, 48.0)),
            )
            .on_press(move |_cx| {
                let curve = flui_animation::ArcCurve::new(flui_animation::curves::EASE_IN_OUT);
                if forward {
                    pager.next_page(Duration::from_millis(300), curve);
                } else {
                    pager.previous_page(Duration::from_millis(300), curve);
                }
            })
            .boxed()
        };
        Row::new(vec![
            button(&self.previous, "Show the page before", false),
            button(&self.next, "Show the page after", true),
        ])
    }
}

impl std::fmt::Debug for PageButtons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PageButtons")
    }
}

impl std::fmt::Debug for PageButtonsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PageButtonsState")
    }
}

/// Page commands whose single-pointer path is the buttons of `link`'s
/// companion.
fn companion_page_alternatives(link: &CompanionLink<PageIntent>) -> GestureAlternatives {
    GestureAlternatives::new(Alternative::Companion(
        CompanionSpec::new(
            GestureCommand::new("Previous page", PageIntent::Previous)
                .key(SingleActivator::character("[").control()),
            link,
        )
        .command(
            GestureCommand::new("Next page", PageIntent::Next)
                .key(SingleActivator::character("]").control()),
        ),
    ))
    .expect("two distinct labels, both carried by the link")
}

/// The pager over three pages, with the buttons above or below it when
/// `buttons`.
fn buttons_over_pager(
    link: &CompanionLink<PageIntent>,
    controller: &PageController,
    buttons: bool,
    buttons_first: bool,
) -> impl View {
    let pages: Vec<BoxedView> = (0..3_u8)
        .map(|index| ColoredBox::new(Color::rgb(40 * index, 20, 30)).boxed())
        .collect();
    let pager =
        PageView::new(pages, companion_page_alternatives(link)).controller(controller.clone());
    let mut column = vec![Expanded::new(pager).boxed()];
    if buttons {
        let row = PageButtons {
            link: link.clone(),
            pager: controller.clone(),
        }
        .boxed();
        if buttons_first {
            column.insert(0, row);
        } else {
            column.push(row);
        }
    }
    Column::new(column)
}

/// Matrix row: a `PageView` whose Previous and Next buttons sit beside it,
/// outside it. A swipe toward the reading start turns back a page, as
/// "Previous page" does; each command is reached by assistive technology, by
/// its key and by the button that performs it with each pointer, to the same
/// page.
pub(crate) fn page_view_with_buttons_beside_it() {
    let mount = || {
        let link = CompanionLink::<PageIntent>::new();
        // On the middle page, so both commands turn one.
        let controller = PageController::with_params(1, ViewportFraction::FULL);
        let model = controller.clone();
        AlternativesSubject::new(
            buttons_over_pager(&link, &controller, true, true),
            &companion_page_alternatives(&link),
            move || model.page().map(|page| (page * 1000.0).round() as i64),
        )
    };
    let mut swipe = MouseScript::new("swipe back").press(Offset::new(60.0, 200.0));
    for step in 1..=10 {
        swipe = swipe
            .wait(Duration::from_millis(16))
            .move_to(Offset::new(60.0 + 30.0 * f64::from(step), 200.0));
    }
    let swipe = swipe
        .wait(Duration::from_millis(300))
        .release()
        .build()
        .expect("one whole press");
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "PageView / buttons beside it",
        tight(500.0, 300.0),
        swipe,
        mount,
    ));
}

/// A link names the node of the first companion that has one, forgets a
/// companion when its registration drops, and stays usable while it is read:
/// a resolver, or the captures of a registration being dropped, may register,
/// leave and read the link.
pub(crate) fn a_companion_link_follows_its_registrations() {
    let link = CompanionLink::<PageIntent>::new();
    let alternatives = companion_page_alternatives(&link);
    let Alternative::Companion(spec) = alternatives.alternative().clone() else {
        panic!("the alternatives rely on a companion");
    };
    let command = |label: &str| {
        alternatives
            .commands()
            .find(|command| command.label() == label)
            .expect("the command")
            .clone()
    };
    let (next, previous) = (command("Next page"), command("Previous page"));
    assert_eq!(spec.node_for(&next), Err(CompanionGap::Unregistered));
    assert!(!link.is_fulfilled());

    let (first, second) = (SemanticsAnchor::new(), SemanticsAnchor::new());
    let named = first.clone();
    let earlier = link.register(move |intent| match intent {
        PageIntent::Previous => Some(named.clone()),
        PageIntent::Next => None,
    });
    let named = second.clone();
    let later = link.register(move |_| Some(named.clone()));
    assert_eq!(spec.node_for(&previous), Ok(first), "the first to name one");
    assert_eq!(spec.node_for(&next), Ok(second.clone()), "or the next");

    drop(earlier);
    assert_eq!(spec.node_for(&previous), Ok(second));
    drop(later);
    assert_eq!(spec.node_for(&next), Err(CompanionGap::Unregistered));
    assert_eq!(link, link.clone(), "a clone is the same link");
    assert_ne!(link, CompanionLink::new(), "another link is another");

    // A resolver that drops another registration and registers a third while
    // the link is being read.
    let doomed: Rc<RefCell<Option<CompanionRegistration>>> =
        Rc::new(RefCell::new(Some(link.register(|_| None))));
    let joined: Rc<RefCell<Vec<CompanionRegistration>>> = Rc::default();
    let (leaving, collected, reader) = (Rc::clone(&doomed), Rc::clone(&joined), link.clone());
    let busy = link.register(move |_| {
        drop(leaving.borrow_mut().take());
        collected
            .borrow_mut()
            .push(reader.register(|_| Some(SemanticsAnchor::new())));
        None
    });
    assert_eq!(
        spec.node_for(&next),
        Err(CompanionGap::Unavailable),
        "a read answers for the companions registered when it began"
    );
    assert!(doomed.borrow().is_none(), "and the one that left is gone");
    assert!(
        spec.node_for(&next).is_ok(),
        "the next read finds the one registered during the first"
    );
    drop((busy, joined.take()));
    assert!(!link.is_fulfilled());

    // Captures that read and register on the link as they drop.
    struct TouchesLink(CompanionLink<PageIntent>);
    impl Drop for TouchesLink {
        fn drop(&mut self) {
            let _ = self.0.is_fulfilled();
            drop(self.0.register(|_| None));
        }
    }
    let probe = TouchesLink(link.clone());
    let registration = link.register(move |_| {
        let _ = &probe;
        None
    });
    drop(registration);
    assert!(
        !link.is_fulfilled(),
        "a registration's captures may use the link"
    );
}

/// A companion command must perform an intent its link carries.
pub(crate) fn a_companion_command_must_perform_an_intent_its_link_carries() {
    use flui_widgets::AnchorIntent;

    let link = CompanionLink::<PageIntent>::new();
    let foreign = GestureAlternatives::new(Alternative::Companion(
        CompanionSpec::new(GestureCommand::new("Next page", PageIntent::Next), &link)
            .command(GestureCommand::new("Collapse sheet", AnchorIntent::Next)),
    ));
    assert_eq!(
        foreign.err(),
        Some(GestureAlternativesError::ForeignIntent {
            label: "Collapse sheet".to_owned()
        })
    );
}

/// Alternatives that rely on a companion say so once the frame they mounted
/// in has run, unless a companion registered by then, in whichever order the
/// two mounted.
pub(crate) fn unfulfilled_companion_alternatives_are_reported() {
    let warnings = |buttons: bool, buttons_first: bool| {
        let link = CompanionLink::<PageIntent>::new();
        let controller = PageController::new();
        let ((), log) = flui_testing::log_capture::capture(|| {
            let mut laid = lay_out(
                buttons_over_pager(&link, &controller, buttons, buttons_first),
                tight(500.0, 300.0),
            );
            laid.pump();
        });
        log.records()
            .iter()
            .filter(|record| {
                record.target == "flui::a11y" && record.message.contains("none is registered")
            })
            .count()
    };
    assert_eq!(warnings(false, true), 1, "no companion at all");
    assert_eq!(warnings(true, true), 0, "a companion mounted before");
    assert_eq!(warnings(true, false), 0, "a companion mounted after");
}
