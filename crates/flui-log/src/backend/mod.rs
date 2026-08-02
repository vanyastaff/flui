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

pub(crate) mod logcat;
pub(crate) mod record;

use core::marker::PhantomData;

use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};
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
    Compact(Box<tracing_subscriber::fmt::Layer<S>>),

    #[cfg(all(
        feature = "hierarchical",
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    ))]
    Hierarchical(
        tracing_forest::ForestLayer<tracing_forest::PrettyPrinter, tracing_forest::tag::NoTag>,
    ),

    #[cfg(target_os = "android")]
    Logcat(logcat::LogcatLayer),

    #[cfg(any(
        target_os = "ios",
        all(target_os = "macos", feature = "apple-unified-logging")
    ))]
    AppleUnified(tracing_oslog::OsLogger),

    #[cfg(target_arch = "wasm32")]
    WebConsole(tracing_wasm::WASMLayer),

    #[allow(
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
/// use flui_log::{LogConfig, PlatformLayer, SubscriberPolicy, install_subscriber};
/// use tracing_subscriber::{Registry, layer::SubscriberExt as _};
///
/// let config = LogConfig::default();
/// let subscriber = Registry::default()
///     .with(config.filter().env_filter()?)
///     .with(PlatformLayer::platform_default(&config));
/// // ... stack further layers here ...
/// install_subscriber(subscriber, SubscriberPolicy::Auto)?;
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

    /// Android logcat, tagged with the event target and falling back to the
    /// identity's display name.
    #[cfg(target_os = "android")]
    #[must_use]
    pub fn logcat(identity: &crate::AppIdentity) -> Self {
        Self {
            sink: Sink::Logcat(logcat::LogcatLayer::new(identity.display_name())),
        }
    }

    /// Apple unified logging, under the identity's subsystem and FLUI's stable
    /// category.
    ///
    /// # Privacy
    ///
    /// `tracing-oslog` renders each event into one already-formatted string and
    /// hands that to `os_log_with_type`, so **every field FLUI emits is public
    /// in the unified log**: `os_log`'s `%{private}` redaction applies to
    /// interpolated arguments, and there are none. Treat a tracing field on an
    /// Apple platform as readable by anyone holding the device's log archive,
    /// and keep secrets and personal data out of them. This crate cannot make
    /// that decision on the producer's behalf — the value must not be recorded
    /// in the first place.
    #[cfg(any(
        target_os = "ios",
        all(target_os = "macos", feature = "apple-unified-logging")
    ))]
    #[must_use]
    pub fn apple_unified_logging(identity: &crate::AppIdentity) -> Self {
        Self {
            sink: Sink::AppleUnified(tracing_oslog::OsLogger::new(
                identity.apple_subsystem(),
                crate::identity::APPLE_CATEGORY,
            )),
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
    Sink::Logcat(logcat::LogcatLayer::new(config.identity().display_name()))
}

#[cfg(target_os = "ios")]
fn default_sink<S>(config: &crate::LogConfig) -> Sink<S> {
    Sink::AppleUnified(tracing_oslog::OsLogger::new(
        config.identity().apple_subsystem(),
        crate::identity::APPLE_CATEGORY,
    ))
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
        crate::DesktopFormat::Compact => Sink::Compact(Box::new(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_line_number(true),
        )),

        #[cfg(feature = "hierarchical")]
        crate::DesktopFormat::Hierarchical => {
            Sink::Hierarchical(tracing_forest::ForestLayer::default())
        }
    }
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
}
