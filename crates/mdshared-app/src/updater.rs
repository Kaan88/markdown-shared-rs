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

/// `eprintln!` for this module, shadowing `std`'s: a stderr that is gone (a closed pipe: os error
/// 232 on Windows, EPIPE elsewhere) is ignored instead of being a panic, which the applications'
/// release builds (`panic = "abort"`) turn into the end of the program. The bytes written are
/// `std`'s. `tests::a_closed_stderr_is_not_a_panic` holds it.
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

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
    /// The manifest of the latest release. The macOS builds read a file of their own beside it
    /// (see [`App::manifest_url`]).
    pub manifest_url: &'static str,
    /// An environment variable that names another manifest, for trying a release before it is
    /// the latest. It is fetched under the same rules: https only, and its hashes are checked.
    pub manifest_env: &'static str,
    /// The file stem of a second executable that ships beside the application's and is replaced
    /// with it (`mdview-engine`), if there is one. The manifest's `engine.*` keys describe it.
    pub engine: Option<&'static str>,
    /// The file stems of further executables beside the application's, replaced with it
    /// (`mdedit-ai`). Only a format-2 manifest lists them (`P.<stem>.url`, `P.<stem>.sha256`), so
    /// an application with any reads a manifest of that format: [`App::manifest_url`] names it.
    pub extras: &'static [&'static str],
    /// The application's entry in Windows' list of installed apps, as `reg.exe` names it
    /// (`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{…}_is1`), if an installer
    /// makes one.
    pub uninstall_key: Option<&'static str>,
}

impl App {
    pub fn current_version(&self) -> Version {
        Version::parse(self.version).expect("the package version is a valid version")
    }

    /// The manifest this build reads: the one named by [`App::manifest_env`] if set, else
    /// [`App::manifest_url`], which a macOS build turns into its own file
    /// (`mdview-update.txt` to `mdview-update-macos.txt`). Builds up to mdview 0.5.0 and mdedit
    /// 0.7.0 refuse a manifest with keys they do not know, so the macOS entries cannot go into the
    /// shared file without stopping their update checks.
    pub fn manifest_url(&self) -> String {
        std::env::var(self.manifest_env).unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                macos_manifest(self.manifest_url)
            } else {
                self.manifest_url.to_string()
            }
        })
    }
}

/// `…/name.txt` to `…/name-macos.txt`; any other URL gets `-macos` at its end.
fn macos_manifest(url: &str) -> String {
    match url.strip_suffix(".txt") {
        Some(stem) => format!("{stem}-macos.txt"),
        None => format!("{url}-macos"),
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
    /// The further executables, in the order of [`App::extras`] (Windows only).
    pub extras: Vec<Download>,
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
    /// The further executables, in the order of [`App::extras`] (Windows only).
    pub extras: Vec<PathBuf>,
}

impl Files {
    fn targets(&self) -> Vec<PathBuf> {
        std::iter::once(self.app.clone())
            .chain(self.engine.clone())
            .chain(self.extras.iter().cloned())
            .collect()
    }
}

/// This process's executable (and the engine beside it, there or not), when this copy can update
/// itself: the folder is not a cargo build's output.
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
    // A missing engine is still this copy's file to replace: an update from a version without
    // one (mdedit before 0.8) brought only the executable, and the next update, or
    // [`restore_engine`] at launch, puts the engine in place.
    let engine = app
        .engine
        .map(|stem| dir.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX)));
    // The same for the further executables (mdedit before 1.1 had no mdedit-ai).
    let extras = app
        .extras
        .iter()
        .map(|stem| dir.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX)))
        .collect();
    Some(Files {
        app: exe,
        engine,
        extras,
    })
}

/// The AppImage this process runs from, when Linux can verify the runtime's open file and
/// its folder is writable. The engine is inside the AppImage. A plain Linux binary updates nothing.
#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn installed_files(_app: &App) -> Option<Files> {
    #[cfg(target_os = "linux")]
    {
        appimage_files(crate::appimage::runtime::current()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The `.app` bundle this process runs from, when this copy can update itself: the bundle's
/// folder can be written to. Not a copy the system runs from a read-only place (an app opened
/// where it was downloaded, before it was moved), nor a bare executable. The engine, if the
/// application has one, is inside the bundle. The owner's decision of 2026-11-16 (it was the
/// download page before).
#[cfg(target_os = "macos")]
pub fn installed_files(_app: &App) -> Option<Files> {
    let bundle = bundle_of(&std::env::current_exe().ok()?)?;
    crate::appimage::folder_is_writable(bundle.parent()?).then_some(Files {
        app: bundle,
        engine: None,
        extras: Vec::new(),
    })
}

/// The bundle `exe` is the executable of (`X.app/Contents/MacOS/exe`), if it is one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bundle_of(exe: &std::path::Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

/// Unpacks a release zip (`ditto -c -k --keepParent`, the bundle at its top) and stages the
/// bundle in it beside `bundle` for [`selfupdate::swap_in`]: only a bundle of the same name with
/// its `Info.plist` and an executable of the running one's name is taken.
#[cfg(target_os = "macos")]
fn stage_bundle(zip: &[u8], bundle: &std::path::Path) -> Result<(), String> {
    let with = |suffix: &str| {
        let mut name = bundle.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    let (archive, unpacked, staged) = (with(".zip"), with(".unzip"), selfupdate::staged(bundle));
    for p in [&archive, &unpacked, &staged] {
        selfupdate::remove_any(p).map_err(|e| e.to_string())?;
    }
    let result = (|| {
        std::fs::write(&archive, zip).map_err(|e| e.to_string())?;
        let status = std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&archive)
            .arg(&unpacked)
            .stdin(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("the download could not be unpacked".to_string());
        }
        let inner = unpacked.join(bundle.file_name().ok_or("no bundle name")?);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_name = exe.file_name().ok_or("no executable name")?;
        if !inner.join("Contents/Info.plist").is_file()
            || !inner.join("Contents/MacOS").join(exe_name).is_file()
        {
            return Err("the download holds no bundle of this application".to_string());
        }
        std::fs::rename(&inner, &staged).map_err(|e| e.to_string())
    })();
    _ = selfupdate::remove_any(&archive);
    _ = selfupdate::remove_any(&unpacked);
    result
}

#[cfg(any(target_os = "linux", test))]
fn appimage_files(app: PathBuf) -> Option<Files> {
    crate::appimage::folder_is_writable(app.parent()?).then_some(Files {
        app,
        engine: None,
        extras: Vec::new(),
    })
}

#[cfg(target_os = "linux")]
fn still_installed(files: &Files) -> bool {
    files.engine.is_none()
        && files.extras.is_empty()
        && crate::appimage::runtime::current().is_some_and(|image| image == files.app)
}

#[cfg(not(target_os = "linux"))]
fn still_installed(_files: &Files) -> bool {
    true
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
        // On Linux the engine is inside the AppImage, on macOS inside the bundle, so the release
        // lists no engine file.
        (_, Platform::LinuxX64 | Platform::MacArm64 | Platform::MacX64) => None,
        // On Windows a release without the engine cannot be installed in place; it is not an offer.
        (Some(_), _) if asset.engine.is_none() => return Ok(None),
        (Some(_), _) => asset.engine.clone(),
        (None, _) => None,
    };
    // The further executables likewise: inside the AppImage or bundle elsewhere; on Windows each
    // must be listed, or the release is not an offer.
    let mut extras = Vec::new();
    if platform == Platform::WindowsX64 {
        for stem in app.extras {
            match asset.extra(stem) {
                Some(d) => extras.push(d.clone()),
                None => return Ok(None),
            }
        }
    }
    Ok(Some(Offer {
        version: manifest.version,
        notes: manifest.notes,
        app: Download {
            url: asset.url,
            sha256: asset.sha256,
        },
        engine,
        extras,
    }))
}

/// For a copy whose engine or a further executable is missing (`files` names a file that is not
/// there): when the manifest is of this very version and lists that file for `platform`, downloads
/// it, checks it against its hash and puts it in place. True when it put any in place. An update
/// from a version that had no such file brings only what that version knew of; this completes it
/// at the next launch.
pub fn restore_engine(
    app: &App,
    files: &Files,
    manifest: &[u8],
    platform: Platform,
    fetch: Fetch,
) -> Result<bool, String> {
    let engine = files.engine.as_ref().filter(|p| !p.exists());
    let extras: Vec<(usize, &PathBuf)> = files
        .extras
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.exists())
        .collect();
    if engine.is_none() && extras.is_empty() {
        return Ok(false);
    }
    let manifest = UpdateManifest::parse(manifest).map_err(|e| e.to_string())?;
    if manifest.version != app.current_version() {
        return Ok(false);
    }
    let Some(asset) = manifest.asset(platform) else {
        return Ok(false);
    };
    let mut wanted: Vec<(&Download, &PathBuf)> = Vec::new();
    if let (Some(target), Some(download)) = (engine, asset.engine.as_ref()) {
        wanted.push((download, target));
    }
    for (i, target) in extras {
        if let Some(download) = app.extras.get(i).and_then(|stem| asset.extra(stem)) {
            wanted.push((download, target));
        }
    }
    let mut restored = false;
    for (download, target) in wanted {
        let bytes = fetch(&download.url, MAX_FILE_BYTES, FILE_TIMEOUT)?;
        if !update::verify_sha256(&bytes, &download.sha256) {
            return Err(format!(
                "{} does not match its SHA-256",
                target.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        let staged = selfupdate::staged(target);
        let written = write_synced(&staged, &bytes)
            .and_then(|()| selfupdate::swap_in(std::slice::from_ref(target)));
        if let Err(e) = written {
            _ = selfupdate::remove_any(&staged);
            return Err(e.to_string());
        }
        restored = true;
    }
    Ok(restored)
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

/// Writes a staged file and has it on the disk before it is swapped in: after a power loss the
/// rename could otherwise be there while the bytes are not (audit 2).
fn write_synced(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(target_os = "linux")]
fn create_staged_appimage(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    // Exclude other accounts before any bytes are written. A later chmod would not
    // revoke a writable descriptor obtained while the file was still being staged.
    std::fs::File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Downloads every file beside its target, checks each against its hash, and swaps them in. On
/// failure nothing is replaced. On Linux, the AppImage's identity must still be verifiable
/// before it stages, swaps or removes files beside the installation, and the swap moves only
/// the file that was verified and the file this call wrote (see `appimage::swap`).
pub fn install(offer: &Offer, files: &Files, fetch: Fetch) -> Result<(), String> {
    install_checked(offer, files, fetch, still_installed)
}

fn install_checked(
    offer: &Offer,
    files: &Files,
    fetch: Fetch,
    verify: impl Fn(&Files) -> bool,
) -> Result<(), String> {
    // What each target is now, noted before the verification that says it is the installation.
    #[cfg(target_os = "linux")]
    let pinned = &mut Pinned::of(&files.targets());
    #[cfg(not(target_os = "linux"))]
    let pinned = &mut Pinned;
    require_installation(verify(files))?;
    #[cfg(target_os = "linux")]
    let _update_lock = crate::appimage::lock_update(&files.app)
        .map_err(|e| format!("cannot lock the installed AppImage for updating: {e}"))?;
    let result = stage_and_swap(offer, files, fetch, &verify, pinned);
    if result.is_err() {
        #[cfg(target_os = "linux")]
        if pinned.any() && verify(files) {
            pinned.discard();
        }
        #[cfg(not(target_os = "linux"))]
        for target in files.targets() {
            _ = selfupdate::remove_any(&selfupdate::staged(&target));
        }
    }
    result
}

fn require_installation(verified: bool) -> Result<(), String> {
    verified
        .then_some(())
        .ok_or_else(|| "the running AppImage no longer identifies this installation".into())
}

// The files a Linux installation holds on to from staging to the swap.
#[cfg(target_os = "linux")]
use crate::appimage::swap::Pinned;

/// Nothing is held elsewhere: the swap there is [`selfupdate::swap_in`], by name.
#[cfg(not(target_os = "linux"))]
struct Pinned;

#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
fn stage_and_swap(
    offer: &Offer,
    files: &Files,
    fetch: Fetch,
    verify: &impl Fn(&Files) -> bool,
    pinned: &mut Pinned,
) -> Result<(), String> {
    let mut pairs = vec![(&offer.app, &files.app)];
    match (&offer.engine, &files.engine) {
        (Some(download), Some(target)) => pairs.push((download, target)),
        (None, None) => {}
        _ => return Err("the release and this copy disagree about the engine".to_string()),
    }
    if offer.extras.len() != files.extras.len() {
        return Err("the release and this copy disagree about their files".to_string());
    }
    pairs.extend(offer.extras.iter().zip(&files.extras));
    for (download, target) in pairs {
        let bytes = fetch(&download.url, MAX_FILE_BYTES, FILE_TIMEOUT)?;
        if !update::verify_sha256(&bytes, &download.sha256) {
            return Err(format!(
                "{} does not match its SHA-256",
                target.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        // A download may take minutes; do not stage beside a path that changed while waiting.
        require_installation(verify(files))?;
        // A macOS bundle comes as a zip of it.
        #[cfg(target_os = "macos")]
        if target.is_dir() {
            stage_bundle(&bytes, target)?;
            continue;
        }
        let staged = selfupdate::staged(target);
        #[cfg(target_os = "linux")]
        {
            use std::io::ErrorKind::AlreadyExists;
            use std::io::Write;
            use std::os::unix::fs::PermissionsExt;
            let f = match create_staged_appimage(&staged) {
                Err(e) if e.kind() == AlreadyExists => {
                    // The name is derived from the verified target and sits beside it.
                    crate::appimage::remove_stale_stage(&staged).map_err(|e| e.to_string())?;
                    create_staged_appimage(&staged)
                }
                result => result,
            }
            .map_err(|e| e.to_string())?;
            // Held open until the swap, which installs this file and no other of its name.
            let f = pinned.hold(target, f);
            f.write_all(&bytes)
                .and_then(|()| f.set_permissions(std::fs::Permissions::from_mode(0o755)))
                .and_then(|()| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(target_os = "linux"))]
        write_synced(&staged, &bytes).map_err(|e| e.to_string())?;
        // An AppImage (or any file written fresh) must be executable once it is swapped in.
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
    }
    require_installation(verify(files))?;
    #[cfg(target_os = "linux")]
    let swapped = pinned.swap();
    #[cfg(not(target_os = "linux"))]
    let swapped = selfupdate::swap_in(&files.targets());
    swapped.map_err(|e| e.to_string())
}

/// `--install-update`: looks for a newer version, installs it as the bar's Update button would,
/// and starts it as the Restart button would; no window. For trying the whole installation on a
/// machine (the macOS runners do, against a manifest named by [`App::manifest_env`]). Returns
/// the process exit code: 0 when the new version was installed or there was nothing newer.
pub fn install_and_restart(app: &App, fetch: Fetch) -> i32 {
    let Some(files) = installed_files(app) else {
        eprintln!("{}: {NOT_INSTALLED}", app.name);
        return 1;
    };
    eprintln!(
        "{} {}; manifest {}",
        app.name,
        app.current_version(),
        app.manifest_url()
    );
    let offer = match check(app, fetch) {
        Ok(Some(offer)) => offer,
        Ok(None) => {
            eprintln!("nothing newer");
            return 0;
        }
        Err(e) => {
            eprintln!("could not look: {e}");
            return 1;
        }
    };
    let target = crate::relaunch::target(Some(&files));
    if let Err(e) = install(&offer, &files, fetch) {
        eprintln!("the update failed: {e}");
        return 1;
    }
    record_version(app, &offer.version);
    eprintln!(
        "installed {} {} at {}",
        app.name,
        offer.version,
        files.app.display()
    );
    if let Some(target) = target
        && let Err(e) = crate::relaunch::spawn(&target)
    {
        eprintln!("could not start it: {e}");
        return 1;
    }
    0
}

/// Removes what an earlier update left beside the executables. On Linux this waits for no
/// installation: while one holds the update lock, nothing is removed until a later call.
pub fn clean_up(files: &Files) {
    clean_up_checked(files, still_installed);
}

fn clean_up_checked(files: &Files, verify: impl Fn(&Files) -> bool) {
    if !verify(files) {
        return;
    }
    // AppImage leftovers are this account's plain files, never directories to be removed
    // recursively, and they are not touched while an installation holds the update lock.
    #[cfg(target_os = "linux")]
    crate::appimage::swap::remove_leftovers(&files.app, &files.targets());
    #[cfg(not(target_os = "linux"))]
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
/// behind and, when `due`, looks for a newer version and posts the answer as an automatic look;
/// the same look puts a missing engine of this version in place ([`restore_engine`]).
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
            let result = fetch(
                &app.manifest_url(),
                update::MAX_MANIFEST_BYTES,
                MANIFEST_TIMEOUT,
            )
            .and_then(|bytes| {
                if let Some(platform) = Platform::current() {
                    _ = restore_engine(&app, &files, &bytes, platform, &fetch);
                }
                offer(&app, &bytes, &app.current_version())
            });
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
    spawn_install_checked(app, offer, files, fetch, post, still_installed);
}

fn spawn_install_checked(
    app: App,
    offer: Offer,
    files: Files,
    fetch: FetchFn,
    post: impl Fn(Event) + Send + 'static,
    verify: fn(&Files) -> bool,
) {
    spawn(&app, move || {
        let result = install_checked(&offer, &files, &fetch, verify);
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
} else if cfg!(target_os = "macos") {
    "Move the app to the Applications folder to update it in place."
} else {
    "Only the AppImage can update itself."
};

/// The update as the application's window shows it: the stage, and whether its bar was closed.
#[derive(Clone, Debug, Default)]
pub struct Updates {
    stage: Stage,
    hidden: bool,
    /// The file a restart starts, known when the installation began (the installed file's path,
    /// before the swap moves anything). Kept only while the stage is `Installed`.
    relaunch: Option<std::path::PathBuf>,
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

    /// Records what a restart would start, for the installation about to begin (see
    /// [`crate::relaunch::target`]; it is checked again when the restart is made). The bar offers
    /// a restart only if there is one.
    pub fn set_relaunch(&mut self, target: Option<std::path::PathBuf>) {
        self.relaunch = target;
    }

    /// The file to start for a restart: only once the update is installed, and only when there is
    /// one to start.
    pub fn relaunch_target(&self) -> Option<&std::path::Path> {
        match self.stage {
            Stage::Installed(_) => self.relaunch.as_deref(),
            _ => None,
        }
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
                self.relaunch = None;
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

    /// The bar's text for `stage` when the bar has a Restart button beside it (`restart`): the
    /// installed version then starts when the reader says so, not the next time the
    /// application is opened. Every other stage, and a bar without the button, reads as
    /// [`bar_label`](Self::bar_label).
    pub fn bar_label_with(app: &App, stage: &Stage, restart: bool) -> String {
        match stage {
            Stage::Installed(version) if restart => {
                format!("{} {version} is installed. Restart to use it.", app.name)
            }
            _ => Self::bar_label(app, stage),
        }
    }

    /// The About dialog's update button: what it would do now.
    pub fn about_label(&self) -> String {
        match &self.stage {
            Stage::Available(offer) => format!("Update to {}", offer.version),
            Stage::Installing(_) => BUSY.to_string(),
            Stage::Installed(_) if self.relaunch.is_some() => crate::relaunch::RESTART.to_string(),
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
            extras: Vec::new(),
        };
        self.stage = match stage {
            "available" => Stage::Available(offer),
            "installing" => Stage::Installing(offer),
            "installed" => Stage::Installed(offer.version),
            // Installed, but nothing to start (a copy the shell cannot restart).
            "installed-plain" => Stage::Installed(offer.version),
            _ => Stage::None,
        };
        self.relaunch = (stage == "installed").then(|| std::path::PathBuf::from("/preview/app"));
        self.hidden = false;
    }
}

#[cfg(test)]
#[path = "updater_tests.rs"]
mod tests;
