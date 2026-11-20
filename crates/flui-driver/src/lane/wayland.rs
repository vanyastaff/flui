//! The Wayland lane: real input into an app in a compositor of the run's own, through libei, and
//! what AT-SPI shows of it.
//!
//! - **Isolation only.** Wayland gives a client no view of input it did not send, so the lane has
//!   no user-activity guard: on the user's own session (`Shared`) it only reads, and its input is
//!   refused at the seam (`lane::admit`, `not_supported` naming `user_activity_guard`). It sends
//!   input into a nested or headless compositor (`Seat`).
//! - **Input.** libei through `reis`, from the socket the compositor names in `LIBEI_SOCKET` or the
//!   RemoteDesktop portal hands over (`EisSource`); a portal that refuses, or none on the bus,
//!   is a lane the host cannot run. An absolute pointer in the compositor's logical pixels inside
//!   the regions it covers ([`mapping`]), buttons by their evdev codes, scroll in logical pixels,
//!   and keys and text through EI's text interface, as keysyms ([`x11::keys`](super::x11::keys),
//!   the one W3C table) and UTF-8, so no keymap is read. Touch and pen are refused before any
//!   event. Positions are exact once measured on a compositor; until then replay compares their
//!   order only.
//! - **Where the window is.** A Wayland client is told nothing of where its window is: the lane
//!   maps points from the origin the compositor's owner gives it (`WaylandLane::window_origin`,
//!   a kiosk shell's fullscreen window at the origin), or answers `not_supported` naming
//!   `window_position`.
//! - **Reading.** The window's AT-SPI tree ([`atspi`](crate::lane::atspi)). The ScreenCast
//!   portal's capture comes as a PipeWire stream, and PipeWire is a C library the workspace's
//!   Linux builds do not link, so the lane captures nothing: `not_supported` naming `os_capture`
//!   (`docs/gaps.md`).
//!
//! [`mapping`] is pure and builds on every host; the rest is Linux with the `wayland` feature.

pub mod mapping;

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod libei;
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod portal;

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub use lane::{EisSource, WaylandLane};

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod lane {
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    use flui_protocol::input::{PointerButton, PointerDevice};
    use flui_protocol::session::{Isolation, Placement};
    use flui_protocol::tree::Tree;
    use reis::ei;

    use super::libei::EiSession;
    use super::mapping::{self, LANE};
    use super::portal::{self, PortalSession};
    use crate::coordinates::{DevicePoint, WindowGeometry};
    use crate::error::{Error, Result};
    use crate::lane::atspi::client::{A11yBus, WindowTarget};
    use crate::lane::screen;
    use crate::lane::x11::keys::{self, Keysym};
    use crate::lane::{AccessibilityApi, ActivityGuard, Lane, LaneCapabilities, PositionFidelity};
    use crate::steps::OsEvent;

    /// Where the lane's EIS socket comes from.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum EisSource {
        /// The socket at this path, which a compositor of the run's own serves (what it names in
        /// `LIBEI_SOCKET`).
        Socket(PathBuf),
        /// The RemoteDesktop portal on the session bus at this address (the process's own when
        /// `None`).
        Portal(Option<String>),
    }

    /// The connection and what keeps it open.
    struct Connected {
        session: EiSession,
        _portal: Option<PortalSession>,
    }

    /// The Wayland lane for the app whose process is `pid`: see the [module docs](super).
    pub struct WaylandLane {
        pid: u32,
        isolation: Isolation,
        source: EisSource,
        session_bus: Option<String>,
        origin: Option<DevicePoint>,
        scale: f64,
        connected: Option<Connected>,
        keys: Vec<Keysym>,
        buttons: Vec<u32>,
    }

    impl std::fmt::Debug for WaylandLane {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WaylandLane")
                .field("pid", &self.pid)
                .field("isolation", &self.isolation)
                .field("source", &self.source)
                .field("connected", &self.connected.is_some())
                .finish_non_exhaustive()
        }
    }

    fn unsent(what: String) -> Error {
        Error::not_supported(
            "os_input_device",
            format!("the wayland lane does not send {what}"),
        )
    }

    impl WaylandLane {
        /// A lane for the app whose process is `pid`, sending through the EIS socket `source`
        /// names, its input landing in `isolation`: see the [module docs](super).
        ///
        /// # Errors
        ///
        /// [`Error::InvalidArgument`] for an isolation mode that is no compositor's, and for the
        /// portal of the driver's own session bus in a compositor of the run's own
        /// ([`mapping::admit_portal`]). A socket or bus the caller names is taken to be the
        /// compositor it started, as an `Xvfb` display is for the X11 lane.
        pub fn try_new(pid: u32, source: EisSource, isolation: Isolation) -> Result<Self> {
            mapping::admit_isolation(isolation)?;
            mapping::admit_portal(source == EisSource::Portal(None), isolation)?;
            Ok(Self {
                pid,
                isolation,
                source,
                session_bus: None,
                origin: None,
                scale: 1.0,
                connected: None,
                keys: Vec::new(),
                buttons: Vec::new(),
            })
        }

        /// The app's window has its top-left device pixel at `origin` in the compositor's space,
        /// as the compositor's owner arranged.
        #[must_use]
        pub fn window_origin(mut self, origin: DevicePoint) -> Self {
            self.origin = Some(origin);
            self
        }

        /// Read the app through the AT-SPI bus of the session bus at `address`.
        #[must_use]
        pub fn with_session_bus(mut self, address: impl Into<String>) -> Self {
            self.session_bus = Some(address.into());
            self
        }

        fn session(&mut self) -> Result<&mut EiSession> {
            if self.connected.is_none() {
                let (socket, portal) = match &self.source {
                    EisSource::Socket(path) => (
                        UnixStream::connect(path).map_err(|error| Error::Lane {
                            lane: LANE.to_owned(),
                            unavailable: true,
                            message: format!(
                                "no EIS server answers at {}: {error}",
                                path.display()
                            ),
                        })?,
                        None,
                    ),
                    EisSource::Portal(bus) => {
                        let portal = portal::open(bus.as_deref())?;
                        let socket =
                            UnixStream::from(portal.socket.try_clone().map_err(|error| {
                                Error::Lane {
                                    lane: LANE.to_owned(),
                                    unavailable: true,
                                    message: format!("the portal's EIS socket: {error}"),
                                }
                            })?);
                        (socket, Some(portal))
                    }
                };
                self.connected = Some(Connected {
                    session: EiSession::connect(socket)?,
                    _portal: portal,
                });
            }
            Ok(&mut self
                .connected
                .as_mut()
                .expect("BUG: the session was connected just above")
                .session)
        }

        fn motion(&mut self, to: DevicePoint) -> Result<()> {
            let scale = self.scale;
            let session = self.session()?;
            let (pointer, device, regions) = session.interface::<ei::PointerAbsolute>()?;
            let (x, y) = mapping::ei_point(to, scale, &regions).ok_or_else(|| {
                Error::outside_target(format!(
                    "({}, {}) lies in no region the compositor's pointer covers; nothing was sent",
                    to.x, to.y
                ))
            })?;
            pointer.motion_absolute(x, y);
            session.frame(&device)
        }

        /// Press or release the button whose evdev code is `code`; what the server took is
        /// recorded, so it is released on every exit.
        fn button_code(&mut self, code: u32, pressed: bool) -> Result<()> {
            let session = self.session()?;
            let (button, device, _) = session.interface::<ei::Button>()?;
            button.button(
                code,
                if pressed {
                    ei::button::ButtonState::Press
                } else {
                    ei::button::ButtonState::Released
                },
            );
            session.frame(&device)?;
            if pressed {
                if !self.buttons.contains(&code) {
                    self.buttons.push(code);
                }
            } else {
                self.buttons.retain(|held| *held != code);
            }
            Ok(())
        }

        fn button(&mut self, button: PointerButton, pressed: bool) -> Result<()> {
            let code = mapping::button_code(button)
                .ok_or_else(|| unsent(format!("the mouse button {button:?}")))?;
            self.button_code(code, pressed)
        }

        fn key(&mut self, keysym: Keysym, pressed: bool) -> Result<()> {
            let session = self.session()?;
            let (text, device, _) = session.interface::<ei::Text>()?;
            text.keysym(
                keysym.0,
                if pressed {
                    ei::keyboard::KeyState::Press
                } else {
                    ei::keyboard::KeyState::Released
                },
            );
            session.frame(&device)?;
            if pressed {
                if !self.keys.contains(&keysym) {
                    self.keys.push(keysym);
                }
            } else {
                self.keys.retain(|held| *held != keysym);
            }
            Ok(())
        }

        fn text(&mut self, typed: &str) -> Result<()> {
            let session = self.session()?;
            let (text, device, _) = session.interface::<ei::Text>()?;
            text.utf8(typed);
            session.frame(&device)
        }

        fn scroll(&mut self, at: DevicePoint, delta_x: i32, delta_y: i32) -> Result<()> {
            self.motion(at)?;
            let scale = self.scale;
            let session = self.session()?;
            let (scroll, device, _) = session.interface::<ei::Scroll>()?;
            scroll.scroll(
                mapping::scroll_distance(delta_x, scale),
                mapping::scroll_distance(delta_y, scale),
            );
            session.frame(&device)
        }

        fn keysym(name: &str) -> Result<Keysym> {
            keys::resolve(name).ok_or_else(|| {
                Error::not_supported(
                    "os_key",
                    format!("the wayland lane has no key named {name:?}"),
                )
            })
        }
    }

    impl Lane for WaylandLane {
        fn name(&self) -> &'static str {
            LANE
        }

        fn capabilities(&self) -> LaneCapabilities {
            LaneCapabilities::new(
                vec![PointerDevice::Mouse],
                true,
                self.isolation,
                PositionFidelity::OrderOnly,
            )
            .accessibility(AccessibilityApi::AtSpi)
        }

        fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
            None
        }

        fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
            self.scale = placement.scale;
            match self.origin {
                Some(origin) => Ok(screen::window_geometry(
                    origin,
                    placement.size,
                    placement.scale,
                )),
                None => Err(Error::not_supported(
                    "window_position",
                    "a Wayland client is told nothing of where its window is: give the wayland \
                     lane the window's origin in the compositor (WaylandLane::window_origin)",
                )),
            }
        }

        fn check(&mut self, events: &[OsEvent]) -> Result<()> {
            let mut texts = false;
            for event in events {
                match event {
                    OsEvent::PointerDown { device, .. }
                    | OsEvent::PointerUp { device, .. }
                    | OsEvent::PointerMove { device, .. }
                        if *device != PointerDevice::Mouse =>
                    {
                        return Err(unsent(format!("{} input", device.name())));
                    }
                    OsEvent::PointerDown { button, .. } | OsEvent::PointerUp { button, .. } => {
                        mapping::button_code(*button)
                            .ok_or_else(|| unsent(format!("the mouse button {button:?}")))?;
                    }
                    OsEvent::KeyDown(name) | OsEvent::KeyUp(name) => {
                        Self::keysym(name)?;
                        texts = true;
                    }
                    OsEvent::Text(_) => texts = true,
                    _ => {}
                }
            }
            // The server's devices are known once connected: a capability it lacks is named
            // before the first event.
            if texts {
                self.session()?.interface::<ei::Text>()?;
            }
            Ok(())
        }

        fn send(&mut self, event: &OsEvent) -> Result<()> {
            match event {
                OsEvent::Pause(duration) => {
                    std::thread::sleep(*duration);
                    Ok(())
                }
                OsEvent::PointerMove { to, .. } => self.motion(*to),
                OsEvent::PointerDown { button, .. } => self.button(*button, true),
                OsEvent::PointerUp { button, .. } => self.button(*button, false),
                OsEvent::Scroll {
                    at,
                    delta_x,
                    delta_y,
                } => self.scroll(*at, *delta_x, *delta_y),
                OsEvent::KeyDown(name) => self.key(Self::keysym(name)?, true),
                OsEvent::KeyUp(name) => self.key(Self::keysym(name)?, false),
                OsEvent::Text(text) => self.text(text),
                // `send_guarded` refuses it first: the lane does not compose.
                OsEvent::Composition(_) => Err(crate::lane::no_composition(LANE)),
            }
        }

        fn release_all(&mut self) -> Result<()> {
            if self.connected.is_none() {
                return Ok(());
            }
            let mut first: Option<Error> = None;
            for keysym in self.keys.clone().into_iter().rev() {
                if let Err(error) = self.key(keysym, false) {
                    first.get_or_insert(error);
                }
            }
            for code in self.buttons.clone() {
                if let Err(error) = self.button_code(code, false) {
                    first.get_or_insert(error);
                }
            }
            match first {
                None => Ok(()),
                Some(first) if self.keys.is_empty() && self.buttons.is_empty() => Err(first),
                Some(first) => Err(Error::ReleaseFailed {
                    lane: LANE.to_owned(),
                    held: self
                        .keys
                        .iter()
                        .map(|keysym| keys::name_of(*keysym))
                        .chain(
                            self.buttons
                                .iter()
                                .map(|code| format!("the button of evdev code {code:#x}")),
                        )
                        .collect::<Vec<String>>(),
                    message: first.to_string(),
                    first: None,
                }),
            }
        }

        fn read_tree(&mut self, placement: &Placement, _app: &Tree) -> Result<Tree> {
            let bus = A11yBus::connect(self.session_bus.as_deref(), LANE)?;
            let mut tree = bus.read(WindowTarget::of(self.pid, placement))?;
            if let Some(root) = tree.roots.first_mut() {
                root.window = Some(placement.window);
            }
            Ok(tree)
        }

        fn invoke(&mut self, placement: &Placement, identifier: &str) -> Result<()> {
            let bus = A11yBus::connect(self.session_bus.as_deref(), LANE)?;
            bus.invoke(WindowTarget::of(self.pid, placement), identifier)
        }

        fn capture_window(&mut self, placement: &Placement) -> Result<crate::pixels::Image> {
            let _ = placement;
            Err(Error::not_supported(
                "os_capture",
                "the ScreenCast portal's capture comes as a PipeWire stream, and the wayland lane \
                 links no PipeWire",
            ))
        }
    }
}
