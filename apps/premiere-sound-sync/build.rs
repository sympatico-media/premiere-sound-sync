use std::collections::HashMap;

fn main() {
    let library = std::env::var("DEP_STUDIO_UI_LIBRARY").expect("studio-ui must be a dependency");
    let mut paths = HashMap::new();
    paths.insert("studio".to_string(), library.into());
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(paths)
        .with_style("cupertino-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compiling Slint UI");
}
