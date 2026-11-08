//! The headless host's text input: a no-op production sink, an opt-in call
//! recorder for tests, and the text-store host a test makes its windows offer.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::PlatformTextInput;
use flui_platform::text_store::TextStoreHost;

use super::WindowTextInput;
use crate::host::Host;
use crate::owner::headless::HeadlessLoop;
use crate::platform::HostPlatform;

/// The production headless text-input sink. It retains no request history.
#[derive(Debug, Default)]
pub struct HeadlessTextInput;

macro_rules! impl_headless_text_input {
    ($type:ty) => {
        impl super::TextInput for Arc<$type> {
            fn window_text_input(&self) -> Option<WindowTextInput> {
                Some(WindowTextInput::Ime(
                    Arc::clone(self) as Arc<dyn PlatformTextInput>
                ))
            }

            fn capture_sizing(
                &self,
            ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
                Ok(crate::TextSizingCaptureState::Unsupported)
            }

            fn invalidate_sizing(&self) {}

            fn fence_sizing(&self) {}

            fn detach(&self) {}
        }
    };
}

impl_headless_text_input!(HeadlessTextInput);

impl PlatformTextInput for HeadlessTextInput {
    fn set_ime_allowed(&self, _: bool) {}

    fn set_ime_cursor_area(&self, _: flui_foundation::geometry::Bounds<f64>) {}

    fn set_field_status(&self, _: flui_platform::text_store::TextStoreStatus) {}

    fn discard_composition(&self) {}
}

impl crate::host::Text<()> for HeadlessTextInput {
    fn for_window((): &()) -> Arc<dyn super::TextInput> {
        Arc::new(Arc::new(Self))
    }
}

/// The headless platform's text-store host: the pull model a window offers
/// when a test asks for it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Make every window this platform opens pull-model: its
    /// [`HostWindow::text_store_host`](crate::HostWindow::text_store_host)
    /// answers a host that `factory` builds, on the owner thread, each time it
    /// is read (the runner reads it once per window). A test-support stand-in
    /// for the Win32 text services (ADR-0135); without it a headless window is
    /// push-model and offers
    /// [`PlatformWindow::text_input`](flui_platform::PlatformWindow::text_input)
    /// alone.
    #[must_use]
    pub fn with_text_store_host(
        self,
        factory: impl Fn() -> Rc<dyn TextStoreHost> + Send + Sync + 'static,
    ) -> Self {
        self.owner()
            .with_state(|state| state.text_store_host = Some(Arc::new(factory)));
        self
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_platform::WindowOptions;
    use flui_platform::text_store::{CompositionEnd, TextStore, TextStoreHost, TextStoreHostError};

    use crate::HeadlessPlatform;
    use crate::platform::Platform;

    struct Host;

    impl TextStoreHost for Host {
        fn focus_store(&self, _: Option<Rc<dyn TextStore>>) {}

        fn complete_composition(
            &self,
            _: &Rc<dyn TextStore>,
        ) -> Result<CompositionEnd, TextStoreHostError> {
            Ok(CompositionEnd::Committed)
        }
    }

    /// Open one window on `platform` and report whether the owner-thread
    /// capability reaches a text-store host on it.
    fn offers_a_host(platform: HeadlessPlatform) -> bool {
        let offered = Rc::new(Cell::new(None));
        let observed = Rc::clone(&offered);
        Box::new(platform)
            .run(crate::owner::app_delivery::testing::on_ready(
                move |owner| {
                    let window = owner
                        .open_window(
                            WindowOptions::default(),
                            flui_testing::window_handler::RecordingWindowHandler::new(),
                        )
                        .expect("headless window")
                        .try_ready()
                        .expect("ready inside on_ready");
                    observed.set(Some(owner.text_store_host(&window).is_some()));
                    Ok(())
                },
            ))
            .expect("headless run");
        offered.get().expect("on_ready ran")
    }

    /// The owner-thread capability reaches a pull-model window's host; a
    /// push-model window (the headless default) answers `None`.
    #[test]
    fn the_owner_platform_reads_a_window_s_text_store_host() {
        assert!(
            offers_a_host(HeadlessPlatform::new().with_text_store_host(|| Rc::new(Host))),
            "pull window"
        );
        assert!(!offers_a_host(HeadlessPlatform::new()), "push window");
    }
}
