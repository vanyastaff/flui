//! Pull to refresh as a participant of the nested scroll channel
//! (ADR-0209): rows of the `nested_scroll_channel` table in
//! `contracts.rs`, each driven by touch and mouse scripts where a contact
//! pulls, and its input-path matrix row in `gesture_alternatives`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::{Axis, Offset};
use flui_interaction::{GestureSettingsSource, RemainderHandoff};
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::{Code, KeyEvent, KeyState, NamedKey};
use flui_testing::a11y::{Action, ActionData, ActionRequest, TreeId};
use flui_testing::{
    AlternativesSubject, GestureAlternativesCheck, MouseScript, PointerScript, TouchScript,
    TrackpadScript, assert_gesture_alternatives,
};
use flui_view::prelude::*;
use flui_widgets::{
    Alternative, ControlSpec, GestureAlternatives, RefreshController, RefreshIndicator,
    RefreshIntent, ScrollController, Scrollable, SingleActivator, SizedBox, VsyncScope,
};

use crate::common::{LaidOut, tight};
use crate::nested_scroll::{
    DEVICES, Device, at, drag, fling, line, mounted, remainder_preset, settle, slow,
};

const FRAME: Duration = Duration::from_millis(16);
const LABEL: &str = "Refresh inbox";

// ---------------------------------------------------------------------------
// A refresh the test completes by hand
// ---------------------------------------------------------------------------

/// What the refreshes did: how many started, which completed, which were
/// dropped unfinished, and the gate that completes them.
#[derive(Default)]
struct Refreshes {
    started: Cell<u32>,
    done: Cell<bool>,
    dropped: Cell<u32>,
    waker: RefCell<Option<Waker>>,
}

impl Refreshes {
    fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Complete every refresh that runs: wake the driver, as an IO
    /// completion would between frames.
    fn complete(&self) {
        self.done.set(true);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// One refresh: pending until the gate opens; counts a drop before then.
struct Refresh {
    refreshes: Rc<Refreshes>,
    finished: bool,
}

impl Future for Refresh {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.refreshes.done.get() {
            self.finished = true;
            return Poll::Ready(());
        }
        *self.refreshes.waker.borrow_mut() = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for Refresh {
    fn drop(&mut self) {
        if !self.finished {
            self.refreshes.dropped.set(self.refreshes.dropped.get() + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

fn alternatives() -> GestureAlternatives {
    GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        RefreshIntent::command(LABEL),
        SizedBox::new(24.0, 24.0),
    )))
    .expect("one labelled command")
}

/// Content that counts its builds.
#[derive(Clone, StatelessView)]
struct Counted {
    builds: Rc<Cell<u32>>,
    axis: Axis,
}

impl std::fmt::Debug for Counted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Counted")
    }
}

impl StatelessView for Counted {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        match self.axis {
            Axis::Vertical => SizedBox::new(200.0, 1000.0),
            Axis::Horizontal => SizedBox::new(1000.0, 200.0),
        }
    }
}

/// A list under a refresh indicator, its refreshes and the content's
/// builds.
struct Inbox {
    list: ScrollController,
    refresh: RefreshController,
    refreshes: Rc<Refreshes>,
    builds: Rc<Cell<u32>>,
}

impl Inbox {
    fn new() -> Self {
        Self {
            list: ScrollController::new(),
            refresh: RefreshController::new(),
            refreshes: Refreshes::new(),
            builds: Rc::new(Cell::new(0)),
        }
    }

    fn view(&self, axis: Axis) -> RefreshIndicator {
        let refreshes = Rc::clone(&self.refreshes);
        RefreshIndicator::new(
            Scrollable::new()
                .controller(self.list.clone())
                .scroll_direction(axis)
                .child(Counted {
                    builds: Rc::clone(&self.builds),
                    axis,
                }),
            alternatives(),
        )
        .axis(axis)
        .controller(self.refresh.clone())
        .on_refresh(move |_cx| {
            refreshes.started.set(refreshes.started.get() + 1);
            Refresh {
                refreshes: Rc::clone(&refreshes),
                finished: false,
            }
        })
    }

    fn mount(&self, axis: Axis) -> LaidOut {
        let vsync = Vsync::new();
        mounted(
            VsyncScope::new(vsync.clone(), self.view(axis)).boxed(),
            vsync,
        )
    }

    /// Mounted under a preset that hands a latched leaf's remainder on at
    /// its edge within the sequence.
    fn mount_handing_off_at_the_edge(&self) -> LaidOut {
        let vsync = Vsync::new();
        let presets = remainder_preset(RemainderHandoff::AtEdgeWithinSequence);
        let tree = crate::scroll::FlingProfile {
            provider: GestureSettingsSource::new(presets).provider(),
            child: self.view(Axis::Vertical).boxed(),
        };
        mounted(VsyncScope::new(vsync.clone(), tree).boxed(), vsync)
    }

    fn started(&self) -> u32 {
        self.refreshes.started.get()
    }
}

/// `script` replayed, with the largest pull seen after any step.
fn replay_watching(laid: &mut LaidOut, inbox: &Inbox, script: &PointerScript) -> f64 {
    let most = Cell::new(0.0_f64);
    laid.replay_with(script, |_, _| {
        most.set(most.get().max(inbox.refresh.pull_distance_px()));
    });
    most.get()
}

/// A slow pull down from near the top by `distance`.
fn pull(device: Device, distance: f64) -> PointerScript {
    slow(device, at(100.0, 40.0), at(100.0, 40.0 + distance))
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// A drag past the threshold at the top refreshes exactly once and leaves
/// the list where it was; only the overlay rebuilds while it pulls, and the
/// pull settles back after the release.
pub(crate) fn pull_past_threshold_refreshes_once() {
    for device in DEVICES {
        let inbox = Inbox::new();
        let mut laid = inbox.mount(Axis::Vertical);
        // A first, short pull: dragging the list gives it the focus, which
        // rebuilds it once.
        laid.replay(&pull(device, 40.0));
        settle(&mut laid);
        assert_eq!(
            inbox.started(),
            0,
            "{device:?}: a short pull refreshes nothing"
        );
        let builds = inbox.builds.get();

        let most = replay_watching(&mut laid, &inbox, &pull(device, 180.0));
        assert!(
            most >= 80.0,
            "{device:?}: the drag pulled past the threshold, {most}"
        );
        assert_eq!(
            inbox.list.pixels(),
            0.0,
            "{device:?}: the list stays at its top"
        );
        assert_eq!(
            inbox.builds.get(),
            builds,
            "{device:?}: a pull rebuilds the overlay, not the content"
        );
        laid.pump_for(FRAME);
        assert_eq!(inbox.started(), 1, "{device:?}: the release refreshes");
        assert!(inbox.refresh.is_refreshing(), "{device:?}");
        settle(&mut laid);
        assert_eq!(inbox.started(), 1, "{device:?}: once");
        assert_eq!(
            inbox.refresh.pull_distance_px(),
            0.0,
            "{device:?}: settled back"
        );
        assert!(
            inbox.refresh.is_refreshing(),
            "{device:?}: until the future completes"
        );
    }
}

/// A drag released short of the threshold refreshes nothing and settles
/// back over frames, not at once; a finger reversing its pull gives it back
/// before the list moves.
pub(crate) fn pull_released_short_settles_back() {
    for device in DEVICES {
        let inbox = Inbox::new();
        let mut laid = inbox.mount(Axis::Vertical);

        let most = replay_watching(&mut laid, &inbox, &pull(device, 60.0));
        assert!(most > 0.0, "{device:?}: the drag pulled");
        assert!(most < 80.0, "{device:?}: short of the threshold, {most}");
        // The first frame anchors the settling run; the next one moves it.
        laid.pump_for(FRAME);
        laid.pump_for(FRAME);
        let settling = inbox.refresh.pull_distance_px();
        assert!(
            settling > 0.0 && settling < most,
            "{device:?}: the pull settles back over frames, {settling} of {most}"
        );
        settle(&mut laid);
        assert_eq!(
            inbox.refresh.pull_distance_px(),
            0.0,
            "{device:?}: and comes to rest"
        );
        assert_eq!(inbox.started(), 0, "{device:?}: nothing refreshed");

        // Down past the threshold, then back up: the pull is given back.
        let mut points = line(at(100.0, 40.0), at(100.0, 200.0), 8);
        points.extend(
            line(at(100.0, 200.0), at(100.0, 30.0), 8)
                .into_iter()
                .skip(1),
        );
        let script = drag(device, &points, FRAME, Duration::from_millis(300));
        let most = replay_watching(&mut laid, &inbox, &script);
        assert!(most >= 80.0, "{device:?}: it was past the threshold");
        settle(&mut laid);
        assert_eq!(
            inbox.started(),
            0,
            "{device:?}: a pull given back refreshes nothing"
        );
        assert_eq!(
            inbox.list.pixels(),
            0.0,
            "{device:?}: the contact stays the pull's"
        );
    }
}

/// A wheel, a trackpad pan and its momentum, and a fling into the top
/// never pull; a cancelled contact past the threshold settles back without
/// refreshing.
pub(crate) fn pull_to_refresh_ignores_wheel_and_momentum_and_resets_on_loss() {
    let inbox = Inbox::new();
    let mut laid = inbox.mount(Axis::Vertical);

    let wheel = MouseScript::new("wheel up at the top")
        .move_to(at(100.0, 150.0))
        .wheel(-3.0)
        .wait(FRAME)
        .wheel(-3.0)
        .build()
        .expect("a hovering mouse");
    assert_eq!(
        replay_watching(&mut laid, &inbox, &wheel),
        0.0,
        "a wheel never pulls"
    );
    let trackpad = TrackpadScript::new("pan and momentum toward the top", at(100.0, 150.0))
        .pan(Offset::new(0.0, 200.0), 8, FRAME)
        .momentum(Offset::new(0.0, 400.0), 12, FRAME)
        .build()
        .expect("a whole pan");
    assert_eq!(
        replay_watching(&mut laid, &inbox, &trackpad),
        0.0,
        "a trackpad pan and its momentum never pull"
    );
    settle(&mut laid);

    for device in DEVICES {
        inbox.list.jump_to(300.0);
        laid.tick();
        let most = replay_watching(
            &mut laid,
            &inbox,
            &fling(device, at(100.0, 40.0), at(100.0, 200.0)),
        );
        settle(&mut laid);
        assert_eq!(
            inbox.list.pixels(),
            0.0,
            "{device:?}: the fling reached the top"
        );
        assert_eq!(most, 0.0, "{device:?}: and pulled nothing");
    }

    // A preset that hands the remainder on at the edge within the contact
    // offers a fling's own steps past the top too: the drag that reaches
    // the top pulls on, its fling does not.
    for device in DEVICES {
        let inbox = Inbox::new();
        let mut laid = inbox.mount_handing_off_at_the_edge();
        inbox.list.jump_to(60.0);
        laid.tick();
        let most = replay_watching(&mut laid, &inbox, &pull(device, 110.0));
        settle(&mut laid);
        assert!(
            most > 0.0,
            "{device:?}: the drag past the top pulls on, {most}"
        );
        assert_eq!(inbox.started(), 0, "{device:?}: short of the threshold");

        inbox.list.jump_to(300.0);
        laid.tick();
        let most = replay_watching(
            &mut laid,
            &inbox,
            &fling(device, at(100.0, 40.0), at(100.0, 200.0)),
        );
        settle(&mut laid);
        assert_eq!(
            inbox.list.pixels(),
            0.0,
            "{device:?}: the fling reached the top"
        );
        assert_eq!(most, 0.0, "{device:?}: its run past the top pulls nothing");
    }

    let cancelled = TouchScript::new("pull, then cancel")
        .down(1, at(100.0, 40.0))
        .wait(FRAME)
        .move_to(1, at(100.0, 120.0))
        .wait(FRAME)
        .move_to(1, at(100.0, 220.0))
        .wait(FRAME)
        .cancel(1)
        .build()
        .expect("one cancelled contact");
    let most = replay_watching(&mut laid, &inbox, &cancelled);
    assert!(
        most >= 80.0,
        "the cancelled contact was past the threshold, {most}"
    );
    settle(&mut laid);
    assert_eq!(
        inbox.refresh.pull_distance_px(),
        0.0,
        "a cancelled pull settles back"
    );
    assert_eq!(inbox.started(), 0, "nothing refreshed");
}

/// While a refresh runs, a pull reveals nothing and refreshes nothing, and
/// so do assistive technology's action and the refresh key.
pub(crate) fn refresh_during_refresh_is_refused() {
    let inbox = Inbox::new();
    let mut laid = inbox.mount(Axis::Vertical);
    laid.enable_semantics();
    laid.replay(&pull(Device::Touch, 180.0));
    settle(&mut laid);
    assert_eq!(inbox.started(), 1);

    let most = replay_watching(&mut laid, &inbox, &pull(Device::Touch, 180.0));
    settle(&mut laid);
    assert_eq!(most, 0.0, "a pull while refreshing reveals nothing");
    invoke_refresh_action(&mut laid);
    focus_row_and_press_f5(&mut laid);
    settle(&mut laid);
    assert_eq!(inbox.started(), 1, "and nothing refreshes again");
    assert!(inbox.refresh.is_refreshing());
}

/// The future `on_refresh` returned completes between frames; the refresh
/// ends when the next frame polls it, never inside the event that completed
/// it, and the next pull refreshes again.
pub(crate) fn refresh_completion_arrives_on_the_next_frame() {
    let inbox = Inbox::new();
    let mut laid = inbox.mount(Axis::Vertical);
    laid.replay(&pull(Device::Touch, 180.0));
    settle(&mut laid);
    assert!(inbox.refresh.is_refreshing());

    inbox.refreshes.complete();
    assert!(
        inbox.refresh.is_refreshing(),
        "the completion waits for the frame"
    );
    laid.pump_for(FRAME);
    assert!(!inbox.refresh.is_refreshing(), "the next frame delivers it");
    assert_eq!(inbox.refreshes.dropped.get(), 0);

    laid.replay(&pull(Device::Mouse, 180.0));
    laid.pump_for(FRAME);
    assert_eq!(inbox.started(), 2, "the next pull refreshes");
}

/// Unmounting the indicator mid-refresh drops its future unfinished and
/// leaves its controller idle; a later completion reaches nothing.
pub(crate) fn unmount_mid_refresh_cancels_it() {
    let inbox = Inbox::new();
    let mut laid = inbox.mount(Axis::Vertical);
    laid.replay(&pull(Device::Touch, 180.0));
    laid.pump_for(FRAME);
    assert!(inbox.refresh.is_refreshing());

    laid.pump_widget(SizedBox::new(300.0, 300.0));
    assert_eq!(
        inbox.refreshes.dropped.get(),
        1,
        "the refresh's future is dropped"
    );
    assert!(
        !inbox.refresh.is_refreshing(),
        "the controller shows it idle"
    );
    inbox.refreshes.complete();
    settle(&mut laid);
    assert!(!inbox.refresh.is_refreshing());
    assert_eq!(inbox.started(), 1);
}

/// A horizontal indicator pulls right from the left edge of a horizontal
/// list, and a vertical drag on it pulls nothing.
pub(crate) fn horizontal_pull_refreshes_from_the_left_edge() {
    for device in DEVICES {
        let inbox = Inbox::new();
        let mut laid = inbox.mount(Axis::Horizontal);
        let most = replay_watching(&mut laid, &inbox, &pull(device, 180.0));
        settle(&mut laid);
        assert_eq!(most, 0.0, "{device:?}: a vertical drag pulls nothing");
        assert_eq!(inbox.started(), 0);

        let most = replay_watching(
            &mut laid,
            &inbox,
            &slow(device, at(20.0, 100.0), at(200.0, 100.0)),
        );
        assert!(most >= 80.0, "{device:?}: a drag right pulls, {most}");
        laid.pump_for(FRAME);
        assert_eq!(inbox.started(), 1, "{device:?}: and refreshes");
    }
}

/// A rebuild with another controller, `on_refresh` and threshold hands the
/// running refresh to the new controller, leaves the old one idle, and the
/// next pull answers to the new threshold and calls the new callback.
pub(crate) fn a_replaced_configuration_takes_over_the_refresh() {
    let first = Inbox::new();
    let second = Inbox {
        list: first.list.clone(),
        refresh: RefreshController::new(),
        refreshes: Refreshes::new(),
        builds: Rc::clone(&first.builds),
    };
    let vsync = Vsync::new();
    let mut laid = mounted(
        VsyncScope::new(vsync.clone(), first.view(Axis::Vertical)).boxed(),
        vsync.clone(),
    );
    laid.replay(&pull(Device::Touch, 180.0));
    settle(&mut laid);
    assert!(first.refresh.is_refreshing());

    laid.pump_widget(
        VsyncScope::new(vsync, second.view(Axis::Vertical).threshold_px(140.0)).boxed(),
    );
    assert!(
        !first.refresh.is_refreshing(),
        "the old controller is left idle"
    );
    assert!(
        second.refresh.is_refreshing(),
        "the new one shows the running refresh"
    );
    first.refreshes.complete();
    laid.pump_for(FRAME);
    assert!(
        !second.refresh.is_refreshing(),
        "its completion reaches the new one"
    );

    laid.replay(&pull(Device::Touch, 130.0));
    settle(&mut laid);
    assert_eq!(second.started(), 0, "the new threshold holds");
    laid.replay(&pull(Device::Touch, 200.0));
    settle(&mut laid);
    assert_eq!(first.started(), 1, "the old callback is not called again");
    assert_eq!(second.started(), 1, "the next refresh calls the new one");
}

/// Alternatives whose first command does not perform `RefreshIntent` are
/// reported: the pull would perform it without refreshing.
pub(crate) fn a_first_command_without_the_refresh_intent_is_reported() {
    #[derive(Debug, Clone, Copy)]
    struct Reload;
    impl flui_widgets::Intent for Reload {}

    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        flui_widgets::GestureCommand::new(LABEL, Reload),
        SizedBox::new(24.0, 24.0),
    )))
    .expect("one labelled command");
    let ((), log) = flui_testing::log_capture::capture(|| {
        let _laid = crate::common::lay_out(
            RefreshIndicator::new(
                Scrollable::new().child(SizedBox::new(200.0, 1000.0)),
                alternatives,
            ),
            tight(300.0, 300.0),
        );
    });
    assert!(
        log.records()
            .iter()
            .any(|record| record.target == "flui::a11y"
                && record.message.contains("does not perform RefreshIntent")),
        "the misplaced command is reported:\n{log}"
    );
}

// ---------------------------------------------------------------------------
// Alternatives
// ---------------------------------------------------------------------------

/// Matrix row: `RefreshIndicator` with a visible Refresh control. The pull,
/// the custom action, F5 and the platform's reload chord in the row, and
/// the control pressed with a mouse, a finger and a dwell pointer each start
/// one refresh; Escape starts none.
pub(crate) fn refresh_indicator_with_a_visible_control() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "RefreshIndicator / visible control",
        tight(300.0, 300.0),
        TouchScript::new("pull")
            .down(1, at(100.0, 40.0))
            .wait(FRAME)
            .move_to(1, at(100.0, 120.0))
            .wait(FRAME)
            .move_to(1, at(100.0, 220.0))
            .wait(Duration::from_millis(300))
            .up(1)
            .build()
            .expect("one whole contact"),
        || {
            let inbox = Inbox::new();
            let view = inbox.view(Axis::Vertical);
            let refreshes = Rc::clone(&inbox.refreshes);
            AlternativesSubject::new(view, &alternatives(), move || refreshes.started.get())
        },
    ));
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn invoke_refresh_action(laid: &mut LaidOut) {
    let (target, id) = {
        let tree = laid.a11y_tree().expect("semantics are enabled");
        tree.nodes()
            .find_map(|node| {
                node.raw()
                    .custom_actions()
                    .iter()
                    .find(|action| action.description == LABEL)
                    .map(|action| (node.id(), action.id))
            })
            .expect("the row offers the refresh action")
    };
    laid.invoke_semantics_action(ActionRequest {
        action: Action::CustomAction,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(ActionData::CustomAction(id)),
    })
    .expect("the action resolves");
}

fn press(laid: &LaidOut, activator: &SingleActivator) {
    let manager = laid.focus_manager();
    for state in [KeyState::Down, KeyState::Up] {
        let event = KeyEvent::new(
            state,
            activator.trigger().clone(),
            Code::Unidentified,
            EventTime::from_nanos(0),
        )
        .with_modifiers(activator.modifiers());
        let _ = manager.dispatch_key_event(&event);
    }
}

fn focus_row_and_press_f5(laid: &mut LaidOut) {
    for _ in 0..16 {
        press(laid, &SingleActivator::named(NamedKey::Tab));
        laid.pump();
        let in_row = laid.a11y_tree().and_then(|tree| {
            tree.focus().map(|node| {
                node.raw()
                    .custom_actions()
                    .iter()
                    .any(|action| action.description == LABEL)
            })
        });
        if in_row == Some(true) {
            press(laid, &SingleActivator::named(NamedKey::F5));
            return;
        }
    }
    panic!("Tab never reached the refresh row");
}
