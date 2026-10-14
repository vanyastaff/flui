//! Optional tracing callouts never own animation failure or delivery custody.

#[cfg(any(debug_assertions, feature = "diagnostics"))]
use flui_foundation::panic::PanicRecovery;
use flui_foundation::panic::RecoveryScope;

/// Run only a subscriber callout, never the animation operation itself.
#[cfg(any(debug_assertions, feature = "diagnostics"))]
pub(crate) fn observe<T>(action: impl FnOnce() -> T) -> Option<T> {
    let mut recovery = PanicRecovery::new();
    let mut output = None;
    recovery.run(|| output = Some(action()));
    // RecoveryFailure retires exact text and retains opaque payloads without
    // invoking arbitrary payload destructors. Observability cannot supersede
    // a source/listener failure or prevent accepted work from being delivered.
    drop(recovery.into_failure());
    output
}

#[cfg(feature = "diagnostics")]
fn retire_span(span: tracing::Span, failed: bool) {
    // Closing a span invokes try_close before its Dispatch field retires.
    // Keep a separate Dispatch alive so a failing close cannot unwind through
    // the last subscriber's arbitrary destructor inside Span::drop.
    let dispatch = span.with_subscriber(|(_, dispatch)| dispatch.clone());
    let closed = observe(|| drop(span)).is_some();
    let mut recovery = PanicRecovery::new();
    if failed || !closed {
        recovery.inherit_failure();
    }
    recovery.retire(dispatch);
    drop(recovery.into_failure());
}

/// A [`ProfileSpan`] named `$name`; an empty one without `diagnostics`.
macro_rules! profile_span {
    ($name:literal) => {{
        #[cfg(feature = "diagnostics")]
        {
            ProfileSpan::new(|| tracing::debug_span!(target: "flui_animation", $name))
        }
        #[cfg(not(feature = "diagnostics"))]
        {
            ProfileSpan {}
        }
    }};
}

/// Own span closure separately from exit: a failing exit cannot double-panic
/// while an entered guard automatically drops the same subscriber's span.
pub(crate) struct ProfileSpan {
    #[cfg(feature = "diagnostics")]
    span: Option<tracing::Span>,
    #[cfg(feature = "diagnostics")]
    failed: bool,
}

impl ProfileSpan {
    #[cfg(feature = "diagnostics")]
    fn new(make_span: impl FnOnce() -> tracing::Span) -> Self {
        let span = observe(make_span);
        let mut failed = span.is_none();
        if let Some(span) = &span {
            failed |=
                observe(|| span.with_subscriber(|(id, dispatch)| dispatch.enter(id))).is_none();
        }
        Self { span, failed }
    }

    /// Caught animation failures remain authoritative during observer cleanup.
    #[inline]
    pub(crate) fn close(self, recovery: &RecoveryScope<'_>) {
        #[cfg(feature = "diagnostics")]
        {
            let mut profile = self;
            profile.finish(recovery.has_failure());
        }
        #[cfg(not(feature = "diagnostics"))]
        {
            let _ = (self, recovery);
        }
    }

    #[cfg(feature = "diagnostics")]
    fn finish(&mut self, failed: bool) {
        if let Some(span) = self.span.take() {
            let exited =
                observe(|| span.with_subscriber(|(id, dispatch)| dispatch.exit(id))).is_some();
            retire_span(span, failed || self.failed || !exited);
        }
    }

    #[inline]
    pub(crate) fn tick() -> Self {
        profile_span!("animation_tick")
    }

    #[inline]
    pub(crate) fn sampling() -> Self {
        profile_span!("animation_sampling")
    }

    #[inline]
    pub(crate) fn listeners() -> Self {
        profile_span!("animation_listeners")
    }
}

#[cfg(feature = "diagnostics")]
impl Drop for ProfileSpan {
    fn drop(&mut self) {
        self.finish(std::thread::panicking());
    }
}

#[cfg(feature = "diagnostics")]
pub(crate) struct ControllerTrace {
    span: Option<tracing::Span>,
}

#[cfg(feature = "diagnostics")]
impl ControllerTrace {
    pub(crate) fn new(origin: &crate::origin::ControllerOrigin) -> Self {
        let label = origin.label.as_deref();
        let created_at = origin.site.location().map(tracing::field::display);
        Self {
            span: observe(|| {
                tracing::debug_span!(
                    target: "flui_animation::diagnostics",
                    parent: None,
                    "animation_controller",
                    label,
                    created_at,
                )
            }),
        }
    }

    pub(crate) fn span(&self) -> Option<&tracing::Span> {
        self.span.as_ref()
    }
}

#[cfg(feature = "diagnostics")]
impl Drop for ControllerTrace {
    fn drop(&mut self) {
        if let Some(span) = self.span.take() {
            retire_span(span, std::thread::panicking());
        }
    }
}
