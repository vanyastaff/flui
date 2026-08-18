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
//! `flui-cli`, the `flui` facade) depends on this crate, and
//! `docs/workspace-layers.toml` enforces that mechanically.
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
//! use flui_log::{LogConfig, SubscriberOwnership, SubscriberPolicy};
//!
//! let installation = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)?;
//! if installation.ownership == SubscriberOwnership::Unchanged {
//!     tracing::debug!("the host already owns logging; FLUI installed nothing");
//! }
//! # Ok::<(), flui_log::SetupError>(())
//! ```
//!
//! # Composing your own stack
//!
//! [`setup`] is the convenience path. A tool that needs its own layers builds
//! the pieces and installs the result itself:
//!
//! ```rust,no_run
//! use flui_log::{InstallPolicy, LogBridgePolicy, LogConfig, PlatformLayer, install_subscriber};
//! use tracing_subscriber::{Registry, layer::SubscriberExt as _, Layer as _};
//!
//! let config = LogConfig::builder().directives("info,flui_view=trace").build();
//! let filter = config.env_filter()?;
//!
//! let subscriber = Registry::default()
//!     .with(PlatformLayer::platform_default(&config).with_filter(filter));
//! //  .with(my_timeline_layer)   <- devtools stacks here
//!
//! install_subscriber(subscriber, InstallPolicy::Install, LogBridgePolicy::Auto)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
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
//! uniformly to both platforms; [`backend::privacy`] is the full contract,
//! including what it deliberately does not cover.
//!
//! # Filtering
//!
//! `RUST_LOG` (or a configured variable) wins over the built-in directives, and
//! the filter is attached to the native sink, not globally to the registry.
//! This lets a timeline or metrics layer choose its own filter. Within the
//! native sink, nothing narrows the result afterwards:
//! `RUST_LOG=flui_view=trace` really does deliver `TRACE`.

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

#[cfg(test)]
mod test_support;

pub use backend::privacy::{
    EventOrigin, FieldKind, FieldPrivacy, PRIVATE_FIELD_SUFFIX, PUBLIC_FIELD_SUFFIX, REDACTED_VALUE,
};
pub use backend::{LogcatPriority, PlatformLayer};
pub use config::{DesktopFormat, LogConfig, LogConfigBuilder};
pub use filter::{DEFAULT_DIRECTIVES, DEFAULT_ENV_VAR, FilterConfig, FilterError};
pub use identity::{
    APPLE_CATEGORY, AppIdentity, AppleBundleId, DEFAULT_DISPLAY_NAME, IdentityError,
    UNIDENTIFIED_APPLE_SUBSYSTEM,
};
pub use ownership::{
    InstallPolicy, LogBridgePolicy, LogBridgeStatus, SetupError, SubscriberInstallation,
    SubscriberOwnership, SubscriberPolicy, install_subscriber,
};

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
/// # use flui_log::{LogConfig, SubscriberPolicy};
/// const DIRECTIVES: &str = "info,wgpu=warn";
/// let config = LogConfig::builder().directives(DIRECTIVES).build();
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
        }),

        // Only an environment override can be retried around. A bad *configured*
        // directive string has no fallback that would not be a guess.
        Err(SetupError::Filter(
            rejected
            @ (FilterError::Environment { .. } | FilterError::EnvironmentNotUnicode { .. }),
        )) => {
            let without_override = config.without_env_override();

            Ok(ResilientSetup {
                installation: setup(&without_override, policy)?,
                rejected_env_override: Some(rejected),
            })
        }

        Err(other) => Err(other),
    }
}

/// Report a message straight to Android's logcat, bypassing `tracing`.
///
/// For exactly one situation: the logging backend itself failed to install, so
/// a `tracing` event would go nowhere. Everything else in FLUI emits through
/// `tracing`.
///
/// Android-only by design rather than by omission. There is no cross-platform
/// "emergency channel" this could generalise to — on desktop it would be
/// stderr, which this workspace bans in shipped code — so the function simply
/// does not exist on other targets, and a caller has to notice that.
#[cfg(target_os = "android")]
pub fn report_to_logcat(identity: &AppIdentity, message: &str) {
    // Tagged with the display name rather than a module path: this is the
    // application talking about itself, not an event from some `flui_*` target.
    let tag = backend::logcat::logcat_tag(identity.display_name(), identity.display_name());

    backend::logcat::write_to_logcat(LogcatPriority::Error, &tag, message);
}

#[cfg(test)]
mod resilient_setup_tests {
    //! Cases that need no process-global state. The one that does — a genuinely
    //! rejected environment override — lives in
    //! `tests/a_rejected_env_override_still_installs.rs`, in its own process.

    use super::{FilterConfig, LogConfig, SubscriberOwnership, SubscriberPolicy};

    #[test]
    fn a_valid_configuration_reports_no_rejection() {
        // The rejection channel must stay quiet on the happy path, or a caller
        // learns to ignore it.
        let config = LogConfig::builder()
            .filter(FilterConfig::new("info").without_env_var())
            .build();

        let setup = super::setup_with_env_fallback(&config, SubscriberPolicy::Inherit)
            .expect("`info` parses");

        assert!(setup.rejected_env_override.is_none());
        assert_eq!(setup.installation.ownership, SubscriberOwnership::Unchanged);
    }

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
}

#[cfg(all(test, target_os = "android"))]
mod android_entry_point_shape {
    //! Compiles only on Android, which is the point: it pins the exact call
    //! shape an Android entry point uses, so a signature change breaks the
    //! Android target check rather than a device build.
    //!
    //! The Android example packages are not `workspace.members` and cannot be
    //! compiled by cargo from this repository today, so this is what stands in
    //! for compiling them.

    use super::{AppIdentity, LogConfig, SubscriberPolicy};

    const DISPLAY_NAME: &str = "flui_android_demo";
    const LOG_DIRECTIVES: &str = "info,flui_engine=debug,wgpu=warn";

    #[test]
    fn the_entry_point_setup_and_report_path_type_checks() {
        let identity = AppIdentity::new(DISPLAY_NAME).unwrap_or_default();
        let config = LogConfig::builder()
            .identity(identity.clone())
            .directives(LOG_DIRECTIVES)
            .build();

        let setup = super::setup_with_env_fallback(&config, SubscriberPolicy::Auto)
            .expect("BUG: LOG_DIRECTIVES must be a valid filter directive string");

        if let Some(error) = setup.rejected_env_override {
            super::report_to_logcat(&identity, &format!("RUST_LOG rejected: {error}"));
        }
    }
}
