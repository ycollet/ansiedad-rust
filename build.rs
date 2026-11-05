fn main() {
    println!("cargo:rerun-if-changed=assets/ansiedad.ico");
    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/ansiedad.ico")
            .compile()
            .expect("failed to embed the ANSIedad application icon");
    }
}
