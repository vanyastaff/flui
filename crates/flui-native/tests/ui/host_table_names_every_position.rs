//! A host's table names a type for every position of the closed list, and its
//! loop answers every launch read, `page_origin` included: one left out is a
//! compile error, never an inert default.

use std::rc::Rc;

use flui_native::host::{Absent, Host, Launch, Loop, position};
use flui_native::{AppHandler, Platform, PlatformError};
use flui_platform::LaunchKey;

struct Idle;

impl Loop for Idle {
    fn run(
        self: Rc<Self>,
        _: Rc<dyn Platform>,
        _: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    fn quit(&self) {}

    fn request_exit_policy_reevaluation(&self) {}
}

impl Launch for Idle {
    fn launch_value(&self, _: LaunchKey) -> Option<String> {
        None
    }
}

struct WithoutStorage;

impl Host for WithoutStorage {
    const NAME: &'static str = "without storage";

    type Loop = Idle;
    type Window = Absent<position::Window>;
    type Displays = Absent<position::Displays>;
    type Input = Absent<position::Input>;
    type Text = Absent<position::Text>;
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = Absent<position::Clipboard>;
    type PrimarySelection = Absent<position::PrimarySelection>;
    type Drag = Absent<position::Drag>;
    type Haptics = Absent<position::Haptics>;
    type Preferences = Absent<position::Preferences>;
    type Lifecycle = Absent<position::Lifecycle>;
    type Chrome = Absent<position::Chrome>;
}

fn main() {}
