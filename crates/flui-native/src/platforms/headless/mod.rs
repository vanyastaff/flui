//! Headless platform implementation for testing

mod drag;
mod platform;

mod window_policy;

pub use drag::HeadlessDragAndDrop;

pub(crate) use crate::display::headless::HeadlessDisplays;
pub use crate::display::headless::{DisplayChange, DisplayChangeError, DisplaySpec};
pub(crate) use drag::DragDouble;
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, FakeTextInput, HeadlessDisplayControls,
    HeadlessHostEvents, HeadlessInputDevices, HeadlessPreferences, MockWindow,
};
pub(crate) use platform::{TextStoreHostFactory, create_mock_window};
pub use window_policy::HeadlessWindowPolicy;

pub use crate::owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
