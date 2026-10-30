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

    fn create_state(&self) -> Self::State {
        GaugeState {
            level: Rc::clone(&self.level),
        }
    }
}

impl View for Gauge {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Gauge> for GaugeState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let battery = cx.capability::<Battery>().expect("the plugin provides it");
        self.level.set(Some(battery.level));
    }

    fn build(&self, _view: &Gauge, _cx: &dyn BuildContext) -> impl IntoView {
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
