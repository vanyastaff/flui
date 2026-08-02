//! Which subscriber policy each entry point gets.
//!
//! `flui-app` is a composition root, so it is one of the few crates allowed a
//! normal dependency on `flui-log`. What it owns is the *policy*: whether a
//! given way of starting FLUI may take the process-global subscriber slot.
//! Building the subscriber itself belongs to `flui-log`, and emitting events
//! belongs to every framework crate through `tracing` alone.

use flui_log::{AppIdentity, LogConfig, SubscriberOwnership, SubscriberPolicy};

use super::AppConfig;

/// Default filter directives for a managed FLUI application.
///
/// Chattier than `flui-log`'s library default because a managed run is almost
/// always a development run. `RUST_LOG` still overrides it, and — unlike the
/// historical logger — nothing narrows the result afterwards, so
/// `RUST_LOG=flui_view=trace` really does deliver `TRACE`.
pub const MANAGED_DIRECTIVES: &str =
    "info,flui_app=debug,flui_view=debug,flui_rendering=debug,wgpu=warn";

/// How FLUI was started, which is what decides who may own logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryPoint {
    /// FLUI owns the process: `run_app`, `run_app_with_config`, `run_direct`,
    /// `run_app_android`.
    ///
    /// A plain `run_app` should print something, so the managed entry point
    /// installs a default — but only into an empty slot. An application that
    /// configured its own subscriber before calling `run_app` keeps it.
    Managed,

    /// FLUI is a guest: a game engine, an editor, a service, or a test process
    /// drives the frame loop and already owns observability.
    ///
    /// Installs nothing, reads nothing, and narrows nothing. This holds even
    /// when no subscriber exists yet: a host that has not installed one *yet*
    /// has not handed the decision to FLUI.
    ///
    /// The host-driven runtime entry point that will carry this is not built
    /// yet. The policy is here, and tested, so the entry point cannot land with
    /// a different one by accident.
    Embedded,
}

impl EntryPoint {
    /// The subscriber policy this entry point starts from.
    #[inline]
    #[must_use]
    pub fn default_subscriber_policy(self) -> SubscriberPolicy {
        match self {
            Self::Managed => SubscriberPolicy::Auto,
            Self::Embedded => SubscriberPolicy::Inherit,
        }
    }
}

/// The log configuration a managed run starts from.
///
/// The window title doubles as the logcat fallback tag. It is **not** promoted
/// to a bundle identifier — an application that wants its real reverse-DNS
/// identity in Apple's unified logging builds its own [`LogConfig`] and calls
/// [`flui_log::setup`] before `run_app`. A title that cannot be a display name
/// (blank, or holding a NUL that the C logging boundary would truncate at)
/// falls back to `flui-log`'s default identity.
#[must_use]
pub fn managed_log_config(config: &AppConfig) -> LogConfig {
    let identity = AppIdentity::new(config.title.as_str()).unwrap_or_default();

    LogConfig::builder()
        .identity(identity)
        .directives(MANAGED_DIRECTIVES)
        .build()
}

/// Apply an entry point's logging policy.
///
/// Never panics and never replaces a subscriber somebody else installed. The
/// returned [`SubscriberOwnership`] says which happened; a caller that does not
/// care can drop it.
///
/// A filter that fails to parse is not fatal to starting an application, so the
/// resolved default is used instead and the problem is reported through the
/// subscriber that ends up in place.
pub fn init_logging(entry_point: EntryPoint, config: &AppConfig) -> SubscriberOwnership {
    let policy = entry_point.default_subscriber_policy();
    let log_config = managed_log_config(config);

    match flui_log::setup(&log_config, policy) {
        Ok(ownership) => ownership,
        Err(error) => {
            // The only way to get here is an unparsable `RUST_LOG`. Fall back
            // to directives that are known good, then say so — an author who
            // mistyped a directive needs to see that, and after this call there
            // is a subscriber to see it through.
            let fallback = LogConfig::builder()
                .identity(log_config.identity().clone())
                .filter(flui_log::FilterConfig::new(MANAGED_DIRECTIVES).without_env_var())
                .build();

            let ownership =
                flui_log::setup(&fallback, policy).unwrap_or(SubscriberOwnership::Inherited);

            tracing::warn!(
                %error,
                "log filter configuration was rejected; using FLUI's default directives"
            );

            ownership
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_managed_run_may_install_but_never_replace() {
        assert_eq!(
            EntryPoint::Managed.default_subscriber_policy(),
            SubscriberPolicy::Auto
        );
    }

    #[test]
    fn an_embedded_run_never_touches_the_hosts_subscriber() {
        assert_eq!(
            EntryPoint::Embedded.default_subscriber_policy(),
            SubscriberPolicy::Inherit
        );
    }

    #[test]
    fn the_window_title_becomes_the_display_name_and_nothing_more() {
        let config = AppConfig::new().with_title("My Game");
        let log_config = managed_log_config(&config);

        assert_eq!(log_config.identity().display_name(), "My Game");
        assert_eq!(
            log_config.identity().bundle_id(),
            None,
            "a window title must never be promoted to a bundle identifier"
        );
        assert_eq!(
            log_config.identity().apple_subsystem(),
            flui_log::UNIDENTIFIED_APPLE_SUBSYSTEM
        );
    }

    #[test]
    fn an_unusable_title_falls_back_to_the_default_identity() {
        let config = AppConfig::new().with_title("");
        let log_config = managed_log_config(&config);

        assert_eq!(
            log_config.identity().display_name(),
            flui_log::DEFAULT_DISPLAY_NAME
        );
    }

    #[test]
    fn managed_directives_do_not_pin_a_ceiling() {
        // The default directive string must not itself be the thing that stops
        // `RUST_LOG=flui_view=trace` from working.
        let config = managed_log_config(&AppConfig::new());
        assert_eq!(config.filter().env_var(), Some("RUST_LOG"));
        assert!(
            config.filter().env_filter().is_ok_and(|_| true),
            "the shipped default directives must parse"
        );
    }
}
