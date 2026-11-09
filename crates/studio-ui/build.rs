// Exposes the Slint library folder to dependents' build scripts as `DEP_STUDIO_UI_LIBRARY`.
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
    println!("cargo:library={}", dir.join("studio.slint").display());
    println!("cargo:rerun-if-changed=ui");
}
