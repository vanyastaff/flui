// Without its `scripted` feature the core host names none of the headless
// host's doubles and controls: no facade feature turns it on.
use flui_native::{
    DisplayChange, DisplayChangeError, DisplaySpec, FakeAccessibility, FakeTextInput,
    HeadlessDeferredWindowOpens, HeadlessDisplayControls, HeadlessDragAndDrop,
    HeadlessExitReevaluation, HeadlessHostEvents, HeadlessHostWindow, HeadlessInputDevices,
    HeadlessOpenWindows, HeadlessOwnerTurns, HeadlessPlatform, HeadlessPreferences,
    HeadlessWindowPolicy, HeadlessWindowSystem, UnknownDisplay,
};

fn main() {}
