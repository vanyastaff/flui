//! The methods the driver composes from the agent's (ADR-0238 §5, §6): its own (`ServedBy::Driver`),
//! and every gesture on the `os` path or for an app whose agent does not answer it (ADR-0400 §2):
//! locate, check actionability, perform by the chosen input path, observe. A gesture the app's
//! agent serves on a path other than `os` goes to the agent whole, from every entry here, carrying
//! the path [`choose_path`] chose as its `via` ([`answered_by_agent`]); the sequence a composed
//! gesture performs is [`gestures`](flui_protocol::input::gestures)'s, the one the agent plays.
//!
//! - **Locate.** A locator is resolved at the moment of the action, never cached (ADR-0238 §4):
//!   strict with several matches is [`Error::Ambiguous`], counting every match and naming the
//!   first ten as outline lines, by the one rule the agent follows too
//!   (`flui_protocol::locator::resolve_strict`);
//!   none waits once on the app's timeline for one to appear (`wait.until`), then is
//!   [`Error::NotFound`].
//! - **Actionability.** Pointer input waits, frame by frame, for the element to be attached,
//!   visible, stable, enabled and to receive the pointer at its point; past the timeout it is
//!   [`Error::NotActionable`] naming the check that never held and what covered the point. `force`
//!   skips every check but attached.
//! - **Path.** The call's policy, or the session's, chooses the path, in one place for a gesture
//!   the driver composes and one it hands to the agent ([`choose_path`]). A path the gesture or the
//!   session lacks is refused when the policy requires it, and otherwise falls back to synthetic,
//!   then semantic; a fallback never takes the OS path, so a session that did not ask for OS input
//!   never gets it. The answer names the path taken.
//! - **OS input** is admitted by the isolation rule, takes the desktop's lock, is announced before
//!   its first event, is sent through the lane's guard, and is `ran` only when the app's input
//!   journal saw it; otherwise `may_have_run`.

use std::time::Duration;

use flui_protocol::act::{
    ActOutcome, ActParams, Actionability, ActionabilityParams, Effect, EffectKind, JournalSpan,
};
use flui_protocol::error::{ErrorCode, HandleKind};
use flui_protocol::geometry::{LogicalPoint, LogicalRect};
use flui_protocol::input::gestures::{self, Aim as Landing, Defaults};
use flui_protocol::input::{
    ActionSequence, DragParams, GestureOptions, InputKind, InputParams, InputPath, InputPolicy,
    InputSource, Journal, JournalParams, LongPressParams, PinchParams, PointerButton,
    PointerDevice, PressKeysParams, ScrollIntoViewParams, ScrollUntilParams, SwipeParams,
    TapParams, TypeTextParams,
};
use flui_protocol::locator::{LocateParams, Locator, Matches, Pick, Target};
use flui_protocol::method::{self, Addressed, Method};
use flui_protocol::session::{Empty, Isolation, Placement, WindowParams};
use flui_protocol::snapshot::Snapshot;
use flui_protocol::tree::{ElementRef, WindowId};
use flui_protocol::wait::{Condition, WaitEnd, WaitOutcome, WaitParams};
use flui_protocol::wire::ActionName;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::driver::Driver;
use crate::error::{Error, Result};
use crate::os_input::{Announcement, OsDevice};
use crate::steps::{self, OsEvent};

/// The wire names of the methods composed here.
const COMPOSED: &[&str] = &[
    method::ElementDoubleTap::NAME,
    method::ElementDragTo::NAME,
    method::ElementFling::NAME,
    method::ElementLongPress::NAME,
    method::ElementPinch::NAME,
    method::ElementPressKeys::NAME,
    method::ElementScrollIntoView::NAME,
    method::ElementSwipe::NAME,
    method::ElementTap::NAME,
    method::ElementTypeText::NAME,
    method::InputPerform::NAME,
    method::SessionCapabilities::NAME,
];

/// Whether the driver composes the method `name`.
pub(crate) fn composes(name: &str) -> bool {
    COMPOSED.contains(&name)
}

/// Whether the driver decides the input path of the agent's method `name` before the agent hears
/// of it: every gesture it composes, and `element.scroll_until`, which only the agent plays.
pub(crate) fn decides_path(name: &str) -> bool {
    composes(name) || name == method::ElementScrollUntil::NAME
}

/// The input paths a gesture has a form on.
#[derive(Debug, Clone, Copy)]
struct Forms {
    synthetic: bool,
    semantic: bool,
    os: bool,
}

impl Forms {
    /// Pointer or key input, which an OS lane sends too, and a semantics action that does the same
    /// where `semantic`.
    const fn input(semantic: bool) -> Self {
        Self {
            synthetic: true,
            semantic,
            os: true,
        }
    }

    /// A semantics action alone: `element.scroll_into_view`.
    const SEMANTIC: Self = Self {
        synthetic: false,
        semantic: true,
        os: false,
    };

    /// Synthetic input the app's agent plays a step at a frame, reading the tree between steps,
    /// which no lane sends: `element.scroll_until`.
    const AGENT_INPUT: Self = Self {
        synthetic: true,
        semantic: false,
        os: false,
    };
}

/// Who plays a gesture's synthetic or semantic path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Player {
    /// The app's agent, which serves the gesture whole and has both paths of its own.
    Agent,
    /// The driver, through the agent's `input.inject` and `semantics.act`, when it serves them.
    Driver,
}

/// A gesture's parameters, whose options carry the path it takes.
trait GestureParams: Serialize + Addressed + Clone {
    fn options(&self) -> &GestureOptions;
    fn options_mut(&mut self) -> &mut GestureOptions;
}

macro_rules! gesture_params {
    ($($params:ty),* $(,)?) => {
        $(impl GestureParams for $params {
            fn options(&self) -> &GestureOptions {
                &self.options
            }

            fn options_mut(&mut self) -> &mut GestureOptions {
                &mut self.options
            }
        })*
    };
}

gesture_params!(
    TapParams,
    LongPressParams,
    DragParams,
    SwipeParams,
    PinchParams,
    ScrollIntoViewParams,
    ScrollUntilParams,
    TypeTextParams,
    PressKeysParams,
);

/// The policy the app's agent plays the gesture `name` by, when it answers it (ADR-0400 §2): it
/// serves the method, and [`choose_path`] chose a path other than `os`, which the driver composes
/// with its lane. The policy names the chosen path and requires it when the call or the session
/// does, so the agent applies the requirement the driver read: an agent that receives no `via`
/// takes the synthetic path and knows nothing of the session. `None` for a gesture composed here.
fn agent_via(
    driver: &Driver,
    name: &str,
    requested: Option<InputPolicy>,
    forms: Forms,
) -> Result<Option<InputPolicy>> {
    if !driver.serves(name) {
        return Ok(None);
    }
    let policy = requested.unwrap_or_else(|| driver.input_policy());
    let path = choose_path(driver, Some(policy), forms, Player::Agent)?;
    Ok((path != InputPath::Os).then(|| {
        if policy.require {
            InputPolicy::require(path)
        } else {
            InputPolicy::prefer(path)
        }
    }))
}

/// The app's agent's answer to the gesture `M` with `params`, when it answers it ([`agent_via`]),
/// asked with the chosen path as its `via`; `None` for a gesture composed here.
fn answered_by_agent<M>(
    driver: &mut Driver,
    params: &M::Params,
    forms: Forms,
) -> Result<Option<ActOutcome>>
where
    M: Method<Output = ActOutcome>,
    M::Params: GestureParams,
{
    let Some(via) = agent_via(driver, M::NAME, params.options().via, forms)? else {
        return Ok(None);
    };
    let mut params = params.clone();
    params.options_mut().via = Some(via);
    let value = serde_json::to_value(&params).map_err(|error| {
        Error::invalid_argument(format!(
            "the parameters of `{}` did not serialize: {error}",
            M::NAME
        ))
    })?;
    let output = driver.agent_value(M::NAME, M::EFFECT, value, params.holds_for())?;
    crate::driver::decode::<ActOutcome>(M::NAME, output).map(Some)
}

/// Whether `target` is a point, which names no element a semantics action could reach.
fn is_point(target: &Target) -> bool {
    matches!(target, Target::Point { .. })
}

/// Run the driver's method `name` with `params`.
pub(crate) fn call(driver: &mut Driver, name: &'static str, params: Value) -> Result<Value> {
    match name {
        method::ElementScrollUntil::NAME => run(name, params, |p| scroll_until(driver, p)),
        method::ElementTap::NAME => run(name, params, |p| tap(driver, p)),
        method::ElementDoubleTap::NAME => run(name, params, |p| double_tap(driver, p)),
        method::ElementLongPress::NAME => run(name, params, |p| long_press(driver, p)),
        method::ElementDragTo::NAME => run(name, params, |p| drag_to(driver, p)),
        method::ElementSwipe::NAME => run(name, params, |p| swipe(driver, p, false)),
        method::ElementFling::NAME => run(name, params, |p| swipe(driver, p, true)),
        method::ElementPinch::NAME => run(name, params, |p| pinch(driver, p)),
        method::ElementScrollIntoView::NAME => run(name, params, |p| scroll_into_view(driver, p)),
        method::ElementTypeText::NAME => run(name, params, |p| type_text(driver, p)),
        method::ElementPressKeys::NAME => run(name, params, |p| press_keys(driver, p)),
        method::InputPerform::NAME => run(name, params, |p| perform(driver, p)),
        method::SessionCapabilities::NAME => {
            run(name, params, |_: Empty| Ok(driver.capabilities()))
        }
        method::SessionLaunch::NAME | method::SessionAttach::NAME | method::SessionClose::NAME => {
            Err(Error::invalid_argument(format!(
                "`{name}` begins or ends a session, which the front end holds: its Rust form is \
                 `Driver::launch`, `Driver::attach` or `Driver::close`"
            )))
        }
        _ => Err(Error::not_supported(
            name,
            format!("this version of the driver does not compose `{name}`"),
        )),
    }
}

/// Read `params`, run `body`, write its output.
fn run<P: DeserializeOwned, O: Serialize>(
    name: &str,
    params: Value,
    body: impl FnOnce(P) -> Result<O>,
) -> Result<Value> {
    let params = serde_json::from_value(params).map_err(|error| {
        Error::invalid_argument(format!("the parameters do not read as `{name}`'s: {error}"))
    })?;
    serde_json::to_value(body(params)?).map_err(|error| Error::Protocol {
        message: format!("the output of `{name}` did not serialize: {error}"),
    })
}

/// Where an action lands.
#[derive(Debug, Clone, Copy)]
struct Aim {
    window: WindowId,
    element: Option<ElementRef>,
    point: LogicalPoint,
    rect: Option<LogicalRect>,
}

/// The element `target` names now, strictly.
pub(crate) fn resolve(
    driver: &mut Driver,
    target: &Target,
    timeout: Duration,
) -> Result<ElementRef> {
    match target {
        Target::Element(element) => Ok(*element),
        Target::Locator(locator) => resolve_locator(driver, locator, timeout),
        Target::Point { .. } => Err(Error::invalid_argument("a point names no element")),
        _ => Err(Error::invalid_argument(
            "a target of a kind this driver does not know",
        )),
    }
}

/// The one element `locator` matches; waits once, on the app's timeline, for one to appear.
fn resolve_locator(
    driver: &mut Driver,
    locator: &Locator,
    timeout: Duration,
) -> Result<ElementRef> {
    let mut waited = false;
    // A strict pick needs only the candidates it names reported: the rest it counts from
    // `truncated`, so a locator matching thousands answers `ambiguous`, not a reply past the line.
    let mut params = LocateParams::new(locator.clone());
    if locator.pick == Pick::Strict {
        params.max_matches = Some(flui_protocol::locator::STRICT_CANDIDATES);
    }
    loop {
        let found = driver.agent::<method::SemanticsLocate>(params.clone(), Duration::ZERO)?;
        let Matches {
            matches, truncated, ..
        } = found;
        match matches.as_slice() {
            [] if !waited => {
                waited = true;
                let window = match locator.window {
                    Some(window) => window,
                    None => main_window(driver)?,
                };
                let outcome =
                    wait_until(driver, window, Condition::Appears(locator.clone()), timeout)?;
                if outcome.end != WaitEnd::Satisfied {
                    return Err(not_found(locator));
                }
            }
            [] => return Err(not_found(locator)),
            [first, ..] if locator.pick != Pick::Strict => return Ok(first.element),
            reported => {
                return match flui_protocol::locator::resolve_strict(reported.iter(), truncated) {
                    Ok(Some(only)) => Ok(only.element),
                    Ok(None) => Err(not_found(locator)),
                    Err(ambiguity) => Err(Error::Ambiguous {
                        message: format!(
                            "{} elements match a strict locator; narrow it, or pick first, last or nth",
                            ambiguity.matched
                        ),
                        candidates: ambiguity.candidates,
                        retry: ErrorCode::Ambiguous.default_retry(),
                    }),
                };
            }
        }
    }
}

fn not_found(locator: &Locator) -> Error {
    Error::not_found(format!("nothing matches {locator:?}"))
}

/// The app's first window.
pub(crate) fn main_window(driver: &mut Driver) -> Result<WindowId> {
    driver
        .windows()?
        .into_iter()
        .next()
        .ok_or_else(|| Error::not_found("the app has no window with content"))
}

/// Wait in `window` for `condition`, at most `timeout` of the app's clock.
pub(crate) fn wait_until(
    driver: &mut Driver,
    window: WindowId,
    condition: Condition,
    timeout: Duration,
) -> Result<WaitOutcome> {
    let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    let params = WaitParams::new(window, condition).timeout_ms(timeout_ms);
    let holds = Duration::from_millis(params.effective_timeout_ms());
    driver.agent::<method::WaitUntil>(params, holds)
}

/// The facts of `element` once it is actionable, read again after each frame the window presents,
/// until `timeout` has passed on the connection's clock (the virtual one on the headless target).
/// A window that presents no frame before the deadline ends the wait there. The answer is always
/// read after the last wait: an element that became actionable on the frame that ended it is
/// actionable.
fn actionable(
    driver: &mut Driver,
    element: ElementRef,
    force: bool,
    timeout: Duration,
) -> Result<Actionability> {
    let deadline = driver.clock().saturating_add(timeout);
    let mut expired = false;
    loop {
        let facts = driver.agent::<method::ElementActionability>(
            ActionabilityParams::new(element),
            Duration::ZERO,
        )?;
        if !facts.attached {
            return Err(Error::gone(
                HandleKind::Element,
                Some(element.to_string()),
                None,
                format!("{element} is no longer in its window's tree"),
            ));
        }
        let Some(failed) = facts.first_failed() else {
            return Ok(facts);
        };
        if force {
            return Ok(facts);
        }
        if expired {
            let covered_by = facts.covered_by;
            return Err(Error::NotActionable {
                failed: Some(failed),
                message: format!(
                    "{element} was not {} within {} ms{}",
                    failed.name(),
                    timeout.as_millis(),
                    covered_by
                        .as_deref()
                        .map_or_default(|cover| format!("; the point reached {cover}"))
                ),
                covered_by,
                effect: None,
                retry: ErrorCode::NotActionable.default_retry(),
            });
        }
        let left = deadline.saturating_sub(driver.clock());
        let presented = !left.is_zero()
            && wait_until(driver, element.window, Condition::Frames(1), left)?.end
                == WaitEnd::Satisfied;
        // Past the deadline, or no frame to wait for: the next reading is the last.
        expired = !presented || driver.clock() >= deadline;
    }
}

/// Where `target` lands, after the checks its options ask for. The driver reads the element over
/// the wire, which carries its whole bounds and not the part the window shows, so a composed
/// gesture takes its extent from the whole bounds; the agent in the application aims at the shown
/// part. A composed gesture also takes the windowless [`Defaults`]: the driver does not read the
/// window's gesture settings.
fn aim(driver: &mut Driver, target: &Target, options: &GestureOptions) -> Result<Aim> {
    let timeout = timeout_of(driver, options);
    if let Target::Point { window, at } = target {
        return Ok(Aim {
            window: *window,
            element: None,
            point: *at,
            rect: None,
        });
    }
    let element = resolve(driver, target, timeout)?;
    let facts = actionable(driver, element, options.force, timeout)?;
    let point = facts
        .point
        .or_else(|| facts.rect.map(|rect| rect.center()))
        .ok_or_else(|| Error::NotActionable {
            failed: Some(flui_protocol::act::ActionabilityCheck::Visible),
            covered_by: None,
            message: format!("{element} shows no point to aim at"),
            effect: None,
            retry: ErrorCode::NotActionable.default_retry(),
        })?;
    Ok(Aim {
        window: element.window,
        element: Some(element),
        point,
        rect: facts.rect,
    })
}

/// How long a composed act locates and checks: the timeout it names, as
/// [`GestureOptions::named_timeout_ms`] bounds it, or the session's.
pub(crate) fn timeout_of(driver: &Driver, options: &GestureOptions) -> Duration {
    options
        .named_timeout_ms()
        .map_or(driver.timeout, Duration::from_millis)
}

/// The path an action with `forms`, played by `player`, takes under `requested` (the session's
/// policy when `None`). The one rule for every gesture: one the driver composes asks it before it
/// performs, one it hands to the app's agent before it is sent ([`agent_via`]).
fn choose_path(
    driver: &Driver,
    requested: Option<InputPolicy>,
    forms: Forms,
    player: Player,
) -> Result<InputPath> {
    let policy = requested.unwrap_or_else(|| driver.input_policy());
    let by_agent = player == Player::Agent;
    let available = |path: InputPath| match path {
        InputPath::Semantic => {
            forms.semantic && (by_agent || driver.serves(method::SemanticsAct::NAME))
        }
        InputPath::Synthetic => {
            forms.synthetic && (by_agent || driver.serves(method::InputInject::NAME))
        }
        InputPath::Os => forms.os && driver.lane().is_some(),
        _ => false,
    };
    if available(policy.prefer) {
        return Ok(policy.prefer);
    }
    let missing = || {
        Error::not_supported(
            format!("{}_input", policy.prefer.name()),
            format!(
                "the target takes no {} input{}",
                policy.prefer.name(),
                if policy.prefer == InputPath::Os && forms.os {
                    "; attach an OS lane"
                } else {
                    ""
                }
            ),
        )
    };
    if policy.require {
        return Err(missing());
    }
    // A fallback never takes the OS path: OS input is asked for, never fallen into.
    [InputPath::Synthetic, InputPath::Semantic]
        .into_iter()
        .find(|path| available(*path))
        .ok_or_else(missing)
}

/// Perform `sequence` at `aim` by the chosen path; `semantic` is the semantics action that does
/// the same, where one does.
fn act(
    driver: &mut Driver,
    aim: Aim,
    options: &GestureOptions,
    sequence: ActionSequence,
    semantic: Option<ActionName>,
) -> Result<ActOutcome> {
    let path = choose_path(
        driver,
        options.via,
        Forms::input(semantic.is_some() && aim.element.is_some()),
        Player::Driver,
    )?;
    let mut outcome = match (path, semantic, aim.element) {
        (InputPath::Semantic, Some(action), Some(element)) => driver
            .agent::<method::SemanticsAct>(
                ActParams::new(element, action).snapshot(options.snapshot),
                Duration::ZERO,
            )?,
        (InputPath::Os, ..) => perform_os(driver, aim.window, sequence)?,
        _ => inject(driver, aim.window, sequence, options.snapshot)?,
    };
    if outcome.element.is_none() {
        outcome.element = aim.element;
    }
    Ok(outcome)
}

/// Inject `sequence` into `window` through the app's own input path.
fn inject(
    driver: &mut Driver,
    window: WindowId,
    sequence: ActionSequence,
    snapshot: Snapshot,
) -> Result<ActOutcome> {
    let mut params = InputParams::new(window, sequence);
    params.snapshot = snapshot;
    let holds = params.holds_for();
    driver.agent::<method::InputInject>(params, holds)
}

/// Send `sequence` into `window` through the session's OS lane: see the [module docs](self).
fn perform_os(
    driver: &mut Driver,
    window: WindowId,
    sequence: ActionSequence,
) -> Result<ActOutcome> {
    // The same admission `send_guarded` holds, asked before anything is locked or announced.
    lane_run(driver, crate::lane::admit)??;
    let (isolation, composes, name) = driver
        .lane()
        .map(|lane| {
            let capabilities = lane.capabilities();
            (
                capabilities.isolation,
                capabilities.compositions,
                lane.name().to_owned(),
            )
        })
        .ok_or_else(no_lane)?;
    // What no lane sends, and a composition this lane cannot send, are refused before the desktop
    // is locked, the window located or the run announced, as `send_guarded` refuses a
    // composition again before any event.
    let admitted = steps::admit(&sequence)?;
    let composition = sequence
        .sources
        .iter()
        .any(|source| matches!(source, InputSource::Ime { .. }));
    if composition && !composes {
        return Err(crate::lane::no_composition(&name));
    }
    // A headless browser is the session's own: its input lands on no desktop another session's
    // could interleave with.
    if isolation != Isolation::HeadlessBrowser {
        driver.lock_os_input()?;
    }
    let placement: Placement =
        driver.agent::<method::WindowPlacement>(WindowParams::new(window), Duration::ZERO)?;
    let geometry = lane_run(driver, move |lane| lane.locate_window(&placement))??;
    let events = steps::events_of(&admitted, &geometry)?;
    let announcement = match driver.scenario() {
        Some(scenario) if !scenario.devices().is_empty() => {
            Announcement::new(scenario.devices().to_vec(), scenario.duration(), isolation)
        }
        _ => Announcement::new(devices_of(&events), played(&events), isolation),
    };
    driver.announce_once(|| announcement.to_string());
    let before = journal(driver, window, None)?;
    let since = before.span.map(|span| span.to);
    let sent = events.clone();
    lane_run(driver, move |lane| crate::lane::send_guarded(lane, &sent))??;
    // The input went out: from here on, a failure to read what it did leaves its effect unknown,
    // never not run.
    Ok(match journal(driver, window, since) {
        Ok(after) => observed(&events, &after),
        Err(error) => ActOutcome::new(
            InputPath::Os,
            Effect::new(EffectKind::MayHaveRun).detail(format!(
                "the OS accepted the input, and the app's journal could not be read after it: \
                 {error}"
            )),
        ),
    })
}

fn no_lane() -> Error {
    Error::not_supported("os_input", "the session has no OS lane")
}

fn lane_run<R: Send + 'static>(
    driver: &Driver,
    job: impl FnOnce(&mut dyn crate::lane::Lane) -> R + Send + 'static,
) -> Result<R> {
    driver.lane().ok_or_else(no_lane)?.run(job)
}

fn journal(driver: &mut Driver, window: WindowId, since: Option<u64>) -> Result<Journal> {
    let mut params = JournalParams::new(window);
    params.since = since;
    driver.agent::<method::InputJournal>(params, Duration::ZERO)
}

/// What OS input did, by what the app's journal saw after it: `ran` when every kind of input
/// sent reached the app, `may_have_run` otherwise.
fn observed(events: &[OsEvent], journal: &Journal) -> ActOutcome {
    let kinds_sent = events.iter().filter_map(|event| match event {
        OsEvent::PointerDown { .. } => Some(InputKind::PointerDown),
        OsEvent::PointerUp { .. } => Some(InputKind::PointerUp),
        OsEvent::KeyDown(_) => Some(InputKind::KeyDown),
        OsEvent::KeyUp(_) => Some(InputKind::KeyUp),
        OsEvent::Text(_) => Some(InputKind::Text),
        OsEvent::Scroll { .. } => Some(InputKind::Scroll),
        OsEvent::Composition(_) => Some(InputKind::Composition),
        _ => None,
    });
    let seen = |kind: InputKind| journal.entries.iter().any(|entry| entry.kind == kind);
    let mut kinds_sent = kinds_sent.peekable();
    let ran = kinds_sent.peek().is_some() && kinds_sent.all(seen);
    let effect = if ran {
        Effect::new(EffectKind::Ran)
    } else {
        Effect::new(EffectKind::MayHaveRun)
            .detail("the OS accepted the input, and the app's journal did not show all of it")
    };
    let mut outcome = ActOutcome::new(InputPath::Os, effect);
    outcome.observed = journal.span.map(|span| JournalSpan {
        from: span.from,
        to: span.to,
    });
    outcome
}

/// How long sending `events` takes: the sum of their pauses, which end at the plan's end.
fn played(events: &[OsEvent]) -> Duration {
    events
        .iter()
        .filter_map(|event| match event {
            OsEvent::Pause(pause) => Some(*pause),
            _ => None,
        })
        .fold(Duration::ZERO, Duration::saturating_add)
}

fn devices_of(events: &[OsEvent]) -> Vec<OsDevice> {
    let mut devices = Vec::new();
    for event in events {
        let device = match event {
            OsEvent::PointerMove { device, .. }
            | OsEvent::PointerDown { device, .. }
            | OsEvent::PointerUp { device, .. } => match device {
                PointerDevice::Touch => OsDevice::Touch,
                PointerDevice::Pen => OsDevice::Pen,
                _ => OsDevice::Mouse,
            },
            OsEvent::Scroll { .. } => OsDevice::Mouse,
            OsEvent::KeyDown(_)
            | OsEvent::KeyUp(_)
            | OsEvent::Text(_)
            | OsEvent::Composition(_) => OsDevice::Keyboard,
            _ => continue,
        };
        if !devices.contains(&device) {
            devices.push(device);
        }
    }
    devices
}

impl Aim {
    /// Where the gesture lands, as [`gestures`] builds it.
    fn landing(&self) -> Landing {
        Landing::new(self.point, self.rect)
    }
}

/// `params` refused as a gesture's parameters.
fn invalid_gesture(error: &gestures::GestureError) -> Error {
    Error::invalid_argument(error.to_string())
}

/// `element.tap`.
pub(crate) fn tap(driver: &mut Driver, params: TapParams) -> Result<ActOutcome> {
    let semantic = gestures::tap_action(params.button);
    let forms = Forms::input(semantic.is_some() && !is_point(&params.target));
    if let Some(answer) = answered_by_agent::<method::ElementTap>(driver, &params, forms)? {
        return Ok(answer);
    }
    let aim = aim(driver, &params.target, &params.options)?;
    let sequence = gestures::tap(aim.point, params.button, &Defaults::WINDOWLESS);
    act(driver, aim, &params.options, sequence, semantic)
}

/// `element.double_tap`.
pub(crate) fn double_tap(driver: &mut Driver, params: TapParams) -> Result<ActOutcome> {
    let forms = Forms::input(false);
    if let Some(answer) = answered_by_agent::<method::ElementDoubleTap>(driver, &params, forms)? {
        return Ok(answer);
    }
    let aim = aim(driver, &params.target, &params.options)?;
    let sequence = gestures::double_tap(aim.point, params.button, &Defaults::WINDOWLESS);
    act(driver, aim, &params.options, sequence, None)
}

/// `element.long_press`.
pub(crate) fn long_press(driver: &mut Driver, params: LongPressParams) -> Result<ActOutcome> {
    let forms = Forms::input(false);
    if let Some(answer) = answered_by_agent::<method::ElementLongPress>(driver, &params, forms)? {
        return Ok(answer);
    }
    let aim = aim(driver, &params.target, &params.options)?;
    let sequence = gestures::long_press(aim.point, params.duration_ms, &Defaults::WINDOWLESS);
    act(driver, aim, &params.options, sequence, None)
}

/// `element.drag_to`.
pub(crate) fn drag_to(driver: &mut Driver, params: DragParams) -> Result<ActOutcome> {
    let forms = Forms::input(false);
    if let Some(answer) = answered_by_agent::<method::ElementDragTo>(driver, &params, forms)? {
        return Ok(answer);
    }
    let from = aim(driver, &params.target, &params.options)?;
    let to = aim(driver, &params.to, &params.options)?;
    if from.window != to.window {
        return Err(Error::invalid_argument(format!(
            "a drag starts in {} and ends in {}: a pointer stays in one window",
            from.window, to.window
        )));
    }
    let sequence = gestures::drag(from.point, to.point, &Defaults::WINDOWLESS)
        .map_err(|error| invalid_gesture(&error))?;
    act(driver, from, &params.options, sequence, None)
}

/// `element.swipe` (`fling` false) and `element.fling`.
pub(crate) fn swipe(driver: &mut Driver, params: SwipeParams, fling: bool) -> Result<ActOutcome> {
    let forms = Forms::input(false);
    let answered = if fling {
        answered_by_agent::<method::ElementFling>(driver, &params, forms)?
    } else {
        answered_by_agent::<method::ElementSwipe>(driver, &params, forms)?
    };
    if let Some(answer) = answered {
        return Ok(answer);
    }
    let aim = aim(driver, &params.target, &params.options)?;
    let sequence = gestures::swipe(
        aim.landing(),
        params.direction,
        params.distance,
        params.velocity,
        fling,
        &Defaults::WINDOWLESS,
    )
    .map_err(|error| invalid_gesture(&error))?;
    act(driver, aim, &params.options, sequence, None)
}

/// `element.pinch`.
pub(crate) fn pinch(driver: &mut Driver, params: PinchParams) -> Result<ActOutcome> {
    let forms = Forms::input(false);
    if let Some(answer) = answered_by_agent::<method::ElementPinch>(driver, &params, forms)? {
        return Ok(answer);
    }
    let aim = aim(driver, &params.target, &params.options)?;
    let sequence =
        gestures::pinch(aim.landing(), params.scale).map_err(|error| invalid_gesture(&error))?;
    act(driver, aim, &params.options, sequence, None)
}

/// `element.scroll_into_view`: the semantics action that scrolls each scrollable ancestor; the
/// element need not be actionable, since it may be out of view. It has no other path, so a call
/// that requires one is refused by [`choose_path`], whoever plays it.
pub(crate) fn scroll_into_view(
    driver: &mut Driver,
    params: ScrollIntoViewParams,
) -> Result<ActOutcome> {
    let forms = Forms::SEMANTIC;
    if let Some(answer) =
        answered_by_agent::<method::ElementScrollIntoView>(driver, &params, forms)?
    {
        return Ok(answer);
    }
    choose_path(driver, params.options.via, forms, Player::Driver)?;
    let timeout = timeout_of(driver, &params.options);
    let element = resolve(driver, &params.target, timeout)?;
    let mut outcome = driver.agent::<method::SemanticsAct>(
        ActParams::new(element, ActionName::ScrollIntoView).snapshot(params.options.snapshot),
        Duration::ZERO,
    )?;
    outcome.element = Some(element);
    Ok(outcome)
}

/// `element.type_text`: tap the field to focus it, then commit the text.
pub(crate) fn type_text(driver: &mut Driver, params: TypeTextParams) -> Result<ActOutcome> {
    // The focus has a semantic form; the text has none, and the agent refuses a call that
    // requires the semantic path for it, as the composition below does.
    let forms = Forms::input(!is_point(&params.target));
    if let Some(answer) = answered_by_agent::<method::ElementTypeText>(driver, &params, forms)? {
        return Ok(answer);
    }
    let aim = aim(driver, &params.target, &params.options)?;
    focus(driver, aim, &params.options)?;
    act(
        driver,
        aim,
        &params.options,
        gestures::text(&params.text),
        None,
    )
}

/// Tap `aim` to focus it, reading nothing back.
fn focus(driver: &mut Driver, aim: Aim, options: &GestureOptions) -> Result<ActOutcome> {
    let mut quiet = *options;
    quiet.snapshot = Snapshot::None;
    let sequence = gestures::tap(aim.point, PointerButton::Primary, &Defaults::WINDOWLESS);
    act(driver, aim, &quiet, sequence, Some(ActionName::Focus))
}

/// `element.press_keys`: focus the target when there is one, then each chord in turn.
pub(crate) fn press_keys(driver: &mut Driver, params: PressKeysParams) -> Result<ActOutcome> {
    let forms = Forms::input(
        params
            .target
            .as_ref()
            .is_some_and(|target| !is_point(target)),
    );
    if let Some(answer) = answered_by_agent::<method::ElementPressKeys>(driver, &params, forms)? {
        return Ok(answer);
    }
    let sequence = gestures::chords(&params.keys).map_err(|error| invalid_gesture(&error))?;
    let aim = if let Some(target) = &params.target {
        let aim = aim(driver, target, &params.options)?;
        focus(driver, aim, &params.options)?;
        aim
    } else {
        let window = match params.window {
            Some(window) => window,
            None => main_window(driver)?,
        };
        // Keys go to whatever holds focus in the window: no point is aimed at.
        Aim {
            window,
            element: None,
            point: LogicalPoint::new(0.0, 0.0),
            rect: None,
        }
    };
    act(driver, aim, &params.options, sequence, None)
}

/// `element.scroll_until`: played by the app's agent alone, a step at a frame, so it takes the path
/// [`choose_path`] chooses among the synthetic one it has; the driver composes none of it.
pub(crate) fn scroll_until(driver: &mut Driver, params: ScrollUntilParams) -> Result<ActOutcome> {
    let name = method::ElementScrollUntil::NAME;
    answered_by_agent::<method::ElementScrollUntil>(driver, &params, Forms::AGENT_INPUT)?
        .ok_or_else(|| {
            Error::not_supported(name, format!("the app's agent does not serve `{name}`"))
        })
}

/// `input.perform`: an action sequence by the path the policy chooses; an action sequence has no
/// semantic form.
pub(crate) fn perform(driver: &mut Driver, params: InputParams) -> Result<ActOutcome> {
    match choose_path(driver, params.via, Forms::input(false), Player::Driver)? {
        InputPath::Os => perform_os(driver, params.window, params.actions),
        _ => inject(driver, params.window, params.actions, params.snapshot),
    }
}
