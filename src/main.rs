mod config;
mod editor;
mod expr;

use config::{Cell, LiveConfig};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen, ScrollUp, SetSize,
    },
};
use editor::Editor;
use std::{
    env,
    error::Error,
    fs::OpenOptions,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

const HELP: &str = "\
ANSIedad - live-coded ANSI bytebeat visuals

Usage: ansiedad [OPTIONS] [CONFIG]

Without CONFIG, start with a built-in example; no file is needed.
Ctrl-S creates editme.toml. Pass CONFIG explicitly to load an existing file.

  --init           Write an example CONFIG (default: editme.toml; never overwrite)
  --check          Validate CONFIG or the built-in example without a terminal
  --frames N       Exit after N frames (useful for recording/smoke tests)
  --size COLSxROWS Request a terminal size at startup (e.g. 120x40)
                  Linux/macOS: stty size shows rows columns (e.g. 40 120)
  --maximize      Fill the visual pane to fit the current console
  -h, --help       Show help
  -V, --version    Show version

Keys: Tab: switch focus | Ctrl-S: save | Ctrl-Q / Ctrl-C: quit
      Ctrl-P: pause | Ctrl-R: reset | F5: reload from disk
      In visual focus: Space: pause | r: reset | q / Esc: quit
Unsaved drafts require a second quit/reload key to discard.

Requires a UTF-8 terminal with 256-color support.";

struct Args {
    path: Option<PathBuf>,
    init: bool,
    check: bool,
    frames: Option<u64>,
    size: Option<(u16, u16)>,
    maximize: bool,
}

fn parse_size(value: &str) -> Result<(u16, u16), String> {
    let error = || {
        "--size requires COLSxROWS, with columns 1..65535 and rows 4..65535 (e.g. 120x40)"
            .to_string()
    };
    let (columns, rows) = value.split_once('x').ok_or_else(error)?;
    if !columns.bytes().all(|c| c.is_ascii_digit()) || !rows.bytes().all(|c| c.is_ascii_digit()) {
        return Err(error());
    }
    let columns = columns.parse::<u16>().map_err(|_| error())?;
    let rows = rows.parse::<u16>().map_err(|_| error())?;
    if columns == 0 || rows < 4 {
        return Err(error());
    }
    Ok((columns, rows))
}

fn args() -> Result<Option<Args>, String> {
    let mut args = env::args().skip(1);
    let mut result = Args {
        path: None,
        init: false,
        check: false,
        frames: None,
        size: None,
        maximize: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("ansiedad {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--init" => result.init = true,
            "--check" => result.check = true,
            "--maximize" => result.maximize = true,
            "--size" => {
                let value = args
                    .next()
                    .ok_or("--size requires COLSxROWS (e.g. 120x40)")?;
                result.size = Some(parse_size(&value)?);
            }
            "--frames" => {
                let count = args
                    .next()
                    .ok_or("--frames requires a positive integer")?
                    .parse::<u64>()
                    .map_err(|_| "--frames requires a positive integer")?;
                if count == 0 {
                    return Err("--frames requires a positive integer".into());
                }
                result.frames = Some(count);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if result.path.is_none() => {
                result.path = Some(arg.into());
            }
            _ => return Err("only one configuration path is accepted".into()),
        }
    }
    Ok(Some(result))
}

struct Terminal {
    original_size: Option<(u16, u16)>,
}

impl Terminal {
    fn enter(size: Option<(u16, u16)>) -> io::Result<Self> {
        let original_size = if size.is_some() {
            Some(terminal::size()?)
        } else {
            None
        };
        terminal::enable_raw_mode()?;
        let guard = Self { original_size };
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            EnableBracketedPaste,
            Hide
        )?;
        if let Some((columns, rows)) = size {
            execute!(io::stdout(), SetSize(columns, rows))?;
        }
        Ok(guard)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Attempt both operations even if output has become unavailable.
        let output = execute!(
            io::stdout(),
            Print("\x1b[r"),
            ResetColor,
            Show,
            DisableBracketedPaste,
            EnableLineWrap,
            LeaveAlternateScreen
        );
        let resize = if let Some((columns, rows)) = self.original_size {
            execute!(io::stdout(), SetSize(columns, rows))
        } else {
            Ok(())
        };
        let raw = terminal::disable_raw_mode();
        if let Err(e) = output.and(resize).and(raw) {
            eprintln!("Could not restore terminal: {e}");
        }
    }
}

fn cells(config: &config::Config, t: i64, width: u16, height: u16) -> Result<Vec<Cell>, String> {
    (0..i64::from(width) * i64::from(height))
        .map(|offset| config.cell(t.wrapping_add(offset)))
        .collect()
}

fn draw(
    out: &mut impl Write,
    frame: &[Cell],
    width: u16,
    height: u16,
    visual_height: u16,
    scroll: bool,
) -> io::Result<()> {
    let mut colors = None;
    let scroll = scroll && visual_height > 1;
    if scroll {
        // DECSTBM keeps scrolling strictly inside the visual pane.
        queue!(out, Print(format!("\x1b[1;{visual_height}r")))?;
    }
    for y in 0..height {
        if scroll {
            queue!(
                out,
                ResetColor,
                ScrollUp(1),
                MoveTo(0, visual_height - 1),
                Clear(ClearType::CurrentLine)
            )?;
            colors = None;
        } else {
            queue!(out, MoveTo(0, y))?;
        }
        for x in 0..width {
            let cell = frame[usize::from(y) * usize::from(width) + usize::from(x)];
            let next_colors = (cell.background, cell.foreground);
            if colors != Some(next_colors) {
                queue!(
                    out,
                    SetBackgroundColor(Color::AnsiValue(cell.background)),
                    SetForegroundColor(Color::AnsiValue(cell.foreground))
                )?;
                colors = Some(next_colors);
            }
            queue!(out, Print(cell.character))?;
        }
    }
    if scroll {
        queue!(out, Print("\x1b[r"))?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Layout {
    visual_height: u16,
    editor_y: u16,
    editor_height: u16,
    status_y: u16,
}

impl Layout {
    fn new(height: u16) -> Self {
        let visual_height = if height >= 4 { (height - 2) / 2 } else { 0 };
        let editor_y = if visual_height > 0 {
            visual_height + 1
        } else {
            0
        };
        Self {
            visual_height,
            editor_y,
            editor_height: height.saturating_sub(editor_y + 1),
            status_y: height.saturating_sub(1),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Quit,
    Reload,
}

fn run(mut args: Args, mut live: LiveConfig) -> Result<(), Box<dyn Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "interactive mode requires a terminal; use --check to validate a config".into(),
        );
    }
    let _terminal = Terminal::enter(args.size)?;
    let mut out = io::BufWriter::new(io::stdout());
    let mut editor = if args.path.is_some() {
        Editor::new(live.source())
    } else {
        Editor::scratch(live.source())
    };
    let mut editing = true;
    let mut confirm = None;
    let mut notice: Option<String> = None;
    let mut t = 0_i64;
    let mut paused = false;
    let mut dirty = true;
    let mut rendered = 0_u64;
    let mut runtime_error = None;
    let mut next_frame = Instant::now();
    let mut next_reload = Instant::now();
    loop {
        let now = Instant::now();
        if now >= next_reload {
            if !editor.modified()
                && let Some(path) = &args.path
                && live.reload(path)
            {
                editor = Editor::new(live.source());
                dirty = true;
                runtime_error = None;
                next_frame = now;
            }
            next_reload = now + Duration::from_millis(250);
        }
        let (screen_width, screen_height) = terminal::size()?;
        let layout = Layout::new(screen_height);
        let (width, height) = if args.maximize {
            (screen_width, layout.visual_height)
        } else {
            (
                live.config.settings.w.min(screen_width),
                live.config.settings.l.min(layout.visual_height),
            )
        };
        if (!paused && now >= next_frame || dirty) && width > 0 && height > 0 {
            match cells(&live.config, t, width, height) {
                Ok(frame) => {
                    if dirty {
                        queue!(out, ResetColor, Clear(ClearType::All))?;
                    }
                    draw(
                        &mut out,
                        &frame,
                        width,
                        height,
                        layout.visual_height,
                        live.config.settings.m == 1,
                    )?;
                    runtime_error = None;
                }
                Err(e) => runtime_error = Some(e),
            }
            if !paused {
                t = t.wrapping_add(i64::from(width) * i64::from(height));
            }
            rendered += 1;
            let seconds = (1.0 / f64::from(live.config.settings.fps))
                .max(live.config.settings.s * f64::from(height));
            next_frame = now + Duration::from_secs_f64(seconds);
        }
        dirty = false;
        let error = editor
            .error
            .as_ref()
            .or(live.error.as_ref())
            .or(runtime_error.as_ref());
        let urgent = confirm.is_some()
            || notice
                .as_ref()
                .is_some_and(|text| text.starts_with("ERROR:"));
        let message = if let Some(notice) = notice.as_ref().filter(|_| urgent) {
            notice.clone()
        } else if let Some(error) = error {
            format!("ERROR: {error}")
        } else if let Some(notice) = notice.as_ref() {
            notice.clone()
        } else if width == 0 || height == 0 {
            "Resize to at least 1 column and 4 rows | Ctrl-Q quit".into()
        } else {
            format!(
                "{} t={t} {width}x{height} | Ctrl-P pause | Ctrl-R reset | Ctrl-Q quit",
                if paused { "PAUSED" } else { "LIVE" },
            )
        };
        queue!(
            out,
            Hide,
            ResetColor,
            MoveTo(0, layout.status_y),
            Clear(ClearType::CurrentLine),
            Print(editor::window(&message, 0, usize::from(screen_width)))
        )?;
        if layout.visual_height > 0 {
            let (line, col) = editor.position();
            let title = format!(
                "EDITOR{}{} [{}] {line}:{col} | Tab focus | Ctrl-S save | F5 reload",
                if args.path.is_none() {
                    " (scratch)"
                } else {
                    ""
                },
                if editor.modified() { " *" } else { "" },
                if editing { "typing" } else { "visuals" },
            );
            queue!(
                out,
                ResetColor,
                MoveTo(0, layout.visual_height),
                Clear(ClearType::CurrentLine),
                SetForegroundColor(Color::Cyan),
                Print(editor::window(&title, 0, usize::from(screen_width))),
                ResetColor
            )?;
        }
        editor.draw(
            &mut out,
            layout.editor_y,
            layout.editor_height,
            screen_width,
            editing,
        )?;
        out.flush()?;
        if args.frames.is_some_and(|limit| rendered >= limit) {
            if let Some(error) = runtime_error {
                return Err(error.into());
            }
            return Ok(());
        }
        let wait = if paused || width == 0 || height == 0 {
            Duration::from_millis(50)
        } else {
            next_frame
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50))
        };
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    let control = key.modifiers.contains(KeyModifiers::CONTROL);
                    let quitting = control && matches!(key.code, KeyCode::Char('q' | 'c'))
                        || !editing && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'));
                    let reloading = key.code == KeyCode::F(5);
                    if reloading && args.path.is_none() {
                        confirm = None;
                        notice = Some("No file loaded; Ctrl-S creates editme.toml".into());
                        continue;
                    }
                    let action = if quitting {
                        Some(Confirm::Quit)
                    } else if reloading {
                        Some(Confirm::Reload)
                    } else {
                        None
                    };
                    if let Some(action) = action {
                        if editor.modified() && confirm != Some(action) {
                            confirm = Some(action);
                            notice = Some(
                                if action == Confirm::Quit {
                                    "Unsaved draft: Ctrl-S saves; repeat quit to discard and exit"
                                } else {
                                    "Unsaved draft: press F5 again to discard and reload from disk"
                                }
                                .into(),
                            );
                            continue;
                        }
                        if action == Confirm::Quit {
                            return Ok(());
                        }
                        match load_config(args.path.as_deref()) {
                            Ok(updated) => {
                                live = updated;
                                editor = Editor::new(live.source());
                                runtime_error = None;
                                dirty = true;
                                next_frame = Instant::now();
                                notice = Some("Reloaded from disk".into());
                            }
                            Err(error) => notice = Some(format!("ERROR: {error}")),
                        }
                        confirm = None;
                        continue;
                    }
                    confirm = None;
                    notice = None;
                    match key.code {
                        KeyCode::Tab | KeyCode::BackTab => editing = !editing,
                        KeyCode::Esc if editing => editing = false,
                        KeyCode::Char('s') if control => {
                            let path = args.path.clone().unwrap_or_else(|| "editme.toml".into());
                            notice = Some(match editor.save(&path) {
                                Ok(()) => {
                                    live.reload(&path);
                                    let saved = format!("Saved {}", path.display());
                                    args.path = Some(path);
                                    if let Some(error) = &live.error {
                                        format!("ERROR: {error}")
                                    } else {
                                        saved
                                    }
                                }
                                Err(error) => format!("ERROR: {error}"),
                            });
                        }
                        KeyCode::Char('p') if control => {
                            paused = !paused;
                            next_frame = Instant::now();
                        }
                        KeyCode::Char('r') if control || !editing => {
                            t = 0;
                            dirty = true;
                            next_frame = Instant::now();
                        }
                        KeyCode::Char(' ') if !editing => {
                            paused = !paused;
                            next_frame = Instant::now();
                        }
                        _ if editing && editor.input(key, usize::from(layout.editor_height)) => {
                            if let Some(config) = editor.preview() {
                                live.config = config;
                                live.error = None;
                                runtime_error = None;
                                dirty = true;
                                next_frame = Instant::now();
                            }
                        }
                        _ => {}
                    }
                }
                Event::Paste(text) if editing => {
                    editor.insert(&text);
                    confirm = None;
                    notice = None;
                    if let Some(config) = editor.preview() {
                        live.config = config;
                        live.error = None;
                        runtime_error = None;
                        dirty = true;
                        next_frame = Instant::now();
                    }
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
    }
}

fn load_config(path: Option<&Path>) -> Result<LiveConfig, String> {
    match path {
        Some(path) => LiveConfig::load(path),
        None => LiveConfig::from_source(config::STARTER),
    }
}

fn start() -> Result<(), Box<dyn Error>> {
    let Some(args) = args()? else { return Ok(()) };
    if args.init {
        let path = args
            .path
            .as_deref()
            .unwrap_or_else(|| Path::new("editme.toml"));
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(config::STARTER.as_bytes())?;
        println!("Created {}", path.display());
        return Ok(());
    }
    let live = load_config(args.path.as_deref())?;
    if args.check {
        println!(
            "{}: configuration and expressions are valid",
            args.path.as_deref().map_or_else(
                || "built-in example".into(),
                |path| path.display().to_string()
            )
        );
        return Ok(());
    }
    run(args, live)
}

fn main() -> ExitCode {
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ansiedad: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_advances_time_in_row_major_order_and_reports_failed_cells() {
        let config = config::Config::parse("b='t'\nc='144'").unwrap();
        let frame = cells(&config, 254, 2, 2).unwrap();
        assert_eq!(
            frame.iter().map(|cell| cell.background).collect::<Vec<_>>(),
            [254, 255, 0, 1]
        );
        let config = config::Config::parse("b='1/(t-2)'").unwrap();
        assert!(cells(&config, 0, 2, 2).unwrap_err().contains("t=2"));
    }

    #[test]
    fn drawing_scrolls_only_inside_visual_pane_and_restores_margins() {
        let config = config::Config::parse("c='144'").unwrap();
        let frame = cells(&config, 0, 2, 2).unwrap();
        let mut output = Vec::new();
        draw(&mut output, &frame, 2, 2, 10, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.starts_with("\x1b[1;10r"));
        assert!(text.ends_with("\x1b[r"));
        assert_eq!(text.matches("\x1b[10;1H").count(), 2);
        assert_eq!(text.matches("\x1b[1S").count(), 2);
        assert_eq!(text.matches('░').count(), 4);
        let mut output = Vec::new();
        draw(&mut output, &frame[..2], 2, 1, 1, true).unwrap();
        assert!(!String::from_utf8(output).unwrap().contains("\x1b[1S"));
    }

    #[test]
    fn split_layout_never_overlaps_editor_and_visuals() {
        let layout = Layout::new(24);
        assert_eq!(layout.visual_height, 11);
        assert_eq!(layout.editor_y, 12);
        assert_eq!(layout.editor_height, 11);
        assert_eq!(layout.status_y, 23);
        for height in 0..100 {
            let layout = Layout::new(height);
            assert!(layout.editor_y + layout.editor_height <= layout.status_y);
            if layout.visual_height > 0 {
                assert!(layout.visual_height < layout.editor_y);
                assert!(layout.editor_height > 0);
            }
        }
    }
}
