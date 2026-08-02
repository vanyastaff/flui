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

/// What actually happened to the process-global subscriber.
///
/// Returned rather than discarded so the outcome is observable — a caller can
/// log it, assert on it in a test, or surface it in a diagnostic screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriberOwnership {
    /// FLUI installed the subscriber; FLUI owns it.
    Installed,

    /// The subscriber in effect is somebody else's (or none at all, under
    /// [`SubscriberPolicy::Inherit`]). FLUI changed nothing.
    Inherited,
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
    pub fn is_inherited(self) -> bool {
        matches!(self, Self::Inherited)
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
/// Under [`SubscriberPolicy::Inherit`] the subscriber is dropped unused; a
/// caller that wants to avoid building it at all should branch on the policy
/// first, as [`crate::setup`] does.
///
/// # Errors
///
/// Returns [`SetupError::SubscriberAlreadyInstalled`] when
/// [`SubscriberPolicy::Install`] was requested and the slot is taken. The other
/// two policies cannot fail.
///
/// # Concurrency
///
/// Two threads can reach the empty slot at the same time. `set_global_default`
/// settles it atomically, so exactly one of them sees `Ok`. Whoever loses under
/// [`SubscriberPolicy::Auto`] reports [`SubscriberOwnership::Inherited`],
/// because the winner's subscriber is exactly the "existing subscriber" the
/// policy promises to preserve.
pub fn install_subscriber<S>(
    subscriber: S,
    policy: SubscriberPolicy,
) -> Result<SubscriberOwnership, SetupError>
where
    S: Subscriber + Send + Sync + 'static,
{
    match policy {
        SubscriberPolicy::Inherit => {
            drop(subscriber);
            Ok(SubscriberOwnership::Inherited)
        }

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
        SubscriberPolicy::Auto => match tracing::subscriber::set_global_default(subscriber) {
            Ok(()) => Ok(SubscriberOwnership::Installed),
            Err(_) => Ok(SubscriberOwnership::Inherited),
        },

        SubscriberPolicy::Install => tracing::subscriber::set_global_default(subscriber)
            .map(|()| SubscriberOwnership::Installed)
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
    }

    #[test]
    fn ownership_predicates_agree_with_the_variant() {
        assert!(SubscriberOwnership::Installed.is_installed());
        assert!(!SubscriberOwnership::Installed.is_inherited());
        assert!(SubscriberOwnership::Inherited.is_inherited());
        assert!(!SubscriberOwnership::Inherited.is_installed());
    }
}
