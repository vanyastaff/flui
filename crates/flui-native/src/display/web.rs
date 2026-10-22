//! The browser's displays: the screens the page reports and its pixel ratio and
//! animation-frame cadence ([`source`]), the listeners that keep them current
//! (`events`) and the capabilities they need, detected once (`browser`).
//!
//! The rules behind them are pure, and every host runs their tests: the refresh
//! estimate from animation-frame timestamps ([`refresh_estimate`]), the
//! pixel-ratio query ([`dppx_query`]), screen identity ([`screen_identity`]) and
//! the proof a capability exists ([`feature`]).

#[cfg(target_arch = "wasm32")]
mod browser;
pub(crate) mod dppx_query;
#[cfg(target_arch = "wasm32")]
mod events;
pub(crate) mod feature;
pub(crate) mod refresh_estimate;
pub(crate) mod screen_identity;
#[cfg(target_arch = "wasm32")]
pub(crate) mod source;

#[cfg(target_arch = "wasm32")]
pub(crate) use events::{DisplayObservers, PixelRatioWatch};
#[cfg(target_arch = "wasm32")]
pub(crate) use source::WebDisplays;
