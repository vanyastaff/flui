//! Headless platform implementation for testing

mod display;
mod drag;
mod platform;

pub use drag::HeadlessDragAndDrop;

pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessDisplayControls, HeadlessExitReevaluation, HeadlessInputDevices, HeadlessOwnerTurns,
    HeadlessPlatform, MockWindow,
};
