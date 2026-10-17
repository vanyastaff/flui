//! Headless platform implementation for testing

mod drag;
mod platform;

pub use drag::HeadlessDragAndDrop;

pub use platform::{
    FakeAccessibility, FakeHaptics, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessExitReevaluation, HeadlessInputDevices, HeadlessOwnerTurns, HeadlessPlatform,
    MockWindow,
};
