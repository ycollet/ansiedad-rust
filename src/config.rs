use crate::expr::{Constant, Expr, parse_integer, parse_string};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub const STARTER: &str = "\
# Type below for live preview; no file is required.
# Tab: focus | Ctrl-S: save to editme.txt | Ctrl-Q: quit
# Ctrl-P: pause | Ctrl-R: reset | F5: reload a saved file
# t advances per character; b/f are colors, c is a glyph.
b = \"t >> 4\"
f = \"15\"
c = \"144\"
";

#[derive(Debug)]
pub struct Settings {
    pub b: String,
    pub f: String,
    pub c: String,
    pub w: u16,
    pub l: u16,
    pub m: u8,
    pub s: f64,
    pub fps: u16,
    pub constants: BTreeMap<String, Constant>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            b: "0".into(),
            f: "7".into(),
            c: "0".into(),
            w: 64,
            l: 16,
            m: 0,
            s: 0.0,
            fps: 30,
            constants: BTreeMap::new(),
        }
    }
}

impl Settings {
    fn parse(source: &str) -> Result<Self, String> {
        let mut settings = Self::default();
        let mut assigned = BTreeSet::new();
        for (index, line) in source.lines().enumerate() {
            settings
                .parse_line(line, &mut assigned)
                .map_err(|error| format!("line {}: {error}", index + 1))?;
        }
        Ok(settings)
    }

    fn parse_line(&mut self, line: &str, assigned: &mut BTreeSet<String>) -> Result<(), String> {
        let mut start = 0;
        let mut quote = None;
        let mut escaped = false;
        for (pos, character) in line.char_indices() {
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == delimiter {
                    quote = None;
                }
            } else {
                match character {
                    '\'' | '"' => quote = Some(character),
                    ';' => {
                        self.assign(&line[start..pos], assigned)?;
                        start = pos + 1;
                    }
                    '#' => return self.assign(&line[start..pos], assigned),
                    _ => {}
                }
            }
        }
        if quote.is_some() {
            return Err("unterminated string literal".into());
        }
        self.assign(&line[start..], assigned)
    }

    fn assign(&mut self, statement: &str, assigned: &mut BTreeSet<String>) -> Result<(), String> {
        let statement = statement.trim();
        if statement.is_empty() {
            return Ok(());
        }
        let (name, value) = statement.split_once('=').ok_or("expected name = value")?;
        let name = name.trim();
        let value = value.trim();
        let mut bytes = name.bytes();
        if !matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || matches!(name, "t" | "ord")
        {
            return Err(format!("invalid or reserved name {name:?}"));
        }
        if !assigned.insert(name.into()) {
            return Err(format!("duplicate assignment to {name}"));
        }
        let string = || {
            let mut pos = 0;
            let text = parse_string(value, &mut pos).map_err(|e| format!("{name}: {e}"))?;
            if !value[pos..].trim().is_empty() {
                return Err(format!("{name}: unexpected text after string"));
            }
            Ok(text)
        };
        let integer = || parse_integer(value).map_err(|e| format!("{name}: {e}"));
        let dimension =
            || u16::try_from(integer()?).map_err(|_| format!("{name}: integer outside 0..65535"));
        match name {
            "b" => self.b = string()?,
            "f" => self.f = string()?,
            "c" => self.c = string()?,
            "w" => self.w = dimension()?,
            "l" => self.l = dimension()?,
            "fps" => self.fps = dimension()?,
            "m" => {
                self.m =
                    u8::try_from(integer()?).map_err(|_| "m must be 0 (redraw) or 1 (scroll)")?;
            }
            "s" => self.s = value.parse().map_err(|_| "s must be a number")?,
            _ => {
                let constant = if value.starts_with(['\'', '"']) {
                    Constant::Text(string()?)
                } else {
                    Constant::Integer(integer()?)
                };
                self.constants.insert(name.into(), constant);
            }
        }
        Ok(())
    }
}

pub struct Config {
    pub settings: Settings,
    background: Expr,
    foreground: Expr,
    character: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub background: u8,
    pub foreground: u8,
    pub character: char,
}

impl Config {
    pub fn parse(source: &str) -> Result<Self, String> {
        let settings = Settings::parse(source)?;
        if !(1..=512).contains(&settings.w) || !(1..=512).contains(&settings.l) {
            return Err("w and l must be between 1 and 512".into());
        }
        if settings.m > 1 {
            return Err("m must be 0 (redraw) or 1 (scroll)".into());
        }
        if !settings.s.is_finite() || !(0.0..=1.0).contains(&settings.s) {
            return Err("s must be a finite number between 0 and 1 seconds".into());
        }
        if !(1..=240).contains(&settings.fps) {
            return Err("fps must be between 1 and 240".into());
        }
        let parse = |name, source: &str| {
            Expr::parse(source, &settings.constants).map_err(|e| format!("{name}: {e}"))
        };
        let background = parse("b", &settings.b)?;
        let foreground = parse("f", &settings.f)?;
        let character = parse("c", &settings.c)?;
        Ok(Self {
            settings,
            background,
            foreground,
            character,
        })
    }

    pub fn cell(&self, t: i64) -> Result<Cell, String> {
        let eval = |name, expr: &Expr| expr.eval(t).map_err(|e| format!("{name} at t={t}: {e}"));
        Ok(Cell {
            background: eval("b", &self.background)?.rem_euclid(256) as u8,
            foreground: eval("f", &self.foreground)?.rem_euclid(256) as u8,
            character: glyph(eval("c", &self.character)?),
        })
    }
}

// Original unicodes.py: ASCII 32..126, a repeated '~', then CP437 128..255,
// with the final non-breaking space replaced by a visible square.
fn glyph(value: i64) -> char {
    const EXTENDED: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■■";
    let index = value.rem_euclid(224) as u8;
    match index {
        0..=94 => char::from(index + 32),
        95 => '~',
        _ => EXTENDED.chars().nth(usize::from(index - 96)).unwrap(),
    }
}

pub struct LiveConfig {
    pub config: Config,
    last_source: String,
    pub error: Option<String>,
}

impl LiveConfig {
    pub fn source(&self) -> &str {
        &self.last_source
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let source = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_source(&source)
    }

    pub fn from_source(source: &str) -> Result<Self, String> {
        Ok(Self {
            config: Config::parse(source)?,
            last_source: source.into(),
            error: None,
        })
    }

    pub fn reload(&mut self, path: &Path) -> bool {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(e) => {
                self.error = Some(format!("{}: {e}", path.display()));
                return false;
            }
        };
        if source == self.last_source {
            self.error = None;
            return false;
        }
        match Config::parse(&source) {
            Ok(config) => {
                self.config = config;
                self.last_source = source;
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_keeps_last_good_config_and_recovers_after_errors() {
        let path = std::env::temp_dir().join(format!(
            "ansiedad-reload-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let write = |source| fs::write(&path, source).unwrap();
        write("b='1'");
        let mut live = LiveConfig::load(&path).unwrap();
        write("b='t+'");
        assert!(!live.reload(&path));
        assert!(live.error.is_some());
        assert_eq!(live.config.cell(0).unwrap().background, 1);
        write("b='1'");
        assert!(!live.reload(&path));
        assert!(live.error.is_none());
        fs::remove_file(&path).unwrap();
        assert!(!live.reload(&path));
        assert!(live.error.is_some());
        write("b='2'");
        assert!(live.reload(&path));
        assert!(live.error.is_none());
        assert_eq!(live.config.cell(0).unwrap().background, 2);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn example_and_original_glyph_mapping() {
        let config = Config::parse(STARTER).unwrap();
        assert_eq!(config.cell(32).unwrap().background, 2);
        assert_eq!(glyph(0), ' ');
        assert_eq!(glyph(95), '~');
        assert_eq!(glyph(96), 'Ç');
        assert_eq!(glyph(144), '░');
        assert_eq!(glyph(187), '█');
        assert_eq!(glyph(-1), '■');
        assert_eq!(glyph(224), ' ');
        assert_eq!(
            Config::parse("b='-1'\nf='257'\nc='144'")
                .unwrap()
                .cell(0)
                .unwrap(),
            Cell {
                background: 255,
                foreground: 1,
                character: '░'
            }
        );
    }

    #[test]
    fn rejects_invalid_settings_and_reports_runtime_errors() {
        for source in ["w=0", "l=513", "m=2", "s=nan", "s=-1.0", "fps=0", "b='t+'"] {
            assert!(Config::parse(source).is_err(), "{source}");
        }
        let config = Config::parse("b='1/(t-2)'").unwrap();
        assert!(config.cell(2).unwrap_err().contains("b at t=2"));
    }

    #[test]
    fn bitmap_and_scrolling_text_example() {
        let config = Config::parse(
            r#"
BYE = "ANSIedad...."; BITMAP = 0b0001001101010101000100100101011100000001010101010001001101100010
b = "BITMAP >> ((t>>1)+(t>>10))%16 + (t>>7)%4*16 & 1 ^ (t>>9)%2 * t<<2"
f = "t&1"; c = "(t%512<64)*(ord(BYE[t%12])-32)"
"#,
        )
        .unwrap();
        let bitmap = 0x1355_1257_0155_1362_i64;
        let text: Vec<char> = "ANSIedad....".chars().collect();
        for t in 0..4096_i64 {
            let shift = ((t >> 1) + (t >> 10)) % 16 + (t >> 7) % 4 * 16;
            let background = ((bitmap >> shift) & 1) ^ (((t >> 9) % 2 * t) << 2);
            assert_eq!(
                config.cell(t).unwrap(),
                Cell {
                    background: background.rem_euclid(256) as u8,
                    foreground: (t & 1) as u8,
                    character: if t % 512 < 64 {
                        text[(t % 12) as usize]
                    } else {
                        ' '
                    },
                },
                "t={t}"
            );
        }
    }

    #[test]
    fn constants_are_typed_and_errors_include_expression_context() {
        for source in [
            "t=1",
            "ord=1",
            "bad-name=1",
            "=1",
            "value=1.5",
            "value=true",
            "b='missing'",
            "b='TEXT'\nTEXT='abc'",
            "c='ord(N)'\nN=65",
        ] {
            assert!(Config::parse(source).is_err(), "{source}");
        }
        let config = Config::parse("c='ord(TEXT[t])'\nTEXT='abc'").unwrap();
        let error = config.cell(3).unwrap_err();
        assert!(error.contains("c at t=3"), "{error}");
        assert!(error.contains("string index out of range"), "{error}");
    }

    #[test]
    fn separators_comments_and_escapes_respect_quoted_strings() {
        let config = Config::parse(
            r##"
# b="invalid"; ignored=123
; b = "ord(';')"; f = 'ord("#")'; ; # ignored=456
TEXT = "é;#\"'\\"; c = "ord(TEXT[t])-32" # keep Unicode characters intact
w=0x40; l=0b10000; m=0; s=0.01; fps=60;
"##,
        )
        .unwrap();
        assert_eq!(config.settings.w, 64);
        assert_eq!(config.settings.l, 16);
        assert_eq!(config.settings.s, 0.01);
        assert_eq!(config.settings.fps, 60);
        for (t, character) in [';', '#', '"', '\'', '\\'].into_iter().enumerate() {
            let cell = config.cell(t as i64 + 1).unwrap();
            assert_eq!(cell.background, b';');
            assert_eq!(cell.foreground, b'#');
            assert_eq!(cell.character, character);
        }
        let config = Config::parse("TEXT='a\\';#b'; c=\"ord(TEXT[1])-32\"").unwrap();
        assert_eq!(config.cell(0).unwrap().character, '\'');
        let config = Config::parse("b='t==1'; f='0'\r\nc='144' # CRLF\r\n").unwrap();
        assert_eq!(config.cell(1).unwrap().background, 1);
        assert_eq!(config.cell(1).unwrap().character, '░');
    }

    #[test]
    fn rejects_malformed_assignments_with_line_numbers() {
        for statement in [
            "b",
            "b=",
            "b=123",
            "b=='0'",
            "b='0' f='1'",
            "b='0' trailing",
            "b='unterminated; f='1'",
            "b='unterminated",
            r"TEXT='bad\q'",
            "[constants]",
            "9name=1",
            "value=0b2",
            "value=0x",
            "value=9223372036854775808",
            "value=+-1",
            "w='64'",
            "fps=1.5",
            "s='0.1'",
            "value=1; value=2",
            "b='0'; b='1'",
        ] {
            let error = Config::parse(&format!("# first line\n{statement}"))
                .err()
                .unwrap_or_else(|| panic!("accepted {statement}"));
            assert!(error.contains("line 2:"), "{statement}: {error}");
        }
        let config = Config::parse(
            "c='ord(TEXT)-32'; b='LOW&255'; f='HIGH&255'\n\
             TEXT='A'; LOW=-0x8000_0000_0000_0000; HIGH=+0x7fff_ffff_ffff_ffff",
        )
        .unwrap();
        assert_eq!(
            config.cell(0).unwrap(),
            Cell {
                background: 0,
                foreground: 255,
                character: 'A',
            }
        );
    }
}
