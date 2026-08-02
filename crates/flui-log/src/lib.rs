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
//! Every one of them returns a [`SubscriberOwnership`] saying what actually
//! happened, and none of them panics.
//!
//! ```rust,no_run
//! use flui_log::{LogConfig, SubscriberOwnership, SubscriberPolicy};
//!
//! let ownership = flui_log::setup(&LogConfig::default(), SubscriberPolicy::Auto)?;
//! if ownership == SubscriberOwnership::Inherited {
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
//! use flui_log::{LogConfig, PlatformLayer, SubscriberPolicy, install_subscriber};
//! use tracing_subscriber::{Registry, layer::SubscriberExt as _};
//!
//! let config = LogConfig::builder().directives("info,flui_view=trace").build();
//!
//! let subscriber = Registry::default()
//!     .with(config.env_filter()?)
//!     .with(PlatformLayer::platform_default(&config));
//! //  .with(my_timeline_layer)   <- devtools stacks here
//!
//! install_subscriber(subscriber, SubscriberPolicy::Install)?;
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
//! See [`backend`] for what each one does and does not guarantee, including the
//! Apple privacy contract.
//!
//! # Filtering
//!
//! `RUST_LOG` (or a configured variable) wins over the built-in directives, and
//! nothing narrows the result afterwards. `RUST_LOG=flui_view=trace` really does
//! deliver `TRACE`; see [`filter`] for the second-ceiling bug that made this
//! worth stating.

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

pub use backend::{LogcatPriority, PlatformLayer};
pub use config::{DesktopFormat, LogConfig, LogConfigBuilder};
pub use filter::{DEFAULT_DIRECTIVES, DEFAULT_ENV_VAR, FilterConfig, FilterError};
pub use identity::{
    APPLE_CATEGORY, AppIdentity, BundleId, DEFAULT_DISPLAY_NAME, IdentityError,
    UNIDENTIFIED_APPLE_SUBSYSTEM,
};
pub use ownership::{SetupError, SubscriberOwnership, SubscriberPolicy, install_subscriber};

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
) -> Result<SubscriberOwnership, SetupError> {
    if policy == SubscriberPolicy::Inherit {
        return Ok(SubscriberOwnership::Inherited);
    }

    install_subscriber(config.subscriber()?, policy)
}
