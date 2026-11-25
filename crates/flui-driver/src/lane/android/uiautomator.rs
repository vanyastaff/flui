//! What `uiautomator dump` shows of the screen, read as untrusted input: the accessibility nodes
//! the app serves, and the display's turn when it dumped, which the lane compares with the turn
//! `dumpsys window` gave it. The lane reads it only for the tree: where the app's window is and
//! how the display is turned come from `dumpsys window` (`super::device::WINDOWS`), which pauses
//! no accessibility service.
//!
//! The dump is the active window's view hierarchy as accessibility reads it:
//! `<hierarchy rotation="N">` and its `<node>`s, each with its `package`, its `class`, its `text`
//! and `content-desc`, its states (`checkable`, `checked`, `enabled`, `focusable`, `focused`,
//! `selected`) and `bounds="[left,top][right,bottom]"` in display pixels as the display is turned,
//! clipped to the display (AOSP's `AccessibilityNodeInfoDumper`). It is read whole only after its
//! size is checked ([`MAX_DUMP`]), with a bound on its nodes ([`MAX_NODES`]) and on its depth
//! ([`MAX_DEPTH`]).
//!
//! FLUI's nodes carry no `resource-id`: the adapter that serves them (`accesskit_android`) sets
//! none, so the lane finds them by matching the dump to the app's own tree (`super::tree`).

/// The most bytes of a dump the lane reads.
pub const MAX_DUMP: usize = 4 * 1024 * 1024;

/// The most nodes of a dump the lane reads.
pub const MAX_NODES: u32 = 100_000;

/// The deepest a node of a dump may sit.
pub const MAX_DEPTH: usize = 256;

/// Where a session has its dump numbered `dump` written on the device: a directory the shell may
/// write, a file named for the session (`session` is lowercase hex) and the dump, so two sessions
/// on one device never read each other's dump, and no dump reads a file an earlier one left.
#[must_use]
pub fn dump_path(session: &str, dump: u64) -> String {
    format!("/data/local/tmp/flui-driver-{session}-{dump}.xml")
}

/// Every file a session's dumps write, as words of the device's shell: the dumps and their error
/// streams, whatever their number.
#[must_use]
pub fn session_dumps(session: &str) -> String {
    let every = dump_path(session, 0).replace("-0.xml", "-*.xml");
    format!("{every} {every}.err")
}

/// The line that dumps the active window to the session's dump numbered `dump` and prints it, or,
/// where the dump wrote no file, what it said on its error stream ([`DumpFailure`]). Each dump has
/// a path of its own ([`dump_path`]), so a file an earlier dump left (one whose command the host
/// ended at its deadline, before its removal ran) is never printed as this dump's; and the line
/// ends by removing every file of the session's dumps ([`session_dumps`]), so what dumps the host
/// ended left goes with the next that runs to its end.
///
/// The dump connects to accessibility as `UiAutomation` does, which suspends the accessibility
/// services running (TalkBack among them) until it ends.
#[must_use]
pub fn dump_line(session: &str, dump: u64) -> String {
    let path = dump_path(session, dump);
    format!(
        "uiautomator dump {path} >/dev/null 2>{path}.err; cat {path} 2>/dev/null || cat \
         {path}.err; rm -f {}",
        session_dumps(session)
    )
}

/// Why a `uiautomator dump` wrote no file, where it says so: each a state of the device that
/// passes, so the read is busy rather than failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DumpFailure {
    /// The window never went idle within the dump's wait (AOSP `DumpCommand`'s
    /// `waitForIdle(1000, 10000)`, which prints `ERROR: could not get idle state.`): the app
    /// changes its tree all the time.
    NotIdle,
    /// Another `UiAutomation` client holds accessibility (`UiAutomationService … already
    /// registered!`), as a test runner or a second dump does.
    AnotherClient,
}

impl DumpFailure {
    /// The failure the dump's error text `printed` names, if any.
    pub(crate) fn of(printed: &str) -> Option<Self> {
        if printed.contains("could not get idle state") {
            Some(Self::NotIdle)
        } else if printed.contains("already registered") {
            Some(Self::AnotherClient)
        } else {
            None
        }
    }
}

impl std::fmt::Display for DumpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotIdle => {
                "`uiautomator dump` wrote no window: the window never went idle within its \
                 wait, as an app that changes its tree on every frame keeps it; read again once \
                 it settles"
            }
            Self::AnotherClient => {
                "`uiautomator dump` wrote no window: another `UiAutomation` client holds the \
                 device's accessibility; read again once it lets go"
            }
        })
    }
}

/// A rectangle in display pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge, past the last column.
    pub right: i32,
    /// Bottom edge, past the last row.
    pub bottom: i32,
}

impl Bounds {
    /// Its width; 0 for an inverted rectangle.
    #[must_use]
    pub fn width(&self) -> i32 {
        self.right.saturating_sub(self.left).max(0)
    }

    /// Its height; 0 for an inverted rectangle.
    #[must_use]
    pub fn height(&self) -> i32 {
        self.bottom.saturating_sub(self.top).max(0)
    }

    /// Bounds as a dump writes them: `[left,top][right,bottom]`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let numbers: Vec<i32> = text
            .split(['[', ']', ','])
            .filter(|part| !part.is_empty())
            .map(|part| part.trim().parse().ok())
            .collect::<Option<_>>()?;
        let [left, top, right, bottom] = <[i32; 4]>::try_from(numbers).ok()?;
        Some(Self {
            left,
            top,
            right,
            bottom,
        })
    }
}

/// One node of a dump. A dump writes an absent text as an empty one, so the two read alike.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct UiNode {
    /// The application that shows it.
    pub package: String,
    /// Its class name (`AccessibilityNodeInfo.getClassName`): `android.widget.Button`.
    pub class: String,
    /// Its text (`getText`).
    pub text: String,
    /// Its content description (`getContentDescription`), what a screen reader reads as its name.
    pub content_desc: String,
    /// Whether it has a checked state.
    pub checkable: bool,
    /// Whether it is checked.
    pub checked: bool,
    /// Whether it takes interaction.
    pub enabled: bool,
    /// Whether it can take the input focus.
    pub focusable: bool,
    /// Whether it holds the input focus.
    pub focused: bool,
    /// Whether it is selected.
    pub selected: bool,
    /// Where it is; `None` when the dump gave no bounds it could read.
    pub bounds: Option<Bounds>,
    /// The nodes inside it.
    pub children: Vec<UiNode>,
}

/// A dump: how the display is turned, and the active window's nodes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct UiDump {
    /// The display's rotation from its natural orientation when it dumped, in quarter turns
    /// counter-clockwise (`Surface.ROTATION_0` to `ROTATION_270`): the lane reads the tree only
    /// when it is the rotation `dumpsys window` gave it before and after the dump.
    pub rotation: u8,
    /// The window's top nodes.
    pub roots: Vec<UiNode>,
}

impl UiDump {
    /// The dump `xml` is.
    ///
    /// # Errors
    ///
    /// Why it is no dump: too large, not XML, past the node or depth bound, no `hierarchy`.
    pub fn parse(xml: &str) -> Result<Self, String> {
        if xml.len() > MAX_DUMP {
            return Err(format!(
                "the dump holds {} bytes, past the {MAX_DUMP} the lane reads",
                xml.len()
            ));
        }
        let options = roxmltree::ParsingOptions {
            // No DTD and no resolver: a dump that declares or references an entity is refused.
            allow_dtd: false,
            entity_resolver: None,
            nodes_limit: MAX_NODES,
        };
        let document = roxmltree::Document::parse_with_options(xml, options)
            .map_err(|error| format!("the dump is no XML the lane reads: {error}"))?;
        let hierarchy = document.root_element();
        if !hierarchy.has_tag_name("hierarchy") {
            return Err(format!(
                "the dump's root is <{}>, not <hierarchy>",
                hierarchy.tag_name().name()
            ));
        }
        let rotation = hierarchy
            .attribute("rotation")
            .and_then(|rotation| rotation.parse::<u8>().ok())
            .filter(|rotation| *rotation < 4)
            .ok_or_else(|| "the dump names no rotation from 0 to 3".to_owned())?;
        let roots = nodes_of(hierarchy, 0)?;
        Ok(Self { rotation, roots })
    }
}

/// The `<node>` children of `parent`, read to [`MAX_DEPTH`].
fn nodes_of(parent: roxmltree::Node<'_, '_>, depth: usize) -> Result<Vec<UiNode>, String> {
    if depth > MAX_DEPTH {
        return Err(format!("the dump nests nodes deeper than {MAX_DEPTH}"));
    }
    parent
        .children()
        .filter(|child| child.has_tag_name("node"))
        .map(|node| {
            let text = |name: &str| node.attribute(name).unwrap_or_default().to_owned();
            // A state the dump does not write, or writes as anything but `true`, is not held.
            let state = |name: &str| node.attribute(name) == Some("true");
            Ok(UiNode {
                package: text("package"),
                class: text("class"),
                text: text("text"),
                content_desc: text("content-desc"),
                checkable: state("checkable"),
                checked: state("checked"),
                enabled: state("enabled"),
                focusable: state("focusable"),
                focused: state("focused"),
                selected: state("selected"),
                bounds: node.attribute("bounds").and_then(Bounds::parse),
                children: nodes_of(node, depth + 1)?,
            })
        })
        .collect()
}
