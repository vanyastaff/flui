//! Headless platform implementation for testing

mod display;
mod drag;
mod platform;

mod window_policy;

pub use drag::HeadlessDragAndDrop;

pub(crate) use display::HeadlessDisplays;
pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub(crate) use drag::DragDouble;
pub(crate) use platform::HeadlessLoop;
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens,
    HeadlessDisplayControls, HeadlessExitReevaluation, HeadlessHostEvents, HeadlessInputDevices,
    HeadlessOwnerTurns, HeadlessPreferences, MockWindow,
};
pub use window_policy::HeadlessWindowPolicy;
