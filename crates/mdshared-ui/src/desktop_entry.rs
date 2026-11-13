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

/// The MIME types the editor opens: text and source files.
pub const EDITOR_MIME_TYPES: &[&str] = &[
    "text/plain",
    "text/x-log",
    "application/json",
    "application/xml",
    "text/xml",
    "application/x-yaml",
    "application/toml",
    "text/csv",
    "text/markdown",
    "text/x-rust",
    "text/x-python",
    "text/x-csrc",
    "text/x-chdr",
    "text/x-c++src",
    "text/x-c++hdr",
    "text/x-java",
    "text/javascript",
    "application/javascript",
    "application/typescript",
    "text/x-go",
    "text/x-shellscript",
    "application/x-shellscript",
    "text/css",
    "text/html",
    "text/x-ini",
    "text/x-cmake",
    "text/x-lua",
    "text/x-ruby",
    "application/x-php",
    "text/x-sql",
    "text/x-kotlin",
    "text/x-swift",
    "text/x-diff",
    "text/x-patch",
];

/// The MIME types the viewer opens: Markdown.
pub const VIEWER_MIME_TYPES: &[&str] = &["text/markdown", "text/x-markdown"];

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
    // So "Open with" lists the application without a logout. Absent or failing: ignored.
    _ = std::process::Command::new("update-desktop-database")
        .arg(data_home.join("applications"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(Outcome::Written)
}

/// Makes `<name>.desktop` the default application for `mime_types` in the user's `mimeapps.list`
/// (`$XDG_CONFIG_HOME` or `~/.config`), under `[Default Applications]`. Call it only when the
/// reader asks; [`ensure`] never does.
#[cfg(not(windows))]
pub fn make_default(name: &str, mime_types: &[&str]) -> Result<(), String> {
    let dir = config_home(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .ok_or("no configuration directory")?;
    make_default_in(&dir, name, mime_types)
}

/// Whether `<name>.desktop` is the first default for every one of `mime_types`.
#[cfg(not(windows))]
pub fn is_default(name: &str, mime_types: &[&str]) -> bool {
    config_home(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .is_some_and(|dir| is_default_in(&dir, name, mime_types))
}

#[cfg(windows)]
pub fn make_default(_name: &str, _mime_types: &[&str]) -> Result<(), String> {
    Err("not supported on Windows".to_string())
}

#[cfg(windows)]
pub fn is_default(_name: &str, _mime_types: &[&str]) -> bool {
    false
}

#[cfg(not(windows))]
fn config_home(xdg: Option<&std::ffi::OsStr>, home: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let xdg = xdg.map(PathBuf::from).filter(|p| p.is_absolute());
    xdg.or_else(|| {
        home.map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|h| h.join(".config"))
    })
}

#[cfg(not(windows))]
const DEFAULTS: &str = "[Default Applications]";

#[cfg(not(windows))]
fn valid_mime(t: &str) -> bool {
    !t.is_empty() && !t.contains(['=', ';', '\n', '\r', ' ', '[', ']'])
}

#[cfg(not(windows))]
fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[cfg(not(windows))]
fn make_default_in(config: &Path, name: &str, mime_types: &[&str]) -> Result<(), String> {
    if !valid_name(name) || !mime_types.iter().all(|t| valid_mime(t)) {
        return Err("unsafe name or MIME type".to_string());
    }
    let path = config.join("mimeapps.list");
    let old = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let new = with_defaults(&old, name, mime_types);
    if new != old {
        write_atomic(&path, new.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `old` with the defaults for `mime_types` set to `<name>.desktop`; every other byte kept.
#[cfg(not(windows))]
fn with_defaults(old: &str, name: &str, mime_types: &[&str]) -> String {
    let value = format!("{name}.desktop;");
    let mut lines: Vec<String> = old.split_inclusive('\n').map(str::to_string).collect();
    let mut pending: Vec<&str> = mime_types.to_vec();
    let header = lines.iter().position(|l| l.trim_end() == DEFAULTS);
    let Some(start) = header else {
        let mut out = old.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(DEFAULTS);
        out.push('\n');
        for t in mime_types {
            out.push_str(&format!("{t}={value}\n"));
        }
        return out;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    for line in &mut lines[start + 1..end] {
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        if let Some(i) = pending.iter().position(|t| *t == key.trim()) {
            pending.remove(i);
            let eol = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            *line = format!("{key}={value}{eol}");
        }
    }
    if !pending.is_empty() {
        // After the section's last non-blank line, so blank lines before the next section stay.
        let mut at = end;
        while at > start + 1 && lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        if at > 0 && !lines[at - 1].ends_with('\n') {
            lines[at - 1].push('\n');
        }
        let added: Vec<String> = pending.iter().map(|t| format!("{t}={value}\n")).collect();
        lines.splice(at..at, added);
    }
    lines.concat()
}

#[cfg(not(windows))]
fn is_default_in(config: &Path, name: &str, mime_types: &[&str]) -> bool {
    let Ok(text) = std::fs::read_to_string(config.join("mimeapps.list")) else {
        return false;
    };
    let want = format!("{name}.desktop");
    let mut in_defaults = false;
    let mut found: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_defaults = t == DEFAULTS;
        } else if in_defaults
            && let Some((key, value)) = t.split_once('=')
            && value.split(';').next() == Some(want.as_str())
        {
            found.push(key.trim());
        }
    }
    !mime_types.is_empty() && mime_types.iter().all(|t| found.contains(t))
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

    #[test]
    fn defaults_in_an_empty_or_missing_file() {
        let d = dir("def-empty");
        assert!(!is_default_in(&d, "mdtest", &["text/plain"]));
        make_default_in(&d, "mdtest", &["text/plain", "text/markdown"]).unwrap();
        let text = std::fs::read_to_string(d.join("mimeapps.list")).unwrap();
        assert_eq!(
            text,
            "[Default Applications]\ntext/plain=mdtest.desktop;\ntext/markdown=mdtest.desktop;\n"
        );
        assert!(is_default_in(
            &d,
            "mdtest",
            &["text/plain", "text/markdown"]
        ));
        assert!(!is_default_in(&d, "other", &["text/plain"]));
        assert!(!is_default_in(&d, "mdtest", &[]));
        // An existing empty file.
        std::fs::write(d.join("mimeapps.list"), "").unwrap();
        make_default_in(&d, "mdtest", &["text/plain"]).unwrap();
        assert!(is_default_in(&d, "mdtest", &["text/plain"]));
        _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn defaults_leave_everything_else_as_it_was() {
        let d = dir("def-other");
        let before = "# mine\n[Added Associations]\ntext/plain=gedit.desktop;\n\n\
                      [Default Applications]\nimage/png=viewer.desktop;\n\
                      text/plain=gedit.desktop;kate.desktop;\n\n[Removed Associations]\nx/y=z.desktop;";
        std::fs::write(d.join("mimeapps.list"), before).unwrap();
        make_default_in(&d, "mdtest", &["text/plain", "text/x-log"]).unwrap();
        let after = std::fs::read_to_string(d.join("mimeapps.list")).unwrap();
        assert_eq!(
            after,
            "# mine\n[Added Associations]\ntext/plain=gedit.desktop;\n\n\
             [Default Applications]\nimage/png=viewer.desktop;\n\
             text/plain=mdtest.desktop;\ntext/x-log=mdtest.desktop;\n\n\
             [Removed Associations]\nx/y=z.desktop;"
        );
        assert!(is_default_in(&d, "mdtest", &["text/plain", "text/x-log"]));
        // The Added Associations entry for the same type is not a default.
        assert!(!is_default_in(&d, "gedit", &["text/plain"]));
        // Idempotent: a second call changes no byte.
        make_default_in(&d, "mdtest", &["text/plain", "text/x-log"]).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("mimeapps.list")).unwrap(),
            after
        );
        // A file without the section, and without a final newline.
        std::fs::write(
            d.join("mimeapps.list"),
            "[Added Associations]\na/b=c.desktop;",
        )
        .unwrap();
        make_default_in(&d, "mdtest", &["text/plain"]).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("mimeapps.list")).unwrap(),
            "[Added Associations]\na/b=c.desktop;\n\n[Default Applications]\ntext/plain=mdtest.desktop;\n"
        );
        assert!(make_default_in(&d, "mdtest", &["bad type"]).is_err());
        _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_mime_lists_are_well_formed() {
        for t in EDITOR_MIME_TYPES.iter().chain(VIEWER_MIME_TYPES) {
            assert!(valid_mime(t) && t.contains('/'), "{t}");
        }
        assert_eq!(EDITOR_MIME_TYPES.len(), 34);
    }
}
