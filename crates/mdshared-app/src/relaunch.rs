//! Starting the new version once an update is installed, and closing this one.
//!
//! The application closes the way its Exit does (the shell's own code: the session is saved, the
//! editor asks about unsaved text) and, after its window and everything it owns are gone, calls
//! [`spawn`] with the file the updater wrote. The new process is started with [`FLAG`]; before it
//! looks for a running instance (see [`crate::single`]) it calls [`wait_for_predecessor`], which
//! waits until the old process has released its instance lock and port. Without that wait the new
//! process would find the dying one, hand it its files, and exit, and no window would be left.
//!
//! [`target`] decides what to start, and it is a pure function of what the updater reported: the
//! installed file on Windows (never a retired `.old` copy), the `$APPIMAGE` file on Linux, and
//! nothing for a copy that cannot update itself.

use crate::single;
use crate::updater::Files;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The argument the new process is started with. Every application ignores it as a file.
pub const FLAG: &str = "--restarted";

/// The label of the bar's button, and of the About dialog's while the bar is closed.
pub const RESTART: &str = "Restart";

/// How long a restarted process waits for the old one to be gone before it carries on as an
/// ordinary launch (which then hands its files to the old one, as any second launch would).
pub const WAIT: Duration = Duration::from_secs(15);

/// What to start for a restart after `files` were replaced: the application's own file, if it
/// exists and is not a staged (`.new`) or retired (`.old`) copy. `None` when the copy cannot
/// update itself (no `files`), so nothing offers a restart.
pub fn target(files: Option<&Files>) -> Option<PathBuf> {
    let app = &files?.app;
    // A path that is itself a leftover of an update names no version worth starting.
    let leftover = [".new", ".old"]
        .iter()
        .any(|suffix| app.as_os_str().to_string_lossy().ends_with(suffix));
    (app.is_file() && !leftover).then(|| app.clone())
}

/// The command that starts the new version: no files (the session brings back what was open),
/// the flag, no terminal to hold, and on Unix its own process group so the old process ending
/// does not take it along. Under an AppImage the variables the old runtime exported are removed,
/// so the new runtime sets its own.
pub fn command(target: &Path) -> Command {
    let mut command = Command::new(target);
    command
        .arg(FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        for var in ["APPDIR", "ARGV0", "OWD"] {
            command.env_remove(var);
        }
    }
    command
}

/// Starts `target`. Call it last, after the window is closed and the session written.
pub fn spawn(target: &Path) -> std::io::Result<()> {
    command(target).spawn().map(drop)
}

/// Whether the command line says this process was started by a restart.
pub fn is_restart(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|a| a == FLAG)
}

/// For a process started by a restart: waits until no instance recorded beside `settings` runs
/// (at most [`WAIT`]). True when it is gone or this is no restart. Call it before the
/// single-instance hand-over.
pub fn wait_for_predecessor(args: impl IntoIterator<Item = String>, settings: &Path) -> bool {
    !is_restart(args) || single::wait_gone(settings, WAIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selfupdate;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("mdshared-relaunch-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn files(app: PathBuf) -> Files {
        Files { app, engine: None }
    }

    #[test]
    fn the_target_is_the_installed_file_and_nothing_else() {
        let d = dir("target");
        let app = d.join(if cfg!(windows) {
            "mdview.exe"
        } else {
            "mdview.AppImage"
        });
        assert_eq!(
            target(None),
            None,
            "a copy that cannot update has no restart"
        );
        assert_eq!(
            target(Some(&files(app.clone()))),
            None,
            "the file is not there"
        );
        std::fs::write(&app, "new").unwrap();
        assert_eq!(target(Some(&files(app.clone()))), Some(app.clone()));
        // A leftover of an update is never what is started.
        for leftover in [selfupdate::retired(&app), selfupdate::staged(&app)] {
            std::fs::write(&leftover, "old").unwrap();
            assert_eq!(target(Some(&files(leftover))), None);
        }
    }

    #[test]
    fn the_new_process_gets_the_flag_and_no_files() {
        let command = command(Path::new("/opt/mdview"));
        assert_eq!(command.get_program(), "/opt/mdview");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [FLAG]);
        assert!(is_restart([FLAG.to_string()]));
        assert!(is_restart(["a.md".to_string(), FLAG.to_string()]));
        assert!(!is_restart(["a.md".to_string()]));
    }

    #[cfg(unix)]
    #[test]
    fn the_old_appimage_runtime_variables_are_not_passed_on() {
        let command = command(Path::new("/opt/mdview"));
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for var in ["APPDIR", "ARGV0", "OWD"] {
            assert!(removed.iter().any(|r| r == var), "{var}");
        }
    }

    #[test]
    fn spawn_starts_the_file_with_the_flag() {
        // A script that records its arguments stands in for the new version.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let d = dir("spawn");
            let script = d.join("new-version");
            let out = d.join("args.txt");
            std::fs::write(
                &script,
                format!("#!/bin/sh\necho \"$@\" > '{}'\n", out.display()),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            spawn(&script).unwrap();
            let end = std::time::Instant::now() + Duration::from_secs(5);
            while !out.exists() && std::time::Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), FLAG);
        }
    }

    #[test]
    fn a_launch_that_is_no_restart_does_not_wait() {
        let d = dir("nowait");
        let settings = d.join("settings.ini");
        let _receiver = single::listen("test-app", &settings).expect("listen");
        let started = std::time::Instant::now();
        assert!(wait_for_predecessor(["a.md".to_string()], &settings));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
