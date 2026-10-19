//! Headless platform implementation for testing

mod display;
mod platform;
mod window_policy;

pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessDisplayControls, HeadlessExitReevaluation, HeadlessHostEvents, HeadlessOwnerTurns,
    HeadlessPlatform, MockWindow,
};
pub use window_policy::HeadlessWindowPolicy;
