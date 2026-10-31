//! Integration tests of the system services, through the public API on a
//! headless host.

mod dialogs;
mod launcher;
#[cfg(target_os = "linux")]
mod portal;

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::Unsupported;
use flui_platform::capability::{Capability, Plugin};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{LifecycleContext, LifecycleContextExt};
use flui_sdk::widgets::SizedBox;
use flui_testing::{HeadlessHost, HeadlessWindow};

/// What a widget acquired in `init_state`.
type Acquired<C> = Rc<RefCell<Option<Result<<C as Capability>::Handle, Unsupported>>>>;

/// A widget that acquires `C` in `init_state` and hands it out.
struct Acquire<C: Capability> {
    slot: Acquired<C>,
    capability: PhantomData<fn() -> C>,
}

impl<C: Capability> Clone for Acquire<C> {
    fn clone(&self) -> Self {
        Self {
            slot: Rc::clone(&self.slot),
            capability: PhantomData,
        }
    }
}

struct AcquireState<C: Capability> {
    slot: Acquired<C>,
}

impl<C: Capability> StatefulView for Acquire<C> {
    type State = AcquireState<C>;

    fn create_state(&self) -> Self::State {
        AcquireState {
            slot: Rc::clone(&self.slot),
        }
    }
}

impl<C: Capability> View for Acquire<C> {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl<C: Capability> ViewState<Acquire<C>> for AcquireState<C> {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        *self.slot.borrow_mut() = Some(cx.capability::<C>());
    }

    fn build(&self, _view: &Acquire<C>, _cx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// A headless host whose widgets acquire what `plugin` registers, and what
/// a widget in its window acquired of `C`.
fn acquire<C: Capability>(plugin: &dyn Plugin) -> (HeadlessHost, Result<C::Handle, Unsupported>) {
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), plugin);
    let slot: Acquired<C> = Rc::new(RefCell::new(None));
    host.attach(&Acquire::<C> {
        slot: Rc::clone(&slot),
        capability: PhantomData,
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    let acquired = slot
        .borrow_mut()
        .take()
        .expect("the widget acquired in init_state");
    (host, acquired)
}

/// Run the owner's next turn, where completions settle.
fn turn(host: &mut HeadlessHost) {
    let _ = host.pump(Duration::ZERO);
}

/// A slot a completion's outcome lands in.
fn outcome_slot<T>() -> Rc<RefCell<Option<T>>> {
    Rc::new(RefCell::new(None))
}
