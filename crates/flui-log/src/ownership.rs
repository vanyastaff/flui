//! Who owns the process-global subscriber.
//!
//! Installing a subscriber mutates process-global state exactly once per
//! process. That makes it an *ownership* question, not a configuration one: a
//! FLUI application embedded in a game engine, an editor, a test process, or a
//! service may well be running inside a process whose observability is already
//! owned by somebody else.
//!
//! The historical entry points answered that question by panicking. Every
//! policy here answers it without panicking, and every outcome is a value the
//! caller can inspect.

use tracing::Subscriber;

use crate::filter::FilterError;

/// What a caller wants done about the process-global subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SubscriberPolicy {
    /// Install nothing and read nothing; leave the host's observability alone.
    ///
    /// This is the policy for an embedded or host-driven entry point. It holds
    /// even when no subscriber exists yet: a host that has not installed one
    /// *yet* has not delegated the decision to FLUI.
    Inherit,

    /// Install FLUI's platform default only when the slot is still empty.
    ///
    /// An existing subscriber is preserved, never replaced. This is the policy
    /// for the managed entry point, where "a plain `run_app` should print
    /// something" and "do not stomp on an application that configured its own
    /// logging" both have to hold.
    #[default]
    Auto,

    /// Demand ownership of the slot.
    ///
    /// Returns [`SetupError::SubscriberAlreadyInstalled`] when the slot is
    /// taken, so a tool that genuinely needs its own configuration finds out
    /// rather than logging into a subscriber it did not choose.
    Install,
}

impl SubscriberPolicy {
    /// How this policy installs an already-built subscriber, or `None` when it
    /// builds nothing at all.
    ///
    /// `None` is [`Inherit`](Self::Inherit), and it is the reason this is a
    /// method rather than a `From` impl: "installs nothing" and "installs under
    /// some policy" are different arities, and collapsing them would leave a
    /// case that can only be excluded by a runtime assertion.
    #[inline]
    #[must_use]
    pub fn install_policy(self) -> Option<InstallPolicy> {
        match self {
            Self::Inherit => None,
            Self::Auto => Some(InstallPolicy::Auto),
            Self::Install => Some(InstallPolicy::Install),
        }
    }
}

/// Policy for installing an already-built subscriber.
///
/// Unlike [`SubscriberPolicy`], this has no no-op variant. Constructing a
/// subscriber can allocate native resources or start workers, so host-owned
/// setup must return before construction rather than build a value merely to
/// drop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallPolicy {
    /// Install only when the process-global slot is empty.
    Auto,

    /// Require the process-global slot to be empty.
    Install,
}

/// What FLUI may do with the process-global [`log`](mod@tracing_log::log)
/// facade after it installs a tracing subscriber.
///
/// The tracing subscriber and the `log` logger are separate global slots. A
/// host can own either one independently, so permission to install the former
/// must not silently imply permission to claim the latter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LogBridgePolicy {
    /// Install `LogTracer` when the logger slot is empty and preserve an
    /// existing logger when it is already occupied.
    #[default]
    Auto,

    /// Leave the logger slot untouched, even when it is empty.
    ///
    /// Use this when the host owns the `log` facade decision and may install
    /// its logger after FLUI has installed the tracing subscriber.
    Inherit,
}

/// What actually happened to the process-global subscriber slot.
///
/// Returned rather than discarded so the outcome is observable — a caller can
/// log it, assert on it in a test, or surface it in a diagnostic screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriberOwnership {
    /// FLUI installed the subscriber; FLUI owns it.
    Installed,

    /// FLUI left the slot unchanged.
    ///
    /// This deliberately makes no claim that a subscriber exists. Under
    /// [`SubscriberPolicy::Inherit`] the host may install one later.
    Unchanged,
}

impl SubscriberOwnership {
    /// Whether FLUI installed the subscriber now in effect.
    #[inline]
    #[must_use]
    pub fn is_installed(self) -> bool {
        matches!(self, Self::Installed)
    }

    /// Whether the process-global subscriber was left as it was found.
    #[inline]
    #[must_use]
    pub fn is_unchanged(self) -> bool {
        matches!(self, Self::Unchanged)
    }
}

/// What happened to the compatibility bridge for crates that emit through
/// `log` rather than `tracing`.
///
/// Non-exhaustive: this reports on a subsystem outside FLUI's control, so the
/// set of things that can have happened to it grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LogBridgeStatus {
    /// FLUI installed `tracing_log::LogTracer` after taking the subscriber slot.
    Installed,

    /// Another logger already owned the `log` facade and was preserved.
    ExistingLoggerPreserved,

    /// The caller selected [`LogBridgePolicy::Inherit`], so FLUI left the
    /// logger slot untouched.
    Inherited,

    /// FLUI did not install a tracing subscriber, so it did not reach the
    /// bridge policy.
    NotAttempted,
}

/// Complete result of attempting to install a subscriber stack.
///
/// Non-exhaustive because installing a stack is a list of global side effects,
/// and that list has already grown once — from a bare ownership answer to
/// ownership plus the `log` bridge. A future one (a metrics bridge, a panic
/// hook) must be reportable without breaking every caller that reads this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SubscriberInstallation {
    /// What happened to the process-global tracing subscriber.
    pub ownership: SubscriberOwnership,

    /// What happened to the process-global `log` compatibility bridge.
    pub log_bridge: LogBridgeStatus,
}

impl SubscriberInstallation {
    fn installed(log_bridge_policy: LogBridgePolicy) -> Self {
        let log_bridge = match log_bridge_policy {
            LogBridgePolicy::Auto => match tracing_log::LogTracer::init() {
                Ok(()) => LogBridgeStatus::Installed,
                Err(_) => LogBridgeStatus::ExistingLoggerPreserved,
            },
            LogBridgePolicy::Inherit => LogBridgeStatus::Inherited,
        };

        Self {
            ownership: SubscriberOwnership::Installed,
            log_bridge,
        }
    }

    /// Result for a policy that deliberately changed no global state.
    #[must_use]
    pub const fn unchanged() -> Self {
        Self {
            ownership: SubscriberOwnership::Unchanged,
            log_bridge: LogBridgeStatus::NotAttempted,
        }
    }
}

/// Why logging setup could not be completed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SetupError {
    /// [`SubscriberPolicy::Install`] was requested and the slot is taken.
    #[error(
        "a global tracing subscriber is already installed, so `SubscriberPolicy::Install` cannot \
         take ownership; use `SubscriberPolicy::Auto` to keep the existing subscriber or \
         `SubscriberPolicy::Inherit` to install nothing"
    )]
    SubscriberAlreadyInstalled,

    /// The filter could not be resolved, so no subscriber was built.
    #[error(transparent)]
    Filter(#[from] FilterError),
}

/// Apply an ownership policy to an already-built subscriber.
///
/// This is the seam a composition root reaches for when it has stacked its own
/// layers on top of [`crate::LogConfig::subscriber`] (or built a subscriber
/// from scratch) and wants FLUI's ownership semantics rather than
/// `set_global_default`'s panic-or-error pair.
///
/// `log_bridge_policy` is separate because the tracing subscriber and the
/// `log` logger are independent process-global slots. Passing
/// [`LogBridgePolicy::Inherit`] installs only the subscriber and leaves the
/// logger decision to the host.
///
/// # Errors
///
/// Returns [`SetupError::SubscriberAlreadyInstalled`] when
/// [`InstallPolicy::Install`] was requested and the slot is taken.
///
/// # Concurrency
///
/// Two threads can reach the empty slot at the same time. `set_global_default`
/// settles it atomically, so exactly one of them sees `Ok`. Whoever loses under
/// [`InstallPolicy::Auto`] reports [`SubscriberOwnership::Unchanged`],
/// because the winner's subscriber is exactly the "existing subscriber" the
/// policy promises to preserve.
pub fn install_subscriber<S>(
    subscriber: S,
    policy: InstallPolicy,
    log_bridge_policy: LogBridgePolicy,
) -> Result<SubscriberInstallation, SetupError>
where
    S: Subscriber + Send + Sync + 'static,
{
    match policy {
        // `set_global_default` is the whole test. It writes the slot only when
        // the slot is empty and reports the refusal as an error, so trying it
        // *is* asking "is the slot free?" without a window between the question
        // and the answer.
        //
        // Deliberately not `tracing::dispatcher::has_been_set`: that flag is
        // also raised by a thread-local `with_default`, so one scoped
        // subscriber anywhere in the process would make `Auto` believe the
        // global slot was taken and refuse to install for the rest of the
        // program's life. (It is `#[doc(hidden)]` upstream, too.)
        InstallPolicy::Auto => match tracing::subscriber::set_global_default(subscriber) {
            Ok(()) => Ok(SubscriberInstallation::installed(log_bridge_policy)),
            Err(_) => Ok(SubscriberInstallation::unchanged()),
        },

        InstallPolicy::Install => tracing::subscriber::set_global_default(subscriber)
            .map(|()| SubscriberInstallation::installed(log_bridge_policy))
            .map_err(|_| SetupError::SubscriberAlreadyInstalled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Behaviour that depends on the process-global slot lives in
    // `tests/`, one scenario per integration-test binary, because the slot can
    // only be written once per process. What is testable in-process is the
    // part that does not touch it.

    #[test]
    fn auto_is_the_default_policy() {
        assert_eq!(SubscriberPolicy::default(), SubscriberPolicy::Auto);
        assert_eq!(LogBridgePolicy::default(), LogBridgePolicy::Auto);
    }

    #[test]
    fn ownership_predicates_agree_with_the_variant() {
        assert!(SubscriberOwnership::Installed.is_installed());
        assert!(!SubscriberOwnership::Installed.is_unchanged());
        assert!(SubscriberOwnership::Unchanged.is_unchanged());
        assert!(!SubscriberOwnership::Unchanged.is_installed());
    }
}
