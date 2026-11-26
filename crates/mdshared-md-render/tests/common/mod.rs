use std::path::PathBuf;

/// Cargo runs integration tests from the current profile's `deps` directory, one level
/// beneath the engine binary, even when the workspace uses a custom target directory.
pub fn engine_available() -> bool {
    if let Some(path) = std::env::var_os("MDV_ENGINE") {
        return PathBuf::from(path).is_file();
    }
    let name = if cfg!(windows) {
        "mdview-engine.exe"
    } else {
        "mdview-engine"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join(name)))
        .is_some_and(|path| path.is_file())
}
