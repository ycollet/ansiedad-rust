use ansiedad::canvas::{self, CanvasConfig, CanvasFrame, ansi_color};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, TextureOptions, Vec2};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const HELP: &str = "\
ANSIedad GUI - live-coded characters and RGBA pixels

Usage: ansiedad-gui [OPTIONS] [PATCH]

Without PATCH, open a built-in pixel animation. The console app is unchanged.
  --check       Validate the patch without opening a window
  --init        Create an example (default: canvas.txt; never overwrite)
  --frames N    Close after N frames
  -h, --help    Show help
  -V, --version Show version

mode = 0: character grid using b/f/c
mode = 1: pixels using rgba = \"0xRRGGBBAA\"
w/l: columns/rows or pixel width/height, each 1..512
x/y: zero-based coordinates; frame: frame counter; t: sample counter";

#[derive(Default)]
struct Args {
    path: Option<PathBuf>,
    check: bool,
    init: bool,
    frames: Option<u64>,
}

fn args() -> Result<Option<Args>, String> {
    let mut args = env::args().skip(1);
    let mut result = Args::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("ansiedad-gui {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--check" => result.check = true,
            "--init" => result.init = true,
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
            _ if result.path.is_none() => result.path = Some(arg.into()),
            _ => return Err("only one patch path is accepted".into()),
        }
    }
    Ok(Some(result))
}

enum Pending {
    Open(PathBuf),
    Close,
}

struct App {
    source: String,
    saved: String,
    path: Option<PathBuf>,
    path_input: String,
    config: CanvasConfig,
    image: Option<CanvasFrame>,
    texture: Option<egui::TextureHandle>,
    t: i64,
    frame: i64,
    paused: bool,
    dirty: bool,
    checkerboard: bool,
    next_frame: Instant,
    parse_error: Option<String>,
    runtime_error: Option<String>,
    notice: Option<String>,
    pending: Option<Pending>,
    frame_limit: Option<u64>,
    rendered: u64,
    outcome: Arc<Mutex<Option<String>>>,
}

impl App {
    fn new(
        source: String,
        path: Option<PathBuf>,
        config: CanvasConfig,
        frame_limit: Option<u64>,
        outcome: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            saved: source.clone(),
            source,
            path_input: path
                .as_deref()
                .unwrap_or_else(|| Path::new("canvas.txt"))
                .to_string_lossy()
                .into_owned(),
            path,
            config,
            image: None,
            texture: None,
            t: 0,
            frame: 0,
            paused: false,
            dirty: true,
            checkerboard: true,
            next_frame: Instant::now(),
            parse_error: None,
            runtime_error: None,
            notice: None,
            pending: None,
            frame_limit,
            rendered: 0,
            outcome,
        }
    }

    fn modified(&self) -> bool {
        self.source != self.saved
    }

    fn preview(&mut self) {
        self.notice = None;
        match CanvasConfig::parse(&self.source) {
            Ok(config) => {
                self.config = config;
                self.parse_error = None;
                self.runtime_error = None;
                self.dirty = true;
            }
            Err(error) => self.parse_error = Some(error),
        }
    }

    fn save_file(&mut self) -> Result<(), String> {
        CanvasConfig::parse(&self.source)?;
        let path = PathBuf::from(self.path_input.trim());
        if path.as_os_str().is_empty() {
            return Err("Enter a patch path before saving".into());
        }
        if self.path.as_ref() == Some(&path) {
            let disk = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if disk != self.saved {
                return Err("File changed on disk; open/reload it before saving".into());
            }
            fs::write(&path, &self.source)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        } else {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| {
                    format!(
                        "{}: {error} (new paths are never overwritten)",
                        path.display()
                    )
                })?;
            file.write_all(self.source.as_bytes())
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
        self.notice = Some(format!("Saved {}", path.display()));
        self.path = Some(path);
        self.saved.clone_from(&self.source);
        Ok(())
    }

    fn open_file(&mut self, path: PathBuf) -> Result<(), String> {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let config = CanvasConfig::parse(&source)?;
        self.saved.clone_from(&source);
        self.source = source;
        self.path_input = path.to_string_lossy().into_owned();
        self.path = Some(path);
        self.config = config;
        self.parse_error = None;
        self.runtime_error = None;
        self.notice = Some("Opened patch".into());
        self.dirty = true;
        Ok(())
    }

    fn tick(&mut self, context: &egui::Context) {
        let now = Instant::now();
        if self.dirty || !self.paused && now >= self.next_frame {
            match self.config.render(self.t, self.frame) {
                Ok(image) => {
                    if let CanvasFrame::Pixels { size, pixels } = &image {
                        let colors =
                            egui::ColorImage::from_rgba_unmultiplied(*size, pixels.as_flattened());
                        if let Some(texture) = &mut self.texture {
                            texture.set(colors, TextureOptions::NEAREST);
                        } else {
                            self.texture = Some(context.load_texture(
                                "canvas",
                                colors,
                                TextureOptions::NEAREST,
                            ));
                        }
                    } else {
                        self.texture = None;
                    }
                    self.image = Some(image);
                    self.runtime_error = None;
                }
                Err(error) => self.runtime_error = Some(error),
            }
            if !self.paused {
                let settings = self.config.settings();
                self.t = self
                    .t
                    .wrapping_add(i64::from(settings.w) * i64::from(settings.l));
                self.frame = self.frame.wrapping_add(1);
            }
            self.rendered += 1;
            self.dirty = false;
            let settings = self.config.settings();
            let seconds = (1.0 / f64::from(settings.fps)).max(settings.s * f64::from(settings.l));
            self.next_frame = now + Duration::from_secs_f64(seconds);
            if self.frame_limit.is_some_and(|limit| self.rendered >= limit) {
                *self.outcome.lock().expect("frame outcome lock poisoned") =
                    self.runtime_error.clone();
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if !self.paused {
            context
                .request_repaint_after(self.next_frame.saturating_duration_since(Instant::now()));
        }
    }

    fn draw_canvas(&self, ui: &mut egui::Ui) {
        let Some(image) = &self.image else {
            ui.label("The preview will appear here after a valid frame.");
            return;
        };
        let [width, height] = image.size();
        let character_mode = matches!(image, CanvasFrame::Characters { .. });
        let cell = if character_mode {
            Vec2::new(8.0, 16.0)
        } else {
            Vec2::splat(1.0)
        };
        let base = Vec2::new(width as f32 * cell.x, height as f32 * cell.y);
        let available = ui.available_size().max(Vec2::splat(1.0));
        let scale = (available.x / base.x).min(available.y / base.y);
        let scale = if !character_mode && scale >= 1.0 {
            scale.floor()
        } else {
            scale
        };
        let (rect, _) = ui.allocate_exact_size(base * scale, Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        match image {
            CanvasFrame::Pixels { .. } => {
                if self.checkerboard {
                    let tile = 12.0;
                    for y in 0..(rect.height() / tile).ceil() as usize {
                        for x in 0..(rect.width() / tile).ceil() as usize {
                            let square = Rect::from_min_size(
                                rect.min + Vec2::new(x as f32 * tile, y as f32 * tile),
                                Vec2::splat(tile),
                            )
                            .intersect(rect);
                            painter.rect_filled(
                                square,
                                0.0,
                                Color32::from_gray(if (x + y) % 2 == 0 { 48 } else { 80 }),
                            );
                        }
                    }
                }
                if let Some(texture) = &self.texture {
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
            CanvasFrame::Characters { cells, .. } => {
                let cell = cell * scale;
                let font = FontId::monospace(14.0 * scale);
                for (index, value) in cells.iter().enumerate() {
                    let min = rect.min
                        + Vec2::new(
                            (index % width) as f32 * cell.x,
                            (index / width) as f32 * cell.y,
                        );
                    let cell = Rect::from_min_size(min, cell);
                    let [r, g, b] = ansi_color(value.background);
                    painter.rect_filled(cell, 0.0, Color32::from_rgb(r, g, b));
                    if value.character != ' ' {
                        let [r, g, b] = ansi_color(value.foreground);
                        painter.text(
                            cell.center(),
                            egui::Align2::CENTER_CENTER,
                            value.character,
                            font.clone(),
                            Color32::from_rgb(r, g, b),
                        );
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        if context.input(|input| input.viewport().close_requested()) && self.modified() {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Close);
        }
        let save =
            context.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        if save && let Err(error) = self.save_file() {
            self.notice = Some(format!("ERROR: {error}"));
        }
        ui.horizontal(|ui| {
            ui.heading("ANSIedad GUI");
            if ui
                .button(if self.paused { "Resume" } else { "Pause" })
                .clicked()
            {
                self.paused = !self.paused;
                self.next_frame = Instant::now();
            }
            if ui.button("Reset time").clicked() {
                self.t = 0;
                self.frame = 0;
                self.dirty = true;
            }
            ui.checkbox(&mut self.checkerboard, "Transparency checkerboard");
            ui.label(format!(
                "{:?} | frame={} | t={}{}",
                self.config.mode(),
                self.frame,
                self.t,
                if self.modified() { " | unsaved" } else { "" }
            ));
        });
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.text_edit_singleline(&mut self.path_input);
            if ui.button("Open / reload").clicked() {
                let path = PathBuf::from(self.path_input.trim());
                if self.modified() {
                    self.pending = Some(Pending::Open(path));
                } else if let Err(error) = self.open_file(path) {
                    self.notice = Some(format!("ERROR: {error}"));
                }
            }
            if ui.button("Save").clicked()
                && let Err(error) = self.save_file()
            {
                self.notice = Some(format!("ERROR: {error}"));
            }
        });
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::YELLOW, "Discard unsaved changes?");
                if ui.button("Discard").clicked() {
                    match self.pending.take() {
                        Some(Pending::Open(path)) => {
                            if let Err(error) = self.open_file(path) {
                                self.notice = Some(format!("ERROR: {error}"));
                            }
                        }
                        Some(Pending::Close) => {
                            self.saved.clone_from(&self.source);
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        None => {}
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.pending = None;
                }
            });
        }
        if let Some(error) = self
            .notice
            .as_ref()
            .filter(|message| message.starts_with("ERROR:"))
            .or(self.parse_error.as_ref())
            .or(self.runtime_error.as_ref())
        {
            ui.colored_label(Color32::LIGHT_RED, error);
        } else if let Some(notice) = &self.notice {
            ui.label(notice);
        }
        ui.separator();
        ui.columns(2, |columns| {
            columns[0].label("Patch (edit mode = 0 or mode = 1 to switch renderers)");
            let height = columns[0].available_height().max(100.0);
            let mut changed = false;
            egui::ScrollArea::both()
                .max_height(height)
                .show(&mut columns[0], |ui| {
                    changed = ui
                        .add(
                            egui::TextEdit::multiline(&mut self.source)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(30),
                        )
                        .changed();
                });
            if changed {
                self.preview();
            }
            self.tick(&context);
            columns[1].label("Canvas");
            self.draw_canvas(&mut columns[1]);
        });
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    let Some(args) = args()? else {
        return Ok(());
    };
    if args.init {
        let path = args
            .path
            .as_deref()
            .unwrap_or_else(|| Path::new("canvas.txt"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(canvas::STARTER.as_bytes())?;
        println!("Created {}", path.display());
        return Ok(());
    }
    let source = match &args.path {
        Some(path) => {
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?
        }
        None => canvas::STARTER.into(),
    };
    let config = CanvasConfig::parse(&source)?;
    if args.check {
        println!(
            "Graphical patch and expressions are valid ({:?})",
            config.mode()
        );
        return Ok(());
    }
    let outcome = Arc::new(Mutex::new(None));
    let result = Arc::clone(&outcome);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "ANSIedad GUI",
        options,
        Box::new(move |_creation| {
            Ok(Box::new(App::new(
                source,
                args.path,
                config,
                args.frames,
                result,
            )))
        }),
    )?;
    if let Some(error) = outcome.lock().map_err(|error| error.to_string())?.take() {
        return Err(error.into());
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match start() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ansiedad-gui: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(source: &str) -> App {
        App::new(
            source.into(),
            None,
            CanvasConfig::parse(source).unwrap(),
            None,
            Arc::new(Mutex::new(None)),
        )
    }

    #[test]
    fn live_mode_switches_replace_the_preview_without_resetting_time() {
        let context = egui::Context::default();
        let mut app = app("w=1; l=1; c=\"'E'\"");
        app.paused = true;
        app.t = 17;
        app.frame = 5;
        app.tick(&context);
        assert!(matches!(app.image, Some(CanvasFrame::Characters { .. })));
        assert!(app.texture.is_none());
        app.source = "mode=1; w=2; l=1; rgba='0xff400080'".into();
        app.preview();
        app.tick(&context);
        assert_eq!(app.texture.as_ref().unwrap().size(), [2, 1]);
        let Some(CanvasFrame::Pixels { pixels, .. }) = &app.image else {
            panic!("expected pixels");
        };
        assert_eq!(*pixels, [[255, 64, 0, 128]; 2]);
        app.source = "mode=2".into();
        app.preview();
        app.tick(&context);
        assert!(app.parse_error.is_some());
        assert!(matches!(app.image, Some(CanvasFrame::Pixels { .. })));
        app.source = "mode=0; w=1; l=1; c=\"'X'\"".into();
        app.preview();
        app.tick(&context);
        assert!(app.texture.is_none());
        let Some(CanvasFrame::Characters { cells, .. }) = &app.image else {
            panic!("expected characters");
        };
        assert_eq!(cells[0].character, 'X');
        assert_eq!((app.t, app.frame), (17, 5));
    }

    #[test]
    fn playback_keeps_failed_frames_and_advances_time_to_recover() {
        let context = egui::Context::default();
        let mut app = app("w=1; l=1; b='255//(frame-1)'; c='144'");
        app.tick(&context);
        assert_eq!((app.t, app.frame), (1, 1));
        let CanvasFrame::Characters { cells, .. } = app.image.as_ref().unwrap() else {
            panic!("expected characters");
        };
        let previous = cells.clone();
        app.next_frame = Instant::now();
        app.tick(&context);
        assert!(app.runtime_error.as_ref().unwrap().contains("frame=1"));
        let CanvasFrame::Characters { cells, .. } = app.image.as_ref().unwrap() else {
            panic!("expected characters");
        };
        assert_eq!(*cells, previous);
        assert_eq!((app.t, app.frame), (2, 2));
        app.next_frame = Instant::now();
        app.tick(&context);
        assert!(app.runtime_error.is_none());
        app.paused = true;
        app.source = "invalid statement".into();
        app.preview();
        assert!(app.parse_error.is_some());
        assert_eq!(app.config.settings().w, 1);
        app.source = "w=2; l=1; c='144'".into();
        app.preview();
        let position = (app.t, app.frame);
        app.tick(&context);
        assert_eq!((app.t, app.frame), position);
        assert_eq!(app.image.as_ref().unwrap().size(), [2, 1]);
    }

    #[test]
    fn save_rejects_invalid_drafts_external_changes_and_new_path_overwrites() {
        let directory = env::temp_dir().join(format!(
            "ansiedad-gui-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("patch.txt");
        let mut app = app("b='t'");
        app.path_input = path.to_string_lossy().into_owned();
        app.save_file().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='t'");
        app.source = "b='t+'".into();
        assert!(app.save_file().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "b='t'");
        app.source = "b='t+1'".into();
        fs::write(&path, "b='2'").unwrap();
        assert!(app.save_file().unwrap_err().contains("changed on disk"));
        app.path = None;
        assert!(app.save_file().unwrap_err().contains("never overwritten"));
        fs::write(&path, "invalid statement").unwrap();
        assert!(app.open_file(path.clone()).is_err());
        assert_eq!(app.source, "b='t+1'");
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
