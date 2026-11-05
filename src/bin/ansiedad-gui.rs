use ansiedad::canvas::{self, CanvasConfig, CanvasFrame, ansi_color};
use ansiedad::editing::{self, Navigation, Position};
use ansiedad::video::{self, Recorder, VideoJob};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, TextureOptions, Vec2};
use egui::text::{CCursor, CCursorRange};
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
  --export MP4  Export a video without opening a window (requires FFmpeg)
  --seconds N   Export duration in seconds (default: 10)
  --video-fps N Export frame rate, 1..240 (default: patch fps)
  -h, --help    Show help
  -V, --version Show version

mode = 0: character grid using b/f/c
mode = 1: pixels using rgba = \"0xRRGGBBAA\"
w/l: columns/rows or pixel width/height, each 1..512
x/y: zero-based coordinates; frame: frame counter; t: sample counter

Editor: Shift + navigation selects; Ctrl-Shift + arrows selects words/paragraphs.
        Ctrl-X/C/V: cut/copy/paste | Alt-M: toggle comments
        Native Cmd clipboard shortcuts remain available on macOS.
Ctrl-S: save | Ctrl-P: pause/resume | Ctrl-R: reset | Ctrl-Q: quit | F5: reload";

#[derive(Default)]
struct Args {
    path: Option<PathBuf>,
    check: bool,
    init: bool,
    frames: Option<u64>,
    export: Option<PathBuf>,
    seconds: Option<f64>,
    video_fps: Option<u16>,
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
            "--export" => {
                if result.export.is_some() {
                    return Err("--export can only be supplied once".into());
                }
                result.export = Some(args.next().ok_or("--export requires an MP4 path")?.into());
            }
            "--seconds" => {
                let seconds = args
                    .next()
                    .ok_or("--seconds requires a positive number")?
                    .parse::<f64>()
                    .map_err(|_| "--seconds requires a positive number")?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err("--seconds requires a finite positive number".into());
                }
                result.seconds = Some(seconds);
            }
            "--video-fps" => {
                let fps = args
                    .next()
                    .ok_or("--video-fps requires an integer from 1 to 240")?
                    .parse::<u16>()
                    .map_err(|_| "--video-fps requires an integer from 1 to 240")?;
                if !(1..=240).contains(&fps) {
                    return Err("--video-fps must be from 1 to 240".into());
                }
                result.video_fps = Some(fps);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if result.path.is_none() => result.path = Some(arg.into()),
            _ => return Err("only one patch path is accepted".into()),
        }
    }
    if result.export.is_some() {
        if result.init || result.check || result.frames.is_some() {
            return Err("--export cannot be combined with --init, --check, or --frames".into());
        }
    } else if result.seconds.is_some() || result.video_fps.is_some() {
        return Err("--seconds and --video-fps require --export".into());
    }
    Ok(Some(result))
}

fn patch_editor_id() -> egui::Id {
    egui::Id::unique("patch-editor")
}

fn control_navigation(key: egui::Key, modifiers: egui::Modifiers) -> Option<Navigation> {
    if !modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
        return None;
    }
    Some(match key {
        egui::Key::ArrowLeft => Navigation::WordLeft,
        egui::Key::ArrowRight => Navigation::WordRight,
        egui::Key::ArrowUp => Navigation::ParagraphUp,
        egui::Key::ArrowDown => Navigation::ParagraphDown,
        egui::Key::Home => Navigation::Start,
        egui::Key::End => Navigation::End,
        _ => return None,
    })
}

fn move_selection(source: &str, range: &mut CCursorRange, navigation: Navigation, selecting: bool) {
    let position = if !selecting && !range.is_empty() {
        let [start, end] = range.sorted_cursors();
        match navigation {
            Navigation::WordLeft => Position::from_index(source, start.index.0),
            Navigation::WordRight => Position::from_index(source, end.index.0),
            _ => editing::navigate(
                source,
                Position::from_index(source, range.primary.index.0),
                navigation,
            ),
        }
    } else {
        editing::navigate(
            source,
            Position::from_index(source, range.primary.index.0),
            navigation,
        )
    };
    range.primary = CCursor::new(position.index(source));
    if !selecting {
        range.secondary = range.primary;
    }
    range.h_pos = None;
}

fn custom_editor_key(event: &egui::Event) -> bool {
    let egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        ..
    } = event
    else {
        return false;
    };
    control_navigation(*key, *modifiers).is_some()
        || *key == egui::Key::M && *modifiers == egui::Modifiers::ALT
        || modifiers.ctrl
            && !modifiers.alt
            && !modifiers.mac_cmd
            && matches!(
                key,
                egui::Key::S | egui::Key::P | egui::Key::R | egui::Key::Q
            )
        || *key == egui::Key::S && modifiers.command && !modifiers.alt && !modifiers.shift
        || *key == egui::Key::F5
        || cfg!(target_os = "macos")
            && modifiers.ctrl
            && !modifiers.alt
            && !modifiers.mac_cmd
            && !modifiers.shift
            && matches!(key, egui::Key::C | egui::Key::X | egui::Key::V)
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
    clipboard: Option<arboard::Clipboard>,
    reset_editor: bool,
    deferred_events: Vec<egui::Event>,
    video_path: String,
    video_seconds: f64,
    video_fps: u16,
    video_job: Option<VideoJob>,
    recorder: Option<Recorder>,
    video_error: Option<String>,
}

impl App {
    fn new(
        source: String,
        path: Option<PathBuf>,
        config: CanvasConfig,
        frame_limit: Option<u64>,
        outcome: Arc<Mutex<Option<String>>>,
    ) -> Self {
        let video_fps = config.settings().fps;
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
            clipboard: None,
            reset_editor: true,
            deferred_events: Vec::new(),
            video_path: "ansiedad.mp4".into(),
            video_seconds: 10.0,
            video_fps,
            video_job: None,
            recorder: None,
            video_error: None,
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
        self.reset_editor = true;
        Ok(())
    }

    fn request_open(&mut self, path: PathBuf) {
        if self.modified() {
            self.pending = Some(Pending::Open(path));
        } else if let Err(error) = self.open_file(path) {
            self.notice = Some(format!("ERROR: {error}"));
        }
    }

    fn shortcuts(&mut self, context: &egui::Context) {
        let pressed = |modifiers, key| context.input_mut(|input| input.consume_key(modifiers, key));
        if (pressed(egui::Modifiers::CTRL, egui::Key::S)
            || pressed(egui::Modifiers::COMMAND, egui::Key::S))
            && let Err(error) = self.save_file()
        {
            self.notice = Some(format!("ERROR: {error}"));
        }
        if pressed(egui::Modifiers::CTRL, egui::Key::P) {
            self.paused = !self.paused;
            self.next_frame = Instant::now();
        }
        if pressed(egui::Modifiers::CTRL, egui::Key::R) {
            self.t = 0;
            self.frame = 0;
            self.dirty = true;
        }
        if pressed(egui::Modifiers::CTRL, egui::Key::Q) {
            if self.video_job.is_some() {
                self.notice =
                    Some("ERROR: Stop and save or cancel the video before closing".into());
            } else if self.modified() {
                self.pending = Some(Pending::Close);
            } else {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if pressed(egui::Modifiers::NONE, egui::Key::F5) {
            if let Some(path) = self.path.clone() {
                self.request_open(path);
            } else {
                self.notice = Some("No file loaded; save the patch first".into());
            }
        }
    }

    fn patch_editor(&mut self, ui: &mut egui::Ui) -> bool {
        let id = patch_editor_id();
        let context = ui.ctx().clone();
        if self.reset_editor {
            egui::TextEdit::store_state(&context, id, egui::text_edit::TextEditState::default());
            context.memory_mut(|memory| memory.request_focus(id));
            self.reset_editor = false;
        }
        let focused = context.memory(|memory| memory.has_focus(id));
        let mut state = egui::TextEdit::load_state(&context, id).unwrap_or_default();
        let mut range = state.cursor.char_range().unwrap_or_default();
        let mut moved = false;
        if focused {
            context.input_mut(|input| {
                input.events.retain(|event| {
                    if let egui::Event::Key {
                        key,
                        modifiers,
                        pressed: true,
                        ..
                    } = event
                        && let Some(navigation) = control_navigation(*key, *modifiers)
                    {
                        move_selection(&self.source, &mut range, navigation, modifiers.shift);
                        moved = true;
                        false
                    } else {
                        true
                    }
                });
            });
        }
        if moved {
            state.cursor.set_char_range(Some(range));
            state.store(&context, id);
        }
        let comment = focused
            && context.input_mut(|input| input.consume_key(egui::Modifiers::ALT, egui::Key::M));
        let mut output = egui::TextEdit::multiline(&mut self.source)
            .id(id)
            .code_editor()
            .desired_width(f32::INFINITY)
            .desired_rows(30)
            .show(ui);
        if comment {
            let range = output.state.cursor.char_range().unwrap_or_default();
            let mut undoer = output.state.undoer();
            undoer.add_undo(&(range, self.source.clone()));
            let mut cursor = Position::from_index(&self.source, range.primary.index.0);
            let mut anchor = Position::from_index(&self.source, range.secondary.index.0);
            let mut lines: Vec<String> = self.source.split('\n').map(String::from).collect();
            if editing::toggle_comments(&mut lines, &mut cursor, Some(&mut anchor)) {
                self.source = lines.join("\n");
                let range = CCursorRange::two(
                    CCursor::new(anchor.index(&self.source)),
                    CCursor::new(cursor.index(&self.source)),
                );
                undoer.add_undo(&(range, self.source.clone()));
                output.state.set_undoer(undoer);
                output.state.cursor.set_char_range(Some(range));
                output.state.clone().store(&context, id);
                output.response.mark_changed();
                moved = true;
                context.request_repaint();
            }
        }
        if moved && let Some(range) = output.state.cursor.char_range() {
            let cursor = output
                .galley
                .pos_from_cursor(range.primary)
                .translate(output.galley_pos.to_vec2());
            ui.scroll_to_rect(cursor, None);
        }
        output.response.changed()
    }

    fn clipboard_text(&mut self) -> Result<String, String> {
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|error| format!("Clipboard unavailable: {error}"))?,
            );
        }
        self.clipboard
            .as_mut()
            .expect("clipboard initialized")
            .get_text()
            .map_err(|error| format!("Could not read clipboard: {error}"))
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

impl App {
    fn video_controls(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Video export / recording").show(ui, |ui| {
            ui.add_enabled_ui(self.video_job.is_none() && self.pending.is_none(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("MP4:");
                    ui.text_edit_singleline(&mut self.video_path);
                    ui.label("Seconds:");
                    ui.add(egui::DragValue::new(&mut self.video_seconds).speed(0.1));
                    ui.label("FPS:");
                    ui.add(egui::DragValue::new(&mut self.video_fps).range(1..=240));
                });
                ui.horizontal(|ui| {
                    if ui.button("Export from frame 0").clicked() {
                        match VideoJob::offline(
                            &self.source,
                            PathBuf::from(self.video_path.trim()),
                            self.video_seconds,
                            self.video_fps,
                        ) {
                            Ok(job) => {
                                self.video_error = None;
                                self.video_job = Some(job);
                            }
                            Err(error) => self.notice = Some(format!("ERROR: {error}")),
                        }
                    }
                    if ui
                        .add_enabled(self.image.is_some(), egui::Button::new("Start recording"))
                        .clicked()
                        && let Some(image) = &self.image
                    {
                        match Recorder::start(
                            PathBuf::from(self.video_path.trim()),
                            self.video_fps,
                            image,
                        ) {
                            Ok((mut recorder, job)) => match recorder.capture(image) {
                                Ok(()) => {
                                    self.video_error = None;
                                    self.recorder = Some(recorder);
                                    self.video_job = Some(job);
                                }
                                Err(error) => {
                                    job.cancel();
                                    self.notice = Some(format!("ERROR: {error}"));
                                }
                            },
                            Err(error) => self.notice = Some(format!("ERROR: {error}")),
                        }
                    }
                    ui.small("Canvas only; FFmpeg required. Existing files are never overwritten.");
                });
            });
        });
        if let Some(job) = &self.video_job {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{}: {} frames",
                    if self.recorder.is_some() {
                        "Recording"
                    } else {
                        "Exporting / finalizing"
                    },
                    job.frames(),
                ));
                if self.recorder.is_some() && ui.button("Stop and save").clicked() {
                    if let Some(image) = &self.image
                        && let Some(recorder) = &mut self.recorder
                        && let Err(error) = recorder.capture(image)
                    {
                        self.video_error = Some(error);
                        job.cancel();
                    }
                    self.recorder = None;
                }
                if ui.button("Cancel video").clicked() {
                    job.cancel();
                    self.recorder = None;
                }
            });
        }
    }

    fn update_video(&mut self, context: &egui::Context) {
        if let Some(result) = self.video_job.as_mut().and_then(VideoJob::poll) {
            self.recorder = None;
            self.video_job = None;
            self.notice = Some(match result {
                Ok(video::Outcome::Saved(path)) => format!("Saved video {}", path.display()),
                Ok(video::Outcome::Canceled) => self.video_error.take().map_or_else(
                    || "Video canceled; no output file saved".into(),
                    |error| format!("ERROR: {error}"),
                ),
                Err(error) => format!("ERROR: {error}"),
            });
            context.request_repaint();
        }
        if let Some(recorder) = &mut self.recorder
            && let Some(image) = &self.image
            && let Err(error) = recorder.capture(image)
        {
            self.video_error = Some(error);
            if let Some(job) = &self.video_job {
                job.cancel();
            }
            self.recorder = None;
        }
        if self.video_job.is_some() {
            context.request_repaint_after(Duration::from_secs_f64(1.0 / f64::from(self.video_fps)));
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, context: &egui::Context, input: &mut egui::RawInput) {
        let focused = context.memory(|memory| memory.has_focus(patch_editor_id()));
        if focused {
            // Deferred events have already had the text generated by Alt-M removed.
            let mut suppress_alt_text = false;
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                {
                    suppress_alt_text = *key == egui::Key::M && *modifiers == egui::Modifiers::ALT;
                } else if matches!(event, egui::Event::Text(_)) && suppress_alt_text {
                    suppress_alt_text = false;
                    return false;
                }
                true
            });
        }
        self.deferred_events.append(&mut input.events);
        input.events = std::mem::take(&mut self.deferred_events);
        if !focused {
            return;
        }
        // Keep custom shortcuts in order with native text edits from the same input batch.
        if let Some(index) = input.events.iter().position(custom_editor_key) {
            let end = if index == 0 { 1 } else { index };
            self.deferred_events = input.events.split_off(end);
            if !self.deferred_events.is_empty() {
                context.request_repaint();
            }
        }
        let mut events = Vec::with_capacity(input.events.len());
        for event in std::mem::take(&mut input.events) {
            if let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } = &event
                && cfg!(target_os = "macos")
                && modifiers.ctrl
                && !modifiers.alt
                && !modifiers.mac_cmd
                && !modifiers.shift
            {
                match key {
                    egui::Key::C => {
                        events.push(egui::Event::Copy);
                        continue;
                    }
                    egui::Key::X => {
                        events.push(egui::Event::Cut);
                        continue;
                    }
                    egui::Key::V => {
                        match self.clipboard_text() {
                            Ok(text) => events.push(egui::Event::Paste(text)),
                            Err(error) => self.notice = Some(format!("ERROR: {error}")),
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            events.push(event);
        }
        input.events = events;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl App {
    fn show(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        if context.input(|input| input.viewport().close_requested()) {
            if self.video_job.is_some() {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.notice =
                    Some("ERROR: Stop and save or cancel the video before closing".into());
            } else if self.modified() {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Close);
            }
        }
        self.shortcuts(&context);
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
                self.request_open(path);
            }
            if ui.button("Save").clicked()
                && let Err(error) = self.save_file()
            {
                self.notice = Some(format!("ERROR: {error}"));
            }
        });
        self.video_controls(ui);
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
        let error = self
            .notice
            .as_ref()
            .filter(|message| message.starts_with("ERROR:"))
            .or(self.parse_error.as_ref())
            .or(self.runtime_error.as_ref());
        let message = error.or(self.notice.as_ref()).map_or("", String::as_str);
        let text = egui::RichText::new(message);
        let text = if error.is_some() {
            text.color(Color32::LIGHT_RED)
        } else {
            text
        };
        ui.add(egui::Label::new(text).truncate())
            .on_hover_text(message);
        ui.separator();
        ui.columns(2, |columns| {
            columns[0].label("Patch (edit mode = 0 or mode = 1 to switch renderers)");
            let height = columns[0].available_height().max(100.0);
            let mut changed = false;
            egui::ScrollArea::both()
                .max_height(height)
                .show(&mut columns[0], |ui| {
                    changed = self.patch_editor(ui);
                });
            if changed {
                self.preview();
            }
            self.tick(&context);
            columns[1].label("Canvas");
            self.draw_canvas(&mut columns[1]);
        });
        self.update_video(&context);
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
    if let Some(path) = args.export {
        let path = video::export(
            &source,
            &path,
            args.seconds.unwrap_or(10.0),
            args.video_fps.unwrap_or(config.settings().fps),
        )?;
        println!("Saved video {}", path.display());
        return Ok(());
    }
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
        viewport: egui::ViewportBuilder::default()
            .with_app_id("ansiedad-gui")
            .with_inner_size([1100.0, 720.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!(
                "../../assets/ansiedad.png"
            ))?),
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

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn editor_frame(
        app: &mut App,
        context: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut input = egui::RawInput {
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(app, context, &mut input);
        let mut output = context.run_ui(input, |ui| {
            app.shortcuts(ui.ctx());
            if app.patch_editor(ui) {
                app.preview();
            }
        });
        output.textures_delta.clear();
        output
    }

    fn select(context: &egui::Context, anchor: usize, cursor: usize) {
        let mut state = egui::TextEdit::load_state(context, patch_editor_id()).unwrap();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(anchor),
            CCursor::new(cursor),
        )));
        state.store(context, patch_editor_id());
    }

    fn selected(app: &App, context: &egui::Context) -> String {
        egui::TextEdit::load_state(context, patch_editor_id())
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
            .slice_str(&app.source)
            .to_owned()
    }

    #[test]
    fn typing_an_incomplete_assignment_keeps_focus_when_errors_appear_and_clear() {
        let context = egui::Context::default();
        let mut app = app("b='t'");
        app.paused = true;
        let frame = |app: &mut App, events| {
            let mut input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 768.0))),
                ..Default::default()
            };
            eframe::App::raw_input_hook(app, &context, &mut input);
            let mut output = context.run_ui(input, |ui| app.show(ui));
            output.textures_delta.clear();
            assert!(context.memory(|memory| memory.has_focus(patch_editor_id())));
            context.read_response(patch_editor_id()).unwrap().rect
        };
        let editor_rect = frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text("\nNEW".into())]);
        assert!(
            app.parse_error
                .as_deref()
                .unwrap()
                .starts_with("line 2, column 4:")
        );
        assert_eq!(frame(&mut app, vec![]), editor_rect);
        app.notice = Some(format!(
            "ERROR: {}",
            "Long diagnostic message. ".repeat(100)
        ));
        assert_eq!(frame(&mut app, vec![]), editor_rect);
        frame(&mut app, vec![egui::Event::Text(" = 2".into())]);
        assert_eq!(app.source, "b='t'\nNEW = 2");
        assert!(app.parse_error.is_none());
        assert_eq!(frame(&mut app, vec![]), editor_rect);
        frame(&mut app, vec![egui::Event::Text("\nf='15'".into())]);
        assert_eq!(app.source, "b='t'\nNEW = 2\nf='15'");
        assert!(app.parse_error.is_none());
    }

    #[test]
    fn editor_selection_supports_shift_and_control_shift_navigation() {
        let context = egui::Context::default();
        let mut app = app("TEXT='alpha 世界'; c='TEXT[0]'\n\n# last");
        editor_frame(&mut app, &context, vec![]);
        select(&context, 0, 0);
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::ArrowRight, egui::Modifiers::SHIFT)],
        );
        assert_eq!(selected(&app, &context), "T");
        editor_frame(
            &mut app,
            &context,
            vec![key_event(
                egui::Key::ArrowRight,
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            )],
        );
        assert_eq!(selected(&app, &context), "TEXT");
        editor_frame(
            &mut app,
            &context,
            vec![key_event(
                egui::Key::ArrowDown,
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            )],
        );
        assert_eq!(
            selected(&app, &context),
            "TEXT='alpha 世界'; c='TEXT[0]'\n\n"
        );
        editor_frame(
            &mut app,
            &context,
            vec![key_event(
                egui::Key::End,
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            )],
        );
        assert_eq!(selected(&app, &context), app.source);
        editor_frame(
            &mut app,
            &context,
            vec![key_event(
                egui::Key::Home,
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            )],
        );
        assert_eq!(selected(&app, &context), "");
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::End, egui::Modifiers::CTRL)],
        );
        editor_frame(
            &mut app,
            &context,
            vec![key_event(
                egui::Key::ArrowUp,
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            )],
        );
        assert_eq!(selected(&app, &context), "# last");
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::ArrowRight, egui::Modifiers::CTRL)],
        );
        assert_eq!(selected(&app, &context), "");
    }

    #[test]
    fn comment_shortcut_preserves_unicode_selection_and_supports_native_undo_redo() {
        let context = egui::Context::default();
        let source = "  c=\"'é'\"\n\n  # existing\nf='2'";
        let mut app = app(source);
        editor_frame(&mut app, &context, vec![]);
        let end = Position { row: 3, col: 0 }.index(source);
        select(&context, end, 0);
        editor_frame(
            &mut app,
            &context,
            vec![
                key_event(egui::Key::M, egui::Modifiers::ALT),
                egui::Event::Text("µ".into()),
            ],
        );
        let commented = "  # c=\"'é'\"\n\n  # # existing\nf='2'";
        assert_eq!(app.source, commented);
        assert_eq!(
            selected(&app, &context),
            "  # c=\"'é'\"\n\n  # # existing\n"
        );
        assert!(app.parse_error.is_none());
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::Z, egui::Modifiers::COMMAND)],
        );
        assert_eq!(app.source, source);
        editor_frame(
            &mut app,
            &context,
            vec![key_event(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            )],
        );
        assert_eq!(app.source, commented);
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::M, egui::Modifiers::ALT)],
        );
        assert_eq!(app.source, source);
    }

    #[test]
    fn clipboard_events_copy_cut_and_replace_unicode_selection() {
        let context = egui::Context::default();
        let mut app = app("TEXT='é界'; c='TEXT[0]'");
        editor_frame(&mut app, &context, vec![]);
        select(&context, 6, 8);
        let copy = if cfg!(target_os = "macos") {
            key_event(egui::Key::C, egui::Modifiers::CTRL)
        } else {
            egui::Event::Copy
        };
        let output = editor_frame(&mut app, &context, vec![copy]);
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if text == "é界")
        }));
        assert_eq!(selected(&app, &context), "é界");
        let cut = if cfg!(target_os = "macos") {
            key_event(egui::Key::X, egui::Modifiers::CTRL)
        } else {
            egui::Event::Cut
        };
        editor_frame(&mut app, &context, vec![cut]);
        assert_eq!(app.source, "TEXT=''; c='TEXT[0]'");
        editor_frame(&mut app, &context, vec![egui::Event::Paste("AB".into())]);
        assert_eq!(app.source, "TEXT='AB'; c='TEXT[0]'");
        select(&context, 6, 8);
        editor_frame(&mut app, &context, vec![egui::Event::Paste("Ж".into())]);
        assert_eq!(app.source, "TEXT='Ж'; c='TEXT[0]'");
    }

    #[test]
    fn custom_shortcuts_remain_ordered_with_native_typing_in_one_batch() {
        let context = egui::Context::default();
        let mut app = app("c=\"'E'\"");
        editor_frame(&mut app, &context, vec![]);
        select(&context, 0, 0);
        editor_frame(
            &mut app,
            &context,
            vec![
                egui::Event::Text("X".into()),
                key_event(egui::Key::M, egui::Modifiers::ALT),
                egui::Event::Text("µ".into()),
                egui::Event::Text("Y".into()),
            ],
        );
        assert_eq!(app.source, "Xc=\"'E'\"");
        editor_frame(&mut app, &context, vec![]);
        assert_eq!(app.source, "# Xc=\"'E'\"");
        editor_frame(&mut app, &context, vec![]);
        assert_eq!(app.source, "# XYc=\"'E'\"");
    }

    #[test]
    fn editor_shortcuts_leave_other_text_fields_alone() {
        let context = egui::Context::default();
        let mut app = app("b='t'");
        editor_frame(&mut app, &context, vec![]);
        let path_id = egui::Id::unique("path-test");
        context.memory_mut(|memory| memory.request_focus(path_id));
        let mut input = egui::RawInput {
            events: vec![
                key_event(egui::Key::M, egui::Modifiers::ALT),
                egui::Event::Text("µ".into()),
            ],
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &context, &mut input);
        let mut output = context.run_ui(input, |ui| {
            egui::TextEdit::singleline(&mut app.path_input)
                .id(path_id)
                .show(ui);
            app.patch_editor(ui);
        });
        output.textures_delta.clear();
        assert_eq!(app.source, "b='t'");
        assert!(app.path_input.contains('µ'));
    }

    #[test]
    fn save_shortcuts_follow_same_batch_edits_and_reload_uses_the_loaded_path() {
        let path = env::temp_dir().join(format!(
            "ansiedad-gui-shortcuts-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let context = egui::Context::default();
        let mut app = app("b='t'");
        app.path_input = path.to_string_lossy().into_owned();
        app.save_file().unwrap();
        editor_frame(&mut app, &context, vec![]);
        for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::COMMAND] {
            select(&context, 0, 0);
            let before = app.source.clone();
            editor_frame(
                &mut app,
                &context,
                vec![
                    egui::Event::Text("# ".into()),
                    key_event(egui::Key::S, modifiers),
                ],
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), before);
            editor_frame(&mut app, &context, vec![]);
            assert_eq!(fs::read_to_string(&path).unwrap(), format!("# {before}"));
        }
        fs::write(&path, "f='3'").unwrap();
        app.path_input = "not-the-loaded-file.txt".into();
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::F5, egui::Modifiers::NONE)],
        );
        assert_eq!(app.source, "f='3'");
        assert_eq!(app.path.as_ref(), Some(&path));
        app.source = "f='4'".into();
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::F5, egui::Modifiers::NONE)],
        );
        assert_eq!(app.source, "f='4'");
        assert!(matches!(&app.pending, Some(Pending::Open(pending)) if pending == &path));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn global_shortcuts_pause_reset_and_confirm_unsaved_quit() {
        let context = egui::Context::default();
        let mut app = app("b='t'");
        editor_frame(&mut app, &context, vec![]);
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::P, egui::Modifiers::CTRL)],
        );
        assert!(app.paused);
        app.t = 123;
        app.frame = 5;
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::R, egui::Modifiers::CTRL)],
        );
        assert_eq!((app.t, app.frame), (0, 0));
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::F5, egui::Modifiers::NONE)],
        );
        assert!(app.notice.as_ref().unwrap().contains("No file loaded"));
        app.source.push_str("\nf='15'");
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::Q, egui::Modifiers::CTRL)],
        );
        assert!(matches!(app.pending, Some(Pending::Close)));
        editor_frame(
            &mut app,
            &context,
            vec![key_event(egui::Key::M, egui::Modifiers::ALT)],
        );
        assert!(app.paused);
        assert_eq!(app.source, "# b='t'\nf='15'");
        assert!(matches!(app.pending, Some(Pending::Close)));
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
