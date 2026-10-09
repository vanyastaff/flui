//! Portable failure-policy seam for Android's owner-local scroll-factor cache.

use flui_foundation::geometry::DevicePixelRatio;
use flui_platform_api::pointer::{ScrollDelta, ScrollUnit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FactorApi {
    ThemeAttribute,
    ViewConfiguration,
}

impl FactorApi {
    pub(crate) const fn for_sdk(sdk: i32) -> Self {
        if sdk >= 26 {
            Self::ViewConfiguration
        } else {
            Self::ThemeAttribute
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PixelFactors {
    horizontal: f64,
    vertical: f64,
}

impl PixelFactors {
    pub(crate) fn from_physical(
        horizontal: f64,
        vertical: f64,
        ratio: DevicePixelRatio,
    ) -> Option<Self> {
        let (horizontal, vertical) = (ratio.to_logical(horizontal), ratio.to_logical(vertical));
        if horizontal.is_finite() && vertical.is_finite() && horizontal >= 0.0 && vertical >= 0.0 {
            Some(Self {
                horizontal,
                vertical,
            })
        } else {
            None
        }
    }
}

#[derive(Default)]
pub(crate) struct FactorCache {
    accepted: Option<PixelFactors>,
}

impl FactorCache {
    /// A failed refresh retains the accepted logical factors, even across DPI
    /// changes. With no accepted observation the authored compatibility policy
    /// treats an axis unit as one line, without claiming an OS measurement.
    pub(crate) fn observe(&mut self, reading: Option<PixelFactors>) {
        if let Some(reading) = reading {
            self.accepted = Some(reading);
        }
    }

    pub(crate) fn policy(&self) -> AxisPolicy {
        self.accepted
            .map_or(AxisPolicy::AuthoredLines, AxisPolicy::Pixels)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AxisPolicy {
    Pixels(PixelFactors),
    AuthoredLines,
}

impl AxisPolicy {
    pub(crate) fn delta(self, horizontal: f32, vertical: f32) -> Option<ScrollDelta> {
        let (unit, horizontal, vertical) = match self {
            Self::Pixels(factors) => (
                ScrollUnit::Pixels,
                -f64::from(horizontal) * factors.horizontal,
                -f64::from(vertical) * factors.vertical,
            ),
            Self::AuthoredLines => (
                ScrollUnit::Lines,
                -f64::from(horizontal),
                -f64::from(vertical),
            ),
        };
        ScrollDelta::try_new(unit, horizontal, vertical).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_axis_factor_policy() {
        for sdk in [21, 22, 23, 24, 25] {
            assert_eq!(FactorApi::for_sdk(sdk), FactorApi::ThemeAttribute);
        }
        for sdk in [26, 35, 36] {
            assert_eq!(FactorApi::for_sdk(sdk), FactorApi::ViewConfiguration);
        }
        let mut cache = FactorCache::default();
        cache.observe(None);
        let fallback = cache.policy().delta(0.25, -0.5).expect("authored fallback");
        assert_eq!(
            (fallback.unit(), fallback.x(), fallback.y()),
            (ScrollUnit::Lines, -0.25, 0.5)
        );
        cache.observe(PixelFactors::from_physical(
            48.0,
            64.0,
            DevicePixelRatio::new(2.0).expect("density"),
        ));
        for _ in 0..2 {
            cache.observe(None);
            let accepted = cache
                .policy()
                .delta(0.25, -0.5)
                .expect("accepted native factors");
            assert_eq!(
                (accepted.unit(), accepted.x(), accepted.y()),
                (ScrollUnit::Pixels, -6.0, 16.0)
            );
        }
        cache.observe(PixelFactors::from_physical(
            12.0,
            24.0,
            DevicePixelRatio::new(3.0).expect("new density"),
        ));
        let recovered = cache.policy().delta(0.25, -0.5).expect("new context");
        assert_eq!((recovered.x(), recovered.y()), (-1.0, 4.0));
        for value in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(PixelFactors::from_physical(value, 1.0, DevicePixelRatio::ONE).is_none());
        }
        let tiny = DevicePixelRatio::new(f64::from_bits(1)).expect("finite density");
        assert!(PixelFactors::from_physical(1.0, 1.0, tiny).is_none());
        assert!(cache.policy().delta(f32::NAN, 0.0).is_none());
    }
}
