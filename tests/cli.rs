use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn startup_sizing_options_are_validated_without_requiring_a_terminal() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ansiedad"))
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["--size", "120x40", "--check"]).status.success());
    assert!(run(&["--maximize", "--check"]).status.success());
    assert!(
        run(&["--size", "120x40", "--maximize", "--check"])
            .status
            .success()
    );
    for size in [
        "",
        "120",
        "0x40",
        "120x0",
        "120x3",
        "-1x40",
        "120x-1",
        "65536x40",
        "120x65536",
        "120x40x50",
        "+120x40",
    ] {
        let output = run(&["--size", size, "--check"]);
        assert!(!output.status.success(), "{size}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--size"),
            "{size}"
        );
    }
    assert!(!run(&["--size"]).status.success());
}

#[test]
fn file_free_startup_and_explicit_config_errors() {
    let directory = std::env::temp_dir().join(format!(
        "ansiedad-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ansiedad"))
            .args(args)
            .current_dir(&directory)
            .output()
            .unwrap()
    };

    let output = run(&["--check"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("built-in example"));
    assert!(!directory.join("editme.txt").exists());
    let output = run(&["--check", "missing.toml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing.toml"));

    fs::write(directory.join("editme.txt"), "invalid assignment").unwrap();
    assert!(
        run(&["--check"]).status.success(),
        "implicit files must not affect scratch mode"
    );
    assert!(!run(&["--check", "editme.txt"]).status.success());
    assert!(
        !run(&["--init"]).status.success(),
        "--init must not overwrite"
    );
    fs::remove_file(directory.join("editme.txt")).unwrap();
    assert!(run(&["--init"]).status.success());
    let starter = fs::read_to_string(directory.join("editme.txt")).unwrap();
    assert!(starter.contains("Ctrl-S") && starter.contains("b = \"t >> 4\""));
    assert!(run(&["--check", "editme.txt"]).status.success());
    for name in ["editme", "patch.txt"] {
        assert!(run(&["--init", name]).status.success());
        assert!(run(&["--check", name]).status.success());
        fs::remove_file(directory.join(name)).unwrap();
    }
    let patch = directory.join("semicolons.txt");
    fs::write(
        &patch,
        "BYE='ANSIedad....'; BITMAP=0b1010 # text constants need no section\n\
         b='BITMAP&t'; f='t&1'; c='ord(BYE[t%12])-32'\n",
    )
    .unwrap();
    let output = run(&["--check", "semicolons.txt"]);
    assert!(output.status.success(), "{:?}", output);
    fs::write(&patch, "b='0'; invalid statement\n").unwrap();
    let output = run(&["--check", "semicolons.txt"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("line 1, column "));
    fs::remove_file(patch).unwrap();
    fs::remove_file(directory.join("editme.txt")).unwrap();
    fs::remove_dir(directory).unwrap();
}
