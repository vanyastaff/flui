//! Native log sinks and the layer that selects between them.
//!
//! # What each target gets
//!
//! | Target | Default backend | Where the output lands |
//! |---|---|---|
//! | Desktop (incl. macOS) | `tracing_subscriber::fmt`, or `tracing-forest` with the `hierarchical` feature | stdout |
//! | Android | logcat via `__android_log_write` | `adb logcat` |
//! | iOS | Apple unified logging | Console.app, Xcode, `log stream` |
//! | wasm32 | browser console + performance timeline | `DevTools` |
//!
//! macOS deliberately keeps the desktop backend. Unified logging on macOS
//! would make `cargo run` print nothing, which is the opposite of what a
//! desktop developer wants; a bundled macOS application that does want
//! `os_log` enables the `apple-unified-logging` feature and asks for
//! `PlatformLayer::apple_unified_logging` explicitly. (Not an intra-doc link:
//! that constructor only exists on Apple targets, so the link would be broken
//! in every other target's rustdoc.)
//!
//! # No technical ceiling
//!
//! Every backend here accepts *every* level. Whether an event is emitted is
//! decided by the [`EnvFilter`](crate::filter) alone. A backend that also
//! filtered would be a second, invisible ceiling that no `RUST_LOG` directive
//! could raise — see [`crate::filter`] for the regression this prevents.
//!
//! # Privacy: device sinks are private by default
//!
//! Logcat and the Apple unified log are device archives readable outside the
//! application, so those two sinks — and only those two — are constructed
//! behind a redaction stage (the crate-private `redact` module): a dynamic
//! value (string, `Debug` or `Display` rendering, error) is replaced by
//! [`privacy::REDACTED_VALUE`]
//! unless its field name ends in `.public`; a scalar is published unless its
//! name ends in `.private`; a native `tracing` message is published, while a
//! message bridged from the `log` facade is third-party interpolated text and
//! is redacted. The contract, the
//! Apple model it is ported from, and its residual holes live in [`privacy`].
//! The desktop and web-console sinks write to the developer's own terminal or
//! `DevTools` and publish fields verbatim.

pub(crate) mod logcat;
pub mod privacy;
pub(crate) mod record;
pub(crate) mod redact;

use core::marker::PhantomData;

use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
use tracing_subscriber::fmt::format::{Compact, DefaultFields, Format};
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

pub use logcat::LogcatPriority;

/// The selected sink.
///
/// Every variant is target-gated, so exactly the sinks that can exist on this
/// target are present. `Unreachable` carries the subscriber type parameter on
/// targets whose sinks do not mention it themselves; it is uninhabited and
/// therefore never constructed.
enum Sink<S> {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    Compact(Box<tracing_subscriber::fmt::Layer<S, DefaultFields, Format<Compact>, BoxMakeWriter>>),

    #[cfg(all(
        feature = "hierarchical",
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    ))]
    Hierarchical(
        tracing_forest::ForestLayer<tracing_forest::PrettyPrinter, tracing_forest::tag::NoTag>,
    ),

    #[cfg(target_os = "android")]
    Logcat(redact::RedactLayer<logcat::LogcatLayer>),

    #[cfg(any(
        target_os = "ios",
        all(target_os = "macos", feature = "apple-unified-logging")
    ))]
    AppleUnified(redact::RedactLayer<tracing_oslog::OsLogger>),

    #[cfg(target_arch = "wasm32")]
    WebConsole(tracing_wasm::WASMLayer),

    #[expect(
        dead_code,
        reason = "uninhabited by construction; it exists to carry `S` on targets whose sink does not"
    )]
    Unreachable(core::convert::Infallible, PhantomData<fn(S)>),
}

/// Run `$body` against whichever sink variant is present.
///
/// Every arm is target-gated identically to the variant it matches, so on most
/// targets this expands to a single-arm match.
macro_rules! dispatch_sink {
    ($scrutinee:expr, $sink:pat => $body:expr) => {
        match $scrutinee {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
            Sink::Compact($sink) => $body,
            #[cfg(all(
                feature = "hierarchical",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
            ))]
            Sink::Hierarchical($sink) => $body,
            #[cfg(target_os = "android")]
            Sink::Logcat($sink) => $body,
            #[cfg(any(
                target_os = "ios",
                all(target_os = "macos", feature = "apple-unified-logging")
            ))]
            Sink::AppleUnified($sink) => $body,
            #[cfg(target_arch = "wasm32")]
            Sink::WebConsole($sink) => $body,
            Sink::Unreachable(never, _) => match *never {},
        }
    };
}

impl<S> Sink<S> {
    /// Human-readable backend name, for diagnostics and tests.
    fn name(&self) -> &'static str {
        match self {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
            Sink::Compact(_) => "desktop-compact",
            #[cfg(all(
                feature = "hierarchical",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
            ))]
            Sink::Hierarchical(_) => "desktop-hierarchical",
            #[cfg(target_os = "android")]
            Sink::Logcat(_) => "android-logcat",
            #[cfg(any(
                target_os = "ios",
                all(target_os = "macos", feature = "apple-unified-logging")
            ))]
            Sink::AppleUnified(_) => "apple-unified-logging",
            #[cfg(target_arch = "wasm32")]
            Sink::WebConsole(_) => "web-console",
            Sink::Unreachable(never, _) => match *never {},
        }
    }
}

impl<S> core::fmt::Debug for Sink<S> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_tuple("Sink").field(&self.name()).finish()
    }
}

/// The platform's default log sink, as a composable [`Layer`].
///
/// Public and concrete so a composition root can stack its own layers around
/// it — a devtools timeline collector, a metrics exporter, a test capture —
/// without going through [`crate::setup`], and without this crate introducing
/// a `dyn` boundary of its own.
///
/// ```rust,no_run
/// use flui_log::{InstallPolicy, LogBridgePolicy, LogConfig, PlatformLayer, install_subscriber};
/// use tracing_subscriber::{Registry, layer::SubscriberExt as _, Layer as _};
///
/// let config = LogConfig::default();
/// let filter = config.env_filter()?;
/// let subscriber = Registry::default()
///     .with(PlatformLayer::platform_default(&config).with_filter(filter));
/// // ... stack further layers here ...
/// install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct PlatformLayer<S> {
    sink: Sink<S>,
}

impl<S> PlatformLayer<S>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    /// The backend this compilation target uses by default.
    #[must_use]
    pub fn platform_default(config: &crate::LogConfig) -> Self {
        Self {
            sink: default_sink(config),
        }
    }

    /// The desktop formatter, in the requested shape.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    #[must_use]
    pub fn desktop(format: crate::DesktopFormat) -> Self {
        Self {
            sink: desktop_sink(format),
        }
    }

    /// Compact desktop output written to stderr.
    ///
    /// Command-line composition roots use this so diagnostics never corrupt
    /// stdout protocols such as shell completions or JSON output.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    #[must_use]
    pub fn desktop_compact_stderr() -> Self {
        Self {
            sink: compact_sink(BoxMakeWriter::new(std::io::stderr)),
        }
    }

    /// Android logcat, tagged with the event target and falling back to the
    /// identity's display name.
    ///
    /// # Privacy
    ///
    /// Constructed behind the redaction stage: fields are private by default
    /// and publish only under the rules in [`privacy`]. Android itself has no
    /// `os_log`-style redaction knob — logcat is a plain line transport — so
    /// the classification is applied before the line is rendered, and a
    /// redacted value never leaves the process.
    #[cfg(target_os = "android")]
    #[must_use]
    pub fn logcat(identity: &crate::AppIdentity) -> Self {
        Self {
            sink: Sink::Logcat(redact::RedactLayer::new(logcat::LogcatLayer::new(
                identity.display_name(),
            ))),
        }
    }

    /// Apple unified logging, under the identity's subsystem and FLUI's stable
    /// category.
    ///
    /// # Privacy
    ///
    /// Constructed behind the redaction stage: fields are **private by
    /// default** and publish only under the rules in [`privacy`] — a scalar
    /// or a field whose name ends in `.public` is published, everything else
    /// reaches the unified log as `<private>`. The classification is enforced
    /// per event, in-process, before `tracing-oslog` renders; a producer does
    /// not have to know this sink is installed, and nothing a producer forgets
    /// to mark can land in the log archive verbatim.
    ///
    /// One deliberate divergence from native `os_log` redaction: the OS
    /// substitutes `<private>` at *display* time and can reveal the stored
    /// value under the "Enable-Private-Data" developer profile, whereas this
    /// stage redacts before the value ever reaches `os_log_with_type`
    /// (`tracing-oslog` collapses each event into one pre-formatted
    /// `%{public}s` argument, which forecloses per-value `%{private}`
    /// specifiers). The value is therefore never stored — strictly stronger
    /// privacy, at the cost of that reveal workflow. Reclaiming it would take
    /// a FLUI-owned `os_log` shim that keeps public and private renderings as
    /// separate format arguments; the classification layer here is
    /// transport-agnostic on purpose so such a sink could adopt it unchanged.
    ///
    /// # Grouping
    ///
    /// Subsystem and category are fixed when this layer is built. Unlike
    /// logcat's tag, they are **not** derived from the event's target, so
    /// `log stream --predicate` selects a FLUI application as a whole rather
    /// than one crate within it, and the emitting crate appears only inside the
    /// formatted message.
    ///
    /// This is why the logcat sink normalises a record bridged from `log` and
    /// this one has nothing to normalise: there, the target *is* the tag, so
    /// leaving it as the literal `"log"` would collapse every `wgpu` and `naga`
    /// line into one bucket. Here the target was never the grouping key.
    /// Per-crate selection on Apple needs a different mechanism — a category
    /// per subsystem area — and is not something normalisation can supply.
    #[cfg(any(
        target_os = "ios",
        all(target_os = "macos", feature = "apple-unified-logging")
    ))]
    #[must_use]
    pub fn apple_unified_logging(identity: &crate::AppIdentity) -> Self {
        Self {
            sink: Sink::AppleUnified(redact::RedactLayer::new(tracing_oslog::OsLogger::new(
                identity.apple_subsystem(),
                crate::identity::APPLE_CATEGORY,
            ))),
        }
    }

    /// Browser console, with spans reported to the performance timeline.
    #[cfg(target_arch = "wasm32")]
    #[must_use]
    pub fn web_console() -> Self {
        Self {
            sink: web_console_sink(),
        }
    }

    /// Which backend this layer writes to. Stable enough to assert on.
    #[inline]
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        self.sink.name()
    }
}

// --- target-selected default -------------------------------------------------
//
// One cfg-gated constructor per target rather than a cfg ladder inside one
// body, so each target's default is a single readable definition.

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn default_sink<S>(config: &crate::LogConfig) -> Sink<S> {
    desktop_sink(config.desktop_format())
}

#[cfg(target_os = "android")]
fn default_sink<S>(config: &crate::LogConfig) -> Sink<S> {
    Sink::Logcat(redact::RedactLayer::new(logcat::LogcatLayer::new(
        config.identity().display_name(),
    )))
}

#[cfg(target_os = "ios")]
fn default_sink<S>(config: &crate::LogConfig) -> Sink<S> {
    Sink::AppleUnified(redact::RedactLayer::new(tracing_oslog::OsLogger::new(
        config.identity().apple_subsystem(),
        crate::identity::APPLE_CATEGORY,
    )))
}

#[cfg(target_arch = "wasm32")]
fn default_sink<S>(config: &crate::LogConfig) -> Sink<S> {
    let _ = config;
    web_console_sink()
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn desktop_sink<S>(format: crate::DesktopFormat) -> Sink<S> {
    match format {
        // `with_target(true)` diverges from the historical backend, which hid
        // the target. The target is the exact string a `RUST_LOG` directive
        // matches on, so hiding it left an author guessing at what to write.
        crate::DesktopFormat::Compact => compact_sink(BoxMakeWriter::new(std::io::stdout)),

        #[cfg(feature = "hierarchical")]
        crate::DesktopFormat::Hierarchical => {
            Sink::Hierarchical(tracing_forest::ForestLayer::default())
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn compact_sink<S>(writer: BoxMakeWriter) -> Sink<S> {
    Sink::Compact(Box::new(
        tracing_subscriber::fmt::layer()
            .compact()
            .with_writer(writer)
            .with_target(true)
            .with_level(true)
            .with_line_number(true),
    ))
}

#[cfg(target_arch = "wasm32")]
fn web_console_sink<S>() -> Sink<S> {
    // TRACE, not the application's level: the `EnvFilter` is the only filter.
    // `WASMLayer::enabled` compares against this value directly, so anything
    // lower here is exactly the second ceiling this crate exists to remove.
    let config = tracing_wasm::WASMLayerConfigBuilder::new()
        .set_max_level(tracing::Level::TRACE)
        .set_report_logs_in_timings(true)
        .build();

    Sink::WebConsole(tracing_wasm::WASMLayer::new(config))
}

// --- Layer forwarding --------------------------------------------------------

impl<S> Layer<S> for PlatformLayer<S>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_register_dispatch(&self, subscriber: &tracing::Dispatch) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_register_dispatch(sink, subscriber));
    }

    fn on_layer(&mut self, subscriber: &mut S) {
        dispatch_sink!(&mut self.sink, sink => Layer::<S>::on_layer(sink, subscriber));
    }

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        dispatch_sink!(&self.sink, sink => Layer::<S>::register_callsite(sink, metadata))
    }

    fn enabled(&self, metadata: &Metadata<'_>, context: Context<'_, S>) -> bool {
        dispatch_sink!(&self.sink, sink => Layer::<S>::enabled(sink, metadata, context))
    }

    /// Forwarded verbatim, never narrowed.
    ///
    /// A sink that returns `None` has no opinion about the maximum level, which
    /// is what lets the `EnvFilter` decide alone.
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        dispatch_sink!(&self.sink, sink => Layer::<S>::max_level_hint(sink))
    }

    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_new_span(sink, attributes, id, context));
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_record(sink, span, values, context));
    }

    fn on_follows_from(&self, span: &Id, follows: &Id, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_follows_from(sink, span, follows, context));
    }

    fn event_enabled(&self, event: &Event<'_>, context: Context<'_, S>) -> bool {
        dispatch_sink!(&self.sink, sink => Layer::<S>::event_enabled(sink, event, context))
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_event(sink, event, context));
    }

    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_enter(sink, id, context));
    }

    fn on_exit(&self, id: &Id, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_exit(sink, id, context));
    }

    fn on_close(&self, id: Id, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_close(sink, id, context));
    }

    fn on_id_change(&self, old: &Id, new: &Id, context: Context<'_, S>) {
        dispatch_sink!(&self.sink, sink => Layer::<S>::on_id_change(sink, old, new, context));
    }

    #[expect(
        unsafe_code,
        reason = "forwards tracing-subscriber's type-erased layer lookup without changing the pointer"
    )]
    unsafe fn downcast_raw(&self, id: core::any::TypeId) -> Option<*const ()> {
        dispatch_sink!(&self.sink, sink => {
            // SAFETY: the inner layer owns the downcast contract; this wrapper
            // returns its pointer unchanged and does not dereference it.
            unsafe { Layer::<S>::downcast_raw(sink, id) }
        })
    }
}
