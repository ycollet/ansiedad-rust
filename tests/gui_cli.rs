#![cfg(feature = "gui")]

use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn graphical_cli_checks_patches_without_a_desktop_and_never_overwrites() {
    let directory = std::env::temp_dir().join(format!(
        "ansiedad-gui-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ansiedad-gui"))
            .args(args)
            .current_dir(&directory)
            .output()
            .unwrap()
    };
    assert!(run(&["--check"]).status.success());
    assert!(!directory.join("canvas.txt").exists());
    for (name, source) in [
        ("characters.txt", include_str!("../examples/characters.txt")),
        ("pixels.txt", include_str!("../examples/pixels.txt")),
    ] {
        fs::write(directory.join(name), source).unwrap();
        let output = run(&["--check", name]);
        assert!(output.status.success(), "{output:?}");
        fs::remove_file(directory.join(name)).unwrap();
    }
    assert!(run(&["--init"]).status.success());
    assert!(!run(&["--init"]).status.success());
    assert!(run(&["--check", "canvas.txt"]).status.success());
    fs::write(directory.join("canvas.txt"), "mode=2").unwrap();
    let output = run(&["--check", "canvas.txt"]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("line 1, column 6:"));
    assert!(error.contains("mode"));
    assert!(!run(&["--check", "missing.txt"]).status.success());
    for args in [
        vec!["--frames", "0"],
        vec!["--frames"],
        vec!["--unknown"],
        vec!["--export"],
        vec!["--seconds", "1"],
        vec!["--export", "video.mp4", "--seconds", "nan"],
        vec!["--export", "video.mp4", "--seconds", "0"],
        vec!["--export", "video.mp4", "--video-fps", "0"],
        vec!["--export", "video.mp4", "--video-fps", "241"],
        vec!["--export", "video.mp4", "--check"],
        vec!["--export", "video.mp4", "--init"],
        vec!["--export", "video.mp4", "--frames", "1"],
    ] {
        assert!(!run(&args).status.success());
    }
    fs::remove_file(directory.join("canvas.txt")).unwrap();
    fs::remove_dir(directory).unwrap();
}

#[test]
#[ignore = "requires FFmpeg with libx264 and ffprobe on PATH"]
fn headless_mp4_export_encodes_frames_and_preserves_existing_files() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("patch.txt");
    let video = directory.path().join("animation.mp4");
    fs::write(
        &source,
        "mode=1; w=3; l=3; fps=2; rgba='((frame&1)*255)<<24 | 255'",
    )
    .unwrap();
    let run = |output: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_ansiedad-gui"))
            .arg("--export")
            .arg(output)
            .args(["--seconds", "1"])
            .arg(&source)
            .output()
            .unwrap()
    };
    let output = run(&video);
    assert!(output.status.success(), "{output:?}");
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,nb_frames,avg_frame_rate",
            "-of",
            "csv=p=0",
        ])
        .arg(&video)
        .output()
        .unwrap();
    assert!(probe.status.success(), "{probe:?}");
    assert_eq!(
        String::from_utf8_lossy(&probe.stdout).trim(),
        "h264,4,4,2/1,2"
    );
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&video)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
        .output()
        .unwrap();
    assert!(decoded.status.success(), "{decoded:?}");
    assert_eq!(decoded.stdout.len(), 2 * 4 * 4 * 3);
    assert!(decoded.stdout[0] < 10);
    assert!(decoded.stdout[4 * 4 * 3] > 150);
    let original = fs::read(&video).unwrap();
    let output = run(&video);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(fs::read(&video).unwrap(), original);

    fs::write(&source, "w=2; l=1; c=\"'E'\"; b='1'; f='15'; fps=2").unwrap();
    let characters = directory.path().join("characters.mp4");
    let output = run(&characters);
    assert!(output.status.success(), "{output:?}");
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(characters)
        .output()
        .unwrap();
    assert!(probe.status.success());
    assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "16,16");

    fs::write(&source, "mode=1; w=2; l=2; rgba='1/(frame-1)'").unwrap();
    let broken = directory.path().join("broken.mp4");
    let output = run(&broken);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("division or modulo by zero"));
    assert!(!broken.exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
}

#[test]
#[ignore = "requires FFmpeg with libx264 on PATH"]
fn live_recording_finishes_and_cancellation_removes_temporary_files() {
    use ansiedad::{
        canvas::CanvasConfig,
        video::{Outcome, Recorder, VideoJob},
    };
    use std::{
        thread,
        time::{Duration, Instant},
    };

    let wait = |job: &mut VideoJob| {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "video worker did not finish");
            thread::sleep(Duration::from_millis(10));
        }
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("live.mp4");
    let config = CanvasConfig::parse("mode=1; w=4; l=4").unwrap();
    let image = config.render(0, 0).unwrap();
    let (mut recorder, mut job) = Recorder::start(path.clone(), 30, &image).unwrap();
    recorder.capture(&image).unwrap();
    thread::sleep(Duration::from_millis(120));
    recorder.capture(&image).unwrap();
    drop(recorder);
    assert!(matches!(wait(&mut job).unwrap(), Outcome::Saved(saved) if saved == path));
    assert!(
        job.frames() >= 4,
        "paused frames should repeat at the output frame rate"
    );
    assert!(path.exists());

    let canceled = directory.path().join("canceled.mp4");
    let mut job = VideoJob::offline("mode=1; w=4; l=4", canceled.clone(), 3600.0, 30).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while job.frames() == 0 {
        assert!(
            job.poll().is_none(),
            "export stopped before producing a frame"
        );
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    job.cancel();
    assert!(matches!(wait(&mut job).unwrap(), Outcome::Canceled));
    assert!(!canceled.exists());

    let resized = directory.path().join("resized.mp4");
    let (mut recorder, mut job) = Recorder::start(resized.clone(), 30, &image).unwrap();
    let other = CanvasConfig::parse("mode=1; w=8; l=2")
        .unwrap()
        .render(0, 0)
        .unwrap();
    assert!(recorder.capture(&other).unwrap_err().contains("dimensions"));
    job.cancel();
    drop(recorder);
    assert!(matches!(wait(&mut job).unwrap(), Outcome::Canceled));
    assert!(!resized.exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn missing_ffmpeg_is_reported_without_creating_a_video() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing.mp4");
    let output = Command::new(env!("CARGO_BIN_EXE_ansiedad-gui"))
        .env("PATH", directory.path())
        .arg("--export")
        .arg(&path)
        .args(["--seconds", "0.1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Could not start FFmpeg"));
    assert!(!path.exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn encoder_failure_reports_stderr_and_removes_partial_output() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("ffmpeg");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf 'test encoder failure\\n' >&2\nexit 23\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let path = directory.path().join("failed.mp4");
    let output = Command::new(env!("CARGO_BIN_EXE_ansiedad-gui"))
        .env("PATH", directory.path())
        .arg("--export")
        .arg(&path)
        .args(["--seconds", "0.1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("test encoder failure"),
        "{output:?}"
    );
    assert!(!path.exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}
