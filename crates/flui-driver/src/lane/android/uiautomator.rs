//! What `uiautomator dump` shows of the screen, read as untrusted input: where the app's window
//! is and how the display is turned.
//!
//! The dump is the active window's view hierarchy as accessibility reads it:
//! `<hierarchy rotation="N">` and its `<node>`s, each with its `package` and
//! `bounds="[left,top][right,bottom]"` in display pixels as the display is turned, which is all
//! the lane reads of them. It is read whole only after its size is checked ([`MAX_DUMP`]), with a
//! bound on its nodes ([`MAX_NODES`]) and on its depth ([`MAX_DEPTH`]).
//!
//! FLUI's own nodes are not read from it: the adapter that serves them (`accesskit_android`) sets
//! no `resource-id`, so the identifier a projection and an activation look for never reaches it
//! (`docs/gaps.md`).

/// The most bytes of a dump the lane reads.
pub const MAX_DUMP: usize = 4 * 1024 * 1024;

/// The most nodes of a dump the lane reads.
pub const MAX_NODES: u32 = 100_000;

/// The deepest a node of a dump may sit.
pub const MAX_DEPTH: usize = 256;

/// Where a session has its dumps written on the device: a directory the shell may write, a file
/// named for the session (`session` is lowercase hex), so two sessions on one device never read
/// each other's dump.
#[must_use]
pub fn dump_path(session: &str) -> String {
    format!("/data/local/tmp/flui-driver-{session}.xml")
}

/// The line that dumps the active window to `path` and prints the dump: the file is removed
/// whatever the dump's outcome.
///
/// The dump connects to accessibility as `UiAutomation` does, which suspends the accessibility
/// services running (TalkBack among them) until it ends.
#[must_use]
pub fn dump_line(path: &str) -> String {
    format!("uiautomator dump {path} >/dev/null 2>&1 && cat {path}; rm -f {path}")
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

/// One node of a dump.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct UiNode {
    /// The application that shows it.
    pub package: String,
    /// Where it is; `None` when the dump gave no bounds it could read.
    pub bounds: Option<Bounds>,
    /// The nodes inside it.
    pub children: Vec<UiNode>,
}

/// A dump: how the display is turned, and the active window's nodes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct UiDump {
    /// The display's rotation from its natural orientation, in quarter turns counter-clockwise
    /// (`Surface.ROTATION_0` to `ROTATION_270`).
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

    /// The first node, outermost first, that `package` shows at the size `size` within
    /// `tolerance` pixels on each side: the app's surface among its window's views.
    #[must_use]
    pub fn surface_of(&self, package: &str, size: (f64, f64), tolerance: f64) -> Option<Bounds> {
        let fits = |node: &UiNode| {
            node.package == package
                && node.bounds.is_some_and(|bounds| {
                    (f64::from(bounds.width()) - size.0).abs() <= tolerance
                        && (f64::from(bounds.height()) - size.1).abs() <= tolerance
                })
        };
        let mut pending: Vec<&UiNode> = self.roots.iter().rev().collect();
        while let Some(node) = pending.pop() {
            if fits(node) {
                return node.bounds;
            }
            pending.extend(node.children.iter().rev());
        }
        None
    }

    /// The applications whose nodes the window holds at its top, for people.
    #[must_use]
    pub fn packages(&self) -> Vec<&str> {
        let mut packages: Vec<&str> = Vec::new();
        for root in &self.roots {
            if !packages.contains(&root.package.as_str()) {
                packages.push(&root.package);
            }
        }
        packages
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
            Ok(UiNode {
                package: node.attribute("package").unwrap_or_default().to_owned(),
                bounds: node.attribute("bounds").and_then(Bounds::parse),
                children: nodes_of(node, depth + 1)?,
            })
        })
        .collect()
}
