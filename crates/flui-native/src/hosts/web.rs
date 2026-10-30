//! The web host: a canvas in the browser page, on the browser's own event loop.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::Clipboard;

use crate::accessibility::web::WebAccessibility;
use crate::clipboard::web::WebClipboard;
use crate::display::web::WebDisplays;
use crate::drag::web::WebTransfer;
use crate::error::PlatformError;
use crate::haptics::web::WebHaptics;
use crate::host::{Absent, Host, position};
use crate::input::web::events::WebInput;
use crate::lifecycle::web::PageSession;
use crate::owner::signal::OwnerSignal;
use crate::owner::sources::SystemPreferenceSource;
use crate::owner::web::{HostDisplays, HostDrag, HostInput, PageLifecycle, WebHost, WebLoop};
use crate::platform::HostPlatform;
use crate::preferences::web::WebPreferences;
use crate::storage::UserDataDirs;
use crate::text::web::WebTextInput;
use crate::window::web::WebWindow;

/// The web host's table.
#[derive(Debug)]
pub struct Web;

impl Host for Web {
    const NAME: &'static str = "Web (WASM)";

    type Loop = WebLoop;
    type Window = WebWindow;
    type Displays = WebLoop;
    type Input = WebLoop;
    type Text = WebTextInput;
    type Accessibility = WebAccessibility;
    type Clipboard = WebLoop;
    type Drag = WebLoop;
    type Haptics = WebHaptics;
    type Preferences = WebLoop;
    type Lifecycle = WebLoop;
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

impl WebHost for Web {
    fn displays() -> Arc<dyn HostDisplays> {
        WebDisplays::new()
    }

    fn preferences(
        signal: &Arc<OwnerSignal>,
    ) -> Result<Rc<dyn SystemPreferenceSource>, PlatformError> {
        Ok(Rc::new(WebPreferences::new(signal)?))
    }

    fn page() -> Box<dyn PageLifecycle> {
        Box::new(PageSession)
    }

    /// The page's own clipboard, which reads only its own writes.
    fn clipboard() -> Arc<dyn Clipboard> {
        Arc::new(WebClipboard::new())
    }

    /// HTML drag and drop.
    fn drag() -> Arc<dyn HostDrag> {
        Arc::new(WebTransfer::default())
    }

    /// The DOM's pointer, keyboard, wheel and gesture events on the canvas.
    fn input() -> Rc<dyn HostInput> {
        Rc::new(WebInput)
    }
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
        Self::try_from_parts(|positions| positions, |_| WebLoop::new::<Web>())
    }
}
