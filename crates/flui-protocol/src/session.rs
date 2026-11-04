//! The session around the calls: the `hello` that opens a connection, launching or attaching
//! to an app, what the target can do, and its windows (ADR-0238 §2, §5).

use crate::capture::CaptureSource;
use crate::geometry::{LogicalPoint, LogicalSize};
use crate::input::{InputPath, InputPolicy, PointerDevice};
use crate::tree::WindowId;
use crate::version::ProtocolVersion;

/// Parameters of a method that takes none: `{}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Empty {}

/// Parameters naming one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct WindowParams {
    /// The window.
    pub window: WindowId,
}

impl WindowParams {
    /// `window`.
    #[must_use]
    pub const fn new(window: WindowId) -> Self {
        Self { window }
    }
}

vocabulary! {
    /// What a client may do on its connection, named in its hello (ADR-0288 §8).
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ClientRole {
        /// Reads, subscribes and acts. One driver is admitted per app at a time; another
        /// driver's hello is refused `not_driver`. When the app was launched to exit on its
        /// driver's disconnect, this is the connection it exits with. The default.
        #[default]
        Driver => "driver",
        /// Reads and subscribes, beside the driver. A call that acts is refused `not_driver`,
        /// and its disconnect never ends the app.
        Observer => "observer",
    }
}

impl ClientRole {
    /// Whether this is [`Self::Driver`], the role a hello that names none asks for.
    #[must_use]
    pub const fn is_driver(&self) -> bool {
        matches!(self, Self::Driver)
    }
}

/// The first line a client sends: `{"hello": {…}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Hello {
    /// The token the app was launched with.
    pub token: String,
    /// The schema the client speaks; an app that shares none answers `version`.
    pub protocol: ProtocolVersion,
    /// The client and its version, such as `flui-driver/0.1.0`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub client: Option<String>,
    /// What the client may do on this connection; absent, it asks to drive.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "ClientRole::is_driver")
    )]
    pub role: ClientRole,
}

impl Hello {
    /// A hello presenting `token` in `protocol` as the app's driver, naming no client.
    #[must_use]
    pub fn new(token: impl Into<String>, protocol: ProtocolVersion) -> Self {
        Self {
            token: token.into(),
            protocol,
            client: None,
            role: ClientRole::Driver,
        }
    }

    /// Naming the client, such as `flui-driver/0.1.0`.
    #[must_use]
    pub fn with_client(mut self, client: impl Into<String>) -> Self {
        self.client = Some(client.into());
        self
    }

    /// Asking for `role` instead of the driver's.
    #[must_use]
    pub const fn with_role(mut self, role: ClientRole) -> Self {
        self.role = role;
        self
    }
}

/// Who answered a hello.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AppInfo {
    /// The application's name.
    pub name: String,
    /// Its process id, where it has one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub pid: Option<u32>,
    /// The target it runs on, such as `windows`, `android` or `headless`.
    pub target: String,
}

impl AppInfo {
    /// The app `name`, running on `target`.
    #[must_use]
    pub fn new(name: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pid: None,
            target: target.into(),
        }
    }

    /// With its process id.
    #[must_use]
    pub const fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
}

/// What an app answers a hello with.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct HelloReply {
    /// The schema the app speaks on this connection.
    pub protocol: ProtocolVersion,
    /// The app.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub app: Option<AppInfo>,
    /// What it can do.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub capabilities: Option<Capabilities>,
}

impl HelloReply {
    /// A reply in `protocol`, naming neither the app nor what it can do.
    #[must_use]
    pub const fn new(protocol: ProtocolVersion) -> Self {
        Self {
            protocol,
            app: None,
            capabilities: None,
        }
    }

    /// Naming the app.
    #[must_use]
    pub fn with_app(mut self, app: AppInfo) -> Self {
        self.app = Some(app);
        self
    }

    /// Saying what it can do.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }
}

vocabulary! {
    /// Where OS input runs, so it cannot reach the user's own session (R14b).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Isolation {
        /// In the test process: no OS input at all.
        InProcess => "in_process",
        /// The user's own desktop: OS input is announced and guarded.
        Shared => "shared",
        /// Windows Sandbox.
        Sandbox => "sandbox",
        /// A second interactive user session.
        SecondSession => "second_session",
        /// An X virtual framebuffer.
        Xvfb => "xvfb",
        /// A separate seat, or a nested or headless compositor.
        Seat => "seat",
        /// A virtual machine.
        VirtualMachine => "virtual_machine",
        /// An Android emulator.
        Emulator => "emulator",
        /// The iOS Simulator.
        Simulator => "simulator",
        /// A headless browser.
        HeadlessBrowser => "headless_browser",
        /// A physical device.
        Device => "device",
    }
}

/// What a session's target can do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Capabilities {
    /// The input paths it takes.
    pub paths: Vec<InputPath>,
    /// The pointer devices it takes.
    pub pointers: Vec<PointerDevice>,
    /// Where its captures can come from.
    pub captures: Vec<CaptureSource>,
    /// Where its OS input runs, when it takes any.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub isolation: Option<Isolation>,
    /// The methods it answers, by wire name.
    pub methods: Vec<String>,
    /// The layers `record.start` records; none when it does not record.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub record: Vec<crate::record::RecordLayer>,
}

/// `session.launch`: start an app with the agent attached.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct LaunchParams {
    /// The target, such as `windows`, `android:emulator-5554` or `headless`.
    pub target: String,
    /// What to launch: an executable, `.app`, `.apk` or wasm bundle; the project's build when
    /// absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub artifact: Option<String>,
    /// Arguments for the app.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub args: Vec<String>,
    /// The session's input path.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub input: InputPolicy,
    /// Where OS input must run; the lane's default when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub isolation: Option<Isolation>,
}

impl LaunchParams {
    /// The project's build on `target`.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            artifact: None,
            args: Vec::new(),
            input: InputPolicy::default(),
            isolation: None,
        }
    }
}

/// `session.attach`: connect to an app already running with the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AttachParams {
    /// The app's endpoint: a pipe name, socket path or forwarded port.
    pub endpoint: String,
    /// The file holding its token; never the token itself.
    pub token_file: String,
    /// The session's input path.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub input: InputPolicy,
}

/// What `session.launch` and `session.attach` answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SessionInfo {
    /// The schema the app speaks.
    pub protocol: ProtocolVersion,
    /// The app.
    pub app: AppInfo,
    /// Its windows when the session began.
    pub windows: Vec<WindowId>,
    /// What it can do.
    pub capabilities: Capabilities,
}

/// `session.close`: end the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct CloseParams {
    /// Leave a launched app running. An attached app is never ended.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub keep_app: bool,
}

/// What `session.close` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Closed {
    /// The app's exit code, when the session ended it and the OS reported one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub exit: Option<i32>,
}

/// What `windows.list` answers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct WindowList {
    /// The app's windows with content, in the order they opened.
    pub windows: Vec<WindowId>,
}

impl WindowList {
    /// `windows`.
    #[must_use]
    pub fn new(windows: Vec<WindowId>) -> Self {
        Self { windows }
    }
}

/// What `window.placement` answers.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Placement {
    /// The window.
    pub window: WindowId,
    /// Its title, when it has one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub title: Option<String>,
    /// Its surface's size.
    pub size: LogicalSize,
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// Where its surface's top-left is on screen, in logical pixels, where the platform says.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub position: Option<LogicalPoint>,
    /// Whether it is shown, not minimized or occluded as far as the platform says.
    pub visible: bool,
    /// Whether it has keyboard focus.
    pub focused: bool,
}
