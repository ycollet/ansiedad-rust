#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub fn from_index(text: &str, index: usize) -> Self {
        let mut position = Self { row: 0, col: 0 };
        for character in text.chars().take(index) {
            if character == '\n' {
                position.row += 1;
                position.col = 0;
            } else {
                position.col += 1;
            }
        }
        position
    }

    pub fn index(self, text: &str) -> usize {
        let mut index = 0;
        for (row, line) in text.split('\n').enumerate() {
            if row == self.row {
                return index + self.col.min(line.chars().count());
            }
            index += line.chars().count() + 1;
        }
        text.chars().count()
    }
}

#[derive(Clone, Copy)]
pub enum Navigation {
    WordLeft,
    WordRight,
    ParagraphUp,
    ParagraphDown,
    Start,
    End,
}

pub fn navigate(text: &str, mut position: Position, navigation: Navigation) -> Position {
    match navigation {
        Navigation::Start => Position { row: 0, col: 0 },
        Navigation::End => Position::from_index(text, text.chars().count()),
        Navigation::WordLeft | Navigation::WordRight => {
            let chars: Vec<char> = text.chars().collect();
            let mut index = position.index(text);
            if matches!(navigation, Navigation::WordLeft) {
                while index > 0 && chars[index - 1].is_whitespace() {
                    index -= 1;
                }
                if index > 0 {
                    let class = word_class(chars[index - 1]);
                    while index > 0 && word_class(chars[index - 1]) == class {
                        index -= 1;
                    }
                }
            } else {
                if let Some(character) = chars.get(index) {
                    let class = word_class(*character);
                    while index < chars.len() && word_class(chars[index]) == class {
                        index += 1;
                    }
                }
                while index < chars.len() && chars[index].is_whitespace() {
                    index += 1;
                }
            }
            Position::from_index(text, index)
        }
        Navigation::ParagraphUp | Navigation::ParagraphDown => {
            let lines: Vec<&str> = text.split('\n').collect();
            if matches!(navigation, Navigation::ParagraphUp) {
                if position.col == 0 {
                    position.row = position.row.saturating_sub(1);
                }
                while position.row > 0 && lines[position.row].trim().is_empty() {
                    position.row -= 1;
                }
                while position.row > 0 && !lines[position.row - 1].trim().is_empty() {
                    position.row -= 1;
                }
                position.col = 0;
            } else {
                while position.row < lines.len() && !lines[position.row].trim().is_empty() {
                    position.row += 1;
                }
                while position.row < lines.len() && lines[position.row].trim().is_empty() {
                    position.row += 1;
                }
                if position.row == lines.len() {
                    position.row -= 1;
                    position.col = lines[position.row].chars().count();
                } else {
                    position.col = 0;
                }
            }
            position
        }
    }
}

pub fn toggle_comments(
    lines: &mut [String],
    cursor: &mut Position,
    mut anchor: Option<&mut Position>,
) -> bool {
    let (first, last) = match anchor.as_deref() {
        Some(anchor) if anchor != cursor => {
            let start = (*anchor).min(*cursor);
            let end = (*anchor).max(*cursor);
            (start.row, end.row - usize::from(end.col == 0))
        }
        _ => (cursor.row, cursor.row),
    };
    let uncomment = lines[first..=last]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .all(|line| line.trim_start().starts_with('#'));
    let mut changed = false;
    for (row, line) in lines.iter_mut().enumerate().take(last + 1).skip(first) {
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
        adjust(cursor);
        if let Some(anchor) = anchor.as_deref_mut() {
            adjust(anchor);
        }
        changed = true;
    }
    changed
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
