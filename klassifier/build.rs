fn main() {
    // The review window's .slint files, compiled only with the `ui` feature (klassifier-ui).
    #[cfg(feature = "ui")]
    {
        let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
        slint_build::compile_with_config("ui/main.slint", config).expect("cannot compile ui/main.slint");
    }
}
