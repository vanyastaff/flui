//! The alternatives of a behavioural widget's gestures (ADR-0216) as a
//! conformance check.
//!
//! [`assert_gesture_alternatives`] mounts a fresh copy of the subject for
//! every path and performs each of its commands through every path the
//! subject's [`GestureAlternatives`] declare:
//!
//! - the gesture itself, from the [`PointerScript`] the check is given, for
//!   the first command;
//! - assistive technology's custom action, found by the command's label, and
//!   again under reduced motion, both the user's setting and an animator
//!   duration scale of 0, where it must reach the same end state within two
//!   frames, without animation;
//! - each of the command's keys on the running platform, with focus moved
//!   into the row by Tab;
//! - one pointer: a [`VisibleControl`](Alternative::VisibleControl) pressed
//!   with a mouse, a finger and a virtual (dwell or switch) pointer, and once
//!   just outside its drawn bounds but inside the minimum target; a
//!   [`ContextMenu`](Alternative::ContextMenu) opened by a secondary click, a
//!   long press, each of the platform's menu keys, assistive technology's
//!   `ShowContextMenu` and the control focus reveals, and its item chosen by
//!   assistive technology and by a click.
//!
//! Every path must change the subject's model, and to the same value as the
//! reference path (the gesture for the first command, the custom action for
//! the others), leaving the same live-region announcements. Escape with focus
//! in the row must change nothing. No path hovers first, so everything it
//! reaches is reachable without hover.
//!
//! The check reads the menu's items from the semantics tree, so a menu
//! presenter passes only if the items it shows are reachable by assistive
//! technology.

use std::fmt::Debug;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::GestureSettings;
use flui_platform_api::keyboard::{Code, KeyEvent, KeyState, NamedKey};
use flui_platform_api::pointer::PointerButton;
use flui_platform_api::{DeviceId, EventTime, MotionPreference, SystemPreferences, TargetPlatform};
use flui_rendering::constraints::BoxConstraints;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{Alternative, GestureAlternatives, ShowContextMenuIntent, SingleActivator};

use crate::a11y::{A11yTree, Action, ActionData, ActionRequest, Role, TreeId};
use crate::replay::{MouseScript, PointerScript, TouchScript, VirtualScript};
use crate::widgets::{LaidOut, lay_out};

/// Long enough for every built-in long-press timeout.
const HOLD: Duration = Duration::from_millis(1_000);
/// How long a path's effects are given to settle: slides, collapses and
/// deferred deliveries.
const SETTLE: Duration = Duration::from_secs(2);
/// More Tab presses than any subject has stops before its row.
const MAX_TABS: usize = 32;
/// Frames a path has under reduced motion: the action's own and one for a
/// delivery deferred to the next frame. Reduced motion settles an animation
/// at its next tick, so anything slower is an animation that kept playing.
const REDUCED_FRAMES: usize = 2;

/// One mounted copy of the subject: its tree, the alternatives it mounts and
/// a reading of its model.
pub struct AlternativesSubject<M> {
    view: BoxedView,
    alternatives: GestureAlternatives,
    model: Rc<dyn Fn() -> M>,
}

impl<M> AlternativesSubject<M> {
    /// `view` mounts `alternatives`; `model` reads the state its commands
    /// change.
    pub fn new(
        view: impl View,
        alternatives: &GestureAlternatives,
        model: impl Fn() -> M + 'static,
    ) -> Self {
        Self {
            view: view.boxed(),
            alternatives: alternatives.clone(),
            model: Rc::new(model),
        }
    }
}

impl<M> std::fmt::Debug for AlternativesSubject<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlternativesSubject")
            .field("alternatives", &self.alternatives)
            .finish_non_exhaustive()
    }
}

impl<M> std::fmt::Debug for GestureAlternativesCheck<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureAlternativesCheck")
            .field("name", &self.name)
            .field("constraints", &self.constraints)
            .field("gesture", &self.gesture.name())
            .finish_non_exhaustive()
    }
}

/// What [`assert_gesture_alternatives`] checks: how to mount the subject,
/// under which constraints, and the gesture that performs its first command.
pub struct GestureAlternativesCheck<M> {
    name: String,
    constraints: BoxConstraints,
    gesture: PointerScript,
    mount: Rc<dyn Fn() -> AlternativesSubject<M>>,
}

impl<M> GestureAlternativesCheck<M> {
    /// Check the subject `mount` builds afresh for each path, laid out under
    /// `constraints`, whose gesture `gesture` performs its first command.
    pub fn new(
        name: impl Into<String>,
        constraints: BoxConstraints,
        gesture: PointerScript,
        mount: impl Fn() -> AlternativesSubject<M> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            constraints,
            gesture,
            mount: Rc::new(mount),
        }
    }
}

/// What one path left behind.
#[derive(Debug, PartialEq)]
struct Outcome<M> {
    model: M,
    announcements: Vec<String>,
}

/// Run every path of every command of the subject.
///
/// # Panics
///
/// With every path that did not perform its command, performed it with a
/// different effect or announcement, needed hover, or could not find what the
/// alternatives declare; and if Escape changed the model.
#[track_caller]
pub fn assert_gesture_alternatives<M: PartialEq + Debug + 'static>(
    check: &GestureAlternativesCheck<M>,
) {
    let mut run = Run {
        check,
        failures: Vec::new(),
    };
    run.all();
    assert!(
        run.failures.is_empty(),
        "{}: gesture alternatives do not conform:\n{}",
        check.name,
        run.failures.join("\n")
    );
}

struct Run<'a, M> {
    check: &'a GestureAlternativesCheck<M>,
    failures: Vec<String>,
}

/// A mounted copy, its model reading and the initial model.
struct Mounted<M> {
    laid: LaidOut,
    alternatives: GestureAlternatives,
    model: Rc<dyn Fn() -> M>,
}

impl<M: PartialEq + Debug + 'static> Run<'_, M> {
    fn all(&mut self) {
        let probe = self.mount();
        let initial = (probe.model)();
        let commands: Vec<(String, Vec<SingleActivator>)> = probe
            .alternatives
            .commands()
            .map(|command| {
                (
                    command.label().to_owned(),
                    command.keys(TargetPlatform::current()).cloned().collect(),
                )
            })
            .collect();
        let menu = matches!(
            probe.alternatives.alternative(),
            Alternative::ContextMenu(_)
        );
        drop(probe);

        for (index, (label, keys)) in commands.iter().enumerate() {
            let reference = if index == 0 {
                let mut mounted = self.mount();
                mounted.laid.replay(&self.check.gesture);
                Some(("gesture", Self::settle(&mut mounted)))
            } else {
                None
            };
            let assistive = self.path(label, "assistive technology", |_, mounted| {
                Self::invoke_custom_action(mounted, label)
            });
            let reference = match (reference, assistive) {
                (Some(reference), assistive) => {
                    if let Some(outcome) = assistive {
                        self.compare(
                            label,
                            &reference,
                            ("assistive technology", outcome),
                            &initial,
                        );
                    }
                    reference
                }
                (None, Some(assistive)) => ("assistive technology", assistive),
                (None, None) => continue,
            };
            self.require_change(label, &reference, &initial);
            self.reduced_motion_paths(label, &reference, &initial);

            for key in keys {
                let path = format!("key {key:?}");
                if let Some(outcome) = self.path(label, &path, |_, mounted| {
                    Self::focus_row(mounted, label)?;
                    press(mounted, key);
                    Some(())
                }) {
                    self.compare(label, &reference, (&path, outcome), &initial);
                }
            }

            if menu {
                self.menu_paths(label, &reference, &initial);
            } else {
                self.control_paths(label, &reference, &initial);
            }
        }

        self.escape_changes_nothing(commands.first().map(|(label, _)| label.as_str()), &initial);
    }

    fn mount(&self) -> Mounted<M> {
        let subject = (self.check.mount)();
        let mut laid = lay_out(subject.view, self.check.constraints);
        laid.enable_semantics();
        laid.pump();
        Mounted {
            laid,
            alternatives: subject.alternatives,
            model: subject.model,
        }
    }

    /// Mount, run `act`, settle; `None` (with a failure recorded by `act`)
    /// when the path could not be driven.
    fn path(
        &mut self,
        label: &str,
        path: &str,
        act: impl FnOnce(&mut Self, &mut Mounted<M>) -> Option<()>,
    ) -> Option<Outcome<M>> {
        let mut mounted = self.mount();
        if act(self, &mut mounted).is_none() {
            self.failures.push(format!(
                "  - {label:?} by {path}: the path could not be driven"
            ));
            return None;
        }
        Some(Self::settle(&mut mounted))
    }

    fn settle(mounted: &mut Mounted<M>) -> Outcome<M> {
        let step = Duration::from_millis(16);
        let mut elapsed = Duration::ZERO;
        while elapsed < SETTLE {
            mounted.laid.pump_for(step);
            elapsed += step;
        }
        Self::outcome(mounted)
    }

    /// The model and the live-region announcements as they are now.
    fn outcome(mounted: &Mounted<M>) -> Outcome<M> {
        Outcome {
            model: (mounted.model)(),
            announcements: tree(mounted)
                .map(|tree| {
                    tree.nodes()
                        .filter(|node| node.live().is_some())
                        .filter_map(|node| node.label().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Under reduced motion, from the user's setting or from an animator
    /// duration scale of 0 (Android's), the custom action reaches the
    /// reference's end state within [`REDUCED_FRAMES`]: no animation plays
    /// before the model changes.
    fn reduced_motion_paths(&mut self, label: &str, reference: &(&str, Outcome<M>), initial: &M) {
        let sources = [
            ("reduced motion", MotionPreference::Reduce),
            (
                "an animator duration scale of 0",
                MotionPreference::from_duration_scale(0.0)
                    .expect("BUG: a zero duration scale is a valid observation"),
            ),
        ];
        for (source, motion) in sources {
            let path = format!("assistive technology under {source}");
            let mut mounted = self.mount();
            mounted
                .laid
                .set_system_preferences(SystemPreferences::default().with_motion(motion));
            mounted.laid.pump();
            if Self::invoke_custom_action(&mut mounted, label).is_none() {
                self.failures.push(format!(
                    "  - {label:?} by {path}: the path could not be driven"
                ));
                continue;
            }
            for _ in 0..REDUCED_FRAMES {
                mounted.laid.pump_for(Duration::from_millis(16));
            }
            let outcome = Self::outcome(&mounted);
            self.compare(label, reference, (&path, outcome), initial);
        }
    }

    fn require_change(&mut self, label: &str, (path, outcome): &(&str, Outcome<M>), initial: &M) {
        if outcome.model == *initial {
            self.failures.push(format!(
                "  - {label:?} by {path}: the model did not change from {initial:?}"
            ));
        }
    }

    fn compare(
        &mut self,
        label: &str,
        (reference_path, reference): &(&str, Outcome<M>),
        (path, outcome): (&str, Outcome<M>),
        initial: &M,
    ) {
        if outcome.model == *initial {
            self.failures.push(format!(
                "  - {label:?} by {path}: the model did not change from {initial:?}"
            ));
        } else if outcome.model != reference.model {
            self.failures.push(format!(
                "  - {label:?} by {path}: the model is {:?}, by {reference_path} {:?}",
                outcome.model, reference.model
            ));
        }
        if outcome.announcements != reference.announcements {
            self.failures.push(format!(
                "  - {label:?} by {path}: announced {:?}, by {reference_path} {:?}",
                outcome.announcements, reference.announcements
            ));
        }
    }

    fn invoke_custom_action(mounted: &mut Mounted<M>, label: &str) -> Option<()> {
        let (target, id) = {
            let tree = tree(mounted)?;
            tree.nodes().find_map(|node| {
                node.raw()
                    .custom_actions()
                    .iter()
                    .find(|action| action.description == label)
                    .map(|action| (node.id(), action.id))
            })?
        };
        mounted
            .laid
            .invoke_semantics_action(request(
                Action::CustomAction,
                target,
                Some(ActionData::CustomAction(id)),
            ))
            .ok()
    }

    /// Tab until the node offering `label` as a custom action has focus.
    fn focus_row(mounted: &mut Mounted<M>, label: &str) -> Option<()> {
        for _ in 0..MAX_TABS {
            press(mounted, &SingleActivator::named(NamedKey::Tab));
            mounted.laid.pump();
            let focused = tree(mounted)?.focus().is_some_and(|node| {
                node.raw()
                    .custom_actions()
                    .iter()
                    .any(|action| action.description == label)
            });
            if focused {
                return Some(());
            }
        }
        None
    }

    fn control_paths(&mut self, label: &str, reference: &(&str, Outcome<M>), initial: &M) {
        let min_target = GestureSettings::default().min_target();
        for device in [Device::Mouse, Device::Touch, Device::Virtual] {
            let path = format!("its control, pressed with {device:?}");
            if let Some(outcome) = self.path(label, &path, |_, mounted| {
                let center = control_center(mounted, label)?;
                mounted.laid.replay(&device.tap(center));
                Some(())
            }) {
                self.compare(label, reference, (&path, outcome), initial);
            }
        }
        // The minimum target, not the drawn bounds, is what a finger has to
        // hit (WCAG 2.2, 2.5.8): touch it just outside its drawn bounds, on
        // the side away from the content, inside the minimum target.
        let path = "its control, touched beside its drawn bounds";
        let mut drawn_large_enough = false;
        let outcome = self.path(label, path, |_, mounted| {
            let bounds = control_bounds(mounted, label)?;
            let row = row_center(mounted, label)?;
            let center = center_of(bounds);
            let reach = min_target / 2.0 - 1.0;
            let beside = if bounds.y1 - bounds.y0 < min_target {
                Offset::new(center.dx, center.dy + reach)
            } else if bounds.x1 - bounds.x0 < min_target {
                let away = if center.dx >= row.dx { reach } else { -reach };
                Offset::new(center.dx + away, center.dy)
            } else {
                drawn_large_enough = true;
                return Some(());
            };
            mounted.laid.replay(&Device::Touch.tap(beside));
            Some(())
        });
        if !drawn_large_enough && let Some(outcome) = outcome {
            self.compare(label, reference, (path, outcome), initial);
        }
    }

    fn menu_paths(&mut self, label: &str, reference: &(&str, Outcome<M>), initial: &M) {
        let mut openers: Vec<(String, Opener)> = vec![
            ("a secondary click".to_owned(), Opener::SecondaryClick),
            ("a long press".to_owned(), Opener::LongPress),
            ("ShowContextMenu".to_owned(), Opener::Assistive),
            (
                "the control focus reveals".to_owned(),
                Opener::RevealedControl,
            ),
        ];
        for key in ShowContextMenuIntent::default_keys(TargetPlatform::current()) {
            openers.push((format!("the menu key {key:?}"), Opener::Key(key)));
        }
        for (name, opener) in &openers {
            let path = format!("its menu, opened by {name}, item chosen by assistive technology");
            if let Some(outcome) = self.path(label, &path, |run, mounted| {
                run.open_menu(mounted, label, opener)?;
                let target = menu_item(mounted, label)?;
                mounted
                    .laid
                    .invoke_semantics_action(request(Action::Click, target, None))
                    .ok()
            }) {
                self.compare(label, reference, (&path, outcome), initial);
            }
        }
        let path = "its menu, opened by a secondary click, item clicked";
        if let Some(outcome) = self.path(label, path, |run, mounted| {
            run.open_menu(mounted, label, &Opener::SecondaryClick)?;
            let center = {
                let tree = tree(mounted)?;
                let node = tree
                    .nodes()
                    .find(|node| is_menu_item(node.role()) && node.label() == Some(label))?;
                center_of(tree.window_bounds(&node)?)
            };
            mounted.laid.replay(&Device::Mouse.tap(center));
            Some(())
        }) {
            self.compare(label, reference, (path, outcome), initial);
        }
    }

    fn open_menu(&mut self, mounted: &mut Mounted<M>, label: &str, opener: &Opener) -> Option<()> {
        match opener {
            Opener::SecondaryClick => {
                let center = row_center(mounted, label)?;
                let click = MouseScript::new("secondary click")
                    .press_button(center, PointerButton::SECONDARY)
                    .wait(Duration::from_millis(16))
                    .release()
                    .build()
                    .expect("BUG: the secondary click releases its button");
                mounted.laid.replay(&click);
            }
            Opener::LongPress => {
                let center = row_center(mounted, label)?;
                let press = TouchScript::new("long press")
                    .down(1, center)
                    .wait(HOLD)
                    .up(1)
                    .build()
                    .expect("BUG: the long press lifts its finger");
                mounted.laid.replay(&press);
            }
            Opener::Assistive => {
                let target = {
                    let tree = tree(mounted)?;
                    tree.nodes()
                        .find(|node| node.supports_action(Action::ShowContextMenu))?
                        .id()
                };
                mounted
                    .laid
                    .invoke_semantics_action(request(Action::ShowContextMenu, target, None))
                    .ok()?;
            }
            Opener::Key(key) => {
                Self::focus_row(mounted, label)?;
                press(mounted, key);
            }
            Opener::RevealedControl => {
                let before: Vec<String> = buttons(mounted);
                Self::focus_row(mounted, label)?;
                let (revealed, bounds) = {
                    let tree = tree(mounted)?;
                    let node = tree.nodes().find(|node| {
                        node.role() == Role::Button
                            && node
                                .label()
                                .is_some_and(|label| !before.iter().any(|seen| seen == label))
                    })?;
                    (node.id(), tree.window_bounds(&node)?)
                };
                let min_target = GestureSettings::default().min_target();
                if bounds.x1 - bounds.x0 < min_target || bounds.y1 - bounds.y0 < min_target {
                    self.failures.push(format!(
                        "  - {label:?}: the control focus reveals is {}×{}, under the {min_target} minimum target",
                        bounds.x1 - bounds.x0,
                        bounds.y1 - bounds.y0
                    ));
                }
                mounted
                    .laid
                    .invoke_semantics_action(request(Action::Click, revealed, None))
                    .ok()?;
            }
        }
        mounted.laid.pump_for(Duration::from_millis(16));
        mounted.laid.pump();
        Some(())
    }

    fn escape_changes_nothing(&mut self, label: Option<&str>, initial: &M) {
        let Some(label) = label else {
            return;
        };
        if let Some(outcome) = self.path(label, "Escape", |_, mounted| {
            Self::focus_row(mounted, label)?;
            press(mounted, &SingleActivator::named(NamedKey::Escape));
            Some(())
        }) && outcome.model != *initial
        {
            self.failures.push(format!(
                "  - Escape in the row changed the model to {:?}: Escape is never destructive",
                outcome.model
            ));
        }
    }
}

/// How a menu path opens the menu.
enum Opener {
    SecondaryClick,
    LongPress,
    Assistive,
    Key(SingleActivator),
    RevealedControl,
}

/// The pointer a single-pointer path uses.
#[derive(Debug, Clone, Copy)]
enum Device {
    Mouse,
    Touch,
    /// A dwell or switch device reporting its own kind (ADR-0216 §3).
    Virtual,
}

impl Device {
    fn tap(self, at: Offset<f64>) -> PointerScript {
        let wait = Duration::from_millis(16);
        match self {
            Self::Mouse => MouseScript::new("mouse press")
                .press(at)
                .wait(wait)
                .release()
                .build(),
            Self::Touch => TouchScript::new("touch press")
                .down(1, at)
                .wait(wait)
                .up(1)
                .build(),
            Self::Virtual => VirtualScript::new(
                "virtual press",
                DeviceId::try_from(7_u64).expect("BUG: a nonzero device id"),
            )
            .press(at)
            .wait(wait)
            .release()
            .build(),
        }
        .expect("BUG: a press releases its contact")
    }
}

fn tree<M>(mounted: &Mounted<M>) -> Option<A11yTree> {
    mounted.laid.a11y_tree()
}

fn request(action: Action, target: crate::a11y::NodeId, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: target,
        data,
    }
}

/// Press and release `activator`'s key with its modifiers; on-release
/// activators match the release.
fn press<M>(mounted: &Mounted<M>, activator: &SingleActivator) {
    let manager = mounted.laid.focus_manager();
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

fn is_menu_item(role: Role) -> bool {
    matches!(role, Role::MenuItem | Role::Button)
}

fn buttons<M>(mounted: &Mounted<M>) -> Vec<String> {
    tree(mounted)
        .map(|tree| {
            tree.nodes()
                .filter(|node| node.role() == Role::Button)
                .filter_map(|node| node.label().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn menu_item<M>(mounted: &Mounted<M>, label: &str) -> Option<crate::a11y::NodeId> {
    let tree = tree(mounted)?;
    tree.nodes()
        .find(|node| {
            is_menu_item(node.role())
                && node.label() == Some(label)
                && node.supports_action(Action::Click)
        })
        .map(|node| node.id())
}

fn center_of(rect: crate::a11y::A11yRect) -> Offset<f64> {
    Offset::new(
        f64::midpoint(rect.x0, rect.x1),
        f64::midpoint(rect.y0, rect.y1),
    )
}

/// The window rect of the node offering `label` as a custom action: the row.
fn row_center<M>(mounted: &Mounted<M>, label: &str) -> Option<Offset<f64>> {
    let tree = tree(mounted)?;
    let row = tree.nodes().find(|node| {
        node.raw()
            .custom_actions()
            .iter()
            .any(|action| action.description == label)
    })?;
    Some(center_of(tree.window_bounds(&row)?))
}

fn control_bounds<M>(mounted: &Mounted<M>, label: &str) -> Option<crate::a11y::A11yRect> {
    let tree = tree(mounted)?;
    let node = tree.find_by_role_and_label(Role::Button, label).ok()?;
    tree.window_bounds(&node)
}

fn control_center<M>(mounted: &Mounted<M>, label: &str) -> Option<Offset<f64>> {
    control_bounds(mounted, label).map(center_of)
}
