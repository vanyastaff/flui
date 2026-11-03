//! The method table (ADR-0238 §1): every operation the protocol knows, each with its wire
//! name, the side that serves it, its effect class, its MCP tier and its typed parameters and
//! output.
//!
//! [`METHODS`] is the one list. The MCP tool list (in this order, tool name `group_verb`), the
//! CLI's `flui drive <group> <verb>` subcommands, the agent's dispatch and the schema golden
//! file are all read from it, so none of the front ends can know a method the others do not.
//! [`Method`] is sealed: a method exists only by being declared here.
//!
//! The rest of the design's table joins as its tasks land: the OS lanes (`os.*`,
//! `input.os`, `capture.os`), pixel baselines, recording and traces, layout facts, the other
//! audits, review and reports, devices and exploration.

use std::fmt;
use std::time::Duration;

use crate::locator::Target;
use crate::tree::WindowId;

vocabulary! {
    /// Which side answers a method.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ServedBy {
        /// The agent inside the app: what the app's endpoint answers.
        Agent => "agent",
        /// The driver, composing agent methods; the app never sees it.
        Driver => "driver",
        /// A platform lane of the driver: OS input, OS trees, devices.
        Lane => "lane",
    }
}

vocabulary! {
    /// What calling a method can change, which MCP's annotations report.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum MethodEffect {
        /// Nothing in the app.
        ReadOnly => "read_only",
        /// The app's state, as a user's input would. Never resent on timeout.
        Acts => "acts",
        /// Something that cannot be taken back, such as ending the app.
        Destructive => "destructive",
    }
}

vocabulary! {
    /// Where MCP lists a method: the core set by default, or a capability group the client
    /// enables with `--caps` (R28a). Every method stays callable; the tier only decides what
    /// a model is shown.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Tier {
        /// Listed by default: at most [`MAX_CORE`] methods, enough to launch, read, locate,
        /// act, type, wait, capture and audit a screen.
        Core => "core",
        /// Session and window control beyond the core: attach, capabilities, placement,
        /// frame and settle waits, the animation clock.
        Session => "session",
        /// Raw input, the other gestures, actionability and the journal.
        Input => "input",
        /// The widget, render and layer trees.
        Inspect => "inspect",
        /// Captures beyond the presented frame, and baselines.
        Pixels => "pixels",
        /// Events, recordings and traces.
        Record => "record",
        /// Frame performance.
        Perf => "perf",
        /// Audits beyond the screen audit.
        Audit => "audit",
        /// Seeded exploration.
        Explore => "explore",
        /// Layout facts, assertions and snapshots.
        Layout => "layout",
        /// Coordinate targets and OS captures.
        Vision => "vision",
    }
}

/// The most methods [`Tier::Core`] may hold (R28a).
pub const MAX_CORE: usize = 13;

mod sealed {
    pub trait Sealed {}
}

/// One operation of the protocol. Each is an uninhabited type named for its method, so a
/// client writes `driver.call::<SemanticsLocate>(params)` and gets `Matches` back.
///
/// With the `serde` feature every `Params` and `Output` serializes both ways, and with
/// `schemars` each has a JSON schema; generic code states the bound it needs
/// (`where M::Params: serde::Serialize`), or reads the concrete types from
/// [`for_each_method!`](crate::for_each_method).
pub trait Method: sealed::Sealed + 'static {
    /// The wire name, `group.verb`; the CLI path is `<group> <verb>`, the MCP tool
    /// `group_verb`.
    const NAME: &'static str;
    /// Which side answers it.
    const SERVED_BY: ServedBy;
    /// What calling it can change.
    const EFFECT: MethodEffect;
    /// Where MCP lists it.
    const TIER: Tier;
    /// What it takes.
    type Params: fmt::Debug + Clone + 'static;
    /// What it answers.
    type Output: fmt::Debug + Clone + 'static;
}

/// A method one window of the app answers: the agent asks the window its parameters address
/// ([`Addressed::window`]), or the app's only window when they name none. Marked
/// `Agent(window)` in the table, and only there.
///
/// Its parameters and output cross from the agent's threads to the window's owner and back.
pub trait WindowMethod: Method<Params: Addressed + Send, Output: Send> {}

/// Parameters that address one window of the app.
pub trait Addressed {
    /// The window they name; `None` for a locator that names none, which searches the app's
    /// only window.
    fn window(&self) -> Option<WindowId>;

    /// How long the window may hold the request before it answers, past the time it takes to
    /// serve: a wait's timeout. A server bounding the reply adds it.
    fn holds_for(&self) -> Duration {
        Duration::ZERO
    }
}

macro_rules! addressed_by_window_field {
    ($($params:ty),+ $(,)?) => {$(
        impl Addressed for $params {
            fn window(&self) -> Option<WindowId> {
                Some(self.window)
            }
        }
    )+};
}

addressed_by_window_field!(
    crate::audit::AuditParams,
    crate::capture::CaptureParams,
    crate::input::JournalParams,
    crate::inspect::InspectParams,
    crate::motion::MotionParams,
    crate::perf::PerfStartParams,
    crate::events::AnnouncementsParams,
    crate::session::WindowParams,
    crate::tree::ReadParams,
    crate::wait::FramesParams,
    crate::wait::SettledParams,
);

impl Addressed for crate::input::InputParams {
    fn window(&self) -> Option<WindowId> {
        Some(self.window)
    }

    /// The input plays over its span before the answer.
    fn holds_for(&self) -> Duration {
        self.actions.span()
    }
}

impl Addressed for crate::act::ActionabilityParams {
    fn window(&self) -> Option<WindowId> {
        Some(self.element.window)
    }
}

impl Addressed for crate::act::ActParams {
    fn window(&self) -> Option<WindowId> {
        match &self.target {
            Target::Element(element) => Some(element.window),
            Target::Locator(locator) => locator.window,
            Target::Point { window, .. } => Some(*window),
        }
    }
}

impl Addressed for crate::locator::LocateParams {
    fn window(&self) -> Option<WindowId> {
        self.locator.window
    }
}

impl Addressed for crate::wait::WaitParams {
    fn window(&self) -> Option<WindowId> {
        Some(self.window)
    }

    fn holds_for(&self) -> Duration {
        Duration::from_millis(self.effective_timeout_ms())
    }
}

/// A method as data: one row of [`METHODS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MethodInfo {
    /// The wire name, `group.verb`.
    pub name: &'static str,
    /// Which side answers it.
    pub served_by: ServedBy,
    /// What calling it can change.
    pub effect: MethodEffect,
    /// Where MCP lists it.
    pub tier: Tier,
}

impl MethodInfo {
    /// `M`'s row.
    #[must_use]
    pub const fn of<M: Method>() -> Self {
        Self {
            name: M::NAME,
            served_by: M::SERVED_BY,
            effect: M::EFFECT,
            tier: M::TIER,
        }
    }

    /// The group, the part of the name before the dot.
    #[must_use]
    pub fn group(&self) -> &'static str {
        self.name
            .split_once('.')
            .map_or(self.name, |(group, _)| group)
    }

    /// The verb, the part of the name after the dot.
    #[must_use]
    pub fn verb(&self) -> &'static str {
        self.name.split_once('.').map_or("", |(_, verb)| verb)
    }

    /// The MCP tool name: the wire name with `_` for the dot.
    #[must_use]
    pub fn tool_name(&self) -> String {
        self.name.replace('.', "_")
    }
}

/// The method named `name` on the wire.
#[must_use]
pub fn find(name: &str) -> Option<&'static MethodInfo> {
    METHODS.iter().find(|method| method.name == name)
}

/// Hands the method table to a macro, with each method's concrete types.
///
/// `for_each_method!(rows)` expands to `rows! { … }` with one entry per method, sorted by wire
/// name:
///
/// ```text
/// $(#[doc = "…"])* Type = "group.verb", ServedBy $((window))?, MethodEffect, Tier, Params => Output;
/// ```
///
/// `Type` names [`flui_protocol::method::Type`](crate::method), and the three middle words
/// are variants of [`ServedBy`], [`MethodEffect`] and [`Tier`]. `Agent(window)` marks a
/// [`WindowMethod`]. `Params` and `Output` are full paths, so a dispatcher can deserialize them
/// and a schema writer can describe them without a generic bound naming serde or schemars in
/// this crate's signatures (ADR-0089 §2).
#[macro_export]
macro_rules! for_each_method {
    ($callback:ident) => {
        $crate::__method_table! { [$callback] }
    };
}

/// Hands the window methods ([`WindowMethod`]) to a macro, as
/// [`for_each_method!`](crate::for_each_method) hands every method: `for_each_window_method!(rows)`
/// expands to `rows! { … }` with one entry per window method, sorted by wire name:
///
/// ```text
/// Type = "group.verb", MethodEffect, Tier, Params => Output;
/// ```
///
/// A host that answers window methods generates its dispatch from it, so a window method the
/// table gains is one the host's build refuses to compile without.
#[macro_export]
macro_rules! for_each_window_method {
    ($callback:ident) => {
        $crate::__method_table! { [$crate::__window_rows] @rows $callback [] }
    };
}

/// Keeps the window methods of the table, one row at a time, and hands them on.
#[doc(hidden)]
#[macro_export]
macro_rules! __window_rows {
    (@rows $callback:ident [$($kept:tt)*]) => {
        $callback! { $($kept)* }
    };
    (
        @rows $callback:ident [$($kept:tt)*]
        $(#[$meta:meta])*
        $ty:ident = $name:literal, Agent(window), $effect:ident, $tier:ident,
            $params:ty => $output:ty;
        $($rest:tt)*
    ) => {
        $crate::__window_rows! {
            @rows $callback [$($kept)* $ty = $name, $effect, $tier, $params => $output;]
            $($rest)*
        }
    };
    (
        @rows $callback:ident [$($kept:tt)*]
        $(#[$meta:meta])*
        $ty:ident = $name:literal, $served:ident, $effect:ident, $tier:ident,
            $params:ty => $output:ty;
        $($rest:tt)*
    ) => {
        $crate::__window_rows! { @rows $callback [$($kept)*] $($rest)* }
    };
}

/// The table itself, handed to `$callback` after `$prefix`.
#[doc(hidden)]
#[macro_export]
macro_rules! __method_table {
    ([$($callback:tt)+] $($prefix:tt)*) => {
        $($callback)+! {
            $($prefix)*
            /// Accessibility rules over a window's published tree.
            AuditA11y = "audit.a11y", Agent(window), ReadOnly, Audit,
                $crate::audit::AuditParams => $crate::audit::AuditReport;
            /// The screen audit: every rule this build applies, saying what was checked and
            /// what was not.
            AuditScreen = "audit.screen", Driver, ReadOnly, Core,
                $crate::audit::AuditParams => $crate::audit::AuditReport;
            /// The pixels the window last put on screen.
            CapturePresented = "capture.presented", Agent(window), ReadOnly, Core,
                $crate::capture::CaptureParams => $crate::capture::CaptureMeta;
            /// The committed layer tree rendered again offscreen.
            CaptureRendered = "capture.rendered", Agent(window), ReadOnly, Pixels,
                $crate::capture::CaptureParams => $crate::capture::CaptureMeta;
            /// Attached, visible, stable, enabled and receives, from one frame.
            ElementActionability = "element.actionability", Agent(window), ReadOnly, Input,
                $crate::act::ActionabilityParams => $crate::act::Actionability;
            /// Two taps, close enough to count as one double tap.
            ElementDoubleTap = "element.double_tap", Driver, Acts, Input,
                $crate::input::TapParams => $crate::act::ActOutcome;
            /// Press on one target, move to another, release.
            ElementDragTo = "element.drag_to", Driver, Acts, Input,
                $crate::input::DragParams => $crate::act::ActOutcome;
            /// A stroke that ends above the fling velocity.
            ElementFling = "element.fling", Driver, Acts, Input,
                $crate::input::SwipeParams => $crate::act::ActOutcome;
            /// Press and hold.
            ElementLongPress = "element.long_press", Driver, Acts, Input,
                $crate::input::LongPressParams => $crate::act::ActOutcome;
            /// Two contacts spreading or closing.
            ElementPinch = "element.pinch", Driver, Acts, Input,
                $crate::input::PinchParams => $crate::act::ActOutcome;
            /// Key chords, in turn.
            ElementPressKeys = "element.press_keys", Driver, Acts, Core,
                $crate::input::PressKeysParams => $crate::act::ActOutcome;
            /// Scroll until the element is visible.
            ElementScrollIntoView = "element.scroll_into_view", Driver, Acts, Core,
                $crate::input::ScrollIntoViewParams => $crate::act::ActOutcome;
            /// A stroke that ends below the fling velocity.
            ElementSwipe = "element.swipe", Driver, Acts, Input,
                $crate::input::SwipeParams => $crate::act::ActOutcome;
            /// Locate, check actionability, tap, observe.
            ElementTap = "element.tap", Driver, Acts, Core,
                $crate::input::TapParams => $crate::act::ActOutcome;
            /// Focus the element and commit text into it.
            ElementTypeText = "element.type_text", Driver, Acts, Core,
                $crate::input::TypeTextParams => $crate::act::ActOutcome;
            /// Stream events.
            EventsSubscribe = "events.subscribe", Agent, ReadOnly, Record,
                $crate::events::SubscribeParams => $crate::events::Subscription;
            /// Stop a stream.
            EventsUnsubscribe = "events.unsubscribe", Agent, ReadOnly, Record,
                $crate::events::Subscription => $crate::session::Empty;
            /// Synthetic input into the window's own input path.
            InputInject = "input.inject", Agent(window), Acts, Input,
                $crate::input::InputParams => $crate::act::ActOutcome;
            /// The input delivered since a sequence number.
            InputJournal = "input.journal", Agent(window), ReadOnly, Input,
                $crate::input::JournalParams => $crate::input::Journal;
            /// Input by the path the policy chooses.
            InputPerform = "input.perform", Driver, Acts, Input,
                $crate::input::InputParams => $crate::act::ActOutcome;
            /// Release every contact and key the agent holds.
            InputRelease = "input.release", Agent(window), Acts, Input,
                $crate::session::WindowParams => $crate::input::Released;
            /// The layer tree.
            InspectLayers = "inspect.layers", Agent(window), ReadOnly, Inspect,
                $crate::inspect::InspectParams => $crate::inspect::InspectReading;
            /// The render tree.
            InspectRender = "inspect.render", Agent(window), ReadOnly, Inspect,
                $crate::inspect::InspectParams => $crate::inspect::InspectReading;
            /// The widget (element) tree.
            InspectWidgets = "inspect.widgets", Agent(window), ReadOnly, Inspect,
                $crate::inspect::InspectParams => $crate::inspect::InspectReading;
            /// Change, step or read the animation clock (ADR-0176).
            MotionSet = "motion.set", Agent(window), Acts, Session,
                $crate::motion::MotionParams => $crate::motion::MotionState;
            /// Judge runs against a target's budget.
            PerfExpect = "perf.expect", Driver, ReadOnly, Perf,
                $crate::perf::PerfExpectParams => $crate::perf::PerfVerdict;
            /// Begin keeping frame samples.
            PerfStart = "perf.start", Agent(window), ReadOnly, Perf,
                $crate::perf::PerfStartParams => $crate::session::Empty;
            /// End the span and report it.
            PerfStop = "perf.stop", Agent(window), ReadOnly, Perf,
                $crate::session::WindowParams => $crate::perf::PerfReport;
            /// Record what a window's UI runtime applies, sent row by row as `record` events.
            RecordStart = "record.start", Agent, ReadOnly, Record,
                $crate::record::RecordParams => $crate::record::Recording;
            /// End a recording, once its `end` row was sent.
            RecordStop = "record.stop", Agent, ReadOnly, Record,
                $crate::record::RecordStopParams => $crate::record::RecordSummary;
            /// One semantics action on one element.
            SemanticsAct = "semantics.act", Agent(window), Acts, Core,
                $crate::act::ActParams => $crate::act::ActOutcome;
            /// The announcements spoken since a sequence number.
            SemanticsAnnouncements = "semantics.announcements", Agent(window), ReadOnly, Record,
                $crate::events::AnnouncementsParams => $crate::events::AnnouncementJournal;
            /// The elements a locator matches now.
            SemanticsLocate = "semantics.locate", Agent(window), ReadOnly, Core,
                $crate::locator::LocateParams => $crate::locator::Matches;
            /// A window's semantics tree.
            SemanticsRead = "semantics.read", Agent(window), ReadOnly, Core,
                $crate::tree::ReadParams => $crate::tree::Tree;
            /// Connect to an app already running with the agent.
            SessionAttach = "session.attach", Driver, Acts, Session,
                $crate::session::AttachParams => $crate::session::SessionInfo;
            /// What the target can do.
            SessionCapabilities = "session.capabilities", Driver, ReadOnly, Session,
                $crate::session::Empty => $crate::session::Capabilities;
            /// End the session, and the app it launched.
            SessionClose = "session.close", Driver, Destructive, Core,
                $crate::session::CloseParams => $crate::session::Closed;
            /// Start an app with the agent attached.
            SessionLaunch = "session.launch", Driver, Acts, Core,
                $crate::session::LaunchParams => $crate::session::SessionInfo;
            /// Hold out until a window presents more frames.
            WaitFrames = "wait.frames", Agent(window), ReadOnly, Session,
                $crate::wait::FramesParams => $crate::wait::WaitOutcome;
            /// Hold out until a window settles.
            WaitSettled = "wait.settled", Agent(window), ReadOnly, Session,
                $crate::wait::SettledParams => $crate::wait::WaitOutcome;
            /// Hold out for a condition on the frame and event timeline.
            WaitUntil = "wait.until", Agent(window), ReadOnly, Core,
                $crate::wait::WaitParams => $crate::wait::WaitOutcome;
            /// A window's size, scale, position and focus.
            WindowPlacement = "window.placement", Agent(window), ReadOnly, Session,
                $crate::session::WindowParams => $crate::session::Placement;
            /// The app's windows.
            WindowsList = "windows.list", Agent, ReadOnly, Core,
                $crate::session::Empty => $crate::session::WindowList;
        }
    };
}

/// A row's scope: `Agent(window)` declares a [`WindowMethod`]; any other marker, or one on a
/// method the agent does not serve, does not compile.
macro_rules! scoped {
    ($ty:ident, $served:ident) => {};
    ($ty:ident, Agent, window) => {
        impl WindowMethod for $ty {}
    };
}

macro_rules! declare_methods {
    (
        $(
            $(#[$meta:meta])*
            $ty:ident = $name:literal, $served:ident $(($scope:ident))?, $effect:ident,
                $tier:ident, $params:ty => $output:ty;
        )+
    ) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum $ty {}

            impl sealed::Sealed for $ty {}

            impl Method for $ty {
                const NAME: &'static str = $name;
                const SERVED_BY: ServedBy = ServedBy::$served;
                const EFFECT: MethodEffect = MethodEffect::$effect;
                const TIER: Tier = Tier::$tier;
                type Params = $params;
                type Output = $output;
            }

            scoped!($ty, $served $(, $scope)?);
        )+

        /// Every method, sorted by wire name: the order MCP lists tools in.
        ///
        /// Generated from the same list as the method types, so a method cannot be declared
        /// without a row.
        pub const METHODS: &[MethodInfo] = &[$(MethodInfo::of::<$ty>()),+];
    };
}

for_each_method!(declare_methods);
