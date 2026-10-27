//! The window an element is presented in (ADR-0237): `LifecycleContext::window`
//! hands a lifecycle hook a handle that reads the window's state, queues
//! commands through the host's control and goes closed with the window; the
//! host's system notices reach `LifecycleHandle::subscribe_notices` once, in
//! order, and are not replayed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_objects::RenderSizedBox;
use flui_platform::{
    HostNotice, WindowCapabilities, WindowCommand, WindowCommandError, WindowControl,
    WindowGeometry, WindowMode, WindowState,
};
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::__runtime::{LifecycleSource, WindowSource};
use flui_view::prelude::*;
use flui_view::{WindowClosed, WindowHandle};

const FRAME: Duration = Duration::from_millis(16);

/// A control that records the commands it was handed.
#[derive(Clone, Default)]
struct StubControl {
    submitted: Rc<RefCell<Vec<WindowCommand>>>,
    closes: Rc<Cell<usize>>,
}

impl WindowControl for StubControl {
    fn state(&self) -> WindowState {
        let size = Size::new(320.0, 240.0);
        WindowState::new(WindowGeometry::new(None, size), size)
            .with_title(Some("Stub".to_owned()))
            .with_capabilities(WindowCapabilities::ALL)
    }
    fn submit(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        command.admit(&self.state().capabilities)?;
        self.submitted.borrow_mut().push(command);
        Ok(())
    }
    fn request_close(&self) {
        self.closes.set(self.closes.get() + 1);
    }
}

#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }
    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// A view whose `init_state` records what `LifecycleContext::window` answered.
#[derive(Clone, StatefulView)]
struct Asker {
    taken: Rc<RefCell<Vec<Option<WindowHandle>>>>,
}

struct AskerState {
    taken: Rc<RefCell<Vec<Option<WindowHandle>>>>,
}

impl StatefulView for Asker {
    type State = AskerState;
    fn create_state(&self) -> Self::State {
        AskerState {
            taken: Rc::clone(&self.taken),
        }
    }
}

impl ViewState<Asker> for AskerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.taken.borrow_mut().push(ctx.window());
    }

    fn build(&self, _view: &Asker, _ctx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

fn mount(control: Option<StubControl>) -> (Option<WindowSource>, Option<WindowHandle>) {
    let taken = Rc::new(RefCell::new(Vec::new()));
    let mut owners = MountOwners::fresh();
    let source = control.map(|control| {
        let source = WindowSource::new(control);
        owners.build_owner.set_window_handle(source.handle());
        source
    });
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &Asker {
            taken: Rc::clone(&taken),
        },
        owners,
        MountOptions::tight(100.0, 100.0),
    );
    binding.pump_frame(FRAME);
    let handle = taken.borrow().first().cloned().expect("init_state ran");
    (source, handle)
}

#[test]
fn a_lifecycle_hook_receives_the_window_it_is_presented_in() {
    let control = StubControl::default();
    let (_source, handle) = mount(Some(control));
    let handle = handle.expect("a presentation with a window hands it to init_state");
    let state = handle.state().expect("open");
    assert_eq!(state.title.as_deref(), Some("Stub"));
    assert_eq!(state.content_size, Size::new(320.0, 240.0));
    assert_eq!(
        handle.state().map(|state| state.capabilities),
        Ok(WindowCapabilities::ALL)
    );
}

#[test]
fn an_owner_without_a_window_hands_none() {
    let (_source, handle) = mount(None);
    assert!(handle.is_none());
}

#[test]
fn commands_reach_the_control_checked_and_in_order() {
    let control = StubControl::default();
    let submitted = Rc::clone(&control.submitted);
    let (_source, handle) = mount(Some(control));
    let handle = handle.expect("window");
    handle.set_title("A").expect("accepted");
    handle.set_mode(WindowMode::Maximized).expect("accepted");
    assert_eq!(
        handle.set_size(Size::new(0.0, 10.0)),
        Err(WindowCommandError::InvalidGeometry),
        "a size that describes no window is refused before the control"
    );
    handle.hide().expect("accepted");
    assert_eq!(
        *submitted.borrow(),
        [
            WindowCommand::SetTitle("A".into()),
            WindowCommand::SetMode(WindowMode::Maximized),
            WindowCommand::Hide
        ]
    );
}

#[test]
fn closing_asks_the_control_and_does_not_close_the_handle() {
    let control = StubControl::default();
    let closes = Rc::clone(&control.closes);
    let (_source, handle) = mount(Some(control));
    let handle = handle.expect("window");
    handle.request_close().expect("accepted");
    assert_eq!(closes.get(), 1);
    assert!(handle.state().is_ok(), "a vetoed close leaves the window");
}

#[test]
fn the_handle_goes_closed_with_its_window_and_does_not_keep_it_alive() {
    let control = StubControl::default();
    let (source, handle) = mount(Some(control));
    let handle = handle.expect("window");
    let seen = Rc::new(Cell::new(0));
    let counted = Rc::clone(&seen);
    let (_, subscription) = handle
        .subscribe(move |_| counted.set(counted.get() + 1))
        .expect("open");

    let source = source.expect("source");
    source.close();
    assert_eq!(handle.state(), Err(WindowClosed));
    assert_eq!(handle.set_title("late"), Err(WindowCommandError::Closed));
    drop(subscription);

    let (source, handle) = mount(Some(StubControl::default()));
    let handle = handle.expect("window");
    drop(source);
    assert_eq!(
        handle.state(),
        Err(WindowClosed),
        "a handle outliving its host answers closed"
    );
}

/// A control whose first read of the state is overtaken: while it answers, the
/// host publishes a newer state, as an owner turn that interleaves with a
/// subscription would.
struct Overtaken {
    host: Rc<RefCell<Option<Rc<WindowSource>>>>,
    reads: Cell<usize>,
}

impl WindowControl for Overtaken {
    fn state(&self) -> WindowState {
        let size = Size::new(320.0, 240.0);
        let first = WindowState::new(WindowGeometry::new(None, size), size);
        if self.reads.replace(self.reads.get() + 1) == 0 {
            let newer = first
                .successor(first.clone().with_title(Some("newer".to_owned())))
                .expect("a change publishes");
            let host = self.host.borrow().clone().expect("the host is wired");
            host.publish(newer);
            host.drain();
        }
        first
    }
    fn submit(&self, _: WindowCommand) -> Result<(), WindowCommandError> {
        Ok(())
    }
    fn request_close(&self) {}
}

#[test]
fn a_state_published_while_subscribing_is_not_lost() {
    let host = Rc::new(RefCell::new(None));
    let source = Rc::new(WindowSource::new(Overtaken {
        host: Rc::clone(&host),
        reads: Cell::new(0),
    }));
    *host.borrow_mut() = Some(Rc::clone(&source));
    let handle = source.handle();

    let heard = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&heard);
    let (returned, _subscription) = handle
        .subscribe(move |state| seen.borrow_mut().push(state.revision.get()))
        .expect("open");

    let newest_known = heard
        .borrow()
        .last()
        .copied()
        .unwrap_or_else(|| returned.revision.get());
    assert_eq!(
        newest_known,
        1,
        "the subscriber is told of the state published during the subscription \
         (heard {:?}, returned revision {})",
        heard.borrow(),
        returned.revision.get()
    );
    host.borrow_mut().take();
}

#[test]
fn notices_arrive_once_in_order_and_are_not_replayed() {
    let source = LifecycleSource::new();
    let handle = source.handle();
    source.commit_notice(HostNotice::Sleep).expect("open");
    source.drain();

    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&log);
    let token = handle
        .subscribe_notices(move |notice| {
            seen.borrow_mut().push(match notice {
                HostNotice::Wake => "wake",
                HostNotice::SessionLocked => "locked",
                _ => "other",
            });
        })
        .expect("open");
    assert!(log.borrow().is_empty(), "a late subscriber is not replayed");
    source
        .commit_notice(HostNotice::SessionLocked)
        .expect("open");
    source.commit_notice(HostNotice::Wake).expect("open");
    assert!(log.borrow().is_empty(), "committing calls nothing");
    source.drain();
    assert_eq!(*log.borrow(), ["locked", "wake"]);

    drop(token);
    source.commit_notice(HostNotice::Wake).expect("open");
    source.drain();
    assert_eq!(
        log.borrow().len(),
        2,
        "a dropped token cancels the callback"
    );

    source.begin_close();
    assert!(handle.subscribe_notices(|_| {}).is_err());
    assert!(source.commit_notice(HostNotice::Wake).is_err());
}
