//! Headless platform implementation for testing

mod platform;

pub use platform::{
    FakeAccessibility, FakeHaptics, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessExitReevaluation, HeadlessPlatform, MockWindow,
};
