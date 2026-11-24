//! The Simulator's window scripted for the iOS Simulator lane's accessibility read:
//! [`ScriptedSimulatorScreen`].

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use flui_protocol::tree::{Node, Tree};

use crate::error::{Error, Result};
use crate::lane::ios_simulator::accessibility::{
    HostAccessibility, ScreenRequest, no_window, same_element,
};

#[derive(Debug)]
struct Script {
    title: String,
    screen: Tree,
    requests: Vec<ScreenRequest>,
    presses: Vec<Node>,
}

/// The Simulator's window as the Mac's accessibility API would show it, scripted: one window,
/// titled for its device, whose screen is a tree a test builds (its root the screen's group, the
/// iOS app's elements under it, as `HostAccessibility::read` hands them over). A read of another
/// title is refused as no window on screen is; a press finds the element as the Mac's does, by
/// the facts a read gives, and is recorded. Clones share one script, so a test keeps one while
/// the lane owns another.
#[derive(Debug, Clone)]
pub struct ScriptedSimulatorScreen {
    script: Arc<Mutex<Script>>,
}

fn lock(script: &Mutex<Script>) -> MutexGuard<'_, Script> {
    script.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScriptedSimulatorScreen {
    /// A window titled `title` showing `screen`.
    #[must_use]
    pub fn new(title: impl Into<String>, screen: Tree) -> Self {
        Self {
            script: Arc::new(Mutex::new(Script {
                title: title.into(),
                screen,
                requests: Vec::new(),
                presses: Vec::new(),
            })),
        }
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

    /// Every element pressed, as the screen shows it, in order.
    #[must_use]
    pub fn presses(&self) -> Vec<Node> {
        lock(&self.script).presses.clone()
    }
}

/// The node of `nodes` that reads as `wanted`, in document order.
fn found<'a>(nodes: &'a [Node], wanted: &Node) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        if same_element(node, wanted) {
            Some(node)
        } else {
            found(&node.children, wanted)
        }
    })
}

impl HostAccessibility for ScriptedSimulatorScreen {
    fn read(&mut self, screen: &ScreenRequest) -> Result<Tree> {
        let mut script = lock(&self.script);
        script.requests.push(screen.clone());
        if screen.title != script.title {
            return Err(no_window(&screen.title));
        }
        Ok(script.screen.clone())
    }

    fn press(&mut self, screen: &ScreenRequest, element: &Node) -> Result<()> {
        let mut script = lock(&self.script);
        script.requests.push(screen.clone());
        if screen.title != script.title {
            return Err(no_window(&screen.title));
        }
        // The screen's own group is no element of the app's.
        let pressed = script
            .screen
            .roots
            .iter()
            .find_map(|root| found(&root.children, element))
            .cloned()
            .ok_or_else(|| {
                Error::not_found(format!(
                    "the OS accessibility tree shows no element that reads as {} {:?}",
                    element.native_role,
                    element.name.as_deref().unwrap_or_default()
                ))
            })?;
        script.presses.push(pressed);
        Ok(())
    }
}
