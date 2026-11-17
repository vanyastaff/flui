//! A capability a package declares, provides and acquires against the SDK
//! alone (ADR-0152 §4): its marker, its plugin and a widget that acquires it
//! in `init_state`. The headless host stands in for the application that
//! registers the plugin.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_sdk::platform::capability::{Capability, CapabilityRegistrar, Plugin, ProviderContext};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{LifecycleContext, LifecycleContextExt};
use flui_sdk::widgets::SizedBox;
use flui_testing::{HeadlessHost, HeadlessWindow};

/// The package's capability: the battery level, as a percentage.
enum Battery {}

#[derive(Clone)]
struct BatteryHandle {
    level: u8,
}

impl Capability for Battery {
    type Handle = BatteryHandle;
    const NAME: &'static str = "battery";
}

/// The package's plugin: a battery that is always 80% full.
struct BatteryPlugin;

impl Plugin for BatteryPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<Battery>(|_: &ProviderContext<'_>| Ok(BatteryHandle { level: 80 }));
    }
}

#[derive(Clone)]
struct Gauge {
    level: Rc<Cell<Option<u8>>>,
}

struct GaugeState {
    level: Rc<Cell<Option<u8>>>,
}

impl StatefulView for Gauge {
    type State = GaugeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        GaugeState {
            level: Rc::clone(&self.level),
        }
    }
}

impl View for Gauge {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Gauge> for GaugeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let battery = ctx.capability::<Battery>().expect("the plugin provides it");
        self.level.set(Some(battery.level));
    }

    fn build(&self, _view: &Gauge, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

#[test]
fn a_package_declares_registers_and_acquires_its_own_capability() {
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), &BatteryPlugin);
    let level = Rc::new(Cell::new(None));
    host.attach(&Gauge {
        level: Rc::clone(&level),
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(level.get(), Some(80));
}

/// A package's Android picker, written against the SDK alone: what a
/// provider names to start an activity for a result and read its answer.
mod picker {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_sdk::platform::capability::{
        Activity, ActivityResult, Capability, CompletionSource, DataRefused, IntentFrame,
        InvalidResultKey, JniFrame, ProviderContext, ResultCode, ResultIntent, ResultKey,
        ResultOutcome, ResultRefused, ResultRequest, ResultTicket, ResultToken,
    };
    use flui_sdk::platform::{Unsupported, UnsupportedReason};

    /// The picker's one purpose, a constant of the package.
    pub(super) const PICK: ResultKey = match ResultKey::try_new("pick") {
        Ok(key) => key,
        Err(_) => panic!("BUG: a printable key"),
    };

    /// What a pick ended with, as the package's widget hears it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum Picked {
        /// The picker answered; `read` says whether its `Intent` was read.
        Answered { code: ResultCode, read: bool },
        /// No answer, and why.
        Refused(ResultRefused),
    }

    pub(super) enum Picker {}

    #[derive(Clone)]
    pub(super) struct PickerHandle {
        activity: Activity,
        completions: CompletionSource,
        picked: Rc<RefCell<Vec<Picked>>>,
    }

    impl Capability for Picker {
        type Handle = PickerHandle;
        const NAME: &'static str = "picker";
    }

    /// The provider: the Activity the context offers, which only Android's
    /// host does.
    pub(super) fn provide(ctx: &ProviderContext<'_>) -> Result<PickerHandle, Unsupported> {
        let activity = ctx.activity().ok_or(Unsupported::of::<Picker>(
            UnsupportedReason::NotOnThisPlatform,
        ))?;
        Ok(PickerHandle {
            activity,
            completions: ctx.completions().clone(),
            picked: Rc::default(),
        })
    }

    impl PickerHandle {
        /// Start a pick; its outcome lands in what the handle has picked.
        pub(super) fn pick(&self) -> Result<ResultToken, ResultRefused> {
            let request: ResultRequest = self.activity.launch_for_result(
                &self.completions,
                PICK,
                |frame: IntentFrame| {
                    // The package's own `jni` fills the Intent here.
                    !frame.intent().is_null()
                },
            )?;
            let (ticket, completion): (ResultTicket, _) = request.into_parts();
            let token: ResultToken = ticket.token();
            let (activity, picked) = (self.activity.clone(), Rc::clone(&self.picked));
            // The ticket lives with the closure: the pick waits while the
            // completion does, and ends when the widget's runtime drops it.
            completion.on_complete(move |outcome| {
                let _waiting = &ticket;
                let outcome: ResultOutcome = outcome.unwrap_or(Err(ResultRefused::ActivityEnded));
                picked.borrow_mut().push(match outcome {
                    Ok(answer) => {
                        let mut answer: ActivityResult = answer;
                        let data: Option<ResultIntent> = answer.take_data();
                        let read = data
                            .and_then(|data| {
                                let taken: Result<bool, DataRefused> = activity
                                    .take_data(data, |frame: JniFrame| !frame.object().is_null());
                                taken.ok()
                            })
                            .unwrap_or(false);
                        Picked::Answered {
                            code: answer.code(),
                            read,
                        }
                    }
                    Err(refused) => Picked::Refused(refused),
                });
            });
            Ok(token)
        }
    }

    /// A key that is not one names its reason.
    pub(super) fn bad_key() -> Option<InvalidResultKey> {
        ResultKey::try_new("").err()
    }
}

/// What acquiring the picker answered.
type PickerAcquired = Rc<std::cell::RefCell<Option<Result<(), flui_sdk::platform::Unsupported>>>>;

/// The picker's handle, acquired in `init_state`.
#[derive(Clone)]
struct PickerView {
    acquired: PickerAcquired,
}

struct PickerViewState {
    acquired: PickerAcquired,
}

impl StatefulView for PickerView {
    type State = PickerViewState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        PickerViewState {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

impl View for PickerView {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl ViewState<PickerView> for PickerViewState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let acquired = ctx.capability::<picker::Picker>().map(|handle| {
            // On Android the widget would pick from a callback.
            let _ = handle.pick();
        });
        self.acquired.replace(Some(acquired));
    }

    fn build(&self, _view: &PickerView, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

struct PickerPlugin;

impl Plugin for PickerPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<picker::Picker>(picker::provide);
    }
}

/// A package names everything a provider of activity results writes through
/// the SDK alone; on a host without an Activity its provider refuses.
#[test]
fn a_package_provides_activity_results_against_the_sdk_alone() {
    assert_eq!(picker::PICK.as_str(), "pick");
    assert!(picker::bad_key().is_some());
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), &PickerPlugin);
    let acquired = PickerAcquired::default();
    host.attach(&PickerView {
        acquired: Rc::clone(&acquired),
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    let refusal = acquired
        .borrow_mut()
        .take()
        .expect("init_state ran")
        .expect_err("the headless host has no Activity");
    assert_eq!(
        refusal.reason,
        flui_sdk::platform::UnsupportedReason::NotOnThisPlatform
    );
}
