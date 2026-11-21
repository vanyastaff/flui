//! The escape hatch's way to a window (`flui_app::native::with_window`),
//! driven through the headless platform: a real installed presentation and
//! the host window the runner was handed for it. Declared in
//! `owner_dispatch.rs` through `#[path]` because the host it exercises is
//! private to the runner.

use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_native::testing::HeadlessPlatform;
use flui_native::{HostWindow, Platform as _};
use flui_platform::PlatformWindow;

use super::super::host::{
    OwnerHostClearGuard, build_ui_runtime, install_owner_platform, with_owner_platform,
};
use super::super::presentation_window;
use super::super::window_routes::OwnerOpenRouted as _;
use super::*;
use crate::native::with_window;

/// Whether `window` is `host`'s allocation.
fn is(window: &dyn HostWindow, host: &Rc<dyn HostWindow>) -> bool {
    std::ptr::addr_eq(std::ptr::from_ref(window), Rc::as_ptr(host))
}

/// `with_window` reaches the host window the presentation at its address is
/// drawn in, and only there: not for an address nothing is presented at, not
/// from another thread, and not once the presentation is gone.
#[test]
fn with_window_reaches_the_host_window_of_its_presentation_and_no_other() {
    let _clear = OwnerHostClearGuard::arm();
    Box::new(HeadlessPlatform::new())
        .run(crate::app::runner::app_handler(|owner| {
            install_owner_platform(owner)?;
            Ok(())
        }))
        .expect("headless run");
    let host =
        with_owner_platform(|owner| owner.open_routed(flui_platform::WindowOptions::default()))
            .expect("an installed owner")
            .and_then(flui_native::WindowOpen::try_ready)
            .expect("headless windows are ready");
    let window: Rc<dyn PlatformWindow> = Rc::clone(&host) as Rc<dyn PlatformWindow>;
    let runtime = build_ui_runtime(presentation_window(Rc::clone(&host)), 1.0).expect("runtime");
    let dispatcher = install_ui_runtime_alongside(runtime, &window).expect("installed");
    let address = dispatcher.address;

    assert_eq!(
        with_window(address, |reached| is(reached, &host)),
        Some(true),
        "the presentation's own window"
    );
    let elsewhere = PresentationAddress {
        presentation_id: flui_foundation::PresentationId::new(999),
        ..address
    };
    assert_ne!(elsewhere, address);
    assert_eq!(
        with_window(elsewhere, |_| ()),
        None,
        "nothing is presented there"
    );
    let off_thread = std::thread::spawn(move || with_window(address, |_| ()))
        .join()
        .expect("the other thread returns");
    assert_eq!(
        off_thread, None,
        "only the thread that runs the application"
    );

    teardown_platform_ui_runtime();
    assert_eq!(
        with_window(address, |_| ()),
        None,
        "the presentation is gone"
    );
}

/// The `NativeWindowCapability` handle a widget of a presentation acquires,
/// as its acquisition in `create_state` makes it.
fn acquire_native_window(dispatcher: PresentationDispatcher) -> crate::native::NativeWindow {
    use std::cell::RefCell;

    use flui_platform::capability::Capability as _;

    use crate::native::{NativeWindow, NativeWindowCapability};

    let acquired: Rc<RefCell<Option<NativeWindow>>> = Rc::default();
    let into = Rc::clone(&acquired);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |ui_runtime| {
            let handle = ui_runtime.widgets().with_build_owner(|owner| {
                owner
                    .capabilities()
                    .capabilities
                    .as_ref()
                    .expect("a presentation's widgets acquire capabilities")
                    .acquire(
                        std::any::TypeId::of::<NativeWindowCapability>(),
                        NativeWindowCapability::NAME,
                    )
                    .expect("the built-in provider hands a windowed widget its window")
            });
            let handle: Box<NativeWindow> = handle
                .downcast()
                .expect("the capability's handle is a NativeWindow");
            into.replace(Some(*handle));
        })),
    )
    .expect("acquire through the installed owner");
    acquired
        .borrow_mut()
        .take()
        .expect("the callback acquired the handle")
}

/// Each presentation's `NativeWindowCapability` handle reaches the host
/// window that presentation is drawn in and no other: two presentations, two
/// windows, each handle its own. A window that closed is reached by nobody,
/// and once the runtime is gone neither is.
#[test]
fn each_presentations_native_window_reaches_its_own_window() {
    let _clear = OwnerHostClearGuard::arm();
    Box::new(HeadlessPlatform::new())
        .run(crate::app::runner::app_handler(|owner| {
            install_owner_platform(owner)?;
            Ok(())
        }))
        .expect("headless run");
    let open = || {
        let host =
            with_owner_platform(|owner| owner.open_routed(flui_platform::WindowOptions::default()))
                .expect("an installed owner")
                .and_then(flui_native::WindowOpen::try_ready)
                .expect("headless windows are ready");
        let window: Rc<dyn PlatformWindow> = Rc::clone(&host) as Rc<dyn PlatformWindow>;
        let runtime =
            build_ui_runtime(presentation_window(Rc::clone(&host)), 1.0).expect("runtime");
        let dispatcher = install_ui_runtime_alongside(runtime, &window).expect("installed");
        (host, acquire_native_window(dispatcher))
    };
    let (first, first_handle) = open();
    let (second, second_handle) = open();

    for (handle, own, other, which) in [
        (&first_handle, &first, &second, "the first"),
        (&second_handle, &second, &first, "the second"),
    ] {
        assert_eq!(
            handle.with(|reached| (is(reached, own), is(reached, other))),
            Some((true, false)),
            "{which} presentation's handle reaches its own window and not the other"
        );
    }

    use flui_testing::window_commands::AdmitAndApply as _;
    first
        .admit_and_apply(flui_platform::WindowCommand::Close)
        .expect("the first window closes");
    assert_eq!(
        first_handle.with(|_| ()),
        None,
        "a closed window is reached by nobody"
    );
    assert_eq!(
        second_handle.with(|reached| is(reached, &second)),
        Some(true),
        "the other window is still reached"
    );

    teardown_platform_ui_runtime();
    assert_eq!(second_handle.with(|_| ()), None, "the runtime is gone");
}
