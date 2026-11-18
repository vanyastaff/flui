//! Integration tests of the system services, through the public API on a
//! headless host.

mod capability;
mod conformance;
mod dialogs;
mod launcher;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod portal_services;
mod services;
mod shell_services;

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::Unsupported;
use flui_platform::capability::{Capability, Plugin};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{LifecycleContext, LifecycleContextExt};
use flui_sdk::widgets::{Column, SizedBox};
use flui_testing::{HeadlessHost, HeadlessWindow, HeadlessWindowId};

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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        AcquireState {
            slot: Rc::clone(&self.slot),
        }
    }
}

impl<C: Capability> View for Acquire<C> {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl<C: Capability> ViewState<Acquire<C>> for AcquireState<C> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.slot.borrow_mut() = Some(ctx.capability::<C>());
    }

    fn build(&self, _view: &Acquire<C>, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// A headless host whose widgets acquire what `plugin` registers, and what
/// a widget in its window acquired of `C`.
fn acquire<C: Capability>(plugin: &dyn Plugin) -> (HeadlessHost, Result<C::Handle, Unsupported>) {
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), plugin);
    let acquired = acquire_on::<C>(&mut host);
    (host, acquired)
}

/// What a widget mounted on `host` acquired of `C`.
fn acquire_on<C: Capability>(host: &mut HeadlessHost) -> Result<C::Handle, Unsupported> {
    let slot: Acquired<C> = Rc::new(RefCell::new(None));
    host.attach(&Acquire::<C> {
        slot: Rc::clone(&slot),
        capability: PhantomData,
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    slot.borrow_mut()
        .take()
        .expect("the widget acquired in init_state")
}

/// What a widget mounted as the root of `window` on `host` acquired of `C`.
fn acquire_in<C: Capability>(
    host: &mut HeadlessHost,
    window: HeadlessWindowId,
) -> Result<C::Handle, Unsupported> {
    let slot: Acquired<C> = Rc::new(RefCell::new(None));
    host.attach_to(
        window,
        &Acquire::<C> {
            slot: Rc::clone(&slot),
            capability: PhantomData,
        },
    )
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    slot.borrow_mut()
        .take()
        .expect("the widget acquired in init_state")
}

/// What two sibling widgets mounted on `host` each acquired of `C`.
fn acquire_twice_on<C: Capability>(host: &mut HeadlessHost) -> [Result<C::Handle, Unsupported>; 2] {
    let slots: [Acquired<C>; 2] = [Rc::new(RefCell::new(None)), Rc::new(RefCell::new(None))];
    let widget = |slot: &Acquired<C>| Acquire::<C> {
        slot: Rc::clone(slot),
        capability: PhantomData,
    };
    host.attach(&Column::new((widget(&slots[0]), widget(&slots[1]))))
        .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    slots.map(|slot| {
        slot.borrow_mut()
            .take()
            .expect("each widget acquired in init_state")
    })
}

/// What a [`Within`] runs in `init_state`.
type Init = Rc<dyn Fn(&dyn LifecycleContext)>;

/// A widget whose `init_state` runs its closure: several acquisitions in
/// one widget, so in one window.
#[derive(Clone)]
struct Within {
    init: Init,
}

struct WithinState {
    init: Init,
}

impl StatefulView for Within {
    type State = WithinState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        WithinState {
            init: Rc::clone(&self.init),
        }
    }
}

impl View for Within {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Within> for WithinState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        (self.init)(ctx);
    }

    fn build(&self, _view: &Within, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// Mount a widget as the root of `window` (the primary window for `None`)
/// whose `init_state` runs `init`, and pump once.
fn within(
    host: &mut HeadlessHost,
    window: Option<HeadlessWindowId>,
    init: impl Fn(&dyn LifecycleContext) + 'static,
) {
    let widget = Within {
        init: Rc::new(init),
    };
    match window {
        Some(window) => host.attach_to(window, &widget),
        None => host.attach(&widget),
    }
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
}

/// What a widget acquires of `C` inside [`within`].
struct CapabilitySlot<C: Capability> {
    slot: Acquired<C>,
    capability: PhantomData<fn() -> C>,
}

impl<C: Capability> Clone for CapabilitySlot<C> {
    fn clone(&self) -> Self {
        Self {
            slot: Rc::clone(&self.slot),
            capability: PhantomData,
        }
    }
}

impl<C: Capability> CapabilitySlot<C> {
    fn new() -> Self {
        Self {
            slot: Rc::new(RefCell::new(None)),
            capability: PhantomData,
        }
    }

    /// Acquire `C` from `ctx`.
    fn fill(&self, ctx: &dyn LifecycleContext) {
        *self.slot.borrow_mut() = Some(ctx.capability::<C>());
    }

    /// What the widget acquired.
    fn take(&self) -> Result<C::Handle, Unsupported> {
        self.slot
            .borrow_mut()
            .take()
            .expect("the widget acquired in init_state")
    }
}

/// Run the owner's next turn, where completions settle.
fn turn(host: &mut HeadlessHost) {
    let _ = host.pump(Duration::ZERO);
}

/// A slot a completion's outcome lands in.
fn outcome_slot<T>() -> Rc<RefCell<Option<T>>> {
    Rc::new(RefCell::new(None))
}
