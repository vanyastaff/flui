//! The one human rendering of a snapshot.

use core::fmt::{self, Write};

use super::fields::Level;
use super::snapshot::SnapshotNode;

/// What a piece of rendered text is, so a [`Style`] can colour it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// A node's kind.
    Kind,
    /// A node's id.
    Id,
    /// A field's name.
    FieldName,
    /// A field's value.
    Value,
    /// Secondary text: bounds, counts of omitted nodes.
    Dim,
}

/// Decorates rendered text by role. The renderer calls `begin` before and `end` after each
/// piece.
///
/// Both methods fail only when writing to `out` fails.
#[expect(
    clippy::missing_errors_doc,
    reason = "the trait documents its one failure, the writer's"
)]
pub trait Style {
    /// Starts a piece of `role`.
    fn begin(&self, role: Role, out: &mut dyn Write) -> fmt::Result;
    /// Ends it.
    fn end(&self, role: Role, out: &mut dyn Write) -> fmt::Result;
}

/// No decoration: what tests, logs and pipes read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plain;

impl Style for Plain {
    fn begin(&self, _: Role, _: &mut dyn Write) -> fmt::Result {
        Ok(())
    }

    fn end(&self, _: Role, _: &mut dyn Write) -> fmt::Result {
        Ok(())
    }
}

/// ANSI colours, for a terminal.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ansi;

impl Style for Ansi {
    fn begin(&self, role: Role, out: &mut dyn Write) -> fmt::Result {
        out.write_str(match role {
            Role::Kind => "\x1b[1m",
            Role::Id => "\x1b[36m",
            Role::FieldName => "\x1b[2m",
            Role::Value => "\x1b[33m",
            Role::Dim => "\x1b[2;3m",
        })
    }

    fn end(&self, _: Role, out: &mut dyn Write) -> fmt::Result {
        out.write_str("\x1b[0m")
    }
}

/// The style for output to a stream: [`Ansi`] when the stream is a terminal and `NO_COLOR` is
/// unset or empty (<https://no-color.org>), [`Plain`] otherwise.
#[must_use]
pub fn style_for(is_terminal: bool) -> &'static dyn Style {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    if is_terminal && !no_color {
        &Ansi
    } else {
        &Plain
    }
}

/// Renders `root` as an indented outline, one node per line:
///
/// ```text
/// RenderFlex r7  direction=Horizontal  0,0 360x48
///   RenderPadding r12  padding=8  8,8 344x32
///     RenderParagraph r19  text="Save"  0,0 40x14
///   … 2 more
/// ```
///
/// The kind and id come first, then the fields at or under `level` (at [`Level::Summary`] a
/// field equal to its default is left out), then the bounds. Values use their compact,
/// pinned [`Display`](core::fmt::Display).
///
/// # Errors
///
/// Only when writing to `out` fails.
pub fn render(
    root: &SnapshotNode,
    level: Level,
    style: &dyn Style,
    out: &mut dyn Write,
) -> fmt::Result {
    line(root, 0, level, style, out)
}

fn piece(
    style: &dyn Style,
    role: Role,
    out: &mut dyn Write,
    text: fmt::Arguments<'_>,
) -> fmt::Result {
    style.begin(role, out)?;
    out.write_fmt(text)?;
    style.end(role, out)
}

fn line(
    node: &SnapshotNode,
    depth: usize,
    level: Level,
    style: &dyn Style,
    out: &mut dyn Write,
) -> fmt::Result {
    write!(out, "{:width$}", "", width = depth * 2)?;
    style.begin(Role::Kind, out)?;
    out.write_str(node.kind)?;
    if !node.kind_args.is_empty() {
        out.write_str("<")?;
        for (index, arg) in node.kind_args.iter().enumerate() {
            if index > 0 {
                out.write_str(", ")?;
            }
            out.write_str(arg)?;
        }
        out.write_str(">")?;
    }
    style.end(Role::Kind, out)?;
    out.write_str(" ")?;
    piece(style, Role::Id, out, format_args!("{}", node.id))?;
    for field in &node.fields {
        let hidden = field.level > level || (level == Level::Summary && field.is_default());
        if hidden {
            continue;
        }
        out.write_str("  ")?;
        piece(style, Role::FieldName, out, format_args!("{}=", field.name))?;
        piece(style, Role::Value, out, format_args!("{}", field.value))?;
    }
    if let Some(rect) = node.rect {
        out.write_str("  ")?;
        piece(style, Role::Dim, out, format_args!("{rect}"))?;
    }
    out.write_str("\n")?;
    for child in &node.children {
        line(child, depth + 1, level, style, out)?;
    }
    if node.omitted_children > 0 {
        write!(out, "{:width$}", "", width = (depth + 1) * 2)?;
        piece(
            style,
            Role::Dim,
            out,
            format_args!("… {} more", node.omitted_children),
        )?;
        out.write_str("\n")?;
    }
    Ok(())
}

/// The outline at [`Level::Summary`], plain.
impl fmt::Display for SnapshotNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(self, Level::Summary, &Plain, f)
    }
}
