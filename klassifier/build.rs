fn main() {
    // The exe icon (Explorer, taskbar), for the Windows targets. Regenerate the files with
    // tools/make-icons.py.
    println!("cargo:rerun-if-changed=assets/klassifier.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/klassifier.ico")
            .compile()
            .expect("cannot embed assets/klassifier.ico");
    }

    // The review window's .slint files, compiled only with the `ui` feature (klassifier-ui).
    #[cfg(feature = "ui")]
    {
        let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
        slint_build::compile_with_config("ui/main.slint", config).expect("cannot compile ui/main.slint");
    }
}
