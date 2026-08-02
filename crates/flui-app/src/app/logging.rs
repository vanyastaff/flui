//! Managed-application diagnostics composition.
//!
//! A managed entry point may install FLUI's default subscriber into an empty
//! process slot. A future embedded entry point does not need a no-op policy API:
//! it preserves host ownership by never calling this module.

use flui_log::{LogConfig, SubscriberInstallation, SubscriberPolicy};

use super::{AppConfig, DiagnosticsProfile};

/// Developer-oriented defaults for a managed FLUI application.
pub const DEVELOPMENT_DIRECTIVES: &str =
    "info,flui_app=debug,flui_view=debug,flui_rendering=debug,wgpu=warn";

/// Conservative defaults for a shipped FLUI application.
pub const PRODUCTION_DIRECTIVES: &str = "warn,wgpu=error";

/// Build the logging configuration for a managed application.
///
/// Application identity is process-level and independent from the first
/// window's title. The profile supplies defaults only; `RUST_LOG` remains an
/// explicit developer override in either profile.
#[must_use]
pub(crate) fn managed_log_config(config: &AppConfig) -> LogConfig {
    let directives = match config.diagnostics_profile {
        DiagnosticsProfile::Development => DEVELOPMENT_DIRECTIVES,
        DiagnosticsProfile::Production => PRODUCTION_DIRECTIVES,
    };

    LogConfig::builder()
        .identity(config.application_identity.clone())
        .directives(directives)
        .build()
}

/// Install managed defaults without replacing an application-owned subscriber.
///
/// A rejected `RUST_LOG` falls back to the selected profile and is reported
/// after the subscriber exists.
///
/// # Panics
///
/// Panics only if one of this module's built-in directive constants is invalid.
pub(crate) fn init_managed_logging(config: &AppConfig) -> SubscriberInstallation {
    let log_config = managed_log_config(config);
    let setup = flui_log::setup_with_env_fallback(&log_config, SubscriberPolicy::Auto)
        .expect("BUG: managed diagnostics directives must be valid");

    if let Some(error) = setup.rejected_env_override {
        tracing::warn!(
            %error,
            "the RUST_LOG filter was rejected; using FLUI's built-in directives instead"
        );
    }

    setup.installation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_title_does_not_change_application_identity() {
        let config = AppConfig::new().with_title("Document 2 — My Editor");
        let log_config = managed_log_config(&config);

        assert_eq!(log_config.identity().display_name(), "FLUI App");
        assert_eq!(config.title, "Document 2 — My Editor");
    }

    #[test]
    fn an_explicit_application_identity_reaches_the_log_config() {
        let identity = flui_log::AppIdentity::new("My Editor").expect("valid identity");
        let config = AppConfig::new()
            .with_title("Document 2")
            .with_application_identity(identity);

        assert_eq!(
            managed_log_config(&config).identity().display_name(),
            "My Editor"
        );
    }

    #[test]
    fn both_profile_directives_parse() {
        for (profile, directives) in [
            (DiagnosticsProfile::Development, DEVELOPMENT_DIRECTIVES),
            (DiagnosticsProfile::Production, PRODUCTION_DIRECTIVES),
        ] {
            let config = managed_log_config(&AppConfig::new().with_diagnostics_profile(profile));
            assert!(
                config.filter().env_filter_from(None).is_ok(),
                "managed directives must parse: {directives:?}"
            );
        }
    }

    #[test]
    fn production_defaults_do_not_enable_framework_debug_events() {
        let config = managed_log_config(
            &AppConfig::new().with_diagnostics_profile(DiagnosticsProfile::Production),
        );
        assert_eq!(config.filter().directives(), PRODUCTION_DIRECTIVES);
    }

    #[test]
    fn a_managed_run_still_honours_rust_log() {
        let config = managed_log_config(&AppConfig::new());
        assert_eq!(config.filter().env_var(), Some("RUST_LOG"));
    }

    #[test]
    fn a_rejected_environment_override_falls_back_instead_of_failing() {
        let config = managed_log_config(&AppConfig::new());
        let rejected = config
            .filter()
            .env_filter_from(Some("=not a directive="))
            .expect_err("a malformed override must be reported");
        assert!(matches!(
            rejected,
            flui_log::FilterError::Environment { .. }
        ));

        let fallback = flui_log::FilterConfig::new(config.filter().directives()).without_env_var();
        assert!(fallback.env_filter_from(Some("=not a directive=")).is_ok());
    }
}
