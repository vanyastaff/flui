//! Headless platform implementation for testing

mod display;
mod drag;
mod platform;

mod window_policy;

pub use drag::HeadlessDragAndDrop;

pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessDisplayControls, HeadlessExitReevaluation, HeadlessHostEvents, HeadlessInputDevices,
    HeadlessOwnerTurns, HeadlessPlatform, HeadlessPreferences, MockWindow,
};
pub use window_policy::HeadlessWindowPolicy;
