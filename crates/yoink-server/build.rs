/// Ensures the frontend build output exists before compiling the server.
///
/// The backend embeds the Vite SPA files from `frontend/dist/` via
/// `rust-embed`. Building the server without a generated `index.html`
/// produces a broken binary, so fail fast instead of compiling with an
/// empty asset directory.
fn main() {
    let frontend_dir = std::path::Path::new("../../frontend/dist");
    let spa_shell = frontend_dir.join("index.html");

    // `rust-embed` reads these files at compile time, so Cargo needs to
    // rerun this crate when the frontend build output changes.
    println!("cargo:rerun-if-changed={}", frontend_dir.display());

    assert!(
        spa_shell.exists(),
        "missing frontend assets at {}. Run `cd frontend && bun install && bun run build` before building yoink-server",
        spa_shell.display()
    );
}
