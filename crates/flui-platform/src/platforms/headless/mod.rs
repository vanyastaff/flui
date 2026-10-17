//! Headless platform implementation for testing

mod platform;

pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessExitReevaluation, HeadlessOwnerTurns, HeadlessPlatform, MockWindow,
};
