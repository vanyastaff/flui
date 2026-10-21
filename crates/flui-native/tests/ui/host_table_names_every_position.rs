//! A host's table names a type for every position of the closed list: one
//! left out is a compile error, never an inert default.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use flui_native::host::{Absent, Host, Loop, Services, position};
use flui_native::{Platform, PlatformError, PlatformExecutor, PlatformReadyCallback};

struct Idle;

impl Loop for Idle {
    fn run(
        self: Arc<Self>,
        _: Arc<dyn Platform>,
        _: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    fn quit(&self) {}

    fn on_quit(&self, _: Box<dyn FnMut() + Send>) {}

    fn set_exit_policy_hook(&self, _: Box<dyn Fn() -> bool + Send>) {}

    fn request_exit_policy_reevaluation(&self) {}

    fn set_wake_deadline_hook(&self, _: Box<dyn Fn() -> Option<Instant> + Send + Sync>) {}
}

impl Services for Idle {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unreachable!("the table never compiles")
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        Ok(PathBuf::new())
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
    type Drag = Absent<position::Drag>;
    type Haptics = Absent<position::Haptics>;
    type Preferences = Absent<position::Preferences>;
    type Lifecycle = Absent<position::Lifecycle>;
    type Chrome = Absent<position::Chrome>;
}

fn main() {}
