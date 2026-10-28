//! The escape hatch's way to a window (`flui_app::native::with_window`),
//! driven through the headless platform: a real installed presentation and
//! the host window the runner was handed for it. Declared in
//! `owner_dispatch.rs` through `#[path]` because the host it exercises is
//! private to the runner.

use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_native::{HeadlessPlatform, HostWindow, Platform as _};
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
