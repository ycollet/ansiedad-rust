use crate::expr::Expr;
use serde::Deserialize;
use std::{fs, path::Path};

pub const STARTER: &str = "\
# Type below for live preview; no file is required.
# Tab: focus | Ctrl-S: save to editme.txt | Ctrl-Q: quit
# Ctrl-P: pause | Ctrl-R: reset | F5: reload a saved file
# t advances per character; b/f are colors, c is a glyph.
b = \"t >> 4\"
f = \"15\"
c = \"144\"
";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub b: String,
    pub f: String,
    pub c: String,
    pub w: u16,
    pub l: u16,
    pub m: u8,
    pub s: f64,
    pub fps: u16,
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
        }
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
        let settings: Settings = toml::from_str(source).map_err(|e| e.to_string())?;
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
        let parse = |name, source: &str| Expr::parse(source).map_err(|e| format!("{name}: {e}"));
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
        for source in [
            "w=0",
            "l=513",
            "m=2",
            "s=nan",
            "s=-1.0",
            "fps=0",
            "background='t'",
            "b='t+'",
        ] {
            assert!(Config::parse(source).is_err(), "{source}");
        }
        let config = Config::parse("b='1/(t-2)'").unwrap();
        assert!(config.cell(2).unwrap_err().contains("b at t=2"));
    }
}
