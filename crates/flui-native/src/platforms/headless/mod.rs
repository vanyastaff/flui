//! Headless platform implementation for testing

mod drag;
mod platform;

mod window_policy;

pub use drag::HeadlessDragAndDrop;

pub use crate::display::headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplayControls,
};
pub(crate) use drag::DragDouble;
pub use platform::{
    FakeAccessibility, FakeHaptics, FakeSystemChrome, HeadlessInputDevices, MockWindow,
};
pub(crate) use platform::{TextStoreHostFactory, create_mock_window};
pub use window_policy::HeadlessWindowPolicy;

pub use crate::lifecycle::headless::HeadlessHostEvents;
pub use crate::owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
pub use crate::preferences::headless::HeadlessPreferences;
pub use crate::text::headless::FakeTextInput;
