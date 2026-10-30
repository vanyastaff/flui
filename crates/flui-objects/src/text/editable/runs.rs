//! The field's text as runs for assistive technology, built by the shared
//! [`text_runs`](crate::text::runs::text_runs), with the selection.

use flui_foundation::geometry::{Point, Rect, Size};
use flui_painting::typography::TextDirection;
use flui_rendering::semantics::{TextContent, TextRunError, TextRunSelection};

use super::RenderEditable;
use crate::text::runs::text_runs;

impl RenderEditable {
    /// The laid-out text as runs, with the selection, or `None` before
    /// layout. A run the semantics model refuses is a bug in this
    /// description: it is logged and the field publishes its value alone.
    pub(super) fn semantics_text_content(&self) -> Option<TextContent> {
        if !self.painter.has_layout() {
            return None;
        }
        match self.describe_runs() {
            Ok(content) => Some(content),
            Err(error) => {
                tracing::error!(%error, "BUG: a text field's layout described a run the semantics model refuses");
                None
            }
        }
    }

    fn describe_runs(&self) -> Result<TextContent, TextRunError> {
        let runs = text_runs(
            self.plain_text.as_str(),
            self.painter.line_layouts(),
            self.painter.text_direction().unwrap_or(TextDirection::Ltr),
            self.scroll_x,
            |start, _| {
                self.local_rect_for_range(start..start)
                    .unwrap_or_else(|| Rect::from_origin_size(Point::ZERO, Size::ZERO))
            },
        )?;
        let content = TextContent::try_new(runs)?;
        match self.run_selection(&content) {
            Some(selection) => content.with_selection(selection),
            None => Ok(content),
        }
    }

    /// The selection, focus at the caret, as positions in `content`: `None`
    /// when an end falls inside a published character, which the
    /// controller's grapheme snapping rules out.
    fn run_selection(&self, content: &TextContent) -> Option<TextRunSelection> {
        let caret = self.caret_byte_offset;
        let anchor = match &self.selection {
            Some(range) if range.end == caret => range.start,
            Some(range) => range.end,
            None => caret,
        };
        Some(TextRunSelection::new(
            content.position_at(anchor)?,
            content.position_at(caret)?,
        ))
    }
}
