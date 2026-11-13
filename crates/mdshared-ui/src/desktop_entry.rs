//! The desktop entry and icon of an AppImage, so the launcher, the task switcher and "Open with"
//! know the application. An AppImage is a single file; nothing installs it, so on its first run it
//! writes `applications/<name>.desktop` and the 256 px icon under the user's XDG data directory.
//!
//! The rules: only when `$APPIMAGE` names the running AppImage (see `mdshared_app::appimage`); the
//! entry carries an `X-AppImage-Path=` marker line, and a file of that name without the marker is
//! the user's own and is never touched; an entry is written when it is missing or records another
//! path than the current one (the AppImage moved or was updated into a new place); no default
//! application is set (no `mimeapps.list`); every failure is silent. Not built into anything on
//! Windows, where [`ensure`] does nothing.

/// What the application tells the launcher about itself.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// The file stem of the entry and the icon, the icon name, and the window class
    /// (`StartupWMClass`, which must equal the name given to `window::identify`): `mdedit`.
    pub name: &'static str,
    /// `Name=`: `mdedit`.
    pub display_name: &'static str,
    /// `Comment=`: a short description.
    pub comment: &'static str,
    /// The 256 px PNG icon, as embedded in the binary.
    pub icon_png: &'static [u8],
    /// The `MimeType=` list: what the application opens.
    pub mime_types: &'static [&'static str],
    /// The `Categories=` list.
    pub categories: &'static [&'static str],
}

/// Writes the entry and icon for the AppImage this process runs from, on a background thread.
/// Does nothing when `$APPIMAGE` does not name an existing file, and on Windows.
#[cfg(not(windows))]
pub fn ensure(spec: Spec) {
    let Some(appimage) = mdshared_app::appimage::path() else {
        return;
    };
    // A failure to start the thread is as silent as every other one.
    _ = std::thread::Builder::new()
        .name("mdshared-desktop-entry".into())
        .spawn(move || {
            if let Some(data_home) = data_home(
                std::env::var_os("XDG_DATA_HOME").as_deref(),
                std::env::var_os("HOME").as_deref(),
            ) {
                _ = ensure_in(&spec, &appimage, &data_home);
            }
        });
}

#[cfg(windows)]
pub fn ensure(_spec: Spec) {}

#[cfg(not(windows))]
use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME` when it is an absolute path, else `$HOME/.local/share`.
#[cfg(not(windows))]
fn data_home(xdg: Option<&std::ffi::OsStr>, home: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let xdg = xdg.map(PathBuf::from).filter(|p| p.is_absolute());
    xdg.or_else(|| {
        home.map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|h| h.join(".local/share"))
    })
}

#[cfg(not(windows))]
const MARKER: &str = "X-AppImage-Path=";

/// What [`ensure_in`] did.
#[cfg(not(windows))]
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Written,
    UpToDate,
    /// An entry of that name without the marker: the user's, left alone.
    NotOurs,
    /// A name or path that cannot be written safely.
    Refused,
}

/// A value of a string key: backslash escaped, control characters (a newline would start a new
/// key) turned into spaces.
#[cfg(not(windows))]
fn text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .replace('\\', "\\\\")
}

/// `path` as the one argument of an `Exec` line: in double quotes, with `"`, `` ` ``, `$` and `\`
/// escaped for the Exec syntax and `%` doubled; then every backslash doubled again for the string
/// syntax of the file.
#[cfg(not(windows))]
fn exec_quoted(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out.replace('\\', "\\\\")
}

#[cfg(not(windows))]
fn list(items: &[&str]) -> String {
    items.iter().map(|i| format!("{};", text(i))).collect()
}

#[cfg(not(windows))]
fn render(spec: &Spec, appimage: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec={} %F\nIcon={}\n\
         StartupWMClass={}\nMimeType={}\nCategories={}\nTerminal=false\n{MARKER}{}\n",
        text(spec.display_name),
        text(spec.comment),
        exec_quoted(appimage),
        spec.name,
        spec.name,
        list(spec.mime_types),
        list(spec.categories),
        text(appimage),
    )
}

#[cfg(not(windows))]
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The work of [`ensure`], for the AppImage at `appimage` and the data directory `data_home`.
#[cfg(not(windows))]
fn ensure_in(spec: &Spec, appimage: &Path, data_home: &Path) -> std::io::Result<Outcome> {
    let name_ok = !spec.name.is_empty()
        && spec
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    let Some(path) = appimage
        .to_str()
        .filter(|p| name_ok && !p.contains(['\n', '\r']))
    else {
        return Ok(Outcome::Refused);
    };
    let entry = data_home
        .join("applications")
        .join(format!("{}.desktop", spec.name));
    let icon = data_home
        .join("icons/hicolor/256x256/apps")
        .join(format!("{}.png", spec.name));
    let current = format!("{MARKER}{}", text(path));
    match std::fs::read_to_string(&entry) {
        Ok(old) => {
            let mut marked = old.lines().filter(|l| l.starts_with(MARKER));
            let Some(recorded) = marked.next() else {
                return Ok(Outcome::NotOurs);
            };
            if recorded == current && icon.is_file() {
                return Ok(Outcome::UpToDate);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // Unreadable (not UTF-8, no permission): not ours to judge.
        Err(_) => return Ok(Outcome::NotOurs),
    }
    // The icon first, so an entry never names an icon that is not there yet.
    write_atomic(&icon, spec.icon_png)?;
    write_atomic(&entry, render(spec, path).as_bytes())?;
    Ok(Outcome::Written)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    const SPEC: Spec = Spec {
        name: "mdtest",
        display_name: "mdtest",
        comment: "A test; with \\ odd text",
        icon_png: b"png bytes",
        mime_types: &["text/markdown", "text/plain"],
        categories: &["Utility", "TextEditor"],
    };

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("mdshared-desktop-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn entry(d: &Path) -> PathBuf {
        d.join("applications/mdtest.desktop")
    }

    #[test]
    fn writes_the_entry_and_the_icon() {
        let d = dir("write");
        let app = Path::new("/home/u/Apps/mdtest.AppImage");
        assert_eq!(ensure_in(&SPEC, app, &d).unwrap(), Outcome::Written);
        let text = std::fs::read_to_string(entry(&d)).unwrap();
        for line in [
            "[Desktop Entry]",
            "Type=Application",
            "Name=mdtest",
            "Comment=A test; with \\\\ odd text",
            "Exec=\"/home/u/Apps/mdtest.AppImage\" %F",
            "Icon=mdtest",
            "StartupWMClass=mdtest",
            "MimeType=text/markdown;text/plain;",
            "Categories=Utility;TextEditor;",
            "Terminal=false",
            "X-AppImage-Path=/home/u/Apps/mdtest.AppImage",
        ] {
            assert!(
                text.lines().any(|l| l == line),
                "missing {line:?} in\n{text}"
            );
        }
        assert_eq!(
            std::fs::read(d.join("icons/hicolor/256x256/apps/mdtest.png")).unwrap(),
            b"png bytes"
        );
        // Nothing sets a default application.
        assert!(!d.join("mimeapps.list").exists());
        _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rewritten_only_when_missing_or_moved() {
        let d = dir("moved");
        let a = Path::new("/a/mdtest.AppImage");
        assert_eq!(ensure_in(&SPEC, a, &d).unwrap(), Outcome::Written);
        assert_eq!(ensure_in(&SPEC, a, &d).unwrap(), Outcome::UpToDate);
        // The AppImage moved.
        let b = Path::new("/b/mdtest.AppImage");
        assert_eq!(ensure_in(&SPEC, b, &d).unwrap(), Outcome::Written);
        let text = std::fs::read_to_string(entry(&d)).unwrap();
        assert!(text.contains("Exec=\"/b/mdtest.AppImage\" %F"), "{text}");
        assert_eq!(ensure_in(&SPEC, b, &d).unwrap(), Outcome::UpToDate);
        // The icon was deleted: written again.
        std::fs::remove_file(d.join("icons/hicolor/256x256/apps/mdtest.png")).unwrap();
        assert_eq!(ensure_in(&SPEC, b, &d).unwrap(), Outcome::Written);
        _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_users_own_entry_is_never_touched() {
        let d = dir("own");
        std::fs::create_dir_all(d.join("applications")).unwrap();
        std::fs::write(entry(&d), "[Desktop Entry]\nName=mine\n").unwrap();
        let app = Path::new("/a/mdtest.AppImage");
        assert_eq!(ensure_in(&SPEC, app, &d).unwrap(), Outcome::NotOurs);
        assert_eq!(
            std::fs::read_to_string(entry(&d)).unwrap(),
            "[Desktop Entry]\nName=mine\n"
        );
        assert!(!d.join("icons").exists());
        _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn exec_quoting_follows_the_desktop_entry_spec() {
        assert_eq!(exec_quoted("/a b/c"), "\"/a b/c\"");
        // `"` `$` and a backtick get a backslash, which the file then doubles.
        assert_eq!(exec_quoted("/a\"b$c`d"), "\"/a\\\\\"b\\\\$c\\\\`d\"");
        assert_eq!(exec_quoted("/100%/x"), "\"/100%%/x\"");
        assert_eq!(exec_quoted("/a\\b"), "\"/a\\\\\\\\b\"");
    }

    #[test]
    fn unsafe_names_and_paths_are_refused() {
        let d = dir("refused");
        let app = Path::new("/a/new\nline.AppImage");
        assert_eq!(ensure_in(&SPEC, app, &d).unwrap(), Outcome::Refused);
        let bad = Spec {
            name: "../evil",
            ..SPEC
        };
        let ok = Path::new("/a/x.AppImage");
        assert_eq!(ensure_in(&bad, ok, &d).unwrap(), Outcome::Refused);
        assert!(!d.join("applications").exists());
        _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_data_directory() {
        use std::ffi::OsStr;
        let home = Some(OsStr::new("/home/u"));
        assert_eq!(
            data_home(Some(OsStr::new("/x/data")), home),
            Some("/x/data".into())
        );
        assert_eq!(
            data_home(Some(OsStr::new("rel")), home),
            Some("/home/u/.local/share".into())
        );
        assert_eq!(data_home(None, home), Some("/home/u/.local/share".into()));
        assert_eq!(data_home(None, None), None);
    }
}
