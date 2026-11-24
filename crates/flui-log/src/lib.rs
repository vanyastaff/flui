//! Cross-platform logging **backend** for FLUI.
//!
//! # This crate is for composition roots only
//!
//! `flui-log` assembles a `tracing` subscriber: it picks the native sink for
//! the target, resolves the filter, carries the application identity the native
//! sinks need, and applies an explicit ownership policy to the process-global
//! subscriber slot.
//!
//! It is **not** how framework code reaches the logging macros. `flui-view`,
//! `flui-rendering`, `flui-widgets`, `flui-engine` and every other library
//! depend on `tracing` and nothing else — they emit events and have no opinion
//! about where those events go. Only a composition root (`flui-app`,
//! `flui-cli`, the `flui` facade, an example application) depends on this
//! crate; the `allowed-dependents` list in this crate's manifest names them.
//!
//! That split is the whole point. Replacing or removing the default backend
//! must never touch an instrumentation call site.
//!
//! # Ownership, not configuration
//!
//! Installing a subscriber writes process-global state exactly once. FLUI can
//! be embedded in a process whose observability somebody else already owns, so
//! the question "may I install one?" gets an explicit answer rather than a
//! panic:
//!
//! | [`SubscriberPolicy`] | Effect |
//! |---|---|
//! | [`Inherit`](SubscriberPolicy::Inherit) | Installs nothing, reads nothing, changes nothing |
//! | [`Auto`](SubscriberPolicy::Auto) | Installs the platform default *only* if the slot is empty; an existing subscriber is preserved |
//! | [`Install`](SubscriberPolicy::Install) | Demands the slot, and returns [`SetupError::SubscriberAlreadyInstalled`] if it is taken |
//!
//! Every one of them returns a [`SubscriberInstallation`] describing both the
//! subscriber slot and the `log` compatibility bridge. None of them panics.
//!
//! ```rust,no_run
//! use flui_log::config::LogConfig;
//! use flui_log::ownership::{SubscriberOwnership, SubscriberPolicy};
//!
//! let installation = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)?;
//! if installation.ownership == SubscriberOwnership::Unchanged {
//!     tracing::debug!("the host already owns logging; FLUI installed nothing");
//! }
//! # Ok::<(), flui_log::ownership::SetupError>(())
//! ```
//!
//! # Composing your own stack
//!
//! [`setup`] is the convenience path. A tool that needs its own layers builds
//! the pieces and installs the result itself:
//!
//! ```rust,no_run
//! use flui_log::backend::PlatformLayer;
//! use flui_log::config::LogConfig;
//! use flui_log::filter::FilterConfig;
//! use flui_log::ownership::{InstallPolicy, LogBridgePolicy, install_subscriber};
//! use tracing_subscriber::{Registry, layer::SubscriberExt as _, Layer as _};
//!
//! let config = LogConfig::builder()
//!     .filter(FilterConfig::new("info,flui_view=trace"))
//!     .build();
//! let filter = config.filter().targets()?;
//!
//! let subscriber = Registry::default()
//!     .with(PlatformLayer::platform_default(&config).with_filter(filter));
//! //  .with(my_timeline_layer)   <- devtools stacks here
//!
//! install_subscriber(subscriber, InstallPolicy::Install, LogBridgePolicy::Auto)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Contract targets
//!
//! The framework emits its spans and events under `flui.<subsystem>` targets, which a directive
//! selects by prefix (`flui.tree` also selects `flui.tree.reconcile`):
//!
//! | Target | What it carries |
//! |---|---|
//! | `flui.frame` | the `frame` span and its phases: `build`, `layout`, `compositing`, `paint`, `semantics`, `commit`, `submit` (DEBUG), and `layout_root` per relayout root (TRACE) |
//! | `flui.tree` | `element_rebuild` per rebuilt element (TRACE), and the `flui.tree.reconcile` events |
//! | `flui.input` | recognizer spans and the gesture observer |
//! | `flui.animation` | animation tick, sampling and listener spans, and `flui.animation.diagnostics` |
//! | `flui.signals`, `flui.a11y`, `flui.pace`, `flui.gpu` | reactive reads and writes, accessibility fallbacks, frame pacing, the renderer |
//!
//! Frame-path fields are primitives: ids are `u64` and kinds are `&'static str`.
//!
//! # Adding a profiler layer
//!
//! A profiler is another layer on the same registry, filtered to the targets it reads, so it
//! costs nothing for spans it does not admit. Tracy and Chrome's trace format are third-party
//! layers, which a tool adds beside [`PlatformLayer`](backend::PlatformLayer):
//!
//! ```rust,ignore
//! use tracing_subscriber::{filter::Targets, layer::SubscriberExt as _, Layer as _, Registry};
//!
//! let frame_only = Targets::new().with_target("flui.frame", tracing::Level::DEBUG);
//! let (chrome, _guard) = tracing_chrome::ChromeLayerBuilder::new().build();
//! let subscriber = Registry::default()
//!     .with(PlatformLayer::platform_default(&config).with_filter(filter))
//!     .with(chrome.with_filter(frame_only))
//!     .with(tracing_tracy::TracyLayer::default());
//! ```
//!
//! The framework sets no `max_level_*` feature, so a release build can be profiled this way.
//!
//! # Platforms
//!
//! | Target | Backend | Viewer |
//! |---|---|---|
//! | Desktop, incl. macOS | `fmt`, or `tracing-forest` with the `hierarchical` feature | terminal |
//! | Android | logcat | `adb logcat` |
//! | iOS | Apple unified logging | Console.app, Xcode, `log stream` |
//! | wasm32 | browser console + performance timeline | `DevTools` |
//!
//! See [`backend`] for what each one does and does not guarantee.
//!
//! # Privacy on device sinks
//!
//! Logcat and the Apple unified log are archives readable outside the
//! application, so on those two sinks **fields are private by default**:
//! dynamic values (strings, `Debug`/`Display` renderings, errors) are redacted
//! to `<private>` unless the field name ends in `.public`, scalars are
//! published unless the name ends in `.private`, a native `tracing` message is
//! published, and a message bridged from the `log` facade — a third party's
//! fully interpolated string — is redacted.
//! The model is Apple's `%{public}`/`%{private}` contract, applied
//! uniformly to both platforms; [`privacy`] is the full contract,
//! including what it deliberately does not cover.
//!
//! # Filtering
//!
//! `RUST_LOG` (or a configured variable) wins over the built-in directives, and
//! the filter is attached to the native sink, not globally to the registry.
//! This lets a timeline or metrics layer choose its own filter. Within the
//! native sink, nothing narrows the result afterwards:
//! `RUST_LOG=flui_view=trace` really does deliver `TRACE`.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
#![warn(
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::pedantic
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod backend;
pub mod config;
pub mod filter;
pub mod identity;
pub mod ownership;
pub mod privacy;
// The crate's test support: deterministic `tracing` capture behind its own
// subscriber home (ADR-0243: test support is named `testing`).
#[cfg(feature = "testing")]
#[cfg_attr(docsrs, doc(cfg(feature = "testing")))]
pub mod testing;

#[cfg(test)]
mod test_support;

use config::LogConfig;
use filter::FilterError;
use ownership::{
    SetupError, SubscriberInstallation, SubscriberOwnership, SubscriberPolicy, install_subscriber,
};
use tracing::level_filters::STATIC_MAX_LEVEL;

/// Build FLUI's default subscriber and apply an ownership policy to it.
///
/// Under [`SubscriberPolicy::Inherit`] no subscriber is built and the
/// process-global slot is not even read, so an embedded entry point provably
/// cannot disturb its host.
///
/// # Errors
///
/// Returns [`SetupError::Filter`] when the configured directives (or the
/// environment variable overriding them) do not parse, and
/// [`SetupError::SubscriberAlreadyInstalled`] when
/// [`SubscriberPolicy::Install`] cannot take the slot.
pub fn setup(
    config: &LogConfig,
    policy: SubscriberPolicy,
) -> Result<SubscriberInstallation, SetupError> {
    // The `else` arm is `Inherit`, and returning here is what makes the
    // "reads nothing, builds nothing" guarantee structural: the subscriber is
    // never constructed, so there is no host-owned slot to consult and no
    // native resource to allocate and drop.
    let Some(install_policy) = policy.install_policy() else {
        return Ok(SubscriberInstallation::unchanged());
    };

    install_subscriber(
        config.subscriber()?,
        install_policy,
        config.log_bridge_policy(),
    )
}

/// What [`setup_with_env_fallback`] did.
///
/// Non-exhaustive for the same reason as [`SubscriberInstallation`]: it reports
/// what a best-effort startup had to work around, and that list grows.
#[derive(Debug)]
#[non_exhaustive]
pub struct ResilientSetup {
    /// What happened to the process-global subscriber and `log` bridge.
    pub installation: SubscriberInstallation,

    /// The environment override that was rejected, if there was one.
    ///
    /// `Some` means the environment's value did not parse and the
    /// configuration's own directives were used instead. The caller decides how
    /// to report it — and *must* decide, because by construction this cannot be
    /// reported before a subscriber exists.
    pub rejected_env_override: Option<FilterError>,

    /// The most verbose level the installed filter selects, when the build
    /// holds no event of that level (see [the build's ceiling](filter#the-builds-ceiling)).
    ///
    /// `Some` means the filter asks for events `tracing`'s static maximum level
    /// compiled out, so they are not delivered. `None` also when this call
    /// installed no subscriber, whose filter is then not this configuration's.
    pub compiled_out: Option<tracing::level_filters::LevelFilter>,
}

/// The most verbose level `config`'s filter selects above `ceiling`, for a
/// subscriber this call installed; `None` when the call installed none, whose
/// filter is then not this configuration's.
fn selected_beyond(
    config: &LogConfig,
    ownership: SubscriberOwnership,
    ceiling: tracing::level_filters::LevelFilter,
) -> Option<tracing::level_filters::LevelFilter> {
    if !ownership.is_installed() {
        return None;
    }
    // The subscriber was built from this filter a moment ago, so it resolves.
    let targets = config.filter().targets().ok()?;
    filter::beyond(&targets, ceiling)
}

/// Set up logging, falling back to the configured directives when the
/// environment's override is rejected.
///
/// An entry point has a harder problem than [`setup`] answers: a malformed
/// `RUST_LOG` on a user's device must not leave the process with **no**
/// subscriber, because then every subsequent diagnostic — including the one
/// explaining why — vanishes. On a phone that is indistinguishable from the
/// framework having died silently.
///
/// So a rejected environment override is downgraded from fatal to reported: the
/// configuration is retried with the override disabled, and the rejection comes
/// back in [`ResilientSetup::rejected_env_override`] for the caller to surface
/// through whatever channel is still working.
///
/// # Errors
///
/// Returns [`SetupError::Filter`] only when the configuration's *own*
/// directives do not parse. That is not a user-input problem — it is the
/// caller passing an invalid constant — so it stays an error rather than being
/// swallowed. A caller whose directives are a literal should treat it as the
/// programming error it is:
///
/// ```rust,no_run
/// # use flui_log::config::LogConfig;
/// # use flui_log::filter::FilterConfig;
/// # use flui_log::ownership::SubscriberPolicy;
/// const DIRECTIVES: &str = "info,wgpu=warn";
/// let config = LogConfig::builder()
///     .filter(FilterConfig::new(DIRECTIVES))
///     .build();
///
/// let setup = flui_log::setup_with_env_fallback(&config, SubscriberPolicy::Auto)
///     .expect("BUG: the built-in directive constant must parse");
/// ```
pub fn setup_with_env_fallback(
    config: &LogConfig,
    policy: SubscriberPolicy,
) -> Result<ResilientSetup, SetupError> {
    match setup(config, policy) {
        Ok(installation) => Ok(ResilientSetup {
            installation,
            rejected_env_override: None,
            compiled_out: selected_beyond(config, installation.ownership, STATIC_MAX_LEVEL),
        }),

        // Only an environment override can be retried around. A bad *configured*
        // directive string has no fallback that would not be a guess.
        Err(SetupError::Filter(
            rejected
            @ (FilterError::Environment { .. } | FilterError::EnvironmentNotUnicode { .. }),
        )) => {
            let without_override = config.without_env_override();

            let installation = setup(&without_override, policy)?;
            Ok(ResilientSetup {
                installation,
                rejected_env_override: Some(rejected),
                compiled_out: selected_beyond(
                    &without_override,
                    installation.ownership,
                    STATIC_MAX_LEVEL,
                ),
            })
        }

        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod resilient_setup_tests {
    //! Cases that need no process-global state; scenarios that write the
    //! subscriber slot live in `tests/`, one per process.

    use super::{LogConfig, SubscriberPolicy};
    use crate::filter::FilterConfig;

    #[test]
    fn an_invalid_configuration_is_a_caller_bug_and_stays_an_error() {
        // Only an *environment* override is recoverable. Bad configured
        // directives have no fallback that would not be a guess, so they must
        // surface rather than be quietly replaced.
        let config = LogConfig::builder()
            .filter(FilterConfig::new("=not a directive=").without_env_var())
            .build();

        let error = super::setup_with_env_fallback(&config, SubscriberPolicy::Auto)
            .expect_err("invalid configured directives must not be swallowed");

        assert!(matches!(error, super::SetupError::Filter(_)));
    }

    #[test]
    fn a_setup_reports_the_level_above_the_ceiling_only_for_the_subscriber_it_installed() {
        use tracing::level_filters::LevelFilter;

        use crate::ownership::SubscriberOwnership;

        let config = |directives: &str| {
            LogConfig::builder()
                .filter(FilterConfig::new(directives).without_env_var())
                .build()
        };
        for (directives, ownership, ceiling, expected) in [
            (
                "warn,flui_view=debug",
                SubscriberOwnership::Installed,
                LevelFilter::INFO,
                Some(LevelFilter::DEBUG),
            ),
            (
                "warn,flui_view=debug",
                SubscriberOwnership::Installed,
                LevelFilter::TRACE,
                None,
            ),
            // A subscriber the host installed filters by its own directives.
            (
                "warn,flui_view=debug",
                SubscriberOwnership::Unchanged,
                LevelFilter::INFO,
                None,
            ),
            (
                "info",
                SubscriberOwnership::Installed,
                LevelFilter::INFO,
                None,
            ),
        ] {
            assert_eq!(
                super::selected_beyond(&config(directives), ownership, ceiling),
                expected,
                "`{directives}`, {ownership:?}, under a ceiling of {ceiling}"
            );
        }
    }
}
