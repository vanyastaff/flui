//! Frozen traits map a regular system reference font to a numeric point size.

use crate::TextSizingCaptureError as Error;
use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
use objc2::rc::Retained;
use objc2_ui_kit::{
    UIFont, UIFontMetrics, UIFontTextStyleBody, UIFontTextStyleCallout, UIFontTextStyleCaption1,
    UIFontTextStyleCaption2, UIFontTextStyleFootnote, UIFontTextStyleHeadline,
    UIFontTextStyleLargeTitle, UIFontTextStyleSubheadline, UIFontTextStyleTitle1,
    UIFontTextStyleTitle2, UIFontTextStyleTitle3, UIFontWeightRegular, UITraitCollection,
};

pub(crate) struct Snapshot {
    pub(super) traits: Retained<UITraitCollection>,
}

impl Snapshot {
    pub(crate) fn query(&self, request: TextSizeRequest) -> Result<TextSize, Error> {
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
