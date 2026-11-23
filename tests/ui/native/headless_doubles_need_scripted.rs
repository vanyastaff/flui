// Without its `scripted` feature the core host has no `testing` module, so it
// names none of the headless host's doubles and controls: no facade feature
// turns it on.
use flui_native::testing::{
    DisplayChange, DisplayChangeError, DisplaySpec, FakeAccessibility, FakeTextInput,
    HeadlessDeferredWindowOpens, HeadlessDisplayControls, HeadlessDragAndDrop,
    HeadlessExitReevaluation, HeadlessHostEvents, HeadlessHostWindow, HeadlessInputDevices,
    HeadlessOpenWindows, HeadlessOwnerTurns, HeadlessPlatform, HeadlessPreferences,
    HeadlessWindowPolicy, HeadlessWindowSystem, UnknownDisplay,
};

fn main() {}
