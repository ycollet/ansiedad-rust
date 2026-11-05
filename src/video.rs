use crate::canvas::{CanvasConfig, CanvasFrame, ansi_color};
use eframe::egui::{FontDefinitions, FontFamily};
use fontdue::{Font, FontSettings};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;

pub enum Outcome {
    Saved(PathBuf),
    Canceled,
}

#[derive(Default)]
struct Control {
    canceled: AtomicBool,
    frames: AtomicU64,
}

pub struct VideoJob {
    control: Arc<Control>,
    thread: Option<JoinHandle<Result<Outcome, String>>>,
}

impl VideoJob {
    fn spawn(
        task: impl FnOnce(&Control) -> Result<Outcome, String> + Send + 'static,
    ) -> Result<Self, String> {
        let control = Arc::new(Control::default());
        let worker_control = Arc::clone(&control);
        let thread = thread::Builder::new()
            .name("ansiedad-video".into())
            .spawn(move || task(&worker_control))
            .map_err(|error| format!("Could not start video worker: {error}"))?;
        Ok(Self {
            control,
            thread: Some(thread),
        })
    }

    pub fn offline(source: &str, path: PathBuf, seconds: f64, fps: u16) -> Result<Self, String> {
        let config = CanvasConfig::parse(source)?;
        let frames = frame_count(seconds, fps)?;
        validate_path(&path)?;
        Self::spawn(move |control| export_config(config, path, frames, fps, control))
    }

    pub fn cancel(&self) {
        self.control.canceled.store(true, Ordering::Relaxed);
    }

    pub fn frames(&self) -> u64 {
        self.control.frames.load(Ordering::Relaxed)
    }

    pub fn poll(&mut self) -> Option<Result<Outcome, String>> {
        if !self.thread.as_ref()?.is_finished() {
            return None;
        }
        Some(
            self.thread
                .take()
                .expect("video thread exists")
                .join()
                .unwrap_or_else(|_| Err("Video worker panicked".into())),
        )
    }
}

impl Drop for VideoJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            match thread.join() {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => eprintln!("ansiedad video: {error}"),
                Err(_) => eprintln!("ansiedad video: video worker panicked"),
            }
        }
    }
}

struct Capture {
    image: CanvasFrame,
    repeat: u64,
}

pub struct Recorder {
    sender: SyncSender<Capture>,
    started: Instant,
    submitted: u64,
    fps: u16,
    dimensions: [usize; 2],
}

impl Recorder {
    pub fn start(path: PathBuf, fps: u16, image: &CanvasFrame) -> Result<(Self, VideoJob), String> {
        validate_fps(fps)?;
        validate_path(&path)?;
        let dimensions = dimensions(image);
        let (sender, receiver) = mpsc::sync_channel(2);
        let job = VideoJob::spawn(move |control| {
            record_frames(path, fps, dimensions, receiver, control)
        })?;
        Ok((
            Self {
                sender,
                started: Instant::now(),
                submitted: 0,
                fps,
                dimensions,
            },
            job,
        ))
    }

    pub fn capture(&mut self, image: &CanvasFrame) -> Result<(), String> {
        if dimensions(image) != self.dimensions {
            return Err("Canvas dimensions changed during recording; recording canceled".into());
        }
        let due = (self.started.elapsed().as_secs_f64() * f64::from(self.fps)).floor() as u64 + 1;
        if due <= self.submitted {
            return Ok(());
        }
        let frame = Capture {
            image: image.clone(),
            repeat: due - self.submitted,
        };
        match self.sender.try_send(frame) {
            Ok(()) => {
                self.submitted = due;
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                Err("Video encoder cannot keep up; recording canceled (no frames dropped)".into())
            }
            Err(TrySendError::Disconnected(_)) => Err("Video encoder stopped unexpectedly".into()),
        }
    }
}

pub fn export(source: &str, path: &Path, seconds: f64, fps: u16) -> Result<PathBuf, String> {
    let config = CanvasConfig::parse(source)?;
    let frames = frame_count(seconds, fps)?;
    match export_config(config, path.into(), frames, fps, &Control::default())? {
        Outcome::Saved(path) => Ok(path),
        Outcome::Canceled => Err("Video export canceled".into()),
    }
}

fn frame_count(seconds: f64, fps: u16) -> Result<u64, String> {
    validate_fps(fps)?;
    let frames = (seconds * f64::from(fps)).ceil();
    if !seconds.is_finite() || seconds <= 0.0 || frames < 1.0 || frames >= u64::MAX as f64 {
        return Err("Video duration must be a finite positive number of seconds".into());
    }
    Ok(frames as u64)
}

fn validate_fps(fps: u16) -> Result<(), String> {
    if !(1..=240).contains(&fps) {
        return Err("Video frame rate must be between 1 and 240".into());
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), String> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
    {
        return Err("Choose an output path ending in .mp4".into());
    }
    match path.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(format!(
            "{} already exists; videos are never overwritten",
            path.display()
        )),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn export_config(
    config: CanvasConfig,
    path: PathBuf,
    frames: u64,
    fps: u16,
    control: &Control,
) -> Result<Outcome, String> {
    let mut rasterizer = Rasterizer::default();
    let mut pixels = Vec::new();
    let mut t = 0_i64;
    let mut encoder = None;
    for frame in 0..frames {
        if control.canceled.load(Ordering::Relaxed) {
            return Ok(Outcome::Canceled);
        }
        let image = config.render(t, frame as i64)?;
        let size = dimensions(&image);
        let encoder = match &mut encoder {
            Some(encoder) => encoder,
            slot @ None => slot.insert(Encoder::new(path.clone(), size, fps)?),
        };
        rasterizer.render_into(&image, &mut pixels)?;
        encoder.write(&pixels)?;
        control.frames.fetch_add(1, Ordering::Relaxed);
        t = t.wrapping_add(i64::from(config.settings().w) * i64::from(config.settings().l));
    }
    if control.canceled.load(Ordering::Relaxed) {
        return Ok(Outcome::Canceled);
    }
    encoder
        .expect("positive frame count")
        .finish()
        .map(Outcome::Saved)
}

fn record_frames(
    path: PathBuf,
    fps: u16,
    size: [usize; 2],
    receiver: Receiver<Capture>,
    control: &Control,
) -> Result<Outcome, String> {
    let mut rasterizer = Rasterizer::default();
    let mut pixels = Vec::new();
    let mut encoder = Encoder::new(path, size, fps)?;
    loop {
        if control.canceled.load(Ordering::Relaxed) {
            return Ok(Outcome::Canceled);
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(capture) => {
                rasterizer.render_into(&capture.image, &mut pixels)?;
                for _ in 0..capture.repeat {
                    if control.canceled.load(Ordering::Relaxed) {
                        return Ok(Outcome::Canceled);
                    }
                    encoder.write(&pixels)?;
                    control.frames.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if control.canceled.load(Ordering::Relaxed) {
        return Ok(Outcome::Canceled);
    }
    if control.frames.load(Ordering::Relaxed) == 0 {
        return Err("No canvas frames were recorded".into());
    }
    encoder.finish().map(Outcome::Saved)
}

fn dimensions(image: &CanvasFrame) -> [usize; 2] {
    let [width, height] = image.size();
    match image {
        CanvasFrame::Characters { .. } => [width * 8, height * 16],
        CanvasFrame::Pixels { .. } => [width, height],
    }
}

#[derive(Default)]
struct Rasterizer {
    fonts: Vec<Font>,
    glyphs: HashMap<char, [u8; 128]>,
    tiles: HashMap<(char, u8, u8), [u8; 384]>,
}

impl Rasterizer {
    fn glyph(&mut self, character: char) -> Result<&[u8; 128], String> {
        if self.fonts.is_empty() {
            let definitions = FontDefinitions::default();
            for name in &definitions.families[&FontFamily::Monospace] {
                let data = &definitions.font_data[name];
                self.fonts.push(
                    Font::from_bytes(
                        data.bytes(),
                        FontSettings {
                            collection_index: data.index,
                            ..Default::default()
                        },
                    )
                    .map_err(|error| format!("Could not load export font {name}: {error}"))?,
                );
            }
        }
        Ok(self.glyphs.entry(character).or_insert_with(|| {
            let font = self
                .fonts
                .iter()
                .find(|font| font.lookup_glyph_index(character) != 0)
                .unwrap_or(&self.fonts[0]);
            let (metrics, bitmap) = font.rasterize(character, 14.0);
            let mut glyph = [0; 128];
            let left = ((8.0 - metrics.advance_width) / 2.0).round() as i32 + metrics.xmin;
            let baseline = font
                .horizontal_line_metrics(14.0)
                .map_or(13.0, |line| (16.0 - line.new_line_size) / 2.0 + line.ascent);
            let top = baseline.round() as i32 - metrics.ymin - metrics.height as i32;
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let gx = left + x as i32;
                    let gy = top + y as i32;
                    if (0..8).contains(&gx) && (0..16).contains(&gy) {
                        glyph[gy as usize * 8 + gx as usize] = bitmap[y * metrics.width + x];
                    }
                }
            }
            glyph
        }))
    }

    fn tile(
        &mut self,
        character: char,
        background: u8,
        foreground: u8,
    ) -> Result<&[u8; 384], String> {
        let key = (character, background, foreground);
        if !self.tiles.contains_key(&key) {
            let background = ansi_color(background);
            let foreground = ansi_color(foreground);
            let glyph = self.glyph(character)?;
            let mut tile = [0; 384];
            for (pixel, &alpha) in tile.as_chunks_mut::<3>().0.iter_mut().zip(glyph) {
                let alpha = u16::from(alpha);
                for channel in 0..3 {
                    pixel[channel] = ((u16::from(foreground[channel]) * alpha
                        + u16::from(background[channel]) * (255 - alpha)
                        + 127)
                        / 255) as u8;
                }
            }
            // Bound cache memory independently of the number of colors in a long recording.
            if self.tiles.len() >= 4096 {
                self.tiles.clear();
            }
            self.tiles.insert(key, tile);
        }
        Ok(&self.tiles[&key])
    }

    #[cfg(test)]
    fn render(&mut self, image: &CanvasFrame) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        self.render_into(image, &mut output)?;
        Ok(output)
    }

    fn render_into(&mut self, image: &CanvasFrame, output: &mut Vec<u8>) -> Result<(), String> {
        let [width, height] = dimensions(image);
        let stride = width.next_multiple_of(2);
        output.resize(stride * height.next_multiple_of(2) * 3, 0);
        if width != stride {
            for row in output[..height * stride * 3].chunks_exact_mut(stride * 3) {
                row[width * 3..].fill(0);
            }
        }
        output[height * stride * 3..].fill(0);
        match image {
            CanvasFrame::Pixels { size, pixels } => {
                for (source, row) in pixels
                    .chunks_exact(size[0])
                    .zip(output.chunks_exact_mut(stride * 3))
                {
                    for (&[r, g, b, alpha], pixel) in
                        source.iter().zip(row[..width * 3].as_chunks_mut::<3>().0)
                    {
                        *pixel = match alpha {
                            0 => [0; 3],
                            255 => [r, g, b],
                            _ => [r, g, b].map(|value| {
                                ((u16::from(value) * u16::from(alpha) + 127) / 255) as u8
                            }),
                        };
                    }
                }
            }
            CanvasFrame::Characters { size, cells } => {
                for (index, cell) in cells.iter().enumerate() {
                    let tile = self.tile(cell.character, cell.background, cell.foreground)?;
                    let start = ((index / size[0]) * 16 * stride + (index % size[0]) * 8) * 3;
                    for y in 0..16 {
                        let offset = start + y * stride * 3;
                        output[offset..offset + 24].copy_from_slice(&tile[y * 24..y * 24 + 24]);
                    }
                }
            }
        }
        Ok(())
    }
}

struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    log: File,
    temporary: Option<NamedTempFile>,
    path: PathBuf,
    bytes_per_frame: usize,
    reaped: bool,
}

impl Encoder {
    fn new(path: PathBuf, [width, height]: [usize; 2], fps: u16) -> Result<Self, String> {
        validate_fps(fps)?;
        validate_path(&path)?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let temporary = NamedTempFile::new_in(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
        let log = tempfile::tempfile().map_err(|error| format!("FFmpeg log: {error}"))?;
        let width = width.next_multiple_of(2);
        let height = height.next_multiple_of(2);
        let mut command = Command::new("ffmpeg");
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgb24",
            "-video_size",
            &format!("{width}x{height}"),
            "-framerate",
            &fps.to_string(),
            "-i",
            "pipe:0",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ]);
        command
            .arg(temporary.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(
                log.try_clone()
                    .map_err(|error| format!("FFmpeg log: {error}"))?,
            );
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|error| {
            format!("Could not start FFmpeg: {error}. Install ffmpeg with H.264 (libx264) support and put it on PATH.")
        })?;
        let stdin = child.stdin.take();
        Ok(Self {
            child,
            stdin,
            log,
            temporary: Some(temporary),
            path,
            bytes_per_frame: width * height * 3,
            reaped: false,
        })
    }

    fn diagnostics(&mut self) -> String {
        let mut text = String::new();
        let result = self.log.seek(SeekFrom::Start(0)).and_then(|_| {
            Read::by_ref(&mut self.log)
                .take(65536)
                .read_to_string(&mut text)
        });
        if let Err(error) = result {
            return format!("Could not read FFmpeg diagnostics: {error}");
        }
        text.trim().to_owned()
    }

    fn write(&mut self, pixels: &[u8]) -> Result<(), String> {
        if pixels.len() != self.bytes_per_frame {
            return Err("Canvas dimensions changed during recording".into());
        }
        if let Err(error) = self
            .stdin
            .as_mut()
            .expect("encoder stdin exists")
            .write_all(pixels)
        {
            self.stdin.take();
            let status = self
                .child
                .wait()
                .map_err(|wait| format!("FFmpeg wait: {wait}"))?;
            self.reaped = true;
            return Err(format!(
                "FFmpeg input failed: {error} ({status}): {}",
                self.diagnostics()
            ));
        }
        Ok(())
    }

    fn finish(mut self) -> Result<PathBuf, String> {
        self.stdin.take();
        let status = self
            .child
            .wait()
            .map_err(|error| format!("FFmpeg wait: {error}"))?;
        self.reaped = true;
        if !status.success() {
            return Err(format!("FFmpeg failed ({status}): {}", self.diagnostics()));
        }
        self.temporary
            .take()
            .expect("temporary video exists")
            .persist_noclobber(&self.path)
            .map_err(|error| {
                format!(
                    "Could not save {} (existing files are never overwritten): {error}",
                    self.path.display()
                )
            })?;
        Ok(self.path.clone())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        self.stdin.take();
        if !self.reaped {
            if let Err(error) = self.child.kill()
                && error.kind() != std::io::ErrorKind::InvalidInput
            {
                eprintln!("ansiedad video: could not stop FFmpeg: {error}");
            }
            if let Err(error) = self.child.wait() {
                eprintln!("ansiedad video: could not reap FFmpeg: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Cell;

    #[test]
    fn reused_raster_buffers_clear_padding_and_tiles_include_both_colors() {
        let mut rasterizer = Rasterizer::default();
        let mut output = vec![255; 24];
        let image = CanvasFrame::Pixels {
            size: [3, 1],
            pixels: vec![[20, 40, 60, 255]; 3],
        };
        rasterizer.render_into(&image, &mut output).unwrap();
        assert_eq!(&output[..9], &[20, 40, 60, 20, 40, 60, 20, 40, 60]);
        assert!(output[9..].iter().all(|value| *value == 0));
        let first = *rasterizer.tile('E', 1, 15).unwrap();
        assert_ne!(first, *rasterizer.tile('E', 2, 15).unwrap());
        assert_ne!(first, *rasterizer.tile('E', 1, 3).unwrap());
        for colors in 0..4200 {
            rasterizer
                .tile('E', (colors / 256) as u8, colors as u8)
                .unwrap();
        }
        assert_eq!(first, *rasterizer.tile('E', 1, 15).unwrap());
    }

    #[test]
    fn rasterization_composites_alpha_and_pads_odd_dimensions() {
        let image = CanvasFrame::Pixels {
            size: [3, 1],
            pixels: vec![[255, 0, 0, 255], [0, 255, 0, 128], [20, 40, 60, 0]],
        };
        let pixels = Rasterizer::default().render(&image).unwrap();
        assert_eq!(pixels.len(), 4 * 2 * 3);
        assert_eq!(&pixels[..9], &[255, 0, 0, 0, 128, 0, 0, 0, 0]);
        assert!(pixels[9..].iter().all(|value| *value == 0));
    }

    #[test]
    fn character_rasterization_preserves_background_and_draws_unicode_glyphs() {
        let image = CanvasFrame::Characters {
            size: [3, 1],
            cells: [' ', 'E', 'é']
                .map(|character| Cell {
                    background: 1,
                    foreground: 15,
                    character,
                })
                .to_vec(),
        };
        let pixels = Rasterizer::default().render(&image).unwrap();
        for y in 0..16 {
            assert!(
                pixels[y * 24 * 3..(y * 24 + 8) * 3]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [205, 0, 0])
            );
        }
        for cell in 1..3 {
            assert!((0..16).any(|y| {
                pixels[(y * 24 + cell * 8) * 3..(y * 24 + cell * 8 + 8) * 3]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .any(|pixel| *pixel != [205, 0, 0])
            }));
        }
    }

    #[test]
    fn video_options_reject_invalid_duration_rate_and_existing_paths() {
        for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(frame_count(seconds, 30).is_err());
        }
        for fps in [0, 241] {
            assert!(frame_count(1.0, fps).is_err());
        }
        assert_eq!(frame_count(1.01, 30).unwrap(), 31);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("existing.mp4");
        std::fs::write(&path, "original").unwrap();
        assert!(validate_path(&path).unwrap_err().contains("already exists"));
        assert!(validate_path(&directory.path().join("video.webm")).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }
}
