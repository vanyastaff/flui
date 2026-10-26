//! The iOS text input: a window's text size, captured from its view's
//! traits. Frozen traits map a regular system reference font to a numeric
//! point size.

// UIKit's font and text-style constants are extern statics read through
// objc: no safe wrapper. Every `unsafe` block carries its own `// SAFETY:`.
#![expect(unsafe_code)]

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::TextSizingCaptureError as Error;
use crate::owner::apple::ios::{NativeOwner, ViewRef};
use crate::owner::signal::OwnerSignal;
use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_ui_kit::{
    UIFont, UIFontMetrics, UIFontTextStyleBody, UIFontTextStyleCallout, UIFontTextStyleCaption1,
    UIFontTextStyleCaption2, UIFontTextStyleFootnote, UIFontTextStyleHeadline,
    UIFontTextStyleLargeTitle, UIFontTextStyleSubheadline, UIFontTextStyleTitle1,
    UIFontTextStyleTitle2, UIFontTextStyleTitle3, UIFontWeightRegular, UITraitCollection,
    UITraitEnvironment, UIView,
};

/// The text position of the iOS table: each window's text size, captured
/// from its view's traits and kept until they change. UIKit hands out no
/// input method here, so a window answers no text input.
///
/// Public where it is defined, as the host's table names it as its text
/// position, and nameable only inside the crate.
#[derive(Debug)]
pub struct IosTextSizing;

impl crate::host::Text<ViewRef> for IosTextSizing {
    fn for_window(window: &ViewRef) -> Arc<dyn crate::text::TextInput> {
        Arc::new(ViewTextSizing {
            view: NativeOwner::new(window.view.clone(), window.marker),
            captured: NativeOwner::new(RefCell::new(None), window.marker),
            authority: parking_lot::Mutex::new(Arc::new(AtomicBool::new(true))),
            signal: window.signal.clone(),
        })
    }
}

/// One window's text size: the view it is read from, the capture it handed
/// out last, and the authority that capture stays current under.
struct ViewTextSizing {
    view: NativeOwner<Retained<UIView>>,
    captured: NativeOwner<RefCell<Option<crate::CapturedTextSizing>>>,
    /// Cleared when the view's traits change, the window closes or the
    /// platform fences captures; a capture made under it is current while it
    /// holds.
    authority: parking_lot::Mutex<Arc<AtomicBool>>,
    /// The owner, woken when what was captured no longer holds.
    signal: std::sync::Weak<OwnerSignal>,
}

impl crate::text::TextInput for ViewTextSizing {
    fn window_text_input(&self) -> Option<crate::text::WindowTextInput> {
        None
    }

    /// The last capture while it holds, otherwise the view's traits now, on
    /// the main thread. A capture whose traits changed while they were read
    /// is refused, so no stale size is handed out.
    fn capture_sizing(&self) -> Result<crate::TextSizingCaptureState, Error> {
        let marker = MainThreadMarker::new().ok_or(Error::WrongThread)?;
        let cached = self.captured.get(marker).borrow().clone();
        if let Some(cached) = cached.filter(crate::CapturedTextSizing::is_current) {
            return Ok(crate::TextSizingCaptureState::Ready(cached));
        }
        let authority = Arc::clone(&self.authority.lock());
        let view = self.view.get(marker);
        let traits =
            objc2::exception::catch(std::panic::AssertUnwindSafe(|| view.traitCollection()))
                .map_err(|exception| {
                    drop(exception);
                    Error::Native {
                        message: "UIKit trait capture failed".into(),
                    }
                })?;
        if !authority.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        let capture = crate::CapturedTextSizing::new(
            Arc::new(Snapshot { traits }),
            authority,
            std::thread::current().id(),
        );
        let retired = self.captured.get(marker).replace(Some(capture.clone()));
        drop(retired);
        Ok(crate::TextSizingCaptureState::Ready(capture))
    }

    /// The view's traits changed: the last capture stops being current, and
    /// the owner is woken to capture again.
    fn invalidate_sizing(&self) {
        let retired =
            std::mem::replace(&mut *self.authority.lock(), Arc::new(AtomicBool::new(true)));
        retired.store(false, Ordering::Release);
        if let Some(marker) = MainThreadMarker::new() {
            let dropped = self.captured.get(marker).borrow_mut().take();
            drop(dropped);
        }
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }

    fn fence_sizing(&self) {
        self.authority.lock().store(false, Ordering::Release);
    }

    /// The window closed: no capture of it stays current.
    fn detach(&self) {
        self.fence_sizing();
    }
}

pub(crate) struct Snapshot {
    pub(crate) traits: Retained<UITraitCollection>,
}

impl crate::text::sizing::NativeTextSize for Snapshot {
    fn query(&self, request: TextSizeRequest) -> Result<TextSize, Error> {
        // SAFETY: immutable SDK NSString constants, all available on iOS 11.
        let style = unsafe {
            match request.profile {
                TextScaleProfile::LargeTitle => UIFontTextStyleLargeTitle,
                TextScaleProfile::Title1 => UIFontTextStyleTitle1,
                TextScaleProfile::Title2 => UIFontTextStyleTitle2,
                TextScaleProfile::Title3 => UIFontTextStyleTitle3,
                TextScaleProfile::Headline => UIFontTextStyleHeadline,
                TextScaleProfile::Body => UIFontTextStyleBody,
                TextScaleProfile::Callout => UIFontTextStyleCallout,
                TextScaleProfile::Subheadline => UIFontTextStyleSubheadline,
                TextScaleProfile::Footnote => UIFontTextStyleFootnote,
                TextScaleProfile::Caption1 => UIFontTextStyleCaption1,
                TextScaleProfile::Caption2 => UIFontTextStyleCaption2,
                _ => return Err(Error::UnsupportedProfile),
            }
        };
        let size = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            // SAFETY: immutable SDK weight constant; TextSize admits the point
            // size and this reference font has never been Dynamic-Type scaled.
            let font = UIFont::systemFontOfSize_weight(request.size.value(), unsafe {
                UIFontWeightRegular
            });
            let metrics = UIFontMetrics::metricsForTextStyle(style);
            let scaled =
                metrics.scaledFontForFont_compatibleWithTraitCollection(&font, Some(&self.traits));
            // SAFETY: owned UIFont queried synchronously on the capsule owner.
            unsafe { scaled.pointSize() }
        }))
        .map_err(|exception| {
            // Retire !Send exceptions on this lane; avoid messaging an exception
            // to manufacture diagnostics and potentially raising another one.
            drop(exception);
            Error::Native {
                message: "UIKit rejected the native text size query".into(),
            }
        })?;
        TextSize::new(size).map_err(Into::into)
    }
}
