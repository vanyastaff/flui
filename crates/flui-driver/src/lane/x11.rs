//! The X11 lane: real input into an app on an X server, what AT-SPI shows of it, and the server's
//! own image of its window.
//!
//! - **Input.** XTEST for the mouse, the wheel and keys: a key as the keycode the server's keymap
//!   puts its keysym on, with Shift around it for the keymap's second level; text as the keys its
//!   characters are on, and a character on no key through a spare key mapped to it for the text
//!   ([`held`]). Positions are exact device pixels: X has no other unit. A scroll of `n` device
//!   pixels turns the wheel by `n` over [`held::WHEEL_STEP`] clicks, at least one, which the app
//!   turns into a distance of its own, so a scroll's distance is not compared, only that it
//!   arrived. XTEST fakes a core pointer and keyboard only: touch and pen are refused before any
//!   event, as is an input-method composition, which goes through the app's own input path
//!   (`input.inject`). A pause is waited out whole, watching the guard as it waits.
//! - **One path for keys.** A key event and typed text alike become keystrokes planned against
//!   the keymap and what the lane holds ([`held::HeldInput`]): the shell-shortcut rule reads those
//!   strokes before a sequence, and the lane sends them, reading the server again before each.
//! - **Safety.** Before every event, and every keystroke, the rules of [`safety`] are read again:
//!   the window is the app's and mapped, it has the keyboard focus, the point reaches it, nothing
//!   physical is held; before a sequence, `Lane::check` refuses a key the keymap cannot type, a
//!   device or button the lane does not send, and a chord the desktop takes. What the lane pressed
//!   or mapped is released or emptied on every exit, and what the server refused stays held to be
//!   released again.
//! - **The guard** (`X11Guard`, on Linux) watches XInput 2's raw events for input the lane did
//!   not send ([`activity`]); a guard whose watcher stopped, or that no longer sees the lane's own
//!   input, ends the run.
//! - **Reading.** The window's AT-SPI tree, projected onto the wire's nodes
//!   ([`atspi`](crate::lane::atspi)), and its pixels through `GetImage` ([`image`]).
//! - **Process.** A launched app leads a process group of its own and dies with the driver's
//!   thread that started it (`PR_SET_PDEATHSIG`); the session ends the group.
//!
//! The pure parts (the keys, the safety rules, the held input and keymap, the activity count and
//! the image layout) build on every host, so their tables run on any CI; the parts that call the X
//! server are `cfg(target_os = "linux")`.

pub mod activity;
pub mod held;
pub mod image;
pub mod keys;
pub mod safety;

#[cfg(target_os = "linux")]
mod guard;
#[cfg(target_os = "linux")]
mod server;

#[cfg(target_os = "linux")]
pub use guard::{STOPPED_WATCHING, X11Guard};
#[cfg(target_os = "linux")]
pub use lane::X11Lane;

#[cfg(target_os = "linux")]
mod lane {
    use flui_protocol::input::{PointerButton, PointerDevice};
    use flui_protocol::session::{Isolation, Placement};
    use flui_protocol::tree::Tree;
    use x11rb::protocol::xproto::Window;

    use super::guard::X11Guard;
    use super::held::{self, HeldButtons, HeldInput, KeyInput, Keymap, Keystroke};
    use super::keys::{self, NO_SYMBOL};
    use super::safety::{self, Desk, Held, Landing, Refusal};
    use super::server::{Fake, Server, lane_error};
    use crate::coordinates::{DevicePoint, WindowGeometry};
    use crate::error::{Error, Result};
    use crate::lane::atspi::client::{A11yBus, WindowTarget};
    use crate::lane::windows::activity;
    use crate::lane::windows::screen;
    use crate::lane::{
        AccessibilityApi, ActivityGuard, Lane, LaneCapabilities, OsCapture, PositionFidelity,
    };
    use crate::steps::OsEvent;

    /// How far a window's device size may be from the app's logical size times its scale: the
    /// server keeps a window in whole pixels.
    const SIZE_TOLERANCE: f64 = 1.0;

    /// The X11 lane for the app whose process is `pid`: see the [module docs](super).
    #[derive(Debug)]
    pub struct X11Lane {
        pid: u32,
        display: Option<String>,
        session_bus: Option<String>,
        isolation: Isolation,
        server: Option<Server>,
        window: Option<Window>,
        guard: X11Guard,
        keys: HeldInput,
        buttons: HeldButtons,
        keymap: Option<Keymap>,
    }

    impl X11Lane {
        /// A lane driving the app whose process is `pid` (`Driver::app_pid`) on the X server
        /// `display` names (`DISPLAY` when `None`), its input landing in `isolation`: the user's
        /// own desktop (`Shared`), announced and guarded, or an X virtual framebuffer the run
        /// started (`Xvfb`), guarded too.
        ///
        /// # Errors
        ///
        /// [`Error::InvalidArgument`] for an isolation mode that is no X server's.
        pub fn try_new(pid: u32, display: Option<String>, isolation: Isolation) -> Result<Self> {
            safety::admit_isolation(isolation).map_err(Refusal::into_error)?;
            Ok(Self {
                pid,
                guard: X11Guard::new(display.clone()),
                display,
                session_bus: None,
                isolation,
                server: None,
                window: None,
                keys: HeldInput::new(),
                buttons: HeldButtons::default(),
                keymap: None,
            })
        }

        /// Read the app through the AT-SPI bus of the session bus at `address`, rather than the
        /// driver's own session bus: the bus the app was started on.
        #[must_use]
        pub fn with_session_bus(mut self, address: impl Into<String>) -> Self {
            self.session_bus = Some(address.into());
            self
        }

        fn server(&mut self) -> Result<&Server> {
            if self.server.is_none() {
                self.server = Some(Server::connect(self.display.as_deref())?);
            }
            Ok(self
                .server
                .as_ref()
                .expect("BUG: the server was connected just above"))
        }

        /// The app's window `placement` describes, among the process's viewable windows, and its
        /// top-left pixel on the screen.
        fn find(&mut self, placement: &Placement) -> Result<(Window, DevicePoint)> {
            let pid = self.pid;
            let server = self.server()?;
            let device = |logical: f64| logical * placement.scale;
            let matching: Vec<Window> = server
                .windows_of(pid)?
                .into_iter()
                .filter(|(_, width, height)| {
                    (f64::from(*width) - device(placement.size.width)).abs() <= SIZE_TOLERANCE
                        && (f64::from(*height) - device(placement.size.height)).abs()
                            <= SIZE_TOLERANCE
                })
                .map(|(window, _, _)| window)
                .collect();
            let window = match matching.as_slice() {
                [window] => *window,
                [] => {
                    return Err(Error::not_found(format!(
                        "process {pid} shows no window of {} x {} at scale {}",
                        placement.size.width, placement.size.height, placement.scale
                    )));
                }
                several => {
                    return Err(lane_error(format!(
                        "process {pid} shows {} windows of {} x {} at scale {}; the lane cannot \
                         tell which is {}",
                        several.len(),
                        placement.size.width,
                        placement.size.height,
                        placement.scale,
                        placement.window
                    )));
                }
            };
            let origin = server.origin(window)?;
            self.window = Some(window);
            Ok((window, origin))
        }

        fn target(&self) -> Result<Window> {
            self.window.ok_or_else(|| {
                lane_error("the lane has no target window: it sends nothing before it locates one")
            })
        }

        /// The keymap, read once a sequence: an app may not change the server's keymap mid-run,
        /// and the lane's own spare keys are recorded where it maps them.
        fn keymap(&mut self) -> Result<Keymap> {
            if let Some(keymap) = &self.keymap {
                return Ok(keymap.clone());
            }
            let keymap = self.server()?.keymap()?;
            self.keymap = Some(keymap.clone());
            Ok(keymap)
        }

        /// The server as the next event finds it, the app's window given the focus once when
        /// another has it.
        fn desk(&self, server: &Server, window: Window) -> Desk {
            let mut desk = Desk::quiet();
            desk.window_alive = server.alive(window, self.pid);
            if !desk.window_alive {
                return desk;
            }
            let (mut focused, mut focus) = server.focused(window);
            if !focused {
                server.give_focus(window);
                (focused, focus) = server.focused(window);
            }
            desk.focused = focused;
            desk.focus = focus.map(|window| server.describe(window));
            desk.buttons = server.buttons_down();
            desk.keys = server.keys_down();
            desk
        }

        /// The safety rules over the server as it is now, for an event landing at `landing`.
        fn checked(&mut self, landing: Option<DevicePoint>) -> Result<()> {
            let window = self.target()?;
            let held = Held {
                buttons: self.buttons.bits(),
                keys: self.keys.keys(),
            };
            self.server()?;
            let server = self
                .server
                .as_ref()
                .expect("BUG: the server was connected just above");
            let desk = self.desk(server, window);
            let landing = landing.map(|point| {
                let (on_target, under) = server.lands_on(window, point);
                Landing::new(point, on_target, under.map(|under| server.describe(under)))
            });
            safety::check_event(&desk, landing.as_ref(), &held).map_err(Refusal::into_error)
        }

        /// Fake `fake`, telling the guard first, then wait for it to come back.
        fn fake(&mut self, fake: Fake) -> Result<()> {
            self.guard.will_send();
            self.server()?.fake(fake)?;
            self.guard.await_echo()
        }

        /// The key event or text `event` is.
        fn key_input(event: &OsEvent) -> Result<Option<KeyInput>> {
            let keysym = |name: &str| {
                keys::resolve(name).ok_or_else(|| {
                    Refusal::UnknownKey {
                        name: name.to_owned(),
                    }
                    .into_error()
                })
            };
            Ok(match event {
                OsEvent::KeyDown(name) => Some(KeyInput::Down {
                    keysym: keysym(name)?,
                    name: name.clone(),
                }),
                OsEvent::KeyUp(name) => Some(KeyInput::Up {
                    keysym: keysym(name)?,
                    name: name.clone(),
                }),
                OsEvent::Text(text) => Some(KeyInput::Text(text.clone())),
                _ => None,
            })
        }

        /// Send the keystrokes of `input` one by one, the server read again before each press,
        /// each recorded as soon as the server took it and then confirmed through the guard.
        fn type_keys(&mut self, input: &KeyInput) -> Result<()> {
            let keymap = self.keymap()?;
            let strokes = self
                .keys
                .plan(input, &keymap)
                .map_err(Refusal::into_error)?;
            for stroke in strokes {
                if let Keystroke::Press(_) = stroke {
                    self.checked(None)?;
                }
                self.stroke(stroke, &keymap)?;
            }
            Ok(())
        }

        fn stroke(&mut self, stroke: Keystroke, keymap: &Keymap) -> Result<()> {
            if matches!(stroke, Keystroke::Press(_) | Keystroke::Release(_)) {
                self.guard.will_send();
            }
            let server = self
                .server
                .as_ref()
                .ok_or_else(|| lane_error("the lane is not connected to the X server"))?;
            let per_keycode = keymap.per_keycode();
            let guard = &mut self.guard;
            self.keys.deliver(
                stroke,
                keymap,
                || send_stroke(server, stroke, per_keycode),
                || guard.await_echo(),
            )
        }

        fn mouse_button(&mut self, button: PointerButton, pressed: bool) -> Result<()> {
            let core = match button {
                PointerButton::Primary => 1,
                PointerButton::Middle => 2,
                PointerButton::Secondary => 3,
                PointerButton::Back => 8,
                PointerButton::Forward => 9,
                _ => {
                    return Err(Refusal::UnsentInput {
                        what: format!("the mouse button {button:?}"),
                    }
                    .into_error());
                }
            };
            self.guard.will_send();
            let server = self
                .server
                .as_ref()
                .ok_or_else(|| lane_error("the lane is not connected to the X server"))?;
            let guard = &mut self.guard;
            self.buttons.deliver(
                core,
                pressed,
                || {
                    server.fake(if pressed {
                        Fake::ButtonPress(core)
                    } else {
                        Fake::ButtonRelease(core)
                    })
                },
                || guard.await_echo(),
            )
        }

        /// Move to `at` and turn the wheel, each click's press and release held by
        /// [`HeldButtons`] as a button's are, so a click left down is released with the rest.
        fn scroll(&mut self, at: DevicePoint, delta_x: i32, delta_y: i32) -> Result<()> {
            self.fake(Fake::Motion(at))?;
            let server = self
                .server
                .as_ref()
                .ok_or_else(|| lane_error("the lane is not connected to the X server"))?;
            // The guard is told of each click before it goes out and waited on after: two uses
            // of one guard by the two steps of each click, on this thread.
            let guard = std::cell::RefCell::new(&mut self.guard);
            self.buttons.scroll(
                delta_x,
                delta_y,
                |button, pressed| {
                    guard.borrow_mut().will_send();
                    server.fake(if pressed {
                        Fake::ButtonPress(button)
                    } else {
                        Fake::ButtonRelease(button)
                    })
                },
                || guard.borrow_mut().await_echo(),
            )
        }

        /// Wait out `pause` whole, looking at the guard between slices.
        fn pause(&mut self, pause: std::time::Duration) -> Result<()> {
            for slice in activity::slices(pause) {
                if let Some(foreign) = self.guard.foreign_input() {
                    return Err(Error::InterruptedByUser {
                        lane: safety::LANE.to_owned(),
                        event: foreign.description,
                    });
                }
                std::thread::sleep(slice);
            }
            Ok(())
        }

        fn window_target(&self, placement: &Placement) -> WindowTarget {
            let device = |logical: f64| {
                let device = (logical * placement.scale).round();
                if device.is_finite() && device >= 0.0 && device <= f64::from(u32::MAX) {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a whole, finite, non-negative number checked inside u32's range"
                    )]
                    let device = device as u32;
                    device
                } else {
                    0
                }
            };
            WindowTarget {
                pid: self.pid,
                size: (device(placement.size.width), device(placement.size.height)),
            }
        }
    }

    /// Send one keystroke to `server`.
    fn send_stroke(server: &Server, stroke: Keystroke, per_keycode: u8) -> Result<()> {
        match stroke {
            Keystroke::Press(key) => server.fake(Fake::KeyPress(key)),
            Keystroke::Release(key) => server.fake(Fake::KeyRelease(key)),
            Keystroke::Map { key, keysym } => server.map_key(key, keysym, per_keycode),
            Keystroke::Unmap(key) => server.map_key(key, NO_SYMBOL, per_keycode),
            Keystroke::Settle => {
                std::thread::sleep(held::MAP_SETTLE);
                Ok(())
            }
        }
    }

    impl Lane for X11Lane {
        fn name(&self) -> &'static str {
            safety::LANE
        }

        fn capabilities(&self) -> LaneCapabilities {
            LaneCapabilities::new(
                vec![PointerDevice::Mouse],
                true,
                self.isolation,
                PositionFidelity::Exact,
            )
            .accessibility(AccessibilityApi::AtSpi)
        }

        fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
            Some(&mut self.guard)
        }

        fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
            let (_, origin) = self.find(placement)?;
            Ok(screen::window_geometry(
                origin,
                placement.size,
                placement.scale,
            ))
        }

        fn check(&mut self, events: &[OsEvent]) -> Result<()> {
            let mut inputs = Vec::new();
            for event in events {
                match event {
                    OsEvent::PointerDown { device, .. }
                    | OsEvent::PointerUp { device, .. }
                    | OsEvent::PointerMove { device, .. }
                        if *device != PointerDevice::Mouse =>
                    {
                        return Err(Refusal::UnsentInput {
                            what: format!(
                                "{} input: XTEST fakes a core pointer and keyboard only",
                                device.name()
                            ),
                        }
                        .into_error());
                    }
                    OsEvent::Scroll {
                        delta_x, delta_y, ..
                    } => held::scroll_within(*delta_x, *delta_y).map_err(Refusal::into_error)?,
                    _ => inputs.extend(Self::key_input(event)?),
                }
            }
            if inputs.is_empty() {
                return Ok(());
            }
            // A new sequence reads the keymap afresh.
            self.keymap = None;
            let keymap = self.keymap()?;
            match self
                .keys
                .shell_hotkey(&inputs, &keymap)
                .map_err(Refusal::into_error)?
            {
                Some(handler) => Err(Refusal::ShellHotkey { handler }.into_error()),
                None => Ok(()),
            }
        }

        fn send(&mut self, event: &OsEvent) -> Result<()> {
            if let OsEvent::Pause(duration) = event {
                return self.pause(*duration);
            }
            if let Some(input) = Self::key_input(event)? {
                return self.type_keys(&input);
            }
            match event {
                OsEvent::PointerMove { to, .. } => {
                    self.checked(Some(*to))?;
                    self.fake(Fake::Motion(*to))
                }
                OsEvent::PointerDown { button, .. } => {
                    self.checked(None)?;
                    self.mouse_button(*button, true)
                }
                OsEvent::PointerUp { button, .. } => {
                    self.checked(None)?;
                    self.mouse_button(*button, false)
                }
                OsEvent::Scroll {
                    at,
                    delta_x,
                    delta_y,
                } => {
                    self.checked(Some(*at))?;
                    self.scroll(*at, *delta_x, *delta_y)
                }
                _ => Ok(()),
            }
        }

        fn release_all(&mut self) -> Result<()> {
            let Some(server) = self.server.as_ref() else {
                return Ok(());
            };
            let keymap = match &self.keymap {
                Some(keymap) => Ok(keymap.clone()),
                None => server.keymap(),
            };
            // A keymap that cannot be read releases no key: what the lane holds is named.
            let keys = match keymap {
                Ok(keymap) => {
                    let per_keycode = keymap.per_keycode();
                    self.keys
                        .release_all(&keymap, |stroke| send_stroke(server, stroke, per_keycode))
                }
                Err(error) => self.keys.unreleased(error),
            };
            let buttons = self
                .buttons
                .release_all(|button| server.fake(Fake::ButtonRelease(button)));
            crate::lane::release_outcome(keys, buttons)
        }

        fn read_tree(&mut self, placement: &Placement) -> Result<Tree> {
            let bus = A11yBus::connect(self.session_bus.as_deref(), safety::LANE)?;
            let mut tree = bus.read(self.window_target(placement))?;
            if let Some(root) = tree.roots.first_mut() {
                root.window = Some(placement.window);
            }
            Ok(tree)
        }

        fn invoke(&mut self, placement: &Placement, identifier: &str) -> Result<()> {
            let bus = A11yBus::connect(self.session_bus.as_deref(), safety::LANE)?;
            bus.invoke(self.window_target(placement), identifier)
        }

        fn capture_window(&mut self, placement: &Placement) -> Result<OsCapture> {
            let (window, _) = self.find(placement)?;
            self.server()?.capture(window)
        }
    }
}
