//! The Mac's half of [`HostAccessibility`]: the device's window found among the windows on
//! screen by its title, its screen group by the window's shape, and the group's elements walked
//! through the shared accessibility client (`lane::ax`).
//!
//! The Simulator's process is not looked up by name: Simulator.app hosts the device's window
//! before Xcode 27 and DeviceHub from it, and AppKit reports DeviceHub's process id as -1. The
//! window server lists the owner of every window on screen with its real process id, so each owner
//! is asked, within [`PROBE_TIMEOUT_SECONDS`], for a window of the device's title; two such
//! windows are refused unless one alone shows a screen.
#![expect(
    unsafe_code,
    reason = "the window list's C call and its constant keys, read as CGWindow.h types them"
)]

use objc2_application_services::AXUIElement;
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowLayer,
    kCGWindowOwnerPID,
};

use flui_protocol::tree::{ElementId, Node, Tree};
use flui_protocol::wire::Checked;

use super::{HostAccessibility, PointFrame, ScreenRequest, no_window, same_element};
use crate::error::{Error, Result};
use crate::lane::ax::{self, Visit, Walker, read_error, timeout};
use crate::lane::ios_simulator::roles;
use crate::lane::ios_simulator::tree::{self, MAX_SCREEN_DEPTH, ScreenCandidate};
use crate::lane::walk::{self, MAX_NODES, Properties};

/// How long an owner of a window on screen may take to list its windows, in seconds: the probe
/// asks every one, and a hung app must not hold the read for the client's whole timeout.
pub const PROBE_TIMEOUT_SECONDS: f32 = 1.0;

/// The most window owners the probe asks.
const MAX_OWNERS: usize = 128;

/// The window level of an ordinary app window (`kCGNormalWindowLevel`).
const NORMAL_LEVEL: i64 = 0;

/// The capability a process that is no trusted accessibility client is refused for, the one the
/// macOS lane names for the same permission.
const ACCESSIBILITY_PERMISSION: &str = "macos_permission_accessibility";

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

impl HostAccessibility for MacAccessibility {
    fn read(&mut self, screen: &ScreenRequest) -> Result<Tree> {
        trusted()?;
        let (group, frame) = find_screen(screen)?;
        let mut walker = Walker::new(|element: &AXUIElement, id, reads: &mut Properties| {
            describe(element, id, reads, frame, screen)
        });
        walk::read(&mut walker, &Visit::root(group), timeout())
    }

    fn press(&mut self, screen: &ScreenRequest, element: &Node) -> Result<()> {
        trusted()?;
        let (group, frame) = find_screen(screen)?;
        let mut walker = Walker::new(|found: &AXUIElement, id, reads: &mut Properties| {
            describe(found, id, reads, frame, screen)
        });
        let label = element
            .name
            .clone()
            .unwrap_or_else(|| element.native_role.clone());
        let found = walk::find_by(
            &mut walker,
            Visit::root(group),
            |node| same_element(node, element),
            &format!("that reads as {} {label:?}", element.native_role),
            timeout(),
        )?;
        let offered = ax::actions(&found.element)
            .map(|names| names.iter().any(|action| action == "AXPress").then_some(()));
        walk::pattern(offered, &label, "AXPress", timeout())?;
        walk::invoked(ax::perform(&found.element, "AXPress"), &label, timeout())
    }
}

/// Refuse a process that is no trusted accessibility client.
fn trusted() -> Result<()> {
    if ax::trusted() {
        return Ok(());
    }
    Err(Error::not_supported(
        ACCESSIBILITY_PERMISSION,
        "the process running the driver has no Accessibility permission, which reading the \
         Simulator's window takes; grant it under System Settings > Privacy & Security > \
         Accessibility, then start the process again",
    ))
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

/// The windows on screen titled `title`.
fn titled_windows(title: &str) -> Result<Vec<CFRetained<AXUIElement>>> {
    let mut found = Vec::new();
    for pid in window_owners()? {
        let app = ax::application(pid, PROBE_TIMEOUT_SECONDS);
        // An owner that does not answer, or answers with a failure, shows no device: the probe
        // asks the next.
        let Ok(windows) = ax::elements(&app, "AXWindows") else {
            continue;
        };
        for window in windows {
            let named = ax::attribute(&window, "AXTitle")
                .ok()
                .flatten()
                .as_deref()
                .and_then(ax::string);
            if named.as_deref() == Some(title) {
                found.push(window);
            }
        }
    }
    Ok(found)
}

/// The groups under `window`, breadth first within [`MAX_SCREEN_DEPTH`] levels and
/// [`MAX_NODES`] elements, that may be its screen ([`tree::is_screen_candidate`]), with their
/// elements. A candidate's own elements are the app's, so the search does not go into one.
fn screen_candidates(
    window: &CFRetained<AXUIElement>,
    into: &mut Vec<(CFRetained<AXUIElement>, ScreenCandidate)>,
) -> Result<()> {
    let mut level: Vec<CFRetained<AXUIElement>> = vec![window.clone()];
    let mut visited = 0_usize;
    for depth in 1..=MAX_SCREEN_DEPTH {
        let mut next = Vec::new();
        for parent in &level {
            let mut index = 0;
            while let Some(child) = ax::child_at(parent, index).map_err(read_error)? {
                index += 1;
                visited += 1;
                if visited > MAX_NODES {
                    return Ok(());
                }
                let role = ax::attribute(&child, "AXRole")
                    .map_err(read_error)?
                    .as_deref()
                    .and_then(ax::string)
                    .unwrap_or_default();
                let subrole = ax::attribute(&child, "AXSubrole")
                    .map_err(read_error)?
                    .as_deref()
                    .and_then(ax::string);
                if tree::is_screen_candidate(&role, subrole.as_deref(), depth)
                    && let Some((x, y, width, height)) = ax::frame(&child).map_err(read_error)?
                {
                    into.push((
                        child,
                        ScreenCandidate {
                            frame: PointFrame::new(x, y, width, height),
                            subrole,
                            depth,
                        },
                    ));
                    continue;
                }
                next.push(child);
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    Ok(())
}

/// The device's screen group in the window `screen` names, and its frame.
fn find_screen(screen: &ScreenRequest) -> Result<(CFRetained<AXUIElement>, PointFrame)> {
    let windows = titled_windows(&screen.title)?;
    if windows.is_empty() {
        return Err(no_window(&screen.title));
    }
    let mut groups = Vec::new();
    for window in &windows {
        screen_candidates(window, &mut groups)?;
    }
    let candidates: Vec<ScreenCandidate> = groups
        .iter()
        .map(|(_, candidate)| candidate.clone())
        .collect();
    let index = tree::choose_screen(&screen.title, &candidates, screen.size)
        .map_err(tree::TreeError::into_error)?;
    let (group, candidate) = groups.swap_remove(index);
    Ok((group, candidate.frame))
}

/// One element of the screen as a node: see the parent module's docs.
fn describe(
    element: &AXUIElement,
    id: ElementId,
    reads: &mut Properties,
    screen_frame: PointFrame,
    screen: &ScreenRequest,
) -> Node {
    let mut read = |name: &'static str| reads.read(|| ax::attribute(element, name)).flatten();
    let ax_role = read("AXRole")
        .as_deref()
        .and_then(ax::string)
        .unwrap_or_default();
    let subrole = read("AXSubrole").as_deref().and_then(ax::string);
    let mut node = Node::new(
        id,
        roles::wire_role(&ax_role, subrole.as_deref()),
        roles::native_role(&ax_role, subrole.as_deref()),
    );
    node.name = read("AXDescription")
        .as_deref()
        .and_then(ax::string)
        .or_else(|| read("AXTitle").as_deref().and_then(ax::string));
    let value = read("AXValue");
    if ax_role == "AXCheckBox" {
        // A toggle's value is its state: `accesskit_ios` hands UIKit "1" or "0".
        node.checked = value
            .as_deref()
            .and_then(|value| {
                ax::boolean(value).or_else(|| ax::string(value).map(|text| text == "1"))
            })
            .map(|on| if on { Checked::True } else { Checked::False });
    } else {
        node.value = value
            .as_deref()
            .and_then(|value| ax::string(value).or_else(|| ax::number_text(value)));
    }
    node.description = read("AXHelp").as_deref().and_then(ax::string);
    node.automation_id = read("AXIdentifier").as_deref().and_then(ax::string);
    node.disabled = read("AXEnabled")
        .as_deref()
        .and_then(ax::boolean)
        .is_some_and(|enabled| !enabled);
    node.focused = read("AXFocused")
        .as_deref()
        .and_then(ax::boolean)
        .unwrap_or(false);
    node.selected = read("AXSelected")
        .as_deref()
        .and_then(ax::boolean)
        .filter(|selected| *selected);
    if let Some((x, y, width, height)) = reads.read(|| ax::frame(element)).flatten() {
        node.rect = tree::window_rect(
            PointFrame::new(x, y, width, height),
            screen_frame,
            screen.size,
            screen.scale,
        );
    }
    node
}
