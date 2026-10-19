//! The X11 adapter against a real X server.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`. The test
//! skips without it, and also skips on any server that is not a bare Xvfb
//! screen: it turns the screen off and on again, which nobody wants done to
//! their desktop.

use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::Duration;

use rustix::event::{EventfdFlags, eventfd};
use x11rb::protocol::randr::{self, Rotation};
use x11rb::rust_connection::RustConnection;

use super::*;

/// A watchdog that ends a wait that never returns: it makes `step` report
/// `Stopped`, which the test then fails on.
fn watchdog(stop: &Arc<rustix::fd::OwnedFd>, after: Duration) {
    let stop = Arc::clone(stop);
    std::thread::spawn(move || {
        std::thread::sleep(after);
        let _ = rustix::io::write(stop.as_fd(), &1_u64.to_ne_bytes());
    });
}

/// Step until the session reports a change, failing if it stops first.
fn until_changed(session: &mut X11Session, stop: &rustix::fd::OwnedFd) {
    loop {
        match session
            .step(stop.as_fd())
            .expect("the session keeps running")
        {
            Step::Changed => return,
            Step::Idle => {}
            Step::Stopped => panic!("no change was published within the time allowed"),
        }
    }
}

/// A refusal from the server (an output unplugged between the request that
/// named it and its reply) is a read that met a configuration in flux; only a
/// failed connection ends the session.
#[test]
fn a_refused_request_is_a_state_in_flux_not_a_dead_session() {
    use x11rb::errors::{ConnectionError, ReplyError};
    use x11rb::x11_utils::X11Error;

    let refusal = X11Error {
        error_kind: x11rb::protocol::ErrorKind::Value,
        error_code: 2,
        sequence: 7,
        bad_value: 0x42,
        minor_opcode: 9,
        major_opcode: 140,
        extension_name: Some("RANDR".into()),
        request_name: Some("GetOutputInfo"),
    };
    assert!(matches!(
        settle::<u8>(Err(ReplyError::X11Error(refusal))),
        Ok(None)
    ));
    assert!(matches!(settle(Ok(3_u8)), Ok(Some(3))));
    assert!(matches!(
        settle::<u8>(Err(ReplyError::ConnectionError(
            ConnectionError::UnknownError
        ))),
        Err(DisplayError::Connection { .. })
    ));
}

#[test]
fn a_randr_reconfiguration_on_a_real_server_is_published() {
    if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
        eprintln!("skipped: no X server (DISPLAY is not set); CI runs this under xvfb-run");
        return;
    }
    let mut session = X11Session::connect().expect("the X server answers");
    let before = session.views();
    if before.len() != 1 || before[0].name != "screen" {
        eprintln!("skipped: not a bare Xvfb screen, which this test reconfigures");
        return;
    }
    let natives = before[0].natives.clone();
    let [crtc] = natives[..] else {
        panic!("BUG: a bare Xvfb screen is driven by one CRTC");
    };

    let control = RustConnection::connect(None)
        .expect("a second connection")
        .0;
    let root = control.setup().roots[0].root;
    let configure = |mode: u32, x: i16, y: i16, outputs: &[randr::Output]| {
        let config = randr::get_screen_resources_current(&control, root)
            .expect("request")
            .reply()
            .expect("resources")
            .config_timestamp;
        randr::set_crtc_config(
            &control,
            crtc,
            x11rb::CURRENT_TIME,
            config,
            x,
            y,
            mode,
            Rotation::ROTATE0,
            outputs,
        )
        .expect("request")
        .reply()
        .expect("the server applies the configuration");
    };
    let info = randr::get_crtc_info(&control, crtc, x11rb::CURRENT_TIME)
        .expect("request")
        .reply()
        .expect("crtc info");

    let stop = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).expect("fd"));
    watchdog(&stop, Duration::from_secs(20));

    // Turning the screen off removes the display.
    configure(0, info.x, info.y, &[]);
    until_changed(&mut session, &stop);
    assert_eq!(session.views(), [], "the screen is off");

    // Turning it back on brings back the same display.
    configure(info.mode, info.x, info.y, &info.outputs);
    until_changed(&mut session, &stop);
    assert_eq!(session.views(), before);

    // Reconfigurations in quick succession: the notifies of the later ones can
    // arrive while an earlier one is being read, and must not be left unseen.
    // However the reads interleave, the session ends on the final state.
    for _ in 0..8 {
        configure(0, info.x, info.y, &[]);
        configure(info.mode, info.x, info.y, &info.outputs);
    }
    while session.views() != before {
        match session
            .step(stop.as_fd())
            .expect("the session keeps running")
        {
            Step::Stopped => panic!("the final state was never read"),
            Step::Changed | Step::Idle => {}
        }
    }

    // A change made while the machine slept sends the process no event, so the
    // snapshot taken after a wake reads the outputs from the server again
    // rather than resolving the list last held. No watcher runs here, as when
    // the display thread has not heard of the change. (Part of this test, not
    // a test of its own: two tests reconfiguring one server would race.)
    use crate::platforms::linux::display::source::LinuxDisplays;
    use crate::shared::system_notice::StaleOnWake;

    let displays = LinuxDisplays::new();
    let outputs = || displays.snapshot().expect("a snapshot").displays().len();
    assert_eq!(outputs(), 1);
    configure(0, info.x, info.y, &[]);
    assert_eq!(outputs(), 1, "nothing told the source the screen went");
    displays.mark_stale();
    assert_eq!(outputs(), 0, "the wake read the server again");
    configure(info.mode, info.x, info.y, &info.outputs);
}
