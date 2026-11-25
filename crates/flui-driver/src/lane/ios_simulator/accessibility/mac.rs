//! The Mac's half of [`HostAccessibility`]: the device's window found among the windows on
//! screen by its title, its screen group by the search of `tree::find_screen`, and the group's
//! elements read through the shared accessibility client (`lane::ax`) by the pure walk
//! (`accessibility::walk_screen`).
//!
//! The Simulator's process is not looked up by name: Simulator.app hosts the device's window
//! before Xcode 27 and DeviceHub from it, and AppKit reports DeviceHub's process id as -1. The
//! window server lists the owner of every window on screen with its real process id, so each owner
//! is asked, within [`PROBE_TIMEOUT_SECONDS`], for its windows of the device's title. Every
//! element the read then asks is given the client's deadline (`ax::MESSAGING_TIMEOUT_SECONDS`) of
//! its own, since whether an element takes its application's timeout is not documented, and
//! carries what it was given ([`TimedElement`]): a timeout names the deadline of the element its
//! call was asked of, or says that element refused it. A failure while the process is no longer
//! trusted is the permission taken back, wherever it comes (`accessibility::read_failure`), and a
//! parent that went away mid-read leaves its children unread (`ax::child_at` answers
//! `ProviderError::Gone`). A window's identity texts are read whole, up to
//! `accessibility::IDENTITY_TEXT_LIMIT`, never through the outline's cut. The screen's frame is
//! read again once the walk is done, and a screen that moved meanwhile answers `Error::Busy`
//! (`accessibility::screen_steady`).
#![expect(
    unsafe_code,
    reason = "the window list's C call and its constant keys, read as CGWindow.h types them"
)]

use std::time::Duration;

use objc2_application_services::AXUIElement;
use objc2_core_foundation::{
    CFArray, CFDictionary, CFIndex, CFNumber, CFRetained, CFString, CFType,
};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowLayer,
    kCGWindowOwnerPID,
};

use super::{
    HostAccessibility, HostScreen, IDENTITY_TEXT_LIMIT, PointFrame, ScreenElement, ScreenRequest,
    ScreenSource, ScreenWalk, identity_too_long, pressed, read_failure, screen_steady, untrusted,
    walk_screen,
};
use crate::error::{Error, Result};
use crate::lane::ax;
use crate::lane::ios_simulator::tree::{self, WindowFacts, WindowTree};
use crate::lane::walk::ProviderError;

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

/// An element the read holds for its own length, with the deadline its calls run under: the
/// client's, or `None` when it refused that one and kept its own.
#[derive(Debug, Clone)]
struct TimedElement {
    element: CFRetained<AXUIElement>,
    deadline: Option<Duration>,
}

impl TimedElement {
    /// `element`, given the client's deadline of its own.
    fn new(element: CFRetained<AXUIElement>) -> Self {
        let deadline =
            ax::set_messaging_timeout(&element, ax::MESSAGING_TIMEOUT_SECONDS).then(ax::timeout);
        Self { element, deadline }
    }
}

/// One read's calls of the API, as the screen search and the walk make them: it keeps the
/// deadline of the element its latest call was asked of, which a failure of that call names.
/// Calls run one at a time and a failure is answered before the next, so it is always the failed
/// call's.
#[derive(Debug, Default)]
struct ScreenRead {
    asking: Option<Duration>,
}

impl ScreenRead {
    /// `element`, about to be asked a call: its deadline is the one a failure names.
    fn ask<'a>(&mut self, element: &'a TimedElement) -> &'a AXUIElement {
        self.asking = element.deadline;
        &element.element
    }

    /// What a failed call answers: [`read_failure`] with the process's trust asked now and the
    /// deadline of the element the call was asked of.
    fn failed(&self, error: ProviderError) -> Error {
        read_failure(error, ax::trusted(), self.asking)
    }

    /// The child of `parent` at `index`, given the read's deadline.
    fn child(
        &mut self,
        parent: &TimedElement,
        index: usize,
    ) -> std::result::Result<Option<TimedElement>, ProviderError> {
        let Ok(index) = CFIndex::try_from(index) else {
            return Ok(None);
        };
        Ok(ax::child_at(self.ask(parent), index)?.map(TimedElement::new))
    }

    /// `element`'s frame in the Mac's points, when it has one.
    fn frame(
        &mut self,
        element: &TimedElement,
    ) -> std::result::Result<Option<PointFrame>, ProviderError> {
        Ok(ax::frame(self.ask(element))?
            .map(|(x, y, width, height)| PointFrame::new(x, y, width, height)))
    }
}

impl HostAccessibility for MacAccessibility {
    fn read(&mut self, screen: &ScreenRequest) -> Result<HostScreen> {
        let (screen, _) = read_screen(screen)?;
        Ok(screen)
    }

    fn press(&mut self, screen: &ScreenRequest, wanted: &ScreenElement) -> Result<()> {
        let (read, walk) = read_screen(screen)?;
        let path = tree::locate(&read, wanted)?;
        let mut level = &walk.elements;
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
        let handle = &found.handle;
        pressed(
            ax::actions(&handle.element)
                .map(|names| names.iter().any(|action| action == "AXPress")),
            || ax::perform(&handle.element, "AXPress"),
            &label,
            handle.deadline,
            ax::trusted,
        )
    }
}

/// The screen `screen` names, read, and the walk with its elements' handles in the same shape.
fn read_screen(screen: &ScreenRequest) -> Result<(HostScreen, ScreenWalk<TimedElement>)> {
    if !ax::trusted() {
        return Err(untrusted());
    }
    let mut read = ScreenRead::default();
    let windows = titled_windows(&screen.title)?
        .into_iter()
        .map(TimedElement::new)
        .collect::<Vec<TimedElement>>();
    let (group, frame) = tree::find_screen(&mut read, screen, windows)?;
    let walk = walk_screen(&mut read, &group).map_err(|error| read.failed(error))?;
    let after = read.frame(&group).map_err(|error| read.failed(error))?;
    screen_steady(frame, after)?;
    Ok((walk.screen(frame), walk))
}

impl ScreenSource for ScreenRead {
    type Element = TimedElement;

    fn child_at(
        &mut self,
        parent: &TimedElement,
        index: usize,
    ) -> std::result::Result<Option<TimedElement>, ProviderError> {
        self.child(parent, index)
    }

    fn element(
        &mut self,
        element: &TimedElement,
    ) -> std::result::Result<ScreenElement, ProviderError> {
        let element = self.ask(element);
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
}

impl WindowTree for ScreenRead {
    type Element = TimedElement;

    fn child_at(
        &mut self,
        element: &TimedElement,
        index: usize,
    ) -> std::result::Result<Option<TimedElement>, ProviderError> {
        self.child(element, index)
    }

    fn failure(&mut self, error: ProviderError) -> Error {
        self.failed(error)
    }

    fn facts(&mut self, element: &TimedElement) -> std::result::Result<WindowFacts, ProviderError> {
        let frame = self.frame(element)?;
        let element = self.ask(element);
        let text = |name: &'static str| -> std::result::Result<Option<String>, ProviderError> {
            Ok(ax::attribute(element, name)?
                .as_deref()
                .and_then(ax::string))
        };
        Ok(WindowFacts {
            role: text("AXRole")?.unwrap_or_default(),
            subrole: text("AXSubrole")?,
            frame,
        })
    }

    fn identity(&mut self, window: &TimedElement) -> Result<Vec<Option<String>>> {
        let window = self.ask(window);
        let mut texts = Vec::new();
        for name in ["AXIdentifier", "AXDocument"] {
            let value = ax::attribute(window, name).map_err(|error| self.failed(error))?;
            let text = match value
                .as_deref()
                .map(|value| ax::whole_string(value, IDENTITY_TEXT_LIMIT))
            {
                None => None,
                Some(Ok(text)) => text,
                Some(Err(units)) => return Err(identity_too_long(name, units)),
            };
            texts.push(text);
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

/// The windows on screen titled `title`, asked within the probe's second. An owner that does not
/// answer, or fails, shows no device and the probe asks the next, unless the failure is the
/// permission taken back.
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
                found.push(window);
            }
        }
    }
    Ok(found)
}
