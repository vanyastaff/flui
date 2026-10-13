//! Panic-payload text extraction and the `BUG:` internal-invariant classifier.
//!
//! Every `catch_unwind` boundary that reads a caught payload needs the
//! same two decidable operations — pull whatever text the payload carries
//! (or honestly admit it has none), and decide whether that text asserts a
//! FLUI-owned invariant rather than reporting a caller- or application-code
//! failure. This is their one home: a boundary that reads payload text
//! calls these two functions and never downcasts the payload itself, so
//! what one boundary reports is what every boundary reports. Boundaries
//! that only `resume_unwind` or discard the payload have no reason to
//! call either.
//!
//! See `docs/PANIC-POLICY.md`'s "The `BUG:` message convention" for the
//! policy [`is_internal_invariant`] mechanizes.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};

/// The text of a panic payload, when it has any.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{}", x)`
/// and every other formatted `panic!` produce a `String`; anything else
/// (constructed with [`std::panic::panic_any`]) is opaque and reported as
/// `None` — never guessed at.
#[must_use]
pub fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

/// Whether `text` asserts a FLUI-owned internal invariant rather than a
/// caller- or application-code failure.
///
/// This classifies only — it never decides how a caught panic is routed
/// (contained, re-raised, logged at what level); that stays a per-site
/// decision. It says whether `text` was written for the `BUG:` convention
/// documented in `docs/PANIC-POLICY.md`: every production-path `expect()`
/// that asserts an invariant FLUI itself must maintain is prefixed
/// `"BUG: "`, so a user hitting it knows immediately the fault is FLUI's,
/// not theirs.
#[must_use]
pub fn is_internal_invariant(text: &str) -> bool {
    text.starts_with("BUG:")
}

/// Retain a discarded opaque panic payload so containment can continue safely.
///
/// A payload may own several fields with panicking destructors. Dropping it
/// inside `catch_unwind` can still abort when a second field panics during the
/// first field's unwind. Exceptional-path retention deliberately leaks the
/// payload; it does not run arbitrary destruction after a caught failure.
/// Exact `&'static str` and `String` payloads are released instead: their
/// destruction cannot invoke user code. No other payload type is inspected
/// for whether its destructor might be safe.
/// Boundaries that propagate the original failure should use `resume_unwind`
/// instead. This operation is for a failure already reported or superseded.
pub fn retain_opaque_payload(payload: Box<dyn Any + Send>) {
    if payload.is::<&'static str>() || payload.is::<String>() {
        drop(payload);
    } else {
        std::mem::forget(payload);
    }
}

/// Carries the first failure across nested callback delivery and retirement.
///
/// Framework delivery seams borrow this context from their enclosing owner.
/// Opaque ownership retires normally until a failure is caught; afterwards it
/// is retained. Call [`Self::finish`] only once all accepted work is delivered.
/// An aggregate that double-panics before containment regains control still
/// follows Rust's abort semantics.
#[doc(hidden)]
pub struct PanicRecovery {
    incoming: bool,
    first: Option<CapturedFailure>,
    blocking_class: Option<FailureClass>,
}

/// Authority is fixed when the local payload is captured. A later channel
/// inheritance cannot demote an already authoritative failure.
enum CapturedFailure {
    Authoritative {
        payload: Box<dyn Any + Send>,
        class: FailureClass,
    },
    InheritedAnnotation {
        payload: Box<dyn Any + Send>,
        class: FailureClass,
    },
}

impl CapturedFailure {
    fn payload(&self) -> &(dyn Any + Send) {
        match self {
            Self::Authoritative { payload, .. } | Self::InheritedAnnotation { payload, .. } => {
                payload.as_ref()
            }
        }
    }

    fn class(&self) -> FailureClass {
        match self {
            Self::Authoritative { class, .. } | Self::InheritedAnnotation { class, .. } => *class,
        }
    }

    fn into_parts(self) -> (Box<dyn Any + Send>, FailureClass) {
        match self {
            Self::Authoritative { payload, class }
            | Self::InheritedAnnotation { payload, class } => (payload, class),
        }
    }
}

/// Postcondition proven at a framework callout's actual capture seam.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    /// A required proposal failed before its valid state commit.
    Computation,
    /// State and outgoing delivery debt committed before the failing callout.
    Delivery,
    /// The callout establishes no reliable postcondition for its partial work.
    Unknown,
}

/// The first local annotation and independently accumulated publication restriction.
/// Dropping this receipt retains opaque payload ownership without user destruction.
/// It cannot resume a payload held under an enclosing owner's earlier failure.
#[doc(hidden)]
pub struct RecoveryFailure {
    captured: Option<CapturedFailure>,
    blocking_class: Option<FailureClass>,
}

impl RecoveryFailure {
    /// Borrow the original payload while this receipt retains its ownership.
    ///
    /// # Panics
    ///
    /// Panics if FLUI violates the live receipt's payload ownership invariant.
    #[must_use]
    pub fn payload(&self) -> &(dyn Any + Send) {
        self.captured
            .as_ref()
            .expect("BUG: live failure receipt owns its payload")
            .payload()
    }

    /// The class stamped at the first failure's actual capture seam.
    ///
    /// # Panics
    ///
    /// Panics if FLUI violates the live receipt's payload ownership invariant.
    #[must_use]
    pub fn class(&self) -> FailureClass {
        self.captured
            .as_ref()
            .expect("BUG: live failure receipt owns its provenance")
            .class()
    }

    /// The first blocking failure, independently of the first payload's class.
    #[must_use]
    pub fn blocking_class(&self) -> Option<FailureClass> {
        self.blocking_class
    }
}

impl std::fmt::Debug for RecoveryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryFailure")
            .field("class", &self.class())
            .field("blocking_class", &self.blocking_class)
            .finish_non_exhaustive()
    }
}

impl Drop for RecoveryFailure {
    fn drop(&mut self) {
        if let Some(captured) = self.captured.take() {
            retain_opaque_payload(captured.into_parts().0);
        }
    }
}

/// Borrowed failure custody during synchronous framework delivery.
///
/// A notification hands its observers the scope of the delivery that is
/// running them. An observer passes it on unchanged to any notification it
/// forwards (for example `ChangeNotifier::notify_listeners_with_recovery`), so a
/// panic anywhere in the chain is caught once, the first failure stays
/// authoritative, and later work in the same delivery still runs. Use
/// [`run`](Self::run) or [`run_with`](Self::run_with) for user code the
/// observer calls, and [`retire`](Self::retire) for values whose destructors
/// may run user code.
///
/// A scope cannot finish or own the enclosing failure. Channel links live on
/// the call stack, so composing relays does not allocate at any depth.
pub struct RecoveryScope<'scope> {
    owner: &'scope mut PanicRecovery,
    channels: Option<&'scope FailureLatch<'scope>>,
    class: FailureClass,
}

impl std::fmt::Debug for RecoveryScope<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryScope")
            .field("has_failure", &self.has_failure())
            .finish_non_exhaustive()
    }
}

struct FailureLatch<'scope> {
    signal: &'scope AtomicBool,
    parent: Option<&'scope Self>,
}

impl std::fmt::Debug for PanicRecovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanicRecovery")
            .field("has_failure", &self.has_failure())
            .finish_non_exhaustive()
    }
}

impl Default for PanicRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl PanicRecovery {
    #[must_use]
    pub fn new() -> Self {
        Self {
            incoming: std::thread::panicking(),
            first: None,
            blocking_class: None,
        }
    }

    /// Preserve a failure already held by the enclosing framework owner.
    /// Delivery continues, while outgoing opaque ownership and later payloads
    /// are retained. This context does not acquire or resume that outer payload.
    pub fn inherit_failure(&mut self) {
        self.incoming = true;
    }

    /// Lend custody without transferring the first failure or its completion.
    pub fn scope(&mut self) -> RecoveryScope<'_> {
        RecoveryScope {
            owner: self,
            channels: None,
            class: FailureClass::Unknown,
        }
    }

    pub fn run(&mut self, action: impl FnOnce()) {
        self.scope().run(action);
    }

    pub fn run_with(&mut self, action: impl FnOnce(&mut RecoveryScope<'_>)) {
        self.scope().run_with(action);
    }

    #[must_use]
    pub fn has_failure(&self) -> bool {
        self.incoming || self.first.is_some()
    }

    /// The class of this operation's first locally captured payload. An earlier
    /// inherited failure remains authoritative at its enclosing owner.
    #[must_use]
    pub fn first_failure_class(&self) -> Option<FailureClass> {
        self.first.as_ref().map(CapturedFailure::class)
    }

    /// The first capture that failed to prove publication safe. This includes
    /// later failures even when an earlier delivery payload is already owned.
    #[must_use]
    pub fn blocking_failure_class(&self) -> Option<FailureClass> {
        self.blocking_class
    }

    /// Transfer the first local annotation after accepted delivery has finished.
    /// Standalone propagation uses [`Self::finish`] instead.
    #[must_use]
    pub fn into_failure(mut self) -> Option<RecoveryFailure> {
        self.first.take().map(|captured| RecoveryFailure {
            captured: Some(captured),
            blocking_class: self.blocking_class,
        })
    }

    pub fn retire<T>(&mut self, value: T) {
        self.scope().retire(value);
    }

    pub fn finish(mut self) {
        match self.first.take() {
            Some(CapturedFailure::Authoritative { payload, .. }) => {
                std::panic::resume_unwind(payload);
            }
            Some(CapturedFailure::InheritedAnnotation { payload, .. }) => {
                retain_opaque_payload(payload);
            }
            None => {}
        }
    }
}

impl RecoveryScope<'_> {
    /// Execute one classified callout. The result reports this action's unwind,
    /// independently of failures already held by its enclosing owner.
    #[doc(hidden)]
    pub fn run_classified(
        &mut self,
        class: FailureClass,
        action: impl FnOnce(&mut RecoveryScope<'_>),
    ) -> bool {
        let mut child = RecoveryScope {
            owner: self.owner,
            channels: self.channels,
            class,
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(&mut child))) {
            Ok(()) => true,
            Err(payload) => {
                child.capture(payload);
                false
            }
        }
    }

    /// Stamp a payload captured at another framework producer without resuming it.
    #[doc(hidden)]
    pub fn capture_classified(&mut self, class: FailureClass, payload: Box<dyn Any + Send>) {
        self.capture_owned(CapturedFailure::Authoritative { payload, class });
    }

    /// Adopt a relay's receipt in capture order, before a later escaped payload.
    /// Inherited annotations never acquire standalone propagation authority.
    /// Earlier blocking evidence remains independent of the first payload.
    #[doc(hidden)]
    pub fn capture_failure(&mut self, mut failure: RecoveryFailure) {
        if self.owner.blocking_class.is_none() {
            self.owner.blocking_class = failure.blocking_class;
        }
        let captured = failure
            .captured
            .take()
            .expect("BUG: live failure receipt owns its captured failure");
        self.capture_owned(captured);
    }

    fn capture_owned(&mut self, captured: CapturedFailure) {
        let class = captured.class();
        if class != FailureClass::Delivery && self.owner.blocking_class.is_none() {
            self.owner.blocking_class = Some(class);
        }
        if self.owner.first.is_some() {
            retain_opaque_payload(captured.into_parts().0);
        } else {
            self.owner.first = Some(match captured {
                CapturedFailure::Authoritative { payload, class } if self.owner.incoming => {
                    CapturedFailure::InheritedAnnotation { payload, class }
                }
                first => first,
            });
        }
        self.mark_active_channels();
    }

    /// Retire ownership whose postcondition is established by its producer.
    #[doc(hidden)]
    pub fn retire_classified<T>(&mut self, class: FailureClass, value: T) {
        if self.has_failure() {
            std::mem::forget(value);
        } else {
            self.run_classified(class, |_| drop(value));
        }
    }

    /// A channel's outgoing snapshot belongs to the callout already classified
    /// by its producer. This proof does not apply to arbitrary caller ownership.
    pub(crate) fn retire_callout_capture<T>(&mut self, value: T) {
        self.retire_classified(self.class, value);
    }

    pub(crate) fn inherit_failure(&mut self) {
        self.owner.inherit_failure();
        self.mark_active_channels();
    }

    /// Keep channel retirement aware of failures caught by nested relays.
    /// Only the failure signal is shared, never callback storage or an owner.
    pub(crate) fn with_failure_latch(
        &mut self,
        signal: &AtomicBool,
        action: impl FnOnce(&mut RecoveryScope<'_>),
    ) {
        if signal.load(Ordering::Relaxed) {
            self.inherit_failure();
        }
        signal.store(self.has_failure(), Ordering::Relaxed);
        let latch = FailureLatch {
            signal,
            parent: self.channels,
        };
        let mut child = RecoveryScope {
            owner: self.owner,
            channels: Some(&latch),
            class: self.class,
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(&mut child)));
        if outcome.is_err() {
            child.mark_active_channels();
        }
        if let Err(payload) = outcome {
            std::panic::resume_unwind(payload);
        }
    }

    fn mark_active_channels(&self) {
        let mut channel = self.channels;
        while let Some(latch) = channel {
            latch.signal.store(true, Ordering::Relaxed);
            channel = latch.parent;
        }
    }

    /// Open the outermost scope of a delivery that does not run inside a
    /// foundation channel, such as a custom source notifying its own observers.
    ///
    /// Every panic inside `delivery` is caught; once it returns, the first
    /// failure resumes on the caller.
    pub fn boundary(delivery: impl FnOnce(&mut RecoveryScope<'_>)) {
        let mut recovery = PanicRecovery::new();
        recovery.run_with(delivery);
        recovery.finish();
    }

    /// Run user code, catching a panic into this delivery's custody instead of
    /// unwinding through the caller.
    pub fn run(&mut self, action: impl FnOnce()) {
        self.run_with(|_| action());
    }

    /// [`run`](Self::run) for code that forwards the scope to nested deliveries.
    pub fn run_with(&mut self, action: impl FnOnce(&mut RecoveryScope<'_>)) {
        self.run_classified(self.class, action);
    }

    pub(crate) fn capture(&mut self, payload: Box<dyn Any + Send>) {
        self.capture_classified(self.class, payload);
    }

    /// Whether this delivery, or an enclosing one, already holds a failure.
    #[must_use]
    pub fn has_failure(&self) -> bool {
        self.owner.has_failure()
    }

    /// Drop `value` inside this delivery's custody. After a failure its
    /// destructor is not run, so user code never executes on a failed path.
    pub fn retire<T>(&mut self, value: T) {
        if self.has_failure() {
            std::mem::forget(value);
        } else {
            self.run_classified(FailureClass::Unknown, |_| drop(value));
        }
    }
}

impl Drop for PanicRecovery {
    fn drop(&mut self) {
        if let Some(captured) = self.first.take() {
            retain_opaque_payload(captured.into_parts().0);
        }
    }
}
