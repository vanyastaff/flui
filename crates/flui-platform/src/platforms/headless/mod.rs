//! Headless platform implementation for testing

mod display;
mod host_limits;
mod platform;

pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub use host_limits::HostLimits;
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessDisplayControls, HeadlessExitReevaluation, HeadlessOwnerTurns, HeadlessPlatform,
    MockWindow,
};
