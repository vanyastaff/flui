//! Headless platform implementation for testing

mod display;
mod platform;

pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessDisplayControls, HeadlessExitReevaluation, HeadlessOwnerTurns, HeadlessPlatform,
    MockWindow,
};
