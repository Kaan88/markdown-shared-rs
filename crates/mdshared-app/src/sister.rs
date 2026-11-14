//! The hand-off between the two sister applications, the viewer (`mdview`) and the editor
//! (`mdedit`): finding the other one, asking it to open a file at a line, keeping it scrolled to a
//! line, and asking it to take a place on the screen. Nothing here runs unless a hand-off happens.
//!
//! A request travels one of two ways. [`open`] starts the other application with arguments, which
//! its own second launch turns into the same request and passes to the running window (or uses
//! itself, when none runs). [`send`] skips the process and hands the request straight to a running
//! window over the single-instance port: it is what live updates use, which can come ten times a
//! second. The wire format is in [`Request::encode`]. `docs/HANDOFF.md` has the whole protocol.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// A rectangle in physical screen pixels, as winit reports a window's outer position and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Larger than any screen, so a damaged value cannot ask for a window of absurd size.
const MAX_EDGE: u32 = 100_000;

impl Rect {
    /// `x,y,w,h`: the form of the `--place` argument and of the `?place=` line. Width and height
    /// must be at least 1.
    pub fn parse(text: &str) -> Option<Rect> {
        let mut parts = text.split(',').map(str::trim);
        let rect = Rect {
            x: parts.next()?.parse().ok()?,
            y: parts.next()?.parse().ok()?,
            w: parts.next()?.parse().ok()?,
            h: parts.next()?.parse().ok()?,
        };
        let sane = (1..=MAX_EDGE).contains(&rect.w) && (1..=MAX_EDGE).contains(&rect.h);
        (parts.next().is_none() && sane).then_some(rect)
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{},{},{}", self.x, self.y, self.w, self.h)
    }
}

/// What one application asks the other to do with one file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub path: PathBuf,
    /// Show or put the caret at this source line (1-based).
    pub line: Option<u32>,
    /// With `line`: the column (1-based). The viewer has no use for it.
    pub column: Option<u32>,
    /// A live update, not an opening: do not raise or focus the window, and do nothing when the
    /// file is not already the one shown. When no window runs, nothing is started.
    pub follow: bool,
    /// Move and resize the window to this rectangle.
    pub place: Option<Rect>,
    /// The sending application's name (`mdview`, `mdedit`).
    pub from: Option<String>,
}

/// The first character of a line that carries an option instead of a path. No absolute path begins
/// with it, and a receiver that does not know the options finds no such file and skips the line.
const OPTION: char = '?';

impl Request {
    pub fn new(path: impl Into<PathBuf>) -> Request {
        Request {
            path: path.into(),
            ..Request::default()
        }
    }

    /// The hand-over message for the application named `app`, as it goes over the single-instance
    /// port: the name, then lines, then an empty line.
    ///
    /// ```text
    /// mdview
    /// /home/u/notes.md
    /// ?line=200
    /// ?col=5
    /// ?place=100,80,960,1000
    /// ?from=mdedit
    ///
    /// ```
    ///
    /// A request with none of the optional fields is a path line alone, byte for byte what an
    /// ordinary launch has always sent. The option lines come after the paths and before the empty
    /// line, which is where an older receiver already reads and skips whatever is not a file. A
    /// `follow` request has `?follow` and gives its path as `?path=` instead of a plain line, so an
    /// older receiver, which would open and raise a plain path, finds nothing to do.
    pub fn encode(&self, app: &str) -> String {
        let path = self.path.display();
        let mut out = format!("{app}\n");
        if self.follow {
            out.push_str(&format!("{OPTION}path={path}\n{OPTION}follow\n"));
        } else {
            out.push_str(&format!("{path}\n"));
        }
        if let Some(line) = self.line {
            out.push_str(&format!("{OPTION}line={line}\n"));
            if let Some(column) = self.column {
                out.push_str(&format!("{OPTION}col={column}\n"));
            }
        }
        if let Some(rect) = self.place {
            out.push_str(&format!("{OPTION}place={rect}\n"));
        }
        if let Some(from) = self.from.as_deref().filter(|f| !f.contains(['\n', '\r'])) {
            out.push_str(&format!("{OPTION}from={from}\n"));
        }
        out.push('\n');
        out
    }

    /// The requests in a message's lines, after the application's name and up to the empty line:
    /// one for each path, all with the options the message carries. Unknown or malformed option
    /// lines are skipped one by one.
    pub fn parse<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<Request> {
        let mut paths = Vec::new();
        let mut options = Request::default();
        for line in lines {
            let Some(option) = line.strip_prefix(OPTION) else {
                paths.push(PathBuf::from(line));
                continue;
            };
            let (key, value) = option.split_once('=').unwrap_or((option, ""));
            let number = || value.parse::<u32>().ok().filter(|&n| n >= 1);
            match key {
                "path" => paths.push(PathBuf::from(value)),
                "line" => options.line = number(),
                "col" => options.column = number(),
                "follow" => options.follow = true,
                "place" => options.place = Rect::parse(value),
                "from" => options.from = Some(value.to_string()).filter(|f| !f.is_empty()),
                _ => {}
            }
        }
        paths
            .into_iter()
            .map(|path| Request {
                path,
                ..options.clone()
            })
            .collect()
    }

    /// The command-line arguments that make `target` build this request: its own second launch
    /// parses them and hands the request to the running window. The editor takes `--goto
    /// <line>[:<column>]`, the viewer `--line <line>`; both take `--follow`, `--place x,y,w,h` and
    /// `--from <name>`. The path comes last.
    pub fn args(&self, target: &str) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        if let Some(line) = self.line {
            match (target, self.column) {
                ("mdedit", Some(column)) => {
                    args.extend(["--goto".into(), format!("{line}:{column}").into()])
                }
                ("mdedit", None) => args.extend(["--goto".into(), line.to_string().into()]),
                _ => args.extend(["--line".into(), line.to_string().into()]),
            }
        }
        if self.follow {
            args.push("--follow".into());
        }
        if let Some(rect) = self.place {
            args.extend(["--place".into(), rect.to_string().into()]);
        }
        if let Some(from) = &self.from {
            args.extend(["--from".into(), from.into()]);
        }
        args.push(self.path.clone().into_os_string());
        args
    }
}

// ---- finding the other application -------------------------------------------------------------

/// What [`find_in`] looks at: the environment, taken apart so a test can set each part.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// `MD_SISTER_<NAME>`: an executable to use before anything else.
    pub over: Option<OsString>,
    /// The running executable.
    pub exe: Option<PathBuf>,
    /// Linux: `$XDG_DATA_HOME` or `~/.local/share`.
    pub data_home: Option<PathBuf>,
    /// Linux: `$APPIMAGE`, the AppImage this process runs from.
    pub appimage: Option<PathBuf>,
    /// Windows: `%LOCALAPPDATA%`.
    pub local_app_data: Option<PathBuf>,
    pub path: Option<OsString>,
}

impl Env {
    /// The environment of this process, for the application `name`.
    pub fn current(name: &str) -> Env {
        let windows = cfg!(windows);
        Env {
            over: std::env::var_os(override_variable(name)),
            exe: std::env::current_exe().ok(),
            data_home: (!windows)
                .then(|| {
                    let env = |v| std::env::var_os(v).map(PathBuf::from);
                    env("XDG_DATA_HOME")
                        .filter(|p| p.is_absolute())
                        .or_else(|| env("HOME").map(|h| h.join(".local/share")))
                })
                .flatten(),
            appimage: (!windows)
                .then(|| std::env::var_os("APPIMAGE").map(PathBuf::from))
                .flatten(),
            local_app_data: windows
                .then(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
                .flatten(),
            path: std::env::var_os("PATH"),
        }
    }
}

/// `MD_SISTER_MDEDIT` for `mdedit`.
fn override_variable(name: &str) -> String {
    let upper = name.to_ascii_uppercase().replace('-', "_");
    format!("MD_SISTER_{upper}")
}

/// A file that can be started: a file, and on Unix one with an execute bit.
fn runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// The executable of the application `name`, looked for in this order: `MD_SISTER_<NAME>`; beside
/// the running executable; the AppImage recorded in `<name>.desktop` (Linux), then another AppImage
/// named `<name>-*.AppImage` beside this one; `%LOCALAPPDATA%\Programs\<name>\<name>.exe`
/// (Windows, where the installers put it); the `PATH`.
pub fn find(name: &str) -> Option<PathBuf> {
    find_in(name, &Env::current(name))
}

/// [`find`] in a given environment.
pub fn find_in(name: &str, env: &Env) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let rules: [&dyn Fn() -> Option<PathBuf>; 6] = [
        &|| {
            env.over
                .as_deref()
                .map(PathBuf::from)
                .filter(|p| runnable(p))
        },
        &|| {
            let beside = env.exe.as_deref()?.parent()?.join(&file);
            runnable(&beside).then_some(beside)
        },
        &|| recorded_appimage(name, env.data_home.as_deref()?),
        &|| appimage_beside(name, env.appimage.as_deref()?),
        &|| {
            let installed = env
                .local_app_data
                .as_deref()?
                .join("Programs")
                .join(name)
                .join(&file);
            runnable(&installed).then_some(installed)
        },
        &|| {
            std::env::split_paths(env.path.as_deref()?)
                .map(|dir| dir.join(&file))
                .find(|p| runnable(p))
        },
    ];
    rules.iter().find_map(|rule| rule())
}

/// The `X-AppImage-Path=` that `mdshared_ui::desktop_entry` records in the application's own entry
/// when it first runs from an AppImage.
fn recorded_appimage(name: &str, data_home: &Path) -> Option<PathBuf> {
    let entry = data_home
        .join("applications")
        .join(format!("{name}.desktop"));
    let text = std::fs::read_to_string(entry).ok()?;
    let recorded = text
        .lines()
        .find_map(|l| l.strip_prefix("X-AppImage-Path="))?;
    // The entry writes a backslash as two.
    let path = PathBuf::from(recorded.replace("\\\\", "\\"));
    runnable(&path).then_some(path)
}

/// An AppImage in the folder of `appimage` named `<name>-*.AppImage`. With several, the one whose
/// name sorts last.
fn appimage_beside(name: &str, appimage: &Path) -> Option<PathBuf> {
    let prefix = format!("{name}-");
    let mut found: Vec<PathBuf> = std::fs::read_dir(appimage.parent()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".AppImage"))
                && runnable(p)
        })
        .collect();
    found.sort();
    found.pop()
}

// ---- starting it and talking to it -------------------------------------------------------------

/// Starts `name` with the arguments that make `request`, detached: no stdio, no console window, a
/// process group of its own. Its own second launch passes the request to a window that is already
/// running. `Err` says what to show the user, as `mdedit is not installed`.
pub fn open(name: &str, request: &Request) -> Result<(), String> {
    let exe = find(name).ok_or_else(|| format!("{name} is not installed"))?;
    spawn(&exe, &request.args(name)).map_err(|e| format!("{name} did not start: {e}"))
}

fn spawn(exe: &Path, args: &[OsString]) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    // Reaps the child when it ends: the other application's own second launch ends at once, and a
    // window that opens lives as long as the user keeps it.
    std::thread::spawn(move || _ = child.wait());
    Ok(())
}

/// The settings file of `name`, whose folder holds the port its running window listens on:
/// `MD_SISTER_<NAME>_SETTINGS` (for tests), else the default.
fn settings_of(name: &str) -> Option<PathBuf> {
    std::env::var_os(format!("{}_SETTINGS", override_variable(name)))
        .map(PathBuf::from)
        .or_else(|| crate::settings::default_settings_path(name))
}

/// Hands `request` to the running window of `name` without starting a process. True when one
/// answered; false when none runs, which for a live link means the other window is gone.
pub fn send(name: &str, request: &Request) -> bool {
    settings_of(name)
        .is_some_and(|settings| crate::single::forward_request(name, &settings, request))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(message: &str) -> Vec<&str> {
        // What a receiver reads: after the name, up to the empty line.
        message
            .lines()
            .skip(1)
            .take_while(|l| !l.is_empty())
            .collect()
    }

    #[test]
    fn a_request_without_options_is_todays_message() {
        let plain = Request::new("/home/u/a.md");
        assert_eq!(plain.encode("mdview"), "mdview\n/home/u/a.md\n\n");
    }

    #[test]
    fn options_come_after_the_path_and_before_the_empty_line() {
        let r = Request {
            line: Some(200),
            column: Some(5),
            place: Some(Rect {
                x: -1920,
                y: 40,
                w: 960,
                h: 1000,
            }),
            from: Some("mdedit".into()),
            ..Request::new("/home/u/a.md")
        };
        let message = r.encode("mdview");
        assert_eq!(
            message,
            "mdview\n/home/u/a.md\n?line=200\n?col=5\n?place=-1920,40,960,1000\n?from=mdedit\n\n"
        );
        assert_eq!(Request::parse(lines(&message)), [r]);
    }

    #[test]
    fn a_follow_request_hides_its_path_from_older_receivers() {
        let r = Request {
            line: Some(7),
            follow: true,
            ..Request::new("/a.md")
        };
        let message = r.encode("mdview");
        assert_eq!(message, "mdview\n?path=/a.md\n?follow\n?line=7\n\n");
        // An older receiver reads every line as a path and keeps the ones that are files. None of
        // these is: it has nothing to open or raise.
        assert!(lines(&message).iter().all(|l| l.starts_with('?')));
        assert_eq!(Request::parse(lines(&message)), [r]);
    }

    #[test]
    fn a_column_needs_a_line_and_odd_values_are_dropped() {
        let r = Request {
            column: Some(3),
            from: Some("two\nlines".into()),
            ..Request::new("/a.md")
        };
        assert_eq!(r.encode("x"), "x\n/a.md\n\n");
        let parsed = Request::parse([
            "/a.md",
            "?line=0",
            "?col=x",
            "?place=1,2,0,4",
            "?from=",
            "?later=1",
            "?",
        ]);
        assert_eq!(parsed, [Request::new("/a.md")]);
    }

    #[test]
    fn several_paths_share_the_options() {
        let parsed = Request::parse(["/a.md", "/b.md", "?line=3"]);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|r| r.line == Some(3)));
        assert_eq!(parsed[1].path, Path::new("/b.md"));
    }

    #[test]
    fn rects() {
        assert_eq!(
            Rect::parse("1,2,3,4"),
            Some(Rect {
                x: 1,
                y: 2,
                w: 3,
                h: 4
            })
        );
        assert_eq!(
            Rect::parse(" -5, 6 ,7,8 "),
            Some(Rect {
                x: -5,
                y: 6,
                w: 7,
                h: 8
            })
        );
        for bad in [
            "",
            "1,2,3",
            "1,2,3,4,5",
            "1,2,0,4",
            "1,2,3,-4",
            "a,b,c,d",
            "0,0,999999,5",
        ] {
            assert_eq!(Rect::parse(bad), None, "{bad}");
        }
        assert_eq!(
            Rect {
                x: -1,
                y: 2,
                w: 3,
                h: 4
            }
            .to_string(),
            "-1,2,3,4"
        );
    }

    #[test]
    fn arguments_for_each_application() {
        let r = Request {
            line: Some(12),
            column: Some(4),
            follow: true,
            place: Some(Rect {
                x: 0,
                y: 0,
                w: 800,
                h: 600,
            }),
            from: Some("mdview".into()),
            ..Request::new("/a.md")
        };
        let text = |target| {
            r.args(target)
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            text("mdedit"),
            "--goto 12:4 --follow --place 0,0,800,600 --from mdview /a.md"
        );
        assert_eq!(
            text("mdview"),
            "--line 12 --follow --place 0,0,800,600 --from mdview /a.md"
        );
        let bare = Request::new("/a.md");
        assert_eq!(bare.args("mdedit"), [OsString::from("/a.md")]);
        let no_column = Request {
            line: Some(9),
            ..bare
        };
        assert_eq!(no_column.args("mdedit")[1], OsString::from("9"));
    }

    // ---- finding ----

    /// A scratch tree: `file` makes an executable file (and its folders), `plain` a non-executable.
    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str) -> Tree {
            let dir =
                std::env::temp_dir().join(format!("mdshared-sister-{tag}-{}", std::process::id()));
            _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tree(dir)
        }

        fn at(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }

        #[cfg_attr(not(unix), allow(unused_variables))]
        fn file(&self, rel: &str, content: &str, executable: bool) -> PathBuf {
            let path = self.at(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = if executable { 0o755 } else { 0o644 };
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const EXE: &str = std::env::consts::EXE_SUFFIX;

    #[test]
    fn the_override_comes_first_and_must_exist() {
        let t = Tree::new("over");
        let over = t.file("tools/fake", "", true);
        let beside = t.file(&format!("app/mdedit{EXE}"), "", true);
        let env = Env {
            over: Some(over.clone().into_os_string()),
            exe: Some(t.at("app/mdview")),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), Some(over));
        let gone = Env {
            over: Some(t.at("nowhere").into_os_string()),
            ..env
        };
        assert_eq!(
            find_in("mdedit", &gone),
            Some(beside),
            "a wrong override falls through"
        );
        assert_eq!(override_variable("mdedit"), "MD_SISTER_MDEDIT");
        assert_eq!(override_variable("md-edit"), "MD_SISTER_MD_EDIT");
    }

    #[test]
    fn beside_the_running_executable() {
        let t = Tree::new("beside");
        let sibling = t.file(&format!("app/mdedit{EXE}"), "", true);
        let env = Env {
            exe: Some(t.at("app/mdview")),
            path: Some(t.at("bin").into_os_string()),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), Some(sibling));
        assert_eq!(find_in("mdother", &env), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_run_is_not_found() {
        let t = Tree::new("noexec");
        t.file("app/mdedit", "", false);
        let env = Env {
            exe: Some(t.at("app/mdview")),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), None);
    }

    #[test]
    fn the_appimage_the_desktop_entry_records() {
        let t = Tree::new("entry");
        let image = t.file("Apps/mdedit-0.4.1-x86_64.AppImage", "", true);
        t.file(
            "share/applications/mdedit.desktop",
            &format!(
                "[Desktop Entry]\nName=mdedit\nX-AppImage-Path={}\n",
                image.display()
            ),
            false,
        );
        let env = Env {
            exe: Some(t.at("elsewhere/mdview")),
            data_home: Some(t.at("share")),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), Some(image));
        // An entry that names a file that is gone says nothing.
        t.file(
            "share/applications/mdedit.desktop",
            "X-AppImage-Path=/nowhere/mdedit.AppImage\n",
            false,
        );
        assert_eq!(find_in("mdedit", &env), None);
    }

    #[test]
    fn another_appimage_beside_this_one() {
        let t = Tree::new("image");
        let mine = t.file("Apps/mdview-0.2.3-x86_64.AppImage", "", true);
        t.file("Apps/mdedit-0.4.0-x86_64.AppImage", "", true);
        let newest = t.file("Apps/mdedit-0.4.1-x86_64.AppImage", "", true);
        t.file("Apps/mdedit-0.4.1-x86_64.AppImage.zsync", "", true);
        t.file("Apps/notes-mdedit.AppImage", "", true);
        let env = Env {
            appimage: Some(mine),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), Some(newest));
    }

    #[test]
    fn the_entry_comes_before_a_neighbour_and_both_before_the_path() {
        let t = Tree::new("order");
        let recorded = t.file("Elsewhere/mdedit-9.AppImage", "", true);
        t.file(
            "share/applications/mdedit.desktop",
            &format!("X-AppImage-Path={}\n", recorded.display()),
            false,
        );
        let mine = t.file("Apps/mdview-1.AppImage", "", true);
        let neighbour = t.file("Apps/mdedit-1.AppImage", "", true);
        let on_path = t.file(&format!("bin/mdedit{EXE}"), "", true);
        let mut env = Env {
            exe: Some(t.at("mnt/usr/bin/mdview")),
            data_home: Some(t.at("share")),
            appimage: Some(mine),
            path: Some(t.at("bin").into_os_string()),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), Some(recorded));
        env.data_home = None;
        assert_eq!(find_in("mdedit", &env), Some(neighbour));
        env.appimage = None;
        assert_eq!(find_in("mdedit", &env), Some(on_path));
        env.path = None;
        assert_eq!(find_in("mdedit", &env), None);
    }

    #[test]
    fn the_per_user_install_folder() {
        let t = Tree::new("local");
        let installed = t.file(&format!("Local/Programs/mdedit/mdedit{EXE}"), "", true);
        let on_path = t.file(&format!("bin/mdedit{EXE}"), "", true);
        let mut env = Env {
            local_app_data: Some(t.at("Local")),
            path: Some(t.at("bin").into_os_string()),
            ..Env::default()
        };
        assert_eq!(find_in("mdedit", &env), Some(installed));
        env.local_app_data = None;
        assert_eq!(find_in("mdedit", &env), Some(on_path));
    }

    #[test]
    fn nothing_found_is_a_message_for_a_toast() {
        // No such application exists, and the override names nothing.
        assert_eq!(
            open("md-no-such-sister", &Request::new("/a.md")),
            Err("md-no-such-sister is not installed".into())
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_starts_the_application_with_the_arguments() {
        let t = Tree::new("spawn");
        let out = t.at("args.txt");
        let script = t.file(
            "fake.sh",
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\n",
                out.display()
            ),
            true,
        );
        let env = Env {
            over: Some(script.into_os_string()),
            ..Env::default()
        };
        let exe = find_in("mdview", &env).unwrap();
        let r = Request {
            line: Some(200),
            follow: true,
            ..Request::new("/a b.md")
        };
        spawn(&exe, &r.args("mdview")).unwrap();
        for _ in 0..200 {
            if std::fs::read_to_string(&out).is_ok_and(|t| t.ends_with("/a b.md\n")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "--line\n200\n--follow\n/a b.md\n"
        );
    }

    #[test]
    fn send_reaches_a_running_window_with_the_options() {
        let t = Tree::new("send");
        let settings = t.at("settings.ini");
        let file = t.file("a.md", "# a\n", false);
        let receiver =
            crate::single::listen_requests_waking("test-sister", &settings, || {}).unwrap();
        let r = Request {
            line: Some(4),
            follow: true,
            from: Some("mdedit".into()),
            ..Request::new(&file)
        };
        assert!(crate::single::forward_request("test-sister", &settings, &r));
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            r
        );
        // A plain launch to the same window is a request with no options.
        assert!(crate::single::forward(
            "test-sister",
            &settings,
            std::slice::from_ref(&file)
        ));
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            Request::new(&file)
        );
        assert!(!crate::single::forward_request("other-app", &settings, &r));
    }
}
