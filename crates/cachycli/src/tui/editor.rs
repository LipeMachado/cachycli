//! Prompt input editor: multi-line text, cursor movement, history, undo.

#[derive(Debug, Clone)]
pub struct Editor {
    pub text: String,
    /// Cursor position as a byte offset into `text`.
    pub cursor: usize,
    undo_stack: Vec<(String, usize)>,
    redo_stack: Vec<(String, usize)>,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub multiline: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history: Vec::new(),
            history_index: None,
            multiline: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn value(&self) -> String {
        self.text.clone()
    }

    fn snapshot(&mut self) {
        self.undo_stack.push((self.text.clone(), self.cursor));
        if self.undo_stack.len() > 200 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.snapshot();
        self.text = text.into();
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.snapshot();
        self.text.clear();
        self.cursor = 0;
        self.history_index = None;
    }

    pub fn insert_char(&mut self, c: char) {
        self.snapshot();
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.snapshot();
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.snapshot();
        let prev = self.prev_boundary(self.cursor);
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        self.snapshot();
        let next = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..next, "");
    }

    pub fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.snapshot();
        let mut idx = self.cursor;
        while idx > 0 {
            let prev = self.prev_boundary(idx);
            let ch = self.text[prev..idx].chars().next().unwrap_or(' ');
            idx = prev;
            if !ch.is_whitespace() && prev == 0 {
                break;
            }
            if ch.is_whitespace() && idx < self.cursor {
                break;
            }
        }
        self.text.replace_range(idx..self.cursor, "");
        self.cursor = idx;
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_boundary(self.cursor);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.next_boundary(self.cursor);
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Move up one display line, preserving the column when possible.
    pub fn move_up(&mut self) {
        let (row, col) = self.cursor_row_col();
        if row == 0 {
            self.history_prev();
            return;
        }
        let lines: Vec<&str> = self.text.split('\n').collect();
        let target_line = lines[row - 1];
        let new_col = col.min(target_line.len());
        let mut offset = 0usize;
        for line in lines.iter().take(row - 1) {
            offset += line.len() + 1;
        }
        self.cursor = offset + new_col;
    }

    pub fn move_down(&mut self) {
        let (row, col) = self.cursor_row_col();
        let lines: Vec<&str> = self.text.split('\n').collect();
        if row + 1 >= lines.len() {
            self.history_next();
            return;
        }
        let target_line = lines[row + 1];
        let new_col = col.min(target_line.len());
        let mut offset = 0usize;
        for line in lines.iter().take(row + 1) {
            offset += line.len() + 1;
        }
        self.cursor = offset + new_col;
    }

    /// 0-based (row, byte-column) of the cursor.
    pub fn cursor_row_col(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let row = before.matches('\n').count();
        let col = before.rsplit('\n').next().map(|s| s.len()).unwrap_or(0);
        (row, col)
    }

    pub fn undo(&mut self) {
        if let Some((text, cursor)) = self.undo_stack.pop() {
            self.redo_stack.push((self.text.clone(), self.cursor));
            self.text = text;
            self.cursor = cursor.min(self.text.len());
        }
    }

    pub fn redo(&mut self) {
        if let Some((text, cursor)) = self.redo_stack.pop() {
            self.undo_stack.push((self.text.clone(), self.cursor));
            self.text = text;
            self.cursor = cursor.min(self.text.len());
        }
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_index {
            Some(0) => 0,
            Some(i) => i - 1,
            None => self.history.len() - 1,
        };
        self.history_index = Some(index);
        self.text = self.history[index].clone();
        self.cursor = self.text.len();
    }

    fn history_next(&mut self) {
        match self.history_index {
            Some(i) if i + 1 < self.history.len() => {
                self.history_index = Some(i + 1);
                self.text = self.history[i + 1].clone();
                self.cursor = self.text.len();
            }
            Some(_) => {
                self.history_index = None;
                self.text.clear();
                self.cursor = 0;
            }
            None => {}
        }
    }

    /// Record a submitted prompt in history.
    pub fn remember(&mut self, text: &str) {
        if !text.trim().is_empty() && self.history.last().map(|h| h != text).unwrap_or(true) {
            self.history.push(text.to_string());
        }
        self.history_index = None;
    }

    fn prev_boundary(&self, idx: usize) -> usize {
        let mut i = idx.saturating_sub(1);
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_boundary(&self, idx: usize) -> usize {
        let mut i = idx + 1;
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        i.min(self.text.len())
    }

    /// The word being completed at the cursor for `@` file references.
    /// Returns `(start_byte, query)` where query excludes the `@`.
    pub fn active_at_reference(&self) -> Option<(usize, String)> {
        let before = &self.text[..self.cursor];
        // find the last '@' that starts a token
        let at_pos = before.rfind('@')?;
        let token = &before[at_pos + 1..];
        if token.contains(char::is_whitespace) {
            return None;
        }
        Some((at_pos, token.to_string()))
    }

    /// The slash command prefix when the input starts with `/` on line 1.
    pub fn active_slash(&self) -> Option<String> {
        let trimmed = self.text.trim_start();
        if !trimmed.starts_with('/') {
            return None;
        }
        if trimmed.contains('\n') {
            return None;
        }
        Some(trimmed.to_string())
    }

    /// Cursor position in display columns on its row.
    pub fn cursor_col_display(&self) -> usize {
        let (_, col) = self.cursor_row_col();
        let line = &self.text[..self.cursor];
        let current_line = line.rsplit('\n').next().unwrap_or("");
        let _ = col;
        unicode_width::UnicodeWidthStr::width(current_line)
    }

    /// Lines for rendering the prompt box.
    pub fn display_lines(&self) -> Vec<(usize, String)> {
        self.text
            .split('\n')
            .enumerate()
            .map(|(i, l)| (i, l.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing() {
        let mut e = Editor::new();
        e.insert_str("hello");
        e.insert_char('!');
        assert_eq!(e.value(), "hello!");
        e.backspace();
        assert_eq!(e.value(), "hello");
        // Undo restores the text removed by backspace; redo re-applies it.
        e.undo();
        assert_eq!(e.value(), "hello!");
        e.redo();
        assert_eq!(e.value(), "hello");
    }

    #[test]
    fn at_reference() {
        let mut e = Editor::new();
        e.insert_str("look at @src/ma");
        let (start, query) = e.active_at_reference().unwrap();
        assert!(e.value()[start..].starts_with('@'));
        assert_eq!(query, "src/ma");
    }
}
