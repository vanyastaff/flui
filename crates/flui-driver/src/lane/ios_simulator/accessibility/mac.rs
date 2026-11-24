//! The Mac's half of [`HostAccessibility`]: the device's window found among the windows on
//! screen by its title, its screen group by the search of `tree::find_screen`, and the group's
//! elements read through the shared accessibility client (`lane::ax`) as they are served.
//!
//! The Simulator's process is not looked up by name: Simulator.app hosts the device's window
//! before Xcode 27 and DeviceHub from it, and AppKit reports DeviceHub's process id as -1. The
//! window server lists the owner of every window on screen with its real process id, so each owner
//! is asked, within [`PROBE_TIMEOUT_SECONDS`], for its windows of the device's title. Every
//! element the read then asks is given [`READ_TIMEOUT_SECONDS`] of its own, so the deadline a
//! timeout names is the one its call ran under. A failure while the process is no longer trusted
//! is the permission taken back, wherever it comes (`accessibility::read_failure`), and a parent
//! that went away mid-read leaves its children unread (`accessibility::child_read`).
#![expect(
    unsafe_code,
    reason = "the window list's C call and its constant keys, read as CGWindow.h types them"
)]

use std::time::Duration;

use objc2_application_services::AXUIElement;
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowLayer,
    kCGWindowOwnerPID,
};

use super::{
    ChildRead, HostAccessibility, HostScreen, PointFrame, ScreenElement, ScreenRequest, child_read,
    read_failure, untrusted,
};
use crate::error::{Error, Result};
use crate::lane::ax;
use crate::lane::ios_simulator::tree::{self, WindowFacts, WindowTree};
use crate::lane::walk::{MAX_DEPTH, MAX_NODES, ProviderError};

/// How long an owner of a window on screen may take to list its windows, in seconds: the probe
/// asks every one, and a hung app must not hold the read for the client's whole timeout.
pub const PROBE_TIMEOUT_SECONDS: f32 = 1.0;

/// The most window owners the probe asks.
const MAX_OWNERS: usize = 128;

/// The window level of an ordinary app window (`kCGNormalWindowLevel`).
const NORMAL_LEVEL: i64 = 0;

/// The Mac's accessibility API on the Simulator's window: see the [module docs](self).
#[derive(Debug, Default)]
pub struct MacAccessibility {
    _private: (),
}

impl MacAccessibility {
    /// The Mac's accessibility API, asked for nothing until a read.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// How long each call of a read may take, in seconds: set on every element the read asks
/// ([`asked`]), since whether an element takes its application's timeout is not documented, so
/// the deadline a timeout names is the one each call ran under.
pub const READ_TIMEOUT_SECONDS: f32 = ax::MESSAGING_TIMEOUT_SECONDS;

/// `element`, its replies awaited [`READ_TIMEOUT_SECONDS`].
fn asked(element: CFRetained<AXUIElement>) -> CFRetained<AXUIElement> {
    ax::wait_for(&element, READ_TIMEOUT_SECONDS);
    element
}

/// What a failed call answers: [`read_failure`] with the process's trust asked now and the
/// deadline each call of a read runs under.
fn failure(error: ProviderError) -> Error {
    read_failure(
        error,
        ax::trusted(),
        Duration::from_secs_f32(READ_TIMEOUT_SECONDS),
    )
}

/// An element read with its handle, which lives for the call that read it.
struct Read {
    element: ScreenElement,
    handle: CFRetained<AXUIElement>,
    children: Vec<Read>,
}

impl HostAccessibility for MacAccessibility {
    fn read(&mut self, screen: &ScreenRequest) -> Result<HostScreen> {
        let (screen, _) = read_screen(screen)?;
        Ok(screen)
    }

    fn press(&mut self, screen: &ScreenRequest, wanted: &ScreenElement) -> Result<()> {
        let (read, handles) = read_screen(screen)?;
        let path = tree::locate(&read, wanted)?;
        let mut level = &handles;
        let mut found = None;
        for index in &path {
            let Some(next) = level.get(*index) else {
                break;
            };
            found = Some(next);
            level = &next.children;
        }
        let found = found.ok_or_else(|| {
            Error::not_found("the element the read located is no longer in its place")
        })?;
        let label = wanted
            .description
            .clone()
            .or_else(|| wanted.title.clone())
            .unwrap_or_else(|| wanted.role.clone());
        let deadline = Duration::from_secs_f32(READ_TIMEOUT_SECONDS);
        let offered = ax::actions(&found.handle)
            .map(|names| names.iter().any(|action| action == "AXPress").then_some(()));
        if offered.is_err() && !ax::trusted() {
            return Err(untrusted());
        }
        crate::lane::walk::pattern(offered, &label, "AXPress", deadline)?;
        let pressed = ax::perform(&found.handle, "AXPress");
        if pressed.is_err() && !ax::trusted() {
            return Err(untrusted());
        }
        crate::lane::walk::invoked(pressed, &label, deadline)
    }
}

/// The screen `screen` names, read, and the handles of its elements in the same shape.
fn read_screen(screen: &ScreenRequest) -> Result<(HostScreen, Vec<Read>)> {
    if !ax::trusted() {
        return Err(untrusted());
    }
    let windows = titled_windows(&screen.title)?;
    let (group, frame) = tree::find_screen(&mut Windows, screen, windows)?;
    let mut budget = Budget {
        visited: 0,
        truncated: false,
    };
    let (handles, cut) = budget.children_of(&group, 0)?;
    let mut read = HostScreen::new(
        frame,
        handles.iter().map(Read::detached).collect::<Vec<_>>(),
    );
    read.truncated = budget.truncated || cut;
    Ok((read, handles))
}

impl Read {
    /// The element with its children, without the handles.
    fn detached(&self) -> ScreenElement {
        let mut element = self.element.clone();
        element.children = self.children.iter().map(Self::detached).collect();
        element
    }
}

/// A walk's count of elements and whether a budget cut it short.
struct Budget {
    visited: usize,
    truncated: bool,
}

impl Budget {
    /// The children of `parent`, `depth` levels under the screen, each read with its own, within
    /// [`MAX_NODES`] elements and [`MAX_DEPTH`] levels, and whether `parent` went away before its
    /// children were all read ([`child_read`]).
    fn children_of(&mut self, parent: &AXUIElement, depth: usize) -> Result<(Vec<Read>, bool)> {
        let mut children = Vec::new();
        let mut index = 0;
        loop {
            let child = match child_read(ax::child_at(parent, index)).map_err(failure)? {
                ChildRead::Child(child) => asked(child),
                ChildRead::End => break,
                ChildRead::Cut => {
                    self.truncated = true;
                    return Ok((children, true));
                }
            };
            index += 1;
            if self.visited >= MAX_NODES {
                self.truncated = true;
                break;
            }
            self.visited += 1;
            let mut element = match element(&child) {
                Ok(element) => element,
                // An element that went away while it was read is left out.
                Err(ProviderError::Gone) => continue,
                Err(error) => return Err(failure(error)),
            };
            let grandchildren = if depth + 1 >= MAX_DEPTH {
                let more = !matches!(
                    child_read(ax::child_at(&child, 0)).map_err(failure)?,
                    ChildRead::End
                );
                element.children_unread = more;
                self.truncated |= more;
                Vec::new()
            } else {
                let (grandchildren, cut) = self.children_of(&child, depth + 1)?;
                element.children_unread = cut;
                grandchildren
            };
            children.push(Read {
                element,
                handle: child,
                children: grandchildren,
            });
        }
        Ok((children, false))
    }
}

/// `element`'s attributes as they are served.
fn element(element: &AXUIElement) -> std::result::Result<ScreenElement, ProviderError> {
    let text = |name: &'static str| -> std::result::Result<Option<String>, ProviderError> {
        Ok(ax::attribute(element, name)?
            .as_deref()
            .and_then(ax::string))
    };
    let flag = |name: &'static str| -> std::result::Result<Option<bool>, ProviderError> {
        Ok(ax::attribute(element, name)?
            .as_deref()
            .and_then(ax::boolean))
    };
    let mut read = ScreenElement::new(text("AXRole")?.unwrap_or_default());
    read.subrole = text("AXSubrole")?;
    read.description = text("AXDescription")?;
    read.title = text("AXTitle")?;
    read.value = ax::attribute(element, "AXValue")?
        .as_deref()
        .and_then(|value| ax::string(value).or_else(|| ax::number_text(value)));
    read.help = text("AXHelp")?;
    read.identifier = text("AXIdentifier")?;
    read.enabled = flag("AXEnabled")?;
    read.focused = flag("AXFocused")?.unwrap_or(false);
    read.selected = flag("AXSelected")?;
    read.frame =
        ax::frame(element)?.map(|(x, y, width, height)| PointFrame::new(x, y, width, height));
    Ok(read)
}

/// The windows on screen as `tree::find_screen` walks them.
struct Windows;

impl WindowTree for Windows {
    type Element = CFRetained<AXUIElement>;

    fn children(&mut self, element: &Self::Element) -> Result<Vec<Self::Element>> {
        let mut children = Vec::new();
        let mut index = 0;
        while children.len() < MAX_NODES {
            match child_read(ax::child_at(element, index)).map_err(failure)? {
                ChildRead::Child(child) => children.push(asked(child)),
                // A parent gone takes its later children with it: the search chooses among
                // the groups it saw, and none of those could be a screen still drawn.
                ChildRead::End | ChildRead::Cut => break,
            }
            index += 1;
        }
        Ok(children)
    }

    fn facts(&mut self, element: &Self::Element) -> Result<WindowFacts> {
        let text = |name: &'static str| -> Result<Option<String>> {
            Ok(ax::attribute(element, name)
                .map_err(failure)?
                .as_deref()
                .and_then(ax::string))
        };
        Ok(WindowFacts {
            role: text("AXRole")?.unwrap_or_default(),
            subrole: text("AXSubrole")?,
            frame: ax::frame(element)
                .map_err(failure)?
                .map(|(x, y, width, height)| PointFrame::new(x, y, width, height)),
        })
    }

    fn identity(&mut self, window: &Self::Element) -> Result<Vec<Option<String>>> {
        let mut texts = Vec::new();
        for name in ["AXIdentifier", "AXDocument"] {
            texts.push(
                ax::attribute(window, name)
                    .map_err(failure)?
                    .as_deref()
                    .and_then(ax::string),
            );
        }
        Ok(texts)
    }
}

/// The processes that own an ordinary window on screen, front to back, this one left out.
fn window_owners() -> Result<Vec<i32>> {
    let option =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let list =
        CGWindowListCopyWindowInfo(option, kCGNullWindowID).ok_or_else(|| Error::Platform {
            effect: None,
            message: "the window server listed no windows".to_owned(),
            retry: flui_protocol::error::Retry::Never,
        })?;
    // SAFETY: `CGWindowListCopyWindowInfo` returns an array of dictionaries keyed by strings
    // (CGWindow.h).
    let list =
        unsafe { CFRetained::cast_unchecked::<CFArray<CFDictionary<CFString, CFType>>>(list) };
    // SAFETY: the keys are constant strings Quartz exports.
    let (pid_key, layer_key) = unsafe { (kCGWindowOwnerPID, kCGWindowLayer) };
    let number = |dictionary: &CFDictionary<CFString, CFType>, key: &CFString| {
        dictionary
            .get(key)
            .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
    };
    let own = i64::from(std::process::id());
    let mut owners: Vec<i32> = Vec::new();
    for window in &*list {
        let window: &CFDictionary<CFString, CFType> = &window;
        let (Some(pid), Some(layer)) = (number(window, pid_key), number(window, layer_key)) else {
            continue;
        };
        let Ok(pid) = i32::try_from(pid) else {
            continue;
        };
        if layer == NORMAL_LEVEL && i64::from(pid) != own && !owners.contains(&pid) {
            owners.push(pid);
            if owners.len() >= MAX_OWNERS {
                break;
            }
        }
    }
    Ok(owners)
}

/// The windows on screen titled `title`. An owner that does not answer, or fails, shows no
/// device and the probe asks the next, unless the failure is the permission taken back.
fn titled_windows(title: &str) -> Result<Vec<CFRetained<AXUIElement>>> {
    let mut found = Vec::new();
    for pid in window_owners()? {
        let app = ax::application(pid, PROBE_TIMEOUT_SECONDS);
        let windows = match ax::elements(&app, "AXWindows") {
            Ok(windows) => windows,
            Err(_) if !ax::trusted() => return Err(untrusted()),
            Err(_) => continue,
        };
        for window in windows {
            let named = match ax::attribute(&window, "AXTitle") {
                Ok(named) => named.as_deref().and_then(ax::string),
                Err(_) if !ax::trusted() => return Err(untrusted()),
                Err(_) => continue,
            };
            if named.as_deref() == Some(title) {
                // Asked within the probe's second until now; read within the read's deadline.
                found.push(asked(window));
            }
        }
    }
    Ok(found)
}
