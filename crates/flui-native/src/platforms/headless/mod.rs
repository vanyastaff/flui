//! Headless platform implementation for testing

mod platform;

pub use crate::display::headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplayControls,
};
pub use crate::input::headless::HeadlessInputDevices;
pub(crate) use platform::TextStoreHostFactory;

pub use crate::lifecycle::headless::HeadlessHostEvents;
pub use crate::owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
pub use crate::preferences::headless::HeadlessPreferences;
pub use crate::text::headless::FakeTextInput;
// The window feature's headless types, at the paths tests name them by.
pub use crate::window::headless::{FakeSystemChrome, HeadlessWindowPolicy, MockWindow};
