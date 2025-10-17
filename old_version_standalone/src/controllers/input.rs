//! Input controller for managing text input and editing

use serde::{Deserialize, Serialize};

/// Controls text input and editing state
pub struct InputController {
    /// Buffer for editing
    edit_buffer: String,
    /// Cursor position
    cursor_position: usize,
    /// Selection range (start, end)
    selection: Option<(usize, usize)>,
    /// Input mode
    input_mode: InputMode,
    /// Input formatter
    formatter: Option<Box<dyn Fn(&str) -> String>>,
    /// Input parser
    parser: Option<Box<dyn Fn(&str) -> Result<String, String>>>,
    /// Input mask (e.g., "###-##-####" for SSN)
    mask: Option<String>,
}

/// Input mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    /// Normal text input
    Normal,
    /// Insert mode (overwrites)
    Insert,
    /// Replace mode
    Replace,
    /// Password input (hidden)
    Password,
    /// Numeric only
    Numeric,
    /// Email input
    Email,
}

impl Default for InputController {
    fn default() -> Self {
        Self::new()
    }
}

impl InputController {
    /// Create new input controller
    pub fn new() -> Self {
        Self {
            edit_buffer: String::new(),
            cursor_position: 0,
            selection: None,
            input_mode: InputMode::Normal,
            formatter: None,
            parser: None,
            mask: None,
        }
    }

    /// Set input mode
    pub fn with_mode(mut self, mode: InputMode) -> Self {
        self.input_mode = mode;
        self
    }

    /// Set input mask
    pub fn with_mask(mut self, mask: impl Into<String>) -> Self {
        self.mask = Some(mask.into());
        self
    }

    /// Begin editing with initial value
    pub fn begin_edit(&mut self, value: impl ToString) {
        self.edit_buffer = value.to_string();
        self.cursor_position = self.edit_buffer.len();
        self.selection = None;
    }

    /// Commit edit and return result
    pub fn commit_edit(&mut self) -> Result<String, String> {
        if let Some(parser) = &self.parser {
            parser(&self.edit_buffer)
        } else {
            Ok(self.edit_buffer.clone())
        }
    }

    /// Cancel edit
    pub fn cancel_edit(&mut self) {
        self.edit_buffer.clear();
        self.cursor_position = 0;
        self.selection = None;
    }

    /// Get edit buffer
    pub fn buffer(&self) -> &str {
        &self.edit_buffer
    }

    /// Set edit buffer
    pub fn set_buffer(&mut self, text: impl Into<String>) {
        self.edit_buffer = text.into();
        self.cursor_position = self.edit_buffer.len().min(self.cursor_position);
    }

    /// Get cursor position
    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    /// Set cursor position
    pub fn set_cursor(&mut self, position: usize) {
        self.cursor_position = position.min(self.edit_buffer.len());
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.edit_buffer.len() {
            self.cursor_position += 1;
        }
    }

    /// Move cursor to start
    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
    }

    /// Move cursor to end
    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.edit_buffer.len();
    }

    /// Insert character at cursor
    pub fn insert_char(&mut self, ch: char) {
        // Validate character based on mode
        if !self.is_valid_char(ch) {
            return;
        }

        if self.input_mode == InputMode::Insert && self.cursor_position < self.edit_buffer.len() {
            // Replace character in insert mode
            self.edit_buffer.remove(self.cursor_position);
        }

        self.edit_buffer.insert(self.cursor_position, ch);
        self.cursor_position += 1;

        // Apply mask if present
        if let Some(ref mask) = self.mask.clone() {
            self.apply_mask(&mask);
        }
    }

    /// Delete character at cursor
    pub fn delete_char(&mut self) {
        if self.cursor_position < self.edit_buffer.len() {
            self.edit_buffer.remove(self.cursor_position);
        }
    }

    /// Backspace (delete before cursor)
    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.edit_buffer.remove(self.cursor_position);
        }
    }

    /// Select all text
    pub fn select_all(&mut self) {
        self.selection = Some((0, self.edit_buffer.len()));
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Delete selection
    pub fn delete_selection(&mut self) {
        if let Some((start, end)) = self.selection {
            let start = start.min(self.edit_buffer.len());
            let end = end.min(self.edit_buffer.len());
            self.edit_buffer.drain(start..end);
            self.cursor_position = start;
            self.selection = None;
        }
    }

    /// Get selected text
    pub fn selected_text(&self) -> Option<&str> {
        self.selection.map(|(start, end)| {
            let start = start.min(self.edit_buffer.len());
            let end = end.min(self.edit_buffer.len());
            &self.edit_buffer[start..end]
        })
    }

    /// Format display text
    pub fn format_display(&self, text: &str) -> String {
        if self.input_mode == InputMode::Password {
            "•".repeat(text.len())
        } else if let Some(formatter) = &self.formatter {
            formatter(text)
        } else {
            text.to_string()
        }
    }

    /// Check if character is valid for input mode
    fn is_valid_char(&self, ch: char) -> bool {
        match self.input_mode {
            InputMode::Numeric => ch.is_numeric() || ch == '.' || ch == '-',
            InputMode::Email => ch.is_alphanumeric() || "@.-_+".contains(ch),
            _ => true,
        }
    }

    /// Apply mask to buffer
    fn apply_mask(&mut self, mask: &str) {
        // Simple mask implementation
        // # = digit, A = letter, * = any
        let mut result = String::new();
        let mut chars = self.edit_buffer.chars();

        for mask_char in mask.chars() {
            match mask_char {
                '#' => {
                    if let Some(ch) = chars.next() {
                        if ch.is_numeric() {
                            result.push(ch);
                        }
                    }
                }
                'A' => {
                    if let Some(ch) = chars.next() {
                        if ch.is_alphabetic() {
                            result.push(ch);
                        }
                    }
                }
                '*' => {
                    if let Some(ch) = chars.next() {
                        result.push(ch);
                    }
                }
                literal => {
                    result.push(literal);
                }
            }
        }

        self.edit_buffer = result;
    }
}