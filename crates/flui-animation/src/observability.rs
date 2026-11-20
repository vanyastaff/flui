//! Optional tracing callouts never own animation failure or delivery custody.

use flui_foundation::panic::FirstPanic;

// Run only a subscriber callout, never the animation operation itself. A
// subscriber's panic is discarded when the collector drops: observability
// cannot supersede a source or listener failure, nor stop accepted work.
pub(crate) fn observe<T>(action: impl FnOnce() -> T) -> Option<T> {
    FirstPanic::new().contain(action)
}

#[cfg(feature = "diagnostics")]
fn retire_span(span: tracing::Span) {
    // Closing a span invokes try_close before its Dispatch field retires.
    // Keep a separate Dispatch alive so a failing close cannot unwind through
    // the last subscriber's arbitrary destructor inside Span::drop.
    let dispatch = span.with_subscriber(|(_, dispatch)| dispatch.clone());
    observe(|| drop(span));
    observe(|| drop(dispatch));
}

/// A [`ProfileSpan`] named `$name`; an empty one without `diagnostics`.
macro_rules! profile_span {
    ($name:literal) => {{
        #[cfg(feature = "diagnostics")]
        {
            ProfileSpan::new(|| tracing::debug_span!(target: "flui.animation", $name))
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
}

impl ProfileSpan {
    #[cfg(feature = "diagnostics")]
    fn new(make_span: impl FnOnce() -> tracing::Span) -> Self {
        let span = observe(make_span);
        if let Some(span) = &span {
            observe(|| span.with_subscriber(|(id, dispatch)| dispatch.enter(id)));
        }
        Self { span }
    }

    /// Exit and close the span here rather than where the value drops. A
    /// subscriber that panics is discarded ([`observe`]), so a caught
    /// animation failure stays the one its recovery resumes.
    #[inline]
    pub(crate) fn close(self) {
        #[cfg(feature = "diagnostics")]
        {
            let mut profile = self;
            profile.finish();
        }
        #[cfg(not(feature = "diagnostics"))]
        {
            let _ = self;
        }
    }

    #[cfg(feature = "diagnostics")]
    fn finish(&mut self) {
        if let Some(span) = self.span.take() {
            observe(|| span.with_subscriber(|(id, dispatch)| dispatch.exit(id)));
            retire_span(span);
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
        self.finish();
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
                    target: "flui.animation.diagnostics",
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
            retire_span(span);
        }
    }
}
