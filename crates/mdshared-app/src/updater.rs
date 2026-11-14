//! An application that updates itself in place: look for a newer version, download its
//! executables, check their hashes, and swap them in.
//!
//! The manifest format, version ordering and SHA-256 are [`crate::update`]; the swap is
//! [`crate::selfupdate`]. This module ties them together for one application, described by an
//! [`App`], and keeps what the reader sees in [`Updates`]. It does no networking of its own: the
//! caller hands in a [`Fetch`] (`mdshared_fetch::fetch_file` in the applications).
//!
//! [`check`] and [`install`] block, so they run on a thread of their own: [`spawn_at_launch`],
//! [`spawn_check`] and [`spawn_install`] start one and hand the [`Event`] to the shell's `post`,
//! which sends it to its event loop. The binaries are not code-signed, so the hash in the
//! manifest, fetched over TLS, is the only integrity check there is.

use crate::selfupdate;
use crate::update::{self, Download, Platform, UpdateManifest, Version};
use std::path::PathBuf;
use std::time::Duration;

const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);
/// For every file of a release on a slow line.
const FILE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;

/// The application that updates itself.
#[derive(Clone, Copy, Debug)]
pub struct App {
    /// The name the reader knows it by, for the messages: `mdview`.
    pub name: &'static str,
    /// The running version: the application's own `env!("CARGO_PKG_VERSION")`.
    pub version: &'static str,
    /// The manifest of the latest release.
    pub manifest_url: &'static str,
    /// An environment variable that names another manifest, for trying a release before it is
    /// the latest. It is fetched under the same rules: https only, and its hashes are checked.
    pub manifest_env: &'static str,
    /// The file stem of a second executable that ships beside the application's and is replaced
    /// with it (`mdview-engine`), if there is one. The manifest's `engine.*` keys describe it.
    pub engine: Option<&'static str>,
    /// The application's entry in Windows' list of installed apps, as `reg.exe` names it
    /// (`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{…}_is1`), if an installer
    /// makes one.
    pub uninstall_key: Option<&'static str>,
}

impl App {
    pub fn current_version(&self) -> Version {
        Version::parse(self.version).expect("the package version is a valid version")
    }

    pub fn manifest_url(&self) -> String {
        std::env::var(self.manifest_env).unwrap_or_else(|_| self.manifest_url.to_string())
    }
}

/// A newer version this build can install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    pub notes: Option<String>,
    pub app: Download,
    /// The second executable, for an application that has one.
    pub engine: Option<Download>,
}

/// Where the reader's update stands, for the bar over the page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    None,
    Available(Offer),
    Installing(Offer),
    /// The files are in place; the running process is still the old version.
    Installed(Version),
}

/// What an updater thread reports back.
#[derive(Debug)]
pub enum Event {
    Checked {
        /// The reader asked, so the answer is shown even when there is nothing new.
        manual: bool,
        result: Result<Option<Offer>, String>,
    },
    Installed(Result<(), String>),
}

/// One GET: URL, size cap, time allowed.
pub type Fetch<'a> = &'a dyn Fn(&str, usize, Duration) -> Result<Vec<u8>, String>;

/// The files an update replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Files {
    pub app: PathBuf,
    pub engine: Option<PathBuf>,
}

impl Files {
    fn targets(&self) -> Vec<PathBuf> {
        std::iter::once(self.app.clone())
            .chain(self.engine.clone())
            .collect()
    }
}

/// This process's executable (and the engine beside it), when this copy can update itself: the
/// files are there, and the folder is not a cargo build's output.
#[cfg(windows)]
pub fn installed_files(app: &App) -> Option<Files> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let built = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n, "release" | "debug" | "deps"));
    if built {
        return None;
    }
    let engine = match app.engine {
        Some(stem) => {
            let path = dir.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
            if !path.is_file() {
                return None;
            }
            Some(path)
        }
        None => None,
    };
    Some(Files { app: exe, engine })
}

/// The AppImage this process runs from, when this copy can update itself: `$APPIMAGE` names an
/// existing file whose folder can be written to. The engine, if the application has one, is inside
/// the AppImage, so there is none to replace. A plain Linux binary updates nothing.
#[cfg(not(windows))]
pub fn installed_files(_app: &App) -> Option<Files> {
    appimage_files(std::env::var_os("APPIMAGE").as_deref())
}

/// [`installed_files`] given the value of `$APPIMAGE`.
#[cfg(not(windows))]
fn appimage_files(value: Option<&std::ffi::OsStr>) -> Option<Files> {
    let app = crate::appimage::path_from(value)?;
    crate::appimage::folder_is_writable(app.parent()?).then_some(Files { app, engine: None })
}

/// Reads a manifest: the offer if it names a version newer than `current` with every file this
/// application needs for this platform.
pub fn offer(app: &App, manifest: &[u8], current: &Version) -> Result<Option<Offer>, String> {
    let Some(platform) = Platform::current() else {
        return Ok(None);
    };
    offer_for(app, manifest, current, platform)
}

/// [`offer`] for a given platform.
fn offer_for(
    app: &App,
    manifest: &[u8],
    current: &Version,
    platform: Platform,
) -> Result<Option<Offer>, String> {
    let manifest = UpdateManifest::parse(manifest).map_err(|e| e.to_string())?;
    let Some(asset) = update::newer(current, &manifest, platform) else {
        return Ok(None);
    };
    let engine = match (app.engine, platform) {
        // On Linux the engine is inside the AppImage, so the release lists no engine file.
        (_, Platform::LinuxX64) => None,
        // On Windows a release without the engine cannot be installed in place; it is not an offer.
        (Some(_), _) if asset.engine.is_none() => return Ok(None),
        (Some(_), _) => asset.engine,
        (None, _) => None,
    };
    Ok(Some(Offer {
        version: manifest.version,
        notes: manifest.notes,
        app: Download {
            url: asset.url,
            sha256: asset.sha256,
        },
        engine,
    }))
}

/// Fetches the manifest and reads it.
pub fn check(app: &App, fetch: Fetch) -> Result<Option<Offer>, String> {
    let bytes = fetch(
        &app.manifest_url(),
        update::MAX_MANIFEST_BYTES,
        MANIFEST_TIMEOUT,
    )?;
    offer(app, &bytes, &app.current_version())
}

/// Downloads every file beside its target, checks each against its hash, and swaps them in. On
/// any failure nothing is replaced and the downloads are removed.
pub fn install(offer: &Offer, files: &Files, fetch: Fetch) -> Result<(), String> {
    let result = stage_and_swap(offer, files, fetch);
    if result.is_err() {
        for target in files.targets() {
            _ = std::fs::remove_file(selfupdate::staged(&target));
        }
    }
    result
}

fn stage_and_swap(offer: &Offer, files: &Files, fetch: Fetch) -> Result<(), String> {
    let mut pairs = vec![(&offer.app, &files.app)];
    match (&offer.engine, &files.engine) {
        (Some(download), Some(target)) => pairs.push((download, target)),
        (None, None) => {}
        _ => return Err("the release and this copy disagree about the engine".to_string()),
    }
    for (download, target) in pairs {
        let bytes = fetch(&download.url, MAX_FILE_BYTES, FILE_TIMEOUT)?;
        if !update::verify_sha256(&bytes, &download.sha256) {
            return Err(format!(
                "{} does not match its SHA-256",
                target.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        let staged = selfupdate::staged(target);
        std::fs::write(&staged, &bytes).map_err(|e| e.to_string())?;
        // An AppImage (or any file written fresh) must be executable once it is swapped in.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
    }
    selfupdate::swap_in(&files.targets()).map_err(|e| e.to_string())
}

/// Removes what an earlier update left beside the executables.
pub fn clean_up(files: &Files) {
    selfupdate::clean_up(&files.targets());
}

/// Writes the new version into the installed-apps entry, if the installer made one. Best effort:
/// the entry only says what Settings shows.
#[cfg(windows)]
pub fn record_version(app: &App, version: &Version) {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Some(key) = app.uninstall_key else {
        return;
    };
    let reg = |args: &[&str]| {
        Command::new("reg.exe")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success())
    };
    if !reg(&["query", key, "/v", "DisplayVersion"]) {
        return;
    }
    let version = version.to_string();
    reg(&[
        "add",
        key,
        "/v",
        "DisplayVersion",
        "/t",
        "REG_SZ",
        "/d",
        &version,
        "/f",
    ]);
}

#[cfg(not(windows))]
pub fn record_version(_app: &App, _version: &Version) {}

/// True when a launch should look for a newer version: the reader's setting is on and there is
/// a settings file to remember it in. Every launch looks. It used to be once a day, and a
/// release published an hour after a look then stayed unseen until the next day, which read as
/// the automatic look not working. The times stay in the signature for the callers.
pub fn check_due(enabled: bool, remembers: bool, _last: u64, _now: u64) -> bool {
    enabled && remembers
}

// ---- threads -----------------------------------------------------------------------------------

/// A [`Fetch`] a thread can take with it.
pub type FetchFn = fn(&str, usize, Duration) -> Result<Vec<u8>, String>;

/// Seconds since the Unix epoch, for [`check_due`] and the time of the last look.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Runs `work` on a thread named for the application's updater (`mdview-update`); if the thread
/// cannot start, nothing happens.
fn spawn(app: &App, work: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name(format!("{}-update", app.name))
        .spawn(work);
}

/// At launch, for a copy that can update itself (`files`): clears what an earlier update left
/// behind and, when `due`, looks for a newer version and posts the answer as an automatic look.
pub fn spawn_at_launch(
    app: App,
    files: Files,
    due: bool,
    fetch: FetchFn,
    post: impl Fn(Event) + Send + 'static,
) {
    spawn(&app, move || {
        clean_up(&files);
        if due {
            let result = check(&app, &fetch);
            post(Event::Checked {
                manual: false,
                result,
            });
        }
    });
}

/// Looks for a newer version now and posts the answer.
pub fn spawn_check(app: App, manual: bool, fetch: FetchFn, post: impl Fn(Event) + Send + 'static) {
    spawn(&app, move || {
        let result = check(&app, &fetch);
        post(Event::Checked { manual, result });
    });
}

/// Downloads and swaps in `offer`, records the version with the system on success, and posts
/// how it went.
pub fn spawn_install(
    app: App,
    offer: Offer,
    files: Files,
    fetch: FetchFn,
    post: impl Fn(Event) + Send + 'static,
) {
    spawn(&app, move || {
        let result = install(&offer, &files, &fetch);
        if result.is_ok() {
            record_version(&app, &offer.version);
        }
        post(Event::Installed(result));
    });
}

// ---- what the reader sees ----------------------------------------------------------------------

/// The label of the bar's button.
pub const GO: &str = "Update";
/// The About dialog's button while nothing newer is known.
pub const CHECK: &str = "Check for updates";
const BUSY: &str = "Updating…";
const AT_LAUNCH_ON: &str = "Check at launch: on";
const AT_LAUNCH_OFF: &str = "Check at launch: off";

/// The About dialog's toggle for the look at launch.
pub fn at_launch_label(on: bool) -> &'static str {
    if on { AT_LAUNCH_ON } else { AT_LAUNCH_OFF }
}

/// Shown when the reader asks a copy that is not installed to update.
pub const NOT_INSTALLED: &str = if cfg!(windows) {
    "Only an installed copy can update itself."
} else {
    "Only the AppImage can update itself."
};

/// The update as the application's window shows it: the stage, and whether its bar was closed.
#[derive(Clone, Debug, Default)]
pub struct Updates {
    stage: Stage,
    hidden: bool,
}

impl Updates {
    /// A look for a newer version finished. A failed automatic look says nothing; one the reader
    /// asked for always answers, with the returned message when there is no bar to show.
    pub fn checked(
        &mut self,
        app: &App,
        manual: bool,
        result: Result<Option<Offer>, String>,
    ) -> Option<String> {
        match result {
            Ok(Some(offer)) => {
                // Never step back from an installation under way or done.
                if matches!(self.stage, Stage::None | Stage::Available(_)) {
                    self.stage = Stage::Available(offer);
                    self.hidden = false;
                }
                None
            }
            Ok(None) if manual => Some(format!(
                "{} {} is the newest version.",
                app.name,
                app.current_version()
            )),
            Err(e) if manual => {
                eprintln!("{}: update check failed: {e}", app.name);
                Some("Could not look for a newer version.".to_string())
            }
            _ => None,
        }
    }

    /// The reader chose to update: the offer to install, once.
    pub fn begin(&mut self) -> Option<Offer> {
        let Stage::Available(offer) = &self.stage else {
            return None;
        };
        let offer = offer.clone();
        self.stage = Stage::Installing(offer.clone());
        self.hidden = false;
        Some(offer)
    }

    /// The installation ended. A failure puts the offer back, so the reader can try again, and
    /// returns the message to show.
    pub fn finished(&mut self, app: &App, result: Result<(), String>) -> Option<String> {
        let Stage::Installing(offer) = std::mem::take(&mut self.stage) else {
            return None;
        };
        self.hidden = false;
        match result {
            Ok(()) => {
                self.stage = Stage::Installed(offer.version);
                None
            }
            Err(e) => {
                eprintln!("{}: update failed: {e}", app.name);
                self.stage = Stage::Available(offer);
                Some("The update could not be installed. Nothing was changed.".to_string())
            }
        }
    }

    /// Closes the bar until the stage changes. An installation under way keeps its bar.
    pub fn dismiss(&mut self) {
        if !matches!(self.stage, Stage::Installing(_)) {
            self.hidden = true;
        }
    }

    /// What the bar shows, if it shows.
    pub fn bar(&self) -> Option<&Stage> {
        (!self.hidden && self.stage != Stage::None).then_some(&self.stage)
    }

    /// The update as it stands, shown or not (About offers it again after the bar is closed).
    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// The bar's text for `stage`.
    pub fn bar_label(app: &App, stage: &Stage) -> String {
        let name = app.name;
        match stage {
            Stage::None => String::new(),
            Stage::Available(offer) => {
                let text = format!("{name} {} is available.", offer.version);
                match &offer.notes {
                    Some(notes) => format!("{text} {notes}"),
                    None => text,
                }
            }
            Stage::Installing(offer) => format!("Downloading {name} {}…", offer.version),
            Stage::Installed(version) => {
                format!("{name} {version} is installed. It starts the next time you open {name}.")
            }
        }
    }

    /// The About dialog's update button: what it would do now.
    pub fn about_label(&self) -> String {
        match &self.stage {
            Stage::Available(offer) => format!("Update to {}", offer.version),
            Stage::Installing(_) => BUSY.to_string(),
            _ => CHECK.to_string(),
        }
    }

    /// Puts the bar at a stage (`available`, `installing` or `installed`) with a made-up
    /// version, for a screenshot or a test.
    pub fn preview(&mut self, stage: &str) {
        let download = |name: &str| Download {
            url: format!("https://example.com/{name}"),
            sha256: "0".repeat(64),
        };
        let offer = Offer {
            version: Version::parse("0.2.0").expect("a valid version"),
            notes: Some("Faster scrolling in long documents.".to_string()),
            app: download("app.exe"),
            engine: Some(download("engine.exe")),
        };
        self.stage = match stage {
            "available" => Stage::Available(offer),
            "installing" => Stage::Installing(offer),
            "installed" => Stage::Installed(offer.version),
            _ => Stage::None,
        };
        self.hidden = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::sha256_hex;
    use std::cell::RefCell;

    const APP: App = App {
        name: "mdtest",
        version: "0.2.0",
        manifest_url: "https://example.com/update.txt",
        manifest_env: "MDSHARED_TEST_UPDATE_MANIFEST",
        engine: Some("mdtest-engine"),
        uninstall_key: None,
    };
    const ALONE: App = App {
        engine: None,
        ..APP
    };

    fn manifest(version: &str, app: &[u8], engine: Option<&[u8]>) -> String {
        let p = if cfg!(windows) {
            "windows-x86_64"
        } else {
            "linux-x86_64"
        };
        manifest_for(p, version, app, engine)
    }

    fn manifest_for(p: &str, version: &str, app: &[u8], engine: Option<&[u8]>) -> String {
        let mut m = format!(
            "manifest: 1\nversion: {version}\nnotes: Faster.\n{p}.asset: app.exe\n\
             {p}.url: https://example.com/app\n{p}.sha256: {}\n",
            sha256_hex(app)
        );
        if let Some(engine) = engine {
            m.push_str(&format!(
                "{p}.engine.url: https://example.com/engine\n{p}.engine.sha256: {}\n",
                sha256_hex(engine)
            ));
        }
        m
    }

    fn win_manifest(version: &str, app: &[u8], engine: Option<&[u8]>) -> String {
        manifest_for("windows-x86_64", version, app, engine)
    }

    /// An offer as the Windows build reads a manifest, whatever this test runs on.
    fn win_offer(app: &App, manifest: &str, current: &Version) -> Result<Option<Offer>, String> {
        offer_for(app, manifest.as_bytes(), current, Platform::WindowsX64)
    }

    fn files(name: &str, engine: bool) -> Files {
        let dir =
            std::env::temp_dir().join(format!("mdshared-updater-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files = Files {
            app: dir.join("app.exe"),
            engine: engine.then(|| dir.join("engine.exe")),
        };
        std::fs::write(&files.app, "app 1").unwrap();
        if let Some(engine) = &files.engine {
            std::fs::write(engine, "engine 1").unwrap();
        }
        files
    }

    fn read(p: &std::path::Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    /// A release of `ALONE` whose executable reads "app 2", for the threads (a `FetchFn` cannot
    /// capture anything).
    fn release_fetch(url: &str, _: usize, _: Duration) -> Result<Vec<u8>, String> {
        if url.ends_with("update.txt") {
            Ok(manifest("0.3.0", b"app 2", None).into_bytes())
        } else {
            Ok(b"app 2".to_vec())
        }
    }

    #[test]
    fn the_threads_post_what_happened() {
        use std::sync::mpsc::channel;
        let wait = Duration::from_secs(20);
        let files = files("threads", false);
        let (tx, rx) = channel();
        let post = move |e: Event| _ = tx.send(e);

        // A launch with no look due only cleans up; one that is due answers as automatic.
        spawn_at_launch(ALONE, files.clone(), false, release_fetch, post.clone());
        spawn_at_launch(ALONE, files.clone(), true, release_fetch, post.clone());
        let Ok(Event::Checked {
            manual: false,
            result: Ok(Some(found)),
        }) = rx.recv_timeout(wait)
        else {
            panic!("the look at launch did not post an offer");
        };
        assert_eq!(found.version.to_string(), "0.3.0");

        spawn_check(ALONE, true, release_fetch, post.clone());
        assert!(matches!(
            rx.recv_timeout(wait),
            Ok(Event::Checked {
                manual: true,
                result: Ok(Some(_))
            })
        ));

        spawn_install(ALONE, found, files.clone(), release_fetch, post);
        assert!(matches!(
            rx.recv_timeout(wait),
            Ok(Event::Installed(Ok(())))
        ));
        assert_eq!(read(&files.app), "app 2");
        // Nothing else was posted: the launch that was not due said nothing.
        assert!(rx.try_recv().is_err());
        _ = std::fs::remove_dir_all(files.app.parent().unwrap());
    }

    #[test]
    fn only_a_newer_version_with_every_file_is_an_offer() {
        let current = APP.current_version();
        let newer = win_manifest("0.3.0", b"a", Some(b"e"));
        let o = win_offer(&APP, &newer, &current).unwrap().unwrap();
        assert_eq!(o.version.to_string(), "0.3.0");
        assert_eq!(o.notes.as_deref(), Some("Faster."));
        assert_eq!(o.app.url, "https://example.com/app");
        assert_eq!(o.engine.unwrap().sha256, sha256_hex(b"e"));

        for not_offered in [
            win_manifest("0.2.0", b"a", Some(b"e")),
            win_manifest("0.1.9", b"a", Some(b"e")),
            win_manifest("0.3.0", b"a", None),
        ] {
            assert_eq!(win_offer(&APP, &not_offered, &current), Ok(None));
        }
        assert!(win_offer(&APP, "<html>Not Found</html>", &current).is_err());

        // An application with one executable needs no engine, and ignores one that is listed.
        for engine in [None, Some(&b"e"[..])] {
            let m = win_manifest("0.3.0", b"a", engine);
            let o = win_offer(&ALONE, &m, &current).unwrap().unwrap();
            assert_eq!(o.engine, None);
        }
    }

    #[test]
    fn install_checks_every_hash_before_anything_is_replaced() {
        let files = files("install", true);
        let engine = files.engine.clone().unwrap();
        let m = win_manifest("9.0.0", b"app 2", Some(b"engine 2"));
        let o = win_offer(&APP, &m, &APP.current_version())
            .unwrap()
            .unwrap();

        // The engine arrives damaged: nothing changes, and no download is left behind.
        let asked = RefCell::new(Vec::new());
        let damaged = |url: &str, _: usize, _: Duration| {
            asked.borrow_mut().push(url.to_string());
            Ok(if url.ends_with("/app") {
                b"app 2".to_vec()
            } else {
                b"engine 2, damaged".to_vec()
            })
        };
        let err = install(&o, &files, &damaged).unwrap_err();
        assert!(err.contains("engine.exe"), "{err}");
        assert_eq!(asked.borrow().len(), 2);
        assert_eq!(read(&files.app), "app 1");
        assert_eq!(read(&engine), "engine 1");
        assert!(!selfupdate::staged(&files.app).exists());
        assert!(!selfupdate::staged(&engine).exists());

        // A failed download likewise.
        let offline = |_: &str, _: usize, _: Duration| Err("Timeout".to_string());
        assert_eq!(install(&o, &files, &offline), Err("Timeout".to_string()));
        assert_eq!(read(&files.app), "app 1");

        let good = |url: &str, _: usize, _: Duration| {
            Ok(if url.ends_with("/app") {
                b"app 2".to_vec()
            } else {
                b"engine 2".to_vec()
            })
        };
        install(&o, &files, &good).unwrap();
        assert_eq!(read(&files.app), "app 2");
        assert_eq!(read(&engine), "engine 2");
        assert_eq!(read(&selfupdate::retired(&files.app)), "app 1");
        clean_up(&files);
        assert!(!selfupdate::retired(&files.app).exists());
        assert!(!selfupdate::retired(&engine).exists());
        _ = std::fs::remove_dir_all(files.app.parent().unwrap());
    }

    #[test]
    fn an_application_with_one_executable_installs_one_file() {
        let files = files("alone", false);
        let m = manifest("9.0.0", b"app 2", None);
        let o = offer(&ALONE, m.as_bytes(), &ALONE.current_version())
            .unwrap()
            .unwrap();
        let asked = RefCell::new(0);
        let good = |_: &str, _: usize, _: Duration| {
            *asked.borrow_mut() += 1;
            Ok(b"app 2".to_vec())
        };
        install(&o, &files, &good).unwrap();
        assert_eq!(*asked.borrow(), 1);
        assert_eq!(read(&files.app), "app 2");
        _ = std::fs::remove_dir_all(files.app.parent().unwrap());
    }

    /// A manifest listing both platforms: the Windows one with its engine, the Linux one without.
    fn both_platforms(version: &str) -> String {
        format!(
            "{}{}",
            win_manifest(version, b"win app", Some(b"win engine")),
            manifest_for("linux-x86_64", version, b"linux app", None)
                .lines()
                .skip(3)
                .map(|l| format!("{l}\n"))
                .collect::<String>()
        )
    }

    #[test]
    fn each_platform_picks_its_own_entry_of_a_shared_manifest() {
        let m = both_platforms("0.3.0");
        let current = APP.current_version();
        let win = offer_for(&APP, m.as_bytes(), &current, Platform::WindowsX64);
        let win = win.unwrap().unwrap();
        assert_eq!(win.app.sha256, sha256_hex(b"win app"));
        assert_eq!(win.engine.unwrap().sha256, sha256_hex(b"win engine"));
        let linux = offer_for(&APP, m.as_bytes(), &current, Platform::LinuxX64);
        let linux = linux.unwrap().unwrap();
        assert_eq!(linux.app.sha256, sha256_hex(b"linux app"));
        assert_eq!(linux.engine, None);
        // The application with one executable reads the same manifest the same way.
        let alone = offer_for(&ALONE, m.as_bytes(), &current, Platform::LinuxX64);
        assert_eq!(alone.unwrap().unwrap().app.sha256, sha256_hex(b"linux app"));
    }

    #[test]
    fn a_linux_release_needs_no_engine_key_even_for_an_application_with_an_engine() {
        let m = manifest_for("linux-x86_64", "0.3.0", b"a", None);
        let o = offer_for(
            &APP,
            m.as_bytes(),
            &APP.current_version(),
            Platform::LinuxX64,
        );
        let o = o.unwrap().unwrap();
        assert_eq!(o.engine, None);
        // Windows still demands it.
        let w = manifest_for("windows-x86_64", "0.3.0", b"a", None);
        let w = offer_for(
            &APP,
            w.as_bytes(),
            &APP.current_version(),
            Platform::WindowsX64,
        );
        assert_eq!(w, Ok(None));
    }

    #[cfg(not(windows))]
    #[test]
    fn only_an_appimage_in_a_writable_folder_updates_itself() {
        use std::ffi::OsStr;
        let files = files("appimage", false);
        let dir = files.app.parent().unwrap().to_path_buf();
        let canonical = std::fs::canonicalize(&files.app).unwrap();
        assert_eq!(
            appimage_files(Some(files.app.as_os_str())),
            Some(Files {
                app: canonical,
                engine: None
            })
        );
        assert_eq!(appimage_files(None), None);
        assert_eq!(appimage_files(Some(OsStr::new(""))), None);
        assert_eq!(appimage_files(Some(dir.as_os_str())), None, "a folder");
        assert_eq!(appimage_files(Some(dir.join("gone").as_os_str())), None);
        // A read-only folder: skipped when the tests run as root, which ignores the mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
            if !crate::appimage::folder_is_writable(&dir) {
                assert_eq!(appimage_files(Some(files.app.as_os_str())), None);
            }
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn an_updated_appimage_is_executable_and_the_leftovers_go() {
        use std::os::unix::fs::PermissionsExt;
        let files = files("exec", false);
        // The old file is not executable here, so the bit must come from the update.
        std::fs::set_permissions(&files.app, std::fs::Permissions::from_mode(0o644)).unwrap();
        let m = manifest_for("linux-x86_64", "9.0.0", b"app 2", None);
        let o = offer_for(
            &APP,
            m.as_bytes(),
            &APP.current_version(),
            Platform::LinuxX64,
        );
        let o = o.unwrap().unwrap();
        let good = |_: &str, _: usize, _: Duration| Ok(b"app 2".to_vec());
        install(&o, &files, &good).unwrap();
        assert_eq!(read(&files.app), "app 2");
        let mode = std::fs::metadata(&files.app).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        // The old copy waits beside it as `.old`; clean_up removes it.
        let old = selfupdate::retired(&files.app);
        assert_eq!(read(&old), "app 1");
        clean_up(&files);
        assert!(!old.exists() && !selfupdate::staged(&files.app).exists());
        _ = std::fs::remove_dir_all(files.app.parent().unwrap());
    }

    #[test]
    fn a_cargo_build_does_not_update_itself() {
        // The test binary runs from `target/release/deps`.
        assert_eq!(installed_files(&APP), None);
        assert_eq!(installed_files(&ALONE), None);
    }

    #[test]
    fn a_look_is_due_at_every_launch_when_it_can_be_remembered() {
        assert!(check_due(true, true, 0, 1000), "never looked before");
        assert!(check_due(true, true, 999, 1000), "looked a second ago");
        assert!(!check_due(false, true, 0, 1000), "turned off");
        assert!(!check_due(true, false, 0, 1000), "nowhere to remember");
    }

    #[test]
    fn the_stages_follow_the_reader() {
        let mut u = Updates::default();
        assert_eq!(u.bar(), None);
        // Nothing new, and a failed look: an automatic look says nothing either way.
        assert_eq!(u.checked(&APP, false, Ok(None)), None);
        assert_eq!(u.checked(&APP, false, Err("Timeout".into())), None);
        assert_eq!(u.bar(), None);
        // One the reader asked for answers.
        let said = u.checked(&APP, true, Ok(None)).unwrap();
        assert_eq!(said, "mdtest 0.2.0 is the newest version.");
        assert!(u.checked(&APP, true, Err("Timeout".into())).is_some());
        assert_eq!(u.about_label(), CHECK);

        u.preview("available");
        let Stage::Available(offer) = u.stage().clone() else {
            panic!("an offer");
        };
        assert_eq!(u.checked(&APP, false, Ok(Some(offer.clone()))), None);
        assert_eq!(
            Updates::bar_label(&APP, u.bar().unwrap()),
            "mdtest 0.2.0 is available. Faster scrolling in long documents."
        );
        // Closing the bar hides it; the offer stays for the About dialog.
        u.dismiss();
        assert_eq!(u.bar(), None);
        assert_eq!(u.about_label(), "Update to 0.2.0");

        // A failed installation puts the offer back and says so.
        assert_eq!(u.begin(), Some(offer.clone()));
        assert_eq!(u.begin(), None, "one installation at a time");
        u.dismiss();
        assert!(matches!(u.bar(), Some(Stage::Installing(_))));
        let said = u.finished(&APP, Err("no match".into())).unwrap();
        assert!(said.contains("Nothing was changed"));
        assert!(matches!(u.bar(), Some(Stage::Available(_))));

        assert!(u.begin().is_some());
        assert_eq!(u.finished(&APP, Ok(())), None);
        assert_eq!(u.bar(), Some(&Stage::Installed(offer.version.clone())));
        // A later look does not step back from an installed update.
        u.checked(&APP, false, Ok(Some(offer)));
        assert!(matches!(u.stage(), Stage::Installed(_)));
    }
}
