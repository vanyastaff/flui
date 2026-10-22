//! The web host: a canvas in the browser page, on the browser's own event loop.

use crate::error::PlatformError;
use crate::host::{Absent, Host, position};
use crate::owner::web::WebLoop;
use crate::platform::HostPlatform;
use crate::platforms::web::{
    PageObservers, WebClipboard, WebDisplays, WebHaptics, WebPreferences, WebTransfer, WebWindow,
};
use crate::storage::UserDataDirs;
use crate::text::web::WebTextInput;

/// The web host's table.
#[derive(Debug)]
pub struct Web;

impl Host for Web {
    const NAME: &'static str = "Web (WASM)";

    type Loop = WebLoop;
    type Window = WebWindow;
    type Displays = WebDisplays;
    type Input = WebLoop;
    type Text = std::rc::Rc<WebTextInput>;
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = WebClipboard;
    type Drag = WebTransfer;
    type Haptics = WebHaptics;
    type Preferences = WebPreferences;
    type Lifecycle = PageObservers;
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

/// The web platform: a canvas window on the browser's event loop, over
/// WebAssembly with `wasm-bindgen` and `web-sys`.
pub type WebPlatform = HostPlatform<Web>;

impl HostPlatform<Web> {
    /// Create a new Web platform instance
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the browser's preference queries
    /// cannot be observed.
    pub fn new() -> Result<Self, PlatformError> {
        Ok(Self::from_loop(WebLoop::new()?))
    }
}
