//! A host's table names a type for every position of the closed list: one
//! left out is a compile error, never an inert default.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use flui_native::host::{Absent, Host, Loop, Services, position};
use flui_native::{
    PathPromptOptions, Platform, PlatformError, PlatformExecutor, PlatformReadyCallback, Task,
};

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

    fn open_url(&self, _: &str) {}

    fn reveal_path(&self, _: &Path) {}

    fn open_path(&self, _: &Path) {}

    fn prompt_for_paths(
        &self,
        _: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        Task::ready(Ok(None))
    }

    fn prompt_for_new_path(
        &self,
        _: &Path,
        _: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        Task::ready(Ok(None))
    }

    fn compositor_name(&self) -> &'static str {
        ""
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
