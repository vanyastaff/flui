//! The Simulator's window scripted for the iOS Simulator lane's accessibility read:
//! [`ScriptedSimulatorScreen`].

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::error::Result;
use crate::lane::ios_simulator::accessibility::{
    HostAccessibility, HostScreen, ScreenElement, ScreenRequest, WindowIdentity, window_identity,
};
use crate::lane::ios_simulator::tree;

#[derive(Debug)]
struct Script {
    title: String,
    identity: Vec<Option<String>>,
    screen: HostScreen,
    requests: Vec<ScreenRequest>,
    presses: Vec<ScreenElement>,
}

impl Script {
    /// Whether the window is the one `screen` asks for, as the Mac's read decides it
    /// (`ios_simulator::tree::choose_window`): no window of another title is on screen, and one
    /// of the title is taken by what its attributes name.
    fn choose(&self, screen: &ScreenRequest) -> Result<()> {
        let identities: Vec<WindowIdentity> = if screen.title == self.title {
            vec![window_identity(&screen.udid, &self.identity)]
        } else {
            Vec::new()
        };
        tree::choose_window(screen, &identities).map(|_| ())
    }
}

/// The Simulator's window as the Mac's accessibility API would serve it, scripted: one window,
/// titled for its device, whose screen a test writes out as the API serves it (the screen's
/// frame and its elements' attributes in the Mac's points). The window is taken as the Mac's
/// read takes one (`ios_simulator::tree::choose_window`): a read of another title finds no window
/// on screen, and one of the title is taken by what its attributes name ([`Self::identity`]), so
/// a window that names no UUID is refused while another listed simulator shares the title. A
/// press finds the element as the Mac's does (`ios_simulator::tree::locate`, refusing none and
/// two) and is recorded. Clones share one script, so a test keeps one while the lane owns
/// another.
#[derive(Debug, Clone)]
pub struct ScriptedSimulatorScreen {
    script: Arc<Mutex<Script>>,
}

fn lock(script: &Mutex<Script>) -> MutexGuard<'_, Script> {
    script.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScriptedSimulatorScreen {
    /// A window titled `title` showing `screen`, whose attributes name no UUID.
    #[must_use]
    pub fn new(title: impl Into<String>, screen: HostScreen) -> Self {
        Self {
            script: Arc::new(Mutex::new(Script {
                title: title.into(),
                identity: Vec::new(),
                screen,
                requests: Vec::new(),
                presses: Vec::new(),
            })),
        }
    }

    /// The window answering `attributes` as the texts that could name its simulator (its
    /// `AXIdentifier` and `AXDocument`), read as the Mac's are
    /// (`ios_simulator::accessibility::window_identity`).
    #[must_use]
    pub fn identity(self, attributes: Vec<Option<String>>) -> Self {
        lock(&self.script).identity = attributes;
        self
    }

    /// The window, for the lane ([`IosSimulatorLane::read_through`](crate::lane::ios_simulator::IosSimulatorLane::read_through)).
    #[must_use]
    pub fn accessibility(&self) -> Box<dyn HostAccessibility> {
        Box::new(self.clone())
    }

    /// Every screen a read or a press asked for, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<ScreenRequest> {
        lock(&self.script).requests.clone()
    }

    /// Every element pressed, as the screen serves it, in order.
    #[must_use]
    pub fn presses(&self) -> Vec<ScreenElement> {
        lock(&self.script).presses.clone()
    }
}

impl HostAccessibility for ScriptedSimulatorScreen {
    fn read(&mut self, screen: &ScreenRequest) -> Result<HostScreen> {
        let mut script = lock(&self.script);
        script.requests.push(screen.clone());
        script.choose(screen)?;
        Ok(script.screen.clone())
    }

    fn press(&mut self, screen: &ScreenRequest, wanted: &ScreenElement) -> Result<()> {
        let mut script = lock(&self.script);
        script.requests.push(screen.clone());
        script.choose(screen)?;
        let path = tree::locate(&script.screen, wanted)?;
        let pressed = tree::element_at(&script.screen.elements, &path)
            .expect("BUG: a located path names an element")
            .clone();
        script.presses.push(pressed);
        Ok(())
    }
}
