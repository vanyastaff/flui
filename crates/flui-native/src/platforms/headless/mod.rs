//! Headless platform implementation for testing

mod platform;

mod window_policy;

pub use crate::display::headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplayControls,
};
pub use platform::{
    FakeSystemChrome, HeadlessHostEvents, HeadlessInputDevices, HeadlessPreferences, MockWindow,
};
pub(crate) use platform::{TextStoreHostFactory, create_mock_window};
pub use window_policy::HeadlessWindowPolicy;

pub use crate::owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
pub use crate::text::headless::FakeTextInput;
