//! A window's tree read back by a real AT-SPI client, the protocol Orca
//! speaks: the roles, names, values, hints, states and actions of its nodes,
//! the announcements its live regions and its announcer make, and its focus,
//! on a private session bus with the accessibility bus `at-spi-bus-launcher`
//! starts beside it (gap 36, `docs/gaps.md`).
//!
//! Each text the cases listen for carries a nonce of the run, so no other
//! source on the bus can produce a hit.
//!
//! Its own harness: it starts an X server, a session bus and the
//! accessibility bus before anything connects and names them for the
//! process. A run without Xvfb, `dbus-daemon` or `at-spi-bus-launcher` is
//! skipped, and fails where `FLUI_REQUIRE_ATSPI` is set.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
mod cases {
    use std::io::{BufRead as _, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use atspi::events::EventBodyOwned;
    use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt as _};
    use atspi::proxy::bus::{BusProxy, StatusProxy};
    use atspi::proxy::proxy_ext::ProxyExt as _;
    use atspi::{Interface, Role, State, zbus};
    use flui_foundation::geometry::{Rect, Size};
    use flui_foundation::{RenderId, SemanticsId};
    use flui_native::{HostWindow, Platform as _, X11Platform};
    use flui_platform::WindowOptions;
    use flui_semantics::{
        Announcements, LiveSetting, NumericRange, SemanticsAction, SemanticsConfiguration,
        SemanticsIdentitySource, SemanticsNode, SemanticsOwner, SemanticsRole, TextContent,
        TextDirection, TextRun, TextRunParts, Toggled, TreeUpdate, Urgency,
    };
    use flui_testing::window_handler::RecordingWindowHandler;
    use futures_lite::future::block_on;
    use x11rb::rust_connection::RustConnection;

    /// A case, run against the session's buses.
    type Case = fn(&Session);

    const CASES: &[(&str, Case)] = &[
        (
            "a_node_reads_back_with_its_role_name_value_hint_and_states",
            a_node_reads_back_with_its_role_name_value_hint_and_states,
        ),
        (
            "a_live_region_is_announced_with_its_text",
            a_live_region_is_announced_with_its_text,
        ),
        (
            "an_announcement_is_heard_at_its_urgency",
            an_announcement_is_heard_at_its_urgency,
        ),
        (
            "focus_follows_the_tree_once_the_window_is_active",
            focus_follows_the_tree_once_the_window_is_active,
        ),
    ];

    pub(super) fn main() {
        let mut arguments = libtest_mimic::Arguments::from_args();
        // One thread: an X11 platform belongs to the thread that builds it,
        // and the cases share one accessibility bus.
        arguments.test_threads = Some(1);
        let session = Arc::new((!arguments.list).then(Session::start).flatten());
        let trials = CASES
            .iter()
            .map(|&(name, case)| {
                let session = Arc::clone(&session);
                libtest_mimic::Trial::test(name, move || {
                    match session.as_ref() {
                        Some(session) => case(session),
                        None => eprintln!(
                            "skipped: no Xvfb, dbus-daemon or at-spi-bus-launcher to run against"
                        ),
                    }
                    Ok(())
                })
            })
            .collect();
        libtest_mimic::run(&arguments, trials).exit();
    }

    /// An X server, a session bus and the accessibility bus, which the
    /// process's environment names.
    struct Session {
        children: Vec<Child>,
        /// The client's connection to the accessibility bus.
        bus: zbus::Connection,
        /// Appended to every text a case listens for.
        nonce: String,
        _runtime: TempDir,
    }

    /// A directory removed with its contents when dropped.
    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            for child in self.children.iter_mut().rev() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// Where distributions put `at-spi-bus-launcher`.
    const LAUNCHERS: &[&str] = &[
        "/usr/libexec/at-spi-bus-launcher",
        "/usr/lib/at-spi2-core/at-spi-bus-launcher",
        "/usr/lib/at-spi-bus-launcher",
    ];

    /// How long a case waits for the bus to answer or an event to arrive.
    const PATIENCE: Duration = Duration::from_secs(10);

    impl Session {
        /// Start the X server and both buses, and turn accessibility on as a
        /// desktop does when a screen reader starts; `None` where a program
        /// is missing, which fails where `FLUI_REQUIRE_ATSPI` is set.
        ///
        /// Call it before any thread starts: it sets the environment.
        fn start() -> Option<Self> {
            let launcher = LAUNCHERS.iter().map(Path::new).find(|path| path.exists());
            let (Some(launcher), true, true) =
                (launcher, available("Xvfb"), available("dbus-daemon"))
            else {
                assert!(
                    std::env::var_os("FLUI_REQUIRE_ATSPI").is_none(),
                    "FLUI_REQUIRE_ATSPI is set and Xvfb, dbus-daemon or at-spi-bus-launcher is missing"
                );
                return None;
            };
            let runtime = std::env::temp_dir().join(format!("flui-atspi-{}", std::process::id()));
            std::fs::create_dir_all(&runtime).expect("a runtime directory");
            std::fs::set_permissions(
                &runtime,
                <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .expect("the runtime directory is private");
            let runtime = TempDir(runtime);
            let (server, display) = x_server();
            let mut session_bus = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .arg(format!(
                    "--address=unix:path={}",
                    runtime.0.join("bus").display()
                ))
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("dbus-daemon starts");
            let mut address = String::new();
            BufReader::new(session_bus.stdout.take().expect("piped"))
                .read_line(&mut address)
                .expect("dbus-daemon prints its address");
            // SAFETY: no thread has started; nothing reads the environment
            // while it is set.
            #[expect(
                unsafe_code,
                reason = "the environment is set before any thread exists"
            )]
            unsafe {
                std::env::set_var("DISPLAY", &display);
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address.trim());
                std::env::remove_var("AT_SPI_BUS_ADDRESS");
                std::env::set_var("XDG_RUNTIME_DIR", &runtime.0);
            }
            let launcher = Command::new(launcher)
                .arg("--launch-immediately")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("at-spi-bus-launcher starts");
            let bus = block_on(async {
                let session = zbus::Connection::session()
                    .await
                    .expect("the session bus answers");
                let deadline = Instant::now() + PATIENCE;
                let address = loop {
                    if let Ok(proxy) = BusProxy::new(&session).await
                        && let Ok(address) = proxy.get_address().await
                    {
                        break address;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for the accessibility bus"
                    );
                    async_io::Timer::after(Duration::from_millis(20)).await;
                };
                StatusProxy::new(&session)
                    .await
                    .expect("org.a11y.Status")
                    .set_is_enabled(true)
                    .await
                    .expect("accessibility turns on");
                zbus::connection::Builder::address(address.as_str())
                    .expect("an address")
                    .build()
                    .await
                    .expect("the accessibility bus answers")
            });
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after 1970")
                .subsec_nanos();
            Some(Self {
                children: vec![server, session_bus, launcher],
                bus,
                nonce: format!("{nonce:08x}"),
                _runtime: runtime,
            })
        }

        /// `text` with the run's nonce.
        fn marked(&self, text: &str) -> String {
            format!("{text} {}", self.nonce)
        }

        /// The application's window named `name` on the bus, once the
        /// adapter has registered it.
        fn window(&self, name: &str) -> AccessibleProxy<'_> {
            block_on(async {
                let desktop = AccessibleProxy::builder(&self.bus)
                    .destination("org.a11y.atspi.Registry")
                    .expect("a name")
                    .path("/org/a11y/atspi/accessible/root")
                    .expect("a path")
                    .build()
                    .await
                    .expect("the desktop");
                let deadline = Instant::now() + PATIENCE;
                loop {
                    for application in children(&self.bus, &desktop).await {
                        for window in children(&self.bus, &application).await {
                            if window.name().await.is_ok_and(|found| found == name) {
                                return window;
                            }
                        }
                    }
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for window {name:?} on the accessibility bus"
                    );
                    async_io::Timer::after(Duration::from_millis(20)).await;
                }
            })
        }

        /// The announcements the client hears while `act` runs and for a
        /// moment after, as their politeness (1 polite, 2 assertive) and
        /// text, and the path of each node that took the focus.
        fn hear(&self, act: impl FnOnce()) -> Heard {
            block_on(async {
                let rule = zbus::MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .interface("org.a11y.atspi.Event.Object")
                    .expect("an interface")
                    .build();
                let mut stream = zbus::MessageStream::for_match_rule(rule, &self.bus, None)
                    .await
                    .expect("a match rule");
                act();
                let mut heard = Heard::default();
                let deadline = Instant::now() + Duration::from_millis(500);
                while let Some(Some(Ok(message))) = futures_lite::future::or(
                    async { Some(futures_lite::StreamExt::next(&mut stream).await) },
                    async {
                        async_io::Timer::at(deadline).await;
                        None
                    },
                )
                .await
                {
                    let header = message.header();
                    let (Some(member), Some(path)) = (header.member(), header.path()) else {
                        continue;
                    };
                    let Ok(body) = message.body().deserialize::<EventBodyOwned>() else {
                        continue;
                    };
                    match member.as_str() {
                        "Announcement" => heard.announcements.push((
                            body.detail1,
                            String::try_from(body.any_data).unwrap_or_default(),
                        )),
                        "StateChanged" if body.kind == "focused" && body.detail1 == 1 => {
                            heard.focused.push(path.to_string());
                        }
                        _ => {}
                    }
                }
                heard
            })
        }
    }

    /// What the client heard while a case acted.
    #[derive(Debug, Default)]
    struct Heard {
        announcements: Vec<(i32, String)>,
        focused: Vec<String>,
    }

    impl Heard {
        /// The announcements whose text carries `nonce`.
        fn marked(&self, nonce: &str) -> Vec<(i32, String)> {
            self.announcements
                .iter()
                .filter(|(_, text)| text.contains(nonce))
                .cloned()
                .collect()
        }
    }

    async fn children<'a>(
        bus: &'a zbus::Connection,
        node: &AccessibleProxy<'a>,
    ) -> Vec<AccessibleProxy<'a>> {
        let mut found = Vec::new();
        for child in node.get_children().await.unwrap_or_default() {
            if let Ok(child) = child.into_accessible_proxy(bus).await {
                found.push(child);
            }
        }
        found
    }

    fn available(program: &str) -> bool {
        Command::new(program)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    /// Start Xvfb on a display number no other server holds (see
    /// `x11_window.rs`).
    fn x_server() -> (Child, String) {
        let first = 100 + std::process::id() % 800;
        for number in (first..first + 200).map(|number| 100 + number % 900) {
            let display = format!(":{number}");
            let mut server = Command::new("Xvfb")
                .args([&display, "-screen", "0", "1024x768x24", "-nolisten", "tcp"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Xvfb starts");
            let deadline = Instant::now() + PATIENCE;
            loop {
                if server.try_wait().ok().flatten().is_some() {
                    break;
                }
                if RustConnection::connect(Some(&display)).is_ok() {
                    std::thread::sleep(Duration::from_millis(200));
                    if server.try_wait().ok().flatten().is_none() {
                        return (server, display);
                    }
                    break;
                }
                assert!(Instant::now() < deadline, "timed out waiting for Xvfb");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        panic!("no free X display number");
    }

    /// A window of the X11 host whose semantics owner publishes to the
    /// window's AT-SPI adapter, as a presentation's does, rooted at a window
    /// node named `name`.
    struct Fixture {
        platform: X11Platform,
        window: Rc<dyn HostWindow>,
        owner: SemanticsOwner,
        ids: Vec<(usize, SemanticsId)>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let platform = X11Platform::try_new().expect("the X11 platform");
            let window = platform
                .open_window(
                    WindowOptions::default().with_size(Size::new(400.0, 300.0)),
                    RecordingWindowHandler::new() as _,
                )
                .expect("an X11 window opens");
            let accessibility = window.accessibility().expect("an X11 window is accessible");
            let owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
                accessibility.publish(update.clone());
            }));
            let mut fixture = Self {
                platform,
                window,
                owner,
                ids: Vec::new(),
            };
            fixture.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| {
                c.set_role(SemanticsRole::Window);
                c.set_label(name);
            });
            fixture
        }

        /// Node `index` under node `parent` (the root when `None`), laid out
        /// at `(left, top, width, height)` in logical pixels, configured by
        /// `configure`.
        fn add(
            &mut self,
            parent: Option<usize>,
            index: usize,
            (left, top, width, height): (f64, f64, f64, f64),
            configure: impl FnOnce(&mut SemanticsConfiguration),
        ) -> &mut Self {
            let mut node = SemanticsNode::new().with_source_render_id(RenderId::new(index));
            node.set_rect(Rect::from_ltwh(left, top, width, height));
            configure(node.config_mut());
            let id = self.owner.insert(node);
            match parent {
                Some(parent) => self.owner.add_child(self.id_of(parent), id),
                None => self.owner.set_root(Some(id)),
            }
            self.ids.push((index, id));
            self
        }

        fn id_of(&self, index: usize) -> SemanticsId {
            self.ids
                .iter()
                .find(|(candidate, _)| *candidate == index)
                .map(|(_, id)| *id)
                .expect("the fixture has that node")
        }

        /// Change node `index`'s configuration.
        fn change(
            &mut self,
            index: usize,
            configure: impl FnOnce(&mut SemanticsConfiguration),
        ) -> &mut Self {
            let node = self
                .owner
                .get_mut(self.id_of(index))
                .expect("the node is in the owner");
            configure(node.config_mut());
            node.mark_dirty();
            self
        }

        /// Give the window the X input focus, as a window manager does, and
        /// turn the loop until the window has heard it.
        fn activate(&self) {
            use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
            use x11rb::connection::Connection as _;
            use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus};
            let target = self.window.surface_target();
            let handle = target.window_handle().expect("an open window has a handle");
            let RawWindowHandle::Xcb(handle) = handle.as_raw() else {
                panic!("an X11 window hands out an XCB handle")
            };
            let (connection, _) = RustConnection::connect(None).expect("the X server answers");
            flui_native::testing::x11::run_for(&self.platform, Duration::from_millis(100))
                .expect("the loop turns");
            connection
                .set_input_focus(InputFocus::PARENT, handle.window.get(), x11rb::CURRENT_TIME)
                .expect("request");
            connection.flush().expect("flush");
            flui_native::testing::x11::run_for(&self.platform, Duration::from_millis(200))
                .expect("the loop turns");
        }
    }

    /// A run of `text` on one line, each character ten pixels wide, words
    /// starting after spaces.
    fn run(text: &str) -> TextRun {
        let mut parts = TextRunParts::new(
            text,
            0,
            0,
            TextDirection::Ltr,
            Rect::from_ltwh(0.0, 0.0, 10.0 * text.chars().count() as f64, 20.0),
        );
        let mut previous_space = true;
        for (index, character) in text.chars().enumerate() {
            parts
                .character_lengths
                .push(u8::try_from(character.len_utf8()).expect("a char is at most 4 bytes"));
            parts.character_positions.push(10.0 * index as f32);
            parts.character_widths.push(10.0);
            if previous_space && !character.is_whitespace() {
                parts
                    .word_starts
                    .push(u8::try_from(index).expect("fixture runs are short"));
            }
            previous_space = character.is_whitespace();
        }
        TextRun::try_new(parts).expect("a fixture run is well formed")
    }

    /// One node as the client reads it.
    #[derive(Debug, PartialEq)]
    struct Read {
        role: Role,
        name: String,
        description: String,
        /// The node's text, through the Text interface.
        text: Option<String>,
        /// Current, minimum and maximum, through the Value interface.
        value: Option<(f64, f64, f64)>,
        /// The names of its actions, through the Action interface.
        actions: Vec<String>,
    }

    async fn read(node: &AccessibleProxy<'_>) -> Read {
        let interfaces = node.get_interfaces().await.expect("interfaces");
        let proxies = node.proxies().await.expect("the node's interfaces");
        let text = if interfaces.contains(Interface::Text) {
            let text = proxies.text().await.expect("text");
            Some(text.get_text(0, -1).await.expect("the text"))
        } else {
            None
        };
        let value = if interfaces.contains(Interface::Value) {
            let value = proxies.value().await.expect("value");
            Some((
                value.current_value().await.expect("current"),
                value.minimum_value().await.expect("minimum"),
                value.maximum_value().await.expect("maximum"),
            ))
        } else {
            None
        };
        let actions = if interfaces.contains(Interface::Action) {
            proxies
                .action()
                .await
                .expect("action")
                .get_actions()
                .await
                .expect("the actions")
                .into_iter()
                .map(|action| action.name)
                .collect()
        } else {
            Vec::new()
        };
        Read {
            role: node.get_role().await.expect("the role"),
            name: node.name().await.expect("the name"),
            description: node.description().await.expect("the description"),
            text,
            value,
            actions,
        }
    }

    /// Each property a screen reader speaks reaches the bus, which another
    /// framework's Linux bridge did not do for a field's value and hint: a
    /// field's text and its hint, a checkbox's state, a disabled button, a
    /// slider's value and range.
    fn a_node_reads_back_with_its_role_name_value_hint_and_states(session: &Session) {
        let name = session.marked("Properties");
        let mut fixture = Fixture::new(&name);
        let runs = SemanticsIdentitySource::default()
            .allocate(1)
            .expect("a fresh source has identities");
        let content = TextContent::try_new(vec![run("ada@example.com")])
            .expect("one run")
            .with_run_identities(runs)
            .expect("one identity per run");
        fixture
            .add(Some(1), 2, (10.0, 10.0, 200.0, 30.0), |c| {
                c.set_role(SemanticsRole::TextInput);
                c.set_label("Email");
                c.set_value("ada@example.com");
                c.set_hint("Where receipts go");
                c.set_required(true);
                c.set_text_content(content);
            })
            .add(Some(1), 3, (10.0, 50.0, 100.0, 30.0), |c| {
                c.set_role(SemanticsRole::CheckBox);
                c.set_label("Subscribe");
                c.set_toggled(Some(Toggled::On));
                c.add_action(SemanticsAction::Click, Rc::new(|_| {}));
            })
            .add(Some(1), 4, (10.0, 90.0, 100.0, 30.0), |c| {
                c.set_role(SemanticsRole::Button);
                c.set_label("Delete");
                c.set_hint("Removes the account");
                c.set_enabled(Some(false));
                c.add_action(SemanticsAction::Click, Rc::new(|_| {}));
            })
            .add(Some(1), 5, (10.0, 130.0, 200.0, 30.0), |c| {
                c.set_label("Volume");
                c.set_numeric_range(
                    NumericRange::try_new(30.0, 0.0, 100.0, 1.0).expect("a valid range"),
                );
            });
        fixture.owner.flush();

        let window = session.window(&name);
        let nodes = block_on(async {
            let mut nodes = Vec::new();
            for child in children(&session.bus, &window).await {
                let state = child.get_state().await.expect("the state");
                nodes.push((read(&child).await, state));
            }
            nodes
        });
        let read = |role, name: &str, description: &str| Read {
            role,
            name: name.into(),
            description: description.into(),
            text: None,
            value: None,
            actions: Vec::new(),
        };
        let expected = [
            (
                Read {
                    text: Some("ada@example.com".into()),
                    ..read(Role::Entry, "Email", "Where receipts go")
                },
                &[State::Editable, State::Required, State::Enabled][..],
                &[][..],
            ),
            (
                Read {
                    actions: vec!["click".into()],
                    ..read(Role::CheckBox, "Subscribe", "")
                },
                &[State::Checkable, State::Checked, State::Enabled],
                &[],
            ),
            (
                Read {
                    actions: vec!["click".into()],
                    ..read(Role::Button, "Delete", "Removes the account")
                },
                &[],
                &[State::Enabled, State::Sensitive],
            ),
            (
                Read {
                    value: Some((30.0, 0.0, 100.0)),
                    ..read(Role::Slider, "Volume", "")
                },
                &[],
                &[],
            ),
        ];
        assert_eq!(
            nodes.len(),
            expected.len(),
            "one accessible per node: {nodes:#?}"
        );
        for ((node, state), (expected, on, off)) in nodes.iter().zip(expected) {
            assert_eq!(*node, expected);
            for state_on in on {
                assert!(
                    state.contains(*state_on),
                    "{} is {state_on:?}: {state:?}",
                    node.name
                );
            }
            for state_off in off {
                assert!(
                    !state.contains(*state_off),
                    "{} is not {state_off:?}: {state:?}",
                    node.name
                );
            }
        }
    }

    /// A live region is announced when its own label changes, when a label
    /// inside it changes and when it appears, which is a snack bar's shape (a
    /// status); a region that is off says nothing.
    fn a_live_region_is_announced_with_its_text(session: &Session) {
        let name = session.marked("Live regions");
        let mut fixture = Fixture::new(&name);
        fixture
            .add(Some(1), 2, (10.0, 10.0, 200.0, 30.0), |c| {
                c.set_label("Idle");
                c.set_live_region(LiveSetting::Polite);
            })
            .add(Some(1), 3, (10.0, 50.0, 200.0, 30.0), |c| {
                c.set_live_region(LiveSetting::Assertive);
            })
            .add(Some(3), 4, (10.0, 50.0, 200.0, 30.0), |c| {
                c.set_label("No errors");
            })
            .add(Some(1), 5, (10.0, 90.0, 200.0, 30.0), |c| {
                c.set_role(SemanticsRole::Timer);
                c.set_label("0:00");
            });
        fixture.owner.flush();
        let _ = session.window(&name);

        let (saved, error, archived, tick) = (
            session.marked("Saved"),
            session.marked("Email is invalid"),
            session.marked("Archived"),
            session.marked("0:01"),
        );
        let heard = session.hear(|| {
            fixture
                .change(2, |c| c.set_label(saved.as_str()))
                .change(4, |c| c.set_label(error.as_str()))
                .change(5, |c| c.set_label(tick.as_str()))
                .add(Some(1), 6, (0.0, 250.0, 400.0, 50.0), |c| {
                    c.set_role(SemanticsRole::Status);
                })
                .add(Some(6), 7, (10.0, 260.0, 200.0, 30.0), |c| {
                    c.set_label(archived.as_str());
                });
            fixture.owner.flush();
        });
        let mut heard = heard.marked(&session.nonce);
        heard.sort();
        let mut expected = vec![(1, saved), (2, error), (1, archived)];
        expected.sort();
        assert_eq!(heard, expected);
    }

    /// An announcement is heard at its urgency, and again when it repeats.
    fn an_announcement_is_heard_at_its_urgency(session: &Session) {
        let name = session.marked("Announcements");
        let mut fixture = Fixture::new(&name);
        let ids = SemanticsIdentitySource::default()
            .allocate(2)
            .expect("a fresh source has identities");
        let announcements = Announcements::new(ids[0], ids[1]);
        announcements.set_listening(true);
        fixture.owner.set_announcements(announcements.clone());
        fixture.owner.flush();
        let announcer = announcements.announcer(Arc::new(|| {}));
        let _ = session.window(&name);

        let (copied, lost) = (session.marked("Copied"), session.marked("Connection lost"));
        let heard = session.hear(|| {
            announcer
                .announce(&copied, Urgency::Polite)
                .expect("the announcement is accepted");
            announcer
                .announce(&lost, Urgency::Assertive)
                .expect("the announcement is accepted");
            fixture.owner.flush();
            announcer
                .announce(&copied, Urgency::Polite)
                .expect("the announcement is accepted");
            fixture.owner.flush();
        });
        let heard = heard.marked(&session.nonce);
        let polite: Vec<&str> = heard
            .iter()
            .filter(|(politeness, _)| *politeness == 1)
            .map(|(_, text)| text.trim_end_matches('\u{200B}'))
            .collect();
        let assertive: Vec<&str> = heard
            .iter()
            .filter(|(politeness, _)| *politeness == 2)
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(polite, [copied.as_str(), copied.as_str()], "{heard:?}");
        assert_eq!(assertive, [lost.as_str()], "{heard:?}");
    }

    /// Once the window is the active one, each move of the tree's focus is
    /// heard on the node that took it.
    fn focus_follows_the_tree_once_the_window_is_active(session: &Session) {
        let name = session.marked("Focus");
        let mut fixture = Fixture::new(&name);
        for (index, label, top) in [(2, "First", 10.0), (3, "Second", 50.0)] {
            fixture.add(Some(1), index, (10.0, top, 100.0, 30.0), |c| {
                c.set_role(SemanticsRole::Button);
                c.set_label(label);
                c.add_action(SemanticsAction::Focus, Rc::new(|_| {}));
                c.set_focused(index == 2);
            });
        }
        fixture.owner.flush();
        let window = session.window(&name);
        let second = block_on(async {
            let buttons = children(&session.bus, &window).await;
            buttons[1].inner().path().to_string()
        });
        fixture.activate();

        let heard = session.hear(|| {
            fixture
                .change(2, |c| c.set_focused(false))
                .change(3, |c| c.set_focused(true));
            fixture.owner.flush();
        });
        assert_eq!(heard.focused, [second]);
    }
}
