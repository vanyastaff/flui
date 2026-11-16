//! What an Activity's window insets say in logical pixels: the pure half of
//! the Android insets capture (`window::android`'s `insets`), which reads the
//! system bars, cutout, keyboard and system gesture regions on the Java view
//! thread and hands them here as device-pixel edges.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{DevicePixelRatio, EdgeInsets, Size};
use flui_platform::{KeyboardInset, WindowInsets};

use crate::window::keyboard_motion::KeyboardMotion;

/// Device-pixel edges as top, right, bottom, left.
pub(crate) type Edges = [i32; 4];

/// One reading of the view's insets, in device pixels, at the view's size.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    /// The view's size the reading was taken at.
    pub(crate) size: Size<i32>,
    /// The system bars and the display cutout.
    pub(crate) safe_area: Edges,
    /// The input method's window where it covers the view.
    pub(crate) keyboard: Edges,
    /// The edges system gestures take.
    pub(crate) system_gestures: Edges,
}

impl Sample {
    /// Whether this sample was taken at `size`.
    pub(crate) fn is_for(self, size: Size<i32>) -> bool {
        self.size == size
    }

    /// The sample in logical pixels at `scale`. The keyboard moves along
    /// `motion` when this sample is that motion's end, and is at rest
    /// otherwise: a sample alone says only where the keyboard ends up.
    /// `None` for a sample taken at another size.
    pub(crate) fn logical(
        self,
        size: Size<i32>,
        scale: DevicePixelRatio,
        motion: Option<KeyboardMotion>,
    ) -> Option<WindowInsets> {
        if self.size != size || size.width <= 0 || size.height <= 0 {
            return None;
        }
        let logical = |[top, right, bottom, left]: Edges| {
            EdgeInsets::new(
                scale.to_logical(f64::from(top.clamp(0, size.height))),
                scale.to_logical(f64::from(right.clamp(0, size.width))),
                scale.to_logical(f64::from(bottom.clamp(0, size.height))),
                scale.to_logical(f64::from(left.clamp(0, size.width))),
            )
        };
        let target = logical(self.keyboard);
        let keyboard = match motion.and_then(|motion| {
            motion.keyboard(target, self.keyboard[2].clamp(0, size.height), scale)
        }) {
            Some(moving) => moving,
            None => KeyboardInset::try_at_rest(target).ok()?,
        };
        WindowInsets::try_new(
            logical(self.safe_area),
            keyboard,
            logical(self.system_gestures),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(scale: f64) -> DevicePixelRatio {
        DevicePixelRatio::try_new(scale).expect("a ratio in range")
    }

    #[test]
    fn android_insets_native_metrics() {
        let size = Size::new(1080, 2400);
        let sample = Sample {
            size,
            safe_area: [128, 0, 63, 0],
            keyboard: [0, 0, 840, 0],
            system_gestures: [0, 63, 63, 63],
        };
        assert!(
            sample.is_for(size),
            "a sample is for the size it was taken at"
        );
        assert!(!sample.is_for(Size::new(2400, 1080)), "and no other");
        let insets = sample
            .logical(size, ratio(2.625), None)
            .expect("a sample at its own size");
        assert_eq!(
            insets.safe_area(),
            EdgeInsets::new(128.0 / 2.625, 0.0, 24.0, 0.0)
        );
        assert_eq!(
            insets.keyboard(),
            KeyboardInset::try_at_rest(EdgeInsets::new(0.0, 0.0, 320.0, 0.0))
                .expect("a valid keyboard"),
            "a keyboard sampled from the insets is at rest"
        );
        assert_eq!(
            insets.system_gestures(),
            EdgeInsets::new(0.0, 24.0, 24.0, 24.0)
        );
        assert!(
            sample
                .logical(Size::new(2400, 1080), ratio(2.625), None)
                .is_none(),
            "an old portrait reply cannot enter a landscape frame"
        );
        // At either end of the range a ratio can be, the insets stay finite.
        for scale in [DevicePixelRatio::MIN, DevicePixelRatio::MAX] {
            let insets = sample
                .logical(size, scale, None)
                .expect("finite insets at a ratio in range");
            assert!(insets.safe_area().top.is_finite(), "{scale:?}");
        }
    }
}
