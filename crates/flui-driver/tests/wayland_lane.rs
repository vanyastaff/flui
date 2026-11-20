//! The Wayland lane: its mapping tables on every host, and on Linux its libei client against an
//! EIS server the test plays (`reis`'s server half), with the refusals of a session that is not
//! isolated and of a compositor that does not answer.

use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::wayland::mapping::{self, EiRegion};
use flui_driver::protocol::error::ErrorCode;
use flui_driver::protocol::input::PointerButton;
use flui_driver::protocol::session::Isolation;

#[test]
fn a_device_point_lands_in_the_compositors_logical_space_or_is_refused() {
    let regions = [
        EiRegion {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
        EiRegion {
            x: 800,
            y: 0,
            width: 1024,
            height: 768,
        },
    ];
    assert_eq!(
        mapping::ei_point(DevicePoint::new(100, 50), 1.0, &regions),
        Some((100.0, 50.0))
    );
    assert_eq!(
        mapping::ei_point(DevicePoint::new(200, 100), 2.0, &regions),
        Some((100.0, 50.0)),
        "device pixels over the scale"
    );
    assert_eq!(
        mapping::ei_point(DevicePoint::new(900, 700), 1.0, &regions),
        Some((900.0, 700.0)),
        "the second output"
    );
    for (point, scale) in [
        (DevicePoint::new(-1, 0), 1.0),
        (DevicePoint::new(799, 600), 1.0),
        (DevicePoint::new(1824, 0), 1.0),
        (DevicePoint::new(10, 10), 0.0),
        (DevicePoint::new(10, 10), f64::NAN),
    ] {
        assert_eq!(
            mapping::ei_point(point, scale, &regions),
            None,
            "{point:?} at {scale}"
        );
    }
    assert_eq!(mapping::ei_point(DevicePoint::new(0, 0), 1.0, &[]), None);
}

#[test]
fn a_button_is_its_evdev_code_and_a_scroll_its_logical_distance() {
    assert_eq!(mapping::button_code(PointerButton::Primary), Some(0x110));
    assert_eq!(mapping::button_code(PointerButton::Secondary), Some(0x111));
    assert_eq!(mapping::button_code(PointerButton::Middle), Some(0x112));
    assert_eq!(mapping::button_code(PointerButton::Back), Some(0x113));
    assert_eq!(mapping::button_code(PointerButton::Forward), Some(0x114));
    assert_eq!(mapping::scroll_distance(40, 2.0), 20.0);
    assert_eq!(mapping::scroll_distance(-30, 1.5), -20.0);
    assert_eq!(mapping::scroll_distance(30, 0.0), 0.0);
}

#[test]
fn the_lane_sends_only_into_a_compositor_of_its_own_and_reads_a_shared_session() {
    assert!(mapping::admit_isolation(Isolation::Seat).is_ok());
    assert!(mapping::admit_isolation(Isolation::Shared).is_ok());
    for isolation in [Isolation::Xvfb, Isolation::Sandbox, Isolation::InProcess] {
        let refused = mapping::admit_isolation(isolation).expect_err("refused");
        assert_eq!(refused.code(), Some(ErrorCode::InvalidArgument));
    }
    // The input of a lane with no guard on the shared session is refused at the seam.
    let refused = flui_driver::os_input::admit(Isolation::Shared, false).expect_err("refused");
    assert!(
        matches!(&refused, flui_driver::Error::NotSupported { capability: Some(capability), .. }
            if capability == "user_activity_guard"),
        "{refused:?}"
    );
}

#[test]
fn a_portal_that_refuses_or_ends_cannot_be_verified() {
    assert!(mapping::portal_response(0, "start").is_ok());
    for response in [1, 2, 7] {
        let refused = mapping::portal_response(response, "start").expect_err("refused");
        assert!(refused.is_cannot_verify(), "{refused:?}");
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some("wayland_lane")
        );
    }
}

#[cfg(target_os = "linux")]
mod eis {
    //! The lane's libei client against an EIS server of the test's own.

    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use enumflags2::BitFlags;
    use flui_driver::Error;
    use flui_driver::coordinates::DevicePoint;
    use flui_driver::lane::wayland::{EisSource, WaylandLane};
    use flui_driver::lane::{self, Lane};
    use flui_driver::protocol::input::{PointerButton, PointerDevice};
    use flui_driver::protocol::session::Isolation;
    use flui_driver::steps::OsEvent;
    use reis::PendingRequestResult;
    use reis::eis;
    use reis::handshake::EisHandshaker;
    use reis::request::{DeviceCapability, EisRequest, EisRequestConverter};

    /// What the server heard, in order.
    #[derive(Debug, Clone, PartialEq)]
    enum Heard {
        Motion(f32, f32),
        Button(u32, bool),
        Scroll(f32, f32),
        Keysym(u32, bool),
        Text(String),
        Frame,
    }

    /// Serve one client on `listener`: a seat with an absolute pointer, buttons, scroll and text,
    /// a device of what the client binds covering 1280 by 1024 logical pixels, resumed; every
    /// event it sends is posted, until it leaves or `within` runs out.
    fn serve(listener: &UnixListener, heard: &mpsc::Sender<Heard>, within: Duration) {
        let deadline = Instant::now() + within;
        let (stream, _) = listener.accept().expect("the lane connects");
        let context = eis::Context::new(stream).expect("an EIS context");
        let mut handshaker = EisHandshaker::new(&context, 1);
        // Whether the client is still there, after reading what it sent within 20 ms.
        let read = |context: &eis::Context| -> bool {
            use std::os::fd::AsFd as _;
            let fd = context.as_fd();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let wait = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 20_000_000,
            };
            match rustix::event::poll(&mut fds, Some(&wait)) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
            match context.read() {
                Ok(0) => false,
                Ok(_) => true,
                Err(error) => error.kind() == std::io::ErrorKind::WouldBlock,
            }
        };
        let handshake = 'handshake: loop {
            assert!(Instant::now() < deadline, "the handshake ended in time");
            if !read(&context) {
                return;
            }
            while let Some(PendingRequestResult::Request(request)) = context.pending_request() {
                if let Some(done) = handshaker.handle_request(request).expect("a handshake") {
                    break 'handshake done;
                }
            }
        };
        let mut converter = EisRequestConverter::new(&context, handshake, 2);
        let _seat = converter.handle().add_seat(
            Some("seat"),
            DeviceCapability::PointerAbsolute
                | DeviceCapability::Button
                | DeviceCapability::Scroll
                | DeviceCapability::Text,
        );
        let _ = context.flush();
        while Instant::now() < deadline {
            if !read(&context) {
                return;
            }
            while let Some(PendingRequestResult::Request(request)) = context.pending_request() {
                if converter.handle_request(request).is_err() {
                    return;
                }
            }
            while let Some(request) = converter.next_request() {
                let post = match request {
                    EisRequest::Bind(bind) => {
                        let capabilities: BitFlags<DeviceCapability> = bind.capabilities;
                        let device = bind.seat.add_device(
                            Some("virtual"),
                            eis::device::DeviceType::Virtual,
                            capabilities,
                            |device| device.device().region(0, 0, 1280, 1024, 1.0),
                        );
                        // The converter keeps the device; the handle only resumes it.
                        device.resumed();
                        None
                    }
                    EisRequest::PointerMotionAbsolute(motion) => {
                        Some(Heard::Motion(motion.dx_absolute, motion.dy_absolute))
                    }
                    EisRequest::Button(button) => Some(Heard::Button(
                        button.button,
                        button.state == eis::button::ButtonState::Press,
                    )),
                    EisRequest::ScrollDelta(scroll) => Some(Heard::Scroll(scroll.dx, scroll.dy)),
                    EisRequest::TextKeysym(key) => Some(Heard::Keysym(
                        key.keysym,
                        key.state == eis::keyboard::KeyState::Press,
                    )),
                    EisRequest::TextUtf8(text) => Some(Heard::Text(text.text)),
                    EisRequest::Frame(_) => Some(Heard::Frame),
                    EisRequest::Disconnect => return,
                    _ => None,
                };
                if let Some(post) = post
                    && heard.send(post).is_err()
                {
                    return;
                }
            }
            let _ = context.flush();
        }
    }

    fn pointer(event: impl FnOnce(String, PointerDevice, PointerButton) -> OsEvent) -> OsEvent {
        event(
            "mouse".to_owned(),
            PointerDevice::Mouse,
            PointerButton::Primary,
        )
    }

    /// The lane's events reach a compositor's EIS server as EI events, each in a frame of its own:
    /// an absolute motion at the point's logical place, the primary button by its evdev code, a
    /// scroll, keys as keysyms and text as UTF-8; and what it held is released after.
    #[test]
    fn the_lanes_events_reach_the_compositor_as_ei_events() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let socket = dir.path().join("eis-0");
        let listener = UnixListener::bind(&socket).expect("the EIS socket");
        let (post, heard) = mpsc::channel();
        let server = std::thread::spawn(move || serve(&listener, &post, Duration::from_secs(10)));
        let mut wayland = WaylandLane::try_new(
            std::process::id(),
            EisSource::Socket(socket),
            Isolation::Seat,
        )
        .expect("a compositor of the run's own");
        let events = [
            OsEvent::PointerMove {
                pointer: "mouse".to_owned(),
                device: PointerDevice::Mouse,
                to: DevicePoint::new(100, 50),
            },
            pointer(|pointer, device, button| OsEvent::PointerDown {
                pointer,
                device,
                button,
            }),
            OsEvent::Scroll {
                at: DevicePoint::new(100, 50),
                delta_x: 0,
                delta_y: 40,
            },
            OsEvent::KeyDown("a".to_owned()),
            OsEvent::KeyUp("a".to_owned()),
            OsEvent::KeyDown("Shift".to_owned()),
            OsEvent::Text("hé".to_owned()),
        ];
        lane::send_guarded(&mut wayland, &events).expect("the events went out");
        drop(wayland);
        server.join().expect("the server served");
        let heard: Vec<Heard> = heard
            .try_iter()
            .filter(|heard| *heard != Heard::Frame)
            .collect();
        assert_eq!(
            heard,
            vec![
                Heard::Motion(100.0, 50.0),
                Heard::Button(0x110, true),
                Heard::Motion(100.0, 50.0),
                Heard::Scroll(0.0, 40.0),
                Heard::Keysym(0x61, true),
                Heard::Keysym(0x61, false),
                Heard::Keysym(0xFFE1, true),
                Heard::Text("hé".to_owned()),
                // Released on the way out: the key held, then the button.
                Heard::Keysym(0xFFE1, false),
                Heard::Button(0x110, false),
            ]
        );
    }

    /// A lane with no guard on the user's own session sends nothing: its input is refused at the
    /// seam, before it connects.
    #[test]
    fn the_shared_session_takes_no_input_from_the_lane() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let socket = dir.path().join("eis-0");
        let listener = UnixListener::bind(&socket).expect("the EIS socket");
        listener.set_nonblocking(true).expect("non-blocking");
        let mut wayland = WaylandLane::try_new(
            std::process::id(),
            EisSource::Socket(socket),
            Isolation::Shared,
        )
        .expect("a lane that reads the shared session");
        assert!(wayland.guard().is_none());
        let refused = lane::send_guarded(&mut wayland, &[OsEvent::Text("x".into())]);
        assert!(
            matches!(&refused, Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "user_activity_guard"),
            "{refused:?}"
        );
        assert!(
            listener.accept().is_err(),
            "the lane never connected to the compositor"
        );
    }

    /// A compositor that does not answer is a lane the host cannot run, never a run that passed.
    #[test]
    fn a_compositor_that_does_not_answer_cannot_be_verified() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut wayland = WaylandLane::try_new(
            std::process::id(),
            EisSource::Socket(dir.path().join("nobody")),
            Isolation::Seat,
        )
        .expect("a compositor of the run's own");
        let refused = lane::send_guarded(&mut wayland, &[OsEvent::Text("x".into())])
            .expect_err("no compositor");
        assert!(refused.is_cannot_verify(), "{refused:?}");
    }
}
