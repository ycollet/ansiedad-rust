use crate::config::Config;
use crossterm::{
    cursor::{MoveTo, Show},
    event::{KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    row: usize,
    col: usize,
}

pub struct Editor {
    lines: Vec<String>,
    row: usize,
    col: usize,
    top: usize,
    left: usize,
    anchor: Option<Position>,
    saved: String,
    on_disk: bool,
    pub error: Option<String>,
}

impl Editor {
    pub fn new(source: &str) -> Self {
        Self {
            lines: source.split('\n').map(String::from).collect(),
            row: 0,
            col: 0,
            top: 0,
            left: 0,
            anchor: None,
            saved: source.into(),
            on_disk: true,
            error: None,
        }
    }

    pub fn scratch(source: &str) -> Self {
        let mut editor = Self::new(source);
        editor.on_disk = false;
        editor
    }

    pub fn source(&self) -> String {
        self.lines.join("\n")
    }

    pub fn modified(&self) -> bool {
        self.source() != self.saved
    }

    pub fn position(&self) -> (usize, usize) {
        (self.row + 1, self.col + 1)
    }

    fn byte(&self) -> usize {
        self.byte_at(self.cursor())
    }

    fn cursor(&self) -> Position {
        Position {
            row: self.row,
            col: self.col,
        }
    }

    fn byte_at(&self, position: Position) -> usize {
        self.lines[position.row]
            .char_indices()
            .nth(position.col)
            .map_or(self.lines[position.row].len(), |(byte, _)| byte)
    }

    fn selection(&self) -> Option<(Position, Position)> {
        let anchor = self.anchor?;
        let cursor = self.cursor();
        (anchor != cursor).then_some((anchor.min(cursor), anchor.max(cursor)))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(
            (start.row..=end.row)
                .map(|row| {
                    let left = if row == start.row {
                        self.byte_at(start)
                    } else {
                        0
                    };
                    let right = if row == end.row {
                        self.byte_at(end)
                    } else {
                        self.lines[row].len()
                    };
                    &self.lines[row][left..right]
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    pub fn delete_selection(&mut self) -> bool {
        let selection = self.selection();
        self.anchor = None;
        let Some((start, end)) = selection else {
            return false;
        };
        let tail = self.lines[end.row][self.byte_at(end)..].to_string();
        let byte = self.byte_at(start);
        self.lines[start.row].truncate(byte);
        self.lines[start.row].push_str(&tail);
        self.lines.drain(start.row + 1..end.row + 1);
        self.row = start.row;
        self.col = start.col;
        true
    }

    fn clamp_col(&mut self) {
        self.col = self.col.min(self.lines[self.row].chars().count());
    }

    pub fn insert(&mut self, text: &str) {
        let text = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        let text: String = text
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect();
        if text.is_empty() {
            return;
        }
        self.delete_selection();
        let byte = self.byte();
        let tail = self.lines[self.row].split_off(byte);
        let mut parts = text.split('\n');
        let first = parts.next().unwrap();
        self.lines[self.row].push_str(first);
        self.col += first.chars().count();
        for part in parts {
            self.row += 1;
            self.lines.insert(self.row, part.into());
            self.col = part.chars().count();
        }
        self.lines[self.row].push_str(&tail);
    }

    pub fn input(&mut self, key: KeyEvent, page: usize) -> bool {
        if key.modifiers == KeyModifiers::ALT && key.code == KeyCode::Char('m') {
            return self.toggle_comments();
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let navigation = matches!(
            key.code,
            KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
        );
        if control && !navigation {
            return false;
        }
        if navigation {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                let cursor = self.cursor();
                self.anchor.get_or_insert(cursor);
            } else {
                let selection = self.selection();
                self.anchor = None;
                if let Some((start, end)) = selection {
                    let collapse = match key.code {
                        KeyCode::Left => Some(start),
                        KeyCode::Right => Some(end),
                        _ => None,
                    };
                    if let Some(position) = collapse {
                        self.row = position.row;
                        self.col = position.col;
                        return false;
                    }
                }
            }
        }
        match key.code {
            KeyCode::Char(c) if !c.is_control() => {
                self.insert(&c.to_string());
                return true;
            }
            KeyCode::Enter => {
                self.insert("\n");
                return true;
            }
            KeyCode::Backspace => {
                if self.delete_selection() {
                    return true;
                }
                if self.col > 0 {
                    let end = self.byte();
                    self.col -= 1;
                    let start = self.byte();
                    self.lines[self.row].replace_range(start..end, "");
                    return true;
                } else if self.row > 0 {
                    let line = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].chars().count();
                    self.lines[self.row].push_str(&line);
                    return true;
                }
            }
            KeyCode::Delete => {
                if self.delete_selection() {
                    return true;
                }
                let byte = self.byte();
                if byte < self.lines[self.row].len() {
                    let length = self.lines[self.row][byte..]
                        .chars()
                        .next()
                        .unwrap()
                        .len_utf8();
                    self.lines[self.row].replace_range(byte..byte + length, "");
                    return true;
                } else if self.row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.row + 1);
                    self.lines[self.row].push_str(&next);
                    return true;
                }
            }
            KeyCode::Left if control => self.word_left(),
            KeyCode::Right if control => self.word_right(),
            KeyCode::Up if control => self.paragraph_up(),
            KeyCode::Down if control => self.paragraph_down(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => {
                self.row = self.row.saturating_sub(1);
                self.clamp_col();
            }
            KeyCode::Down => {
                self.row = (self.row + 1).min(self.lines.len() - 1);
                self.clamp_col();
            }
            KeyCode::PageUp => {
                self.row = self.row.saturating_sub(page.max(1));
                self.clamp_col();
            }
            KeyCode::PageDown => {
                self.row = (self.row + page.max(1)).min(self.lines.len() - 1);
                self.clamp_col();
            }
            KeyCode::Home => {
                if control {
                    self.row = 0;
                }
                self.col = 0;
            }
            KeyCode::End => {
                if control {
                    self.row = self.lines.len() - 1;
                }
                self.col = self.lines[self.row].chars().count();
            }
            _ => {}
        }
        false
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        }
    }

    fn move_right(&mut self) {
        if self.col < self.lines[self.row].chars().count() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn previous_char(&self) -> Option<char> {
        if self.col == 0 {
            (self.row > 0).then_some('\n')
        } else {
            self.lines[self.row].chars().nth(self.col - 1)
        }
    }

    fn next_char(&self) -> Option<char> {
        self.lines[self.row]
            .chars()
            .nth(self.col)
            .or_else(|| (self.row + 1 < self.lines.len()).then_some('\n'))
    }

    fn word_left(&mut self) {
        while self.previous_char().is_some_and(char::is_whitespace) {
            self.move_left();
        }
        let class = self.previous_char().map(word_class);
        while class.is_some() && self.previous_char().map(word_class) == class {
            self.move_left();
        }
    }

    fn word_right(&mut self) {
        let class = self.next_char().map(word_class);
        while class.is_some() && self.next_char().map(word_class) == class {
            self.move_right();
        }
        while self.next_char().is_some_and(char::is_whitespace) {
            self.move_right();
        }
    }

    fn paragraph_up(&mut self) {
        if self.col == 0 {
            self.row = self.row.saturating_sub(1);
        }
        while self.row > 0 && self.lines[self.row].trim().is_empty() {
            self.row -= 1;
        }
        while self.row > 0 && !self.lines[self.row - 1].trim().is_empty() {
            self.row -= 1;
        }
        self.col = 0;
    }

    fn paragraph_down(&mut self) {
        while self.row < self.lines.len() && !self.lines[self.row].trim().is_empty() {
            self.row += 1;
        }
        while self.row < self.lines.len() && self.lines[self.row].trim().is_empty() {
            self.row += 1;
        }
        if self.row == self.lines.len() {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        } else {
            self.col = 0;
        }
    }

    fn toggle_comments(&mut self) -> bool {
        let (first, last) = match self.selection() {
            Some((start, end)) => (start.row, end.row - usize::from(end.col == 0)),
            None => (self.row, self.row),
        };
        let uncomment = self.lines[first..=last]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with('#'));
        let mut changed = false;
        for row in first..=last {
            let line = &mut self.lines[row];
            let byte = line.len() - line.trim_start().len();
            if byte == line.len() {
                continue;
            }
            let indent = line[..byte].chars().count();
            let removed = if uncomment {
                let count = if line[byte..].starts_with("# ") { 2 } else { 1 };
                line.replace_range(byte..byte + count, "");
                count
            } else {
                line.insert_str(byte, "# ");
                0
            };
            let adjust = |position: &mut Position| {
                if position.row == row && position.col > indent {
                    position.col = if uncomment {
                        indent + (position.col - indent).saturating_sub(removed)
                    } else {
                        position.col + 2
                    };
                }
            };
            let mut cursor = self.cursor();
            adjust(&mut cursor);
            self.col = cursor.col;
            if let Some(anchor) = &mut self.anchor {
                adjust(anchor);
            }
            changed = true;
        }
        changed
    }

    pub fn preview(&mut self) -> Option<Config> {
        match Config::parse(&self.source()) {
            Ok(config) => {
                self.error = None;
                Some(config)
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        let source = self.source();
        Config::parse(&source)?;
        if self.on_disk {
            let disk = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            if disk != self.saved {
                return Err(
                    "File changed on disk; F5 twice reloads it and discards this draft".into(),
                );
            }
            fs::write(path, &source).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| {
                    format!(
                        "{}: {e} (scratch saves never overwrite existing files)",
                        path.display()
                    )
                })?;
            file.write_all(source.as_bytes())
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        self.saved = source;
        self.on_disk = true;
        self.error = None;
        Ok(())
    }

    pub fn draw(
        &mut self,
        out: &mut impl Write,
        y: u16,
        height: u16,
        width: u16,
        focused: bool,
    ) -> io::Result<()> {
        if height == 0 || width == 0 {
            return Ok(());
        }
        let number_width = self.lines.len().to_string().len() + 2;
        let gutter = if usize::from(width) > number_width {
            number_width
        } else {
            0
        };
        let available = usize::from(width) - gutter;
        let cursor_x: usize = self.lines[self.row]
            .chars()
            .take(self.col)
            .map(char_width)
            .sum();
        self.top = self.top.min(self.row);
        if self.row >= self.top + usize::from(height) {
            self.top = self.row + 1 - usize::from(height);
        }
        self.left = self.left.min(cursor_x);
        if cursor_x >= self.left + available {
            self.left = cursor_x + 1 - available;
        }
        for line in 0..height {
            queue!(
                out,
                ResetColor,
                MoveTo(0, y + line),
                Clear(ClearType::CurrentLine)
            )?;
            let index = self.top + usize::from(line);
            if let Some(text) = self.lines.get(index) {
                if gutter > 0 {
                    let number = format!("{:>digits$} ", index + 1, digits = gutter - 1);
                    queue!(
                        out,
                        SetForegroundColor(Color::DarkGrey),
                        Print(number),
                        ResetColor
                    )?;
                }
                if let Some((start, end)) = self.selection()
                    && (start.row..=end.row).contains(&index)
                {
                    let mut column = 0;
                    let mut highlighted = false;
                    for (col, character) in text.chars().chain(std::iter::once(' ')).enumerate() {
                        let size = char_width(character);
                        let right = column + size;
                        if column >= self.left + available {
                            break;
                        }
                        if right > self.left {
                            let position = Position { row: index, col };
                            let selected = start <= position && position < end;
                            if selected != highlighted {
                                if selected {
                                    queue!(
                                        out,
                                        SetBackgroundColor(Color::DarkBlue),
                                        SetForegroundColor(Color::White)
                                    )?;
                                } else {
                                    queue!(out, ResetColor)?;
                                }
                                highlighted = selected;
                            }
                            if character.is_control()
                                || column < self.left
                                || right > self.left + available
                            {
                                queue!(
                                    out,
                                    Print(" ".repeat(
                                        right.min(self.left + available) - column.max(self.left)
                                    ))
                                )?;
                            } else {
                                queue!(out, Print(character))?;
                            }
                        }
                        column = right;
                    }
                    queue!(out, ResetColor)?;
                } else {
                    queue!(out, Print(window(text, self.left, available)))?;
                }
            }
        }
        if focused {
            let x = gutter + cursor_x - self.left;
            let row = y + (self.row - self.top) as u16;
            queue!(out, MoveTo(x as u16, row), Show)?;
        }
        Ok(())
    }
}

fn word_class(character: char) -> u8 {
    if character.is_whitespace() {
        0
    } else if character.is_alphanumeric() || character == '_' {
        1
    } else {
        2
    }
}

fn char_width(c: char) -> usize {
    if c == '\t' {
        4
    } else {
        UnicodeWidthChar::width(c).unwrap_or(1)
    }
}

pub fn window(text: &str, left: usize, width: usize) -> String {
    let mut result = String::new();
    let mut column = 0;
    for c in text.chars() {
        let size = char_width(c);
        let end = column + size;
        if column >= left + width {
            break;
        }
        if end > left {
            if c.is_control() || column < left || end > left + width {
                result.extend(std::iter::repeat_n(
                    ' ',
                    end.min(left + width) - column.max(left),
                ));
            } else {
                result.push(c);
            }
        }
        column = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn multiline_unicode_editing_and_navigation() {
        let mut editor = Editor::new("b='0'\n# 世界");
        editor.input(key(KeyCode::Down), 5);
        editor.input(key(KeyCode::End), 5);
        editor.input(key(KeyCode::Backspace), 5);
        assert_eq!(editor.source(), "b='0'\n# 世");
        editor.input(key(KeyCode::Home), 5);
        editor.input(key(KeyCode::Backspace), 5);
        assert_eq!(editor.source(), "b='0'# 世");
        editor.insert("\r\nf='2'\x1b\t");
        assert_eq!(editor.source(), "b='0'\nf='2'    # 世");
        editor.input(key(KeyCode::Home), 5);
        editor.input(key(KeyCode::Up), 5);
        editor.input(key(KeyCode::End), 5);
        editor.input(key(KeyCode::Delete), 5);
        assert_eq!(editor.source(), "b='0'f='2'    # 世");
    }

    #[test]
    fn selection_extends_shrinks_reverses_and_replaces_unicode_text() {
        let mut editor = Editor::new("a界b\ncafé\nend");
        let shift = |code| KeyEvent::new(code, KeyModifiers::SHIFT);
        editor.input(key(KeyCode::Right), 5);
        editor.input(shift(KeyCode::Down), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("界b\nc"));
        editor.input(shift(KeyCode::Right), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("界b\nca"));
        editor.input(shift(KeyCode::Up), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("界"));
        editor.input(shift(KeyCode::Left), 5);
        assert!(editor.selected_text().is_none());
        editor.input(shift(KeyCode::Left), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("a"));
        editor.insert("Z\né");
        assert_eq!(editor.source(), "Z\né界b\ncafé\nend");
        assert_eq!(editor.position(), (2, 2));
        assert!(editor.selected_text().is_none());
    }

    #[test]
    fn selection_deletion_handles_newlines_and_the_entire_document() {
        let shift = |code| KeyEvent::new(code, KeyModifiers::SHIFT);
        let mut editor = Editor::new("éx\n世界\nz");
        editor.input(shift(KeyCode::Down), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("éx\n"));
        assert!(editor.input(key(KeyCode::Delete), 5));
        assert_eq!(editor.source(), "世界\nz");
        assert_eq!(editor.position(), (1, 1));
        editor.input(
            KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            5,
        );
        assert_eq!(editor.selected_text().as_deref(), Some("世界\nz"));
        assert!(editor.input(key(KeyCode::Backspace), 5));
        assert_eq!(editor.source(), "");
        assert_eq!(editor.position(), (1, 1));
        assert!(!editor.input(key(KeyCode::Backspace), 5));
        editor.insert("ab");
        editor.input(shift(KeyCode::Home), 5);
        assert!(editor.input(key(KeyCode::Char('x')), 5));
        assert_eq!(editor.source(), "x");
    }

    #[test]
    fn control_shift_navigation_selects_words_paragraphs_and_document_edges() {
        let ctrl_shift = |code| KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let mut editor = Editor::new("alpha_1 + 世界\nnext\n\nlast");
        editor.input(ctrl_shift(KeyCode::Right), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("alpha_1 "));
        editor.input(ctrl_shift(KeyCode::Right), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("alpha_1 + "));
        editor.input(ctrl_shift(KeyCode::Right), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("alpha_1 + 世界\n"));
        editor.input(ctrl_shift(KeyCode::Left), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("alpha_1 + "));
        editor.input(key(KeyCode::Left), 5);
        assert_eq!(editor.position(), (1, 1));
        assert!(editor.selected_text().is_none());
        editor.input(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL), 5);
        editor.input(ctrl_shift(KeyCode::Up), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("last"));
        editor.input(ctrl_shift(KeyCode::Up), 5);
        assert_eq!(
            editor.selected_text().as_deref(),
            Some(editor.source().as_str())
        );
        editor.input(ctrl_shift(KeyCode::Down), 5);
        assert_eq!(editor.selected_text().as_deref(), Some("last"));
        editor.input(ctrl_shift(KeyCode::Home), 5);
        assert_eq!(
            editor.selected_text().as_deref(),
            Some(editor.source().as_str())
        );
        editor.input(ctrl_shift(KeyCode::End), 5);
        assert!(editor.selected_text().is_none());
        assert_eq!(editor.position(), (4, 5));
    }

    #[test]
    fn plain_navigation_collapses_selection_and_empty_paste_preserves_it() {
        let mut editor = Editor::new("a界b");
        editor.input(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT), 1);
        editor.insert("");
        assert_eq!(editor.selected_text().as_deref(), Some("a界b"));
        editor.input(key(KeyCode::Right), 1);
        assert_eq!(editor.position(), (1, 4));
        assert!(editor.selected_text().is_none());
        editor.input(KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT), 1);
        editor.input(key(KeyCode::Left), 1);
        assert_eq!(editor.position(), (1, 1));
        assert!(editor.selected_text().is_none());
    }

    #[test]
    fn toggling_comments_preserves_indentation_selection_and_last_line_boundary() {
        let source = "  b='1'\n\n  # existing\nf='2'";
        let mut editor = Editor::new(source);
        let comment = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT);
        for _ in 0..3 {
            editor.input(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT), 5);
        }
        assert!(editor.input(comment, 5));
        assert_eq!(editor.source(), "  # b='1'\n\n  # # existing\nf='2'");
        assert_eq!(
            editor.selected_text().as_deref(),
            Some("  # b='1'\n\n  # # existing\n")
        );
        assert!(editor.input(comment, 5));
        assert_eq!(editor.source(), source);
        editor.input(key(KeyCode::Home), 5);
        assert!(editor.input(comment, 5));
        assert_eq!(editor.source(), "  b='1'\n\n  # existing\n# f='2'");
        editor.input(key(KeyCode::End), 5);
        assert!(editor.input(comment, 5));
        assert_eq!(editor.source(), source);
        assert_eq!(editor.position(), (4, 6));
        let mut blank = Editor::new("  ");
        assert!(!blank.input(comment, 5));
        assert_eq!(blank.source(), "  ");
        assert!(blank.input(key(KeyCode::Enter), 5));
        assert_eq!(blank.source(), "\n  ");
    }

    #[test]
    fn selection_highlighting_handles_scrolling_wide_characters_and_controls() {
        let mut highlight = Vec::new();
        queue!(highlight, SetBackgroundColor(Color::DarkBlue)).unwrap();
        for width in [1, 6, 80] {
            let mut editor = Editor::new("# 界界界\x1b\nnext");
            editor.input(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT), 1);
            let mut output = Vec::new();
            editor.draw(&mut output, 0, 2, width, true).unwrap();
            if width > 1 {
                assert!(
                    output
                        .windows(highlight.len())
                        .any(|part| part == highlight)
                );
            }
            let output = String::from_utf8(output).unwrap();
            assert!(!output.contains("\x1b\x1b"));
            editor.input(key(KeyCode::Left), 1);
            let mut output = Vec::new();
            editor.draw(&mut output, 0, 2, width, true).unwrap();
            assert!(
                !output
                    .windows(highlight.len())
                    .any(|part| part == highlight)
            );
        }
    }

    #[test]
    fn invalid_draft_does_not_replace_valid_preview() {
        let mut editor = Editor::new("b='1'");
        assert!(!editor.modified());
        editor.input(key(KeyCode::End), 1);
        editor.insert(" +");
        assert!(editor.modified());
        assert!(editor.preview().is_none());
        assert!(editor.error.is_some());
        editor.input(key(KeyCode::Backspace), 1);
        editor.input(key(KeyCode::Backspace), 1);
        assert!(editor.preview().is_some());
        assert!(!editor.modified());
    }

    #[test]
    fn scratch_save_creates_a_file_without_overwriting_existing_content() {
        let path = std::env::temp_dir().join(format!(
            "ansiedad-scratch-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut editor = Editor::scratch("b='t'");
        assert!(!editor.modified());
        fs::write(&path, "b='1'").unwrap();
        assert!(editor.save(&path).unwrap_err().contains("never overwrite"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='1'");
        fs::remove_file(&path).unwrap();
        editor.save(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='t'");
        editor.input(key(KeyCode::End), 1);
        editor.insert("\nf='15'");
        editor.save(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='t'\nf='15'");
        fs::remove_file(&path).unwrap();
        let mut invalid = Editor::scratch("b='t+'");
        assert!(invalid.save(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn save_persists_valid_edits_but_rejects_invalid_drafts_and_conflicts() {
        let path = std::env::temp_dir().join(format!(
            "ansiedad-save-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "b='1'").unwrap();
        let mut editor = Editor::new("b='1'");
        editor.input(key(KeyCode::End), 1);
        editor.insert("\nf='2'");
        editor.save(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='1'\nf='2'");
        assert!(!editor.modified());
        editor.insert("+");
        assert!(editor.save(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='1'\nf='2'");
        editor.input(key(KeyCode::Backspace), 1);
        editor.insert("\nc='144'");
        fs::write(&path, "b='3'").unwrap();
        assert!(editor.save(&path).unwrap_err().contains("changed on disk"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='3'");
        assert!(editor.modified());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn viewport_respects_display_width_and_filters_controls() {
        assert_eq!(window("a界bc", 2, 3), " bc");
        assert_eq!(window("a界bc", 0, 2), "a ");
        assert_eq!(window("\x1b[31m", 0, 6), " [31m");
        let mut editor = Editor::new("# 界界界界\nb='0'");
        editor.input(key(KeyCode::End), 1);
        for (width, height) in [(1, 1), (6, 1), (80, 5)] {
            editor
                .draw(&mut Vec::new(), 4, height, width, true)
                .unwrap();
        }
    }
}
