use std::path::PathBuf;

/// Cargo runs integration tests beneath the profile directory. Look there for the engine
/// even with a custom target directory or a Cargo build with intermediate output directories.
pub fn engine_available() -> bool {
    if let Some(path) = std::env::var_os("MDV_ENGINE") {
        let path = PathBuf::from(path);
        return path.is_absolute() && path.is_file();
    }
    let name = if cfg!(windows) {
        "mdview-engine.exe"
    } else {
        "mdview-engine"
    };
    let path = std::env::current_exe().ok().and_then(|exe| {
        exe.ancestors()
            .skip(1)
            .take(5)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    });
    path.is_some_and(|path| {
        mdshared_md_render::set_engine_path(path);
        true
    })
}
