//! Headless platform implementation for testing

mod display;
mod platform;

mod window_policy;

pub(crate) use display::HeadlessDisplays;
pub use display::{DisplayChange, DisplayChangeError, DisplaySpec};
pub use platform::{
    FakeSystemChrome, HeadlessDisplayControls, HeadlessHostEvents, HeadlessInputDevices,
    HeadlessPreferences, MockWindow,
};
pub(crate) use platform::{TextStoreHostFactory, create_mock_window};
pub use window_policy::HeadlessWindowPolicy;

pub use crate::owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
pub use crate::text::headless::FakeTextInput;
