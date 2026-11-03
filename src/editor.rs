use crate::config::Config;
use crossterm::{
    cursor::{MoveTo, Show},
    event::{KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
use unicode_width::UnicodeWidthChar;

pub struct Editor {
    lines: Vec<String>,
    row: usize,
    col: usize,
    top: usize,
    left: usize,
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
        self.lines[self.row]
            .char_indices()
            .nth(self.col)
            .map_or(self.lines[self.row].len(), |(byte, _)| byte)
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
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            if key.modifiers == KeyModifiers::CONTROL {
                match key.code {
                    KeyCode::Home => {
                        self.row = 0;
                        self.col = 0;
                    }
                    KeyCode::End => {
                        self.row = self.lines.len() - 1;
                        self.col = self.lines[self.row].chars().count();
                    }
                    _ => {}
                }
            }
            return false;
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
            KeyCode::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].chars().count();
                }
            }
            KeyCode::Right => {
                if self.col < self.lines[self.row].chars().count() {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
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
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.lines[self.row].chars().count(),
            _ => {}
        }
        false
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
                queue!(out, Print(window(text, self.left, available)))?;
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
