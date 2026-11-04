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
    assert!(String::from_utf8_lossy(&output.stderr).contains("mode"));
    assert!(!run(&["--check", "missing.txt"]).status.success());
    for args in [vec!["--frames", "0"], vec!["--frames"], vec!["--unknown"]] {
        assert!(!run(&args).status.success());
    }
    fs::remove_file(directory.join("canvas.txt")).unwrap();
    fs::remove_dir(directory).unwrap();
}
