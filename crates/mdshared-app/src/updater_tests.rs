use super::*;
use crate::update::sha256_hex;
use std::cell::RefCell;

// Generic swap tests use manufactured targets even on Linux. There, exercise the same
// staging and swap with a fixture verifier; the public entry points also have guard tests.
#[cfg(target_os = "linux")]
fn install(offer: &Offer, files: &Files, fetch: Fetch) -> Result<(), String> {
    install_checked(offer, files, fetch, |_| true)
}

#[cfg(target_os = "linux")]
fn clean_up(files: &Files) {
    clean_up_checked(files, |_| true);
}

#[cfg(target_os = "linux")]
fn spawn_install(
    app: App,
    offer: Offer,
    files: Files,
    fetch: FetchFn,
    post: impl Fn(Event) + Send + 'static,
) {
    spawn_install_checked(app, offer, files, fetch, post, |_| true);
}

// The historical fixture supplies a path in place of the runtime verifier. Runtime
// identity is tested separately, including rejection through the public entry points.
#[cfg(not(windows))]
fn appimage_files(value: Option<&std::ffi::OsStr>) -> Option<Files> {
    crate::appimage::path_from(value).and_then(super::appimage_files)
}

const APP: App = App {
    name: "mdtest",
    version: "0.2.0",
    manifest_url: "https://example.com/update.txt",
    manifest_env: "MDSHARED_TEST_UPDATE_MANIFEST",
    engine: Some("mdtest-engine"),
    extras: &[],
    uninstall_key: None,
};
const ALONE: App = App {
    engine: None,
    ..APP
};

/// A manifest with an entry for the platform the tests run on.
fn manifest(version: &str, app: &[u8], engine: Option<&[u8]>) -> String {
    let p = Platform::current().map_or("linux-x86_64", Platform::key_prefix);
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
    let dir = std::env::temp_dir().join(format!("mdshared-updater-{name}-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files {
        app: dir.join("app.exe"),
        engine: engine.then(|| dir.join("engine.exe")),
        extras: Vec::new(),
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
    if url.ends_with(".txt") {
        Ok(manifest("0.3.0", b"app 2", None).into_bytes())
    } else {
        Ok(b"app 2".to_vec())
    }
}

#[test]
fn a_missing_engine_of_this_version_is_put_in_place() {
    let files = files("restore", true);
    let engine = files.engine.clone().unwrap();
    let fetch = |url: &str, _: usize, _: Duration| -> Result<Vec<u8>, String> {
        assert!(url.ends_with("/engine"), "{url}");
        Ok(b"engine 2".to_vec())
    };
    let this = win_manifest("0.2.0", b"app 1", Some(b"engine 2"));
    let w = Platform::WindowsX64;
    // There: nothing to do, nothing fetched.
    assert_eq!(
        restore_engine(&APP, &files, this.as_bytes(), w, &fetch),
        Ok(false)
    );
    assert_eq!(read(&engine), "engine 1");
    std::fs::remove_file(&engine).unwrap();
    // A manifest of another version, or one without the engine: left to the update.
    let newer = win_manifest("0.3.0", b"app 2", Some(b"engine 2"));
    assert_eq!(
        restore_engine(&APP, &files, newer.as_bytes(), w, &fetch),
        Ok(false)
    );
    let bare = win_manifest("0.2.0", b"app 1", None);
    assert_eq!(
        restore_engine(&APP, &files, bare.as_bytes(), w, &fetch),
        Ok(false)
    );
    // A wrong hash leaves nothing behind.
    let wrong = win_manifest("0.2.0", b"app 1", Some(b"something else"));
    assert!(restore_engine(&APP, &files, wrong.as_bytes(), w, &fetch).is_err());
    assert!(!engine.exists() && !selfupdate::staged(&engine).exists());
    assert_eq!(
        restore_engine(&APP, &files, this.as_bytes(), w, &fetch),
        Ok(true)
    );
    assert_eq!(read(&engine), "engine 2");
    _ = std::fs::remove_dir_all(files.app.parent().unwrap());
}

#[test]
fn a_bundle_is_found_from_its_executable() {
    let p = |s: &str| std::path::Path::new(s).to_path_buf();
    assert_eq!(
        bundle_of(&p("/Applications/mdview.app/Contents/MacOS/mdview")),
        Some(p("/Applications/mdview.app"))
    );
    assert_eq!(bundle_of(&p("/usr/local/bin/mdview")), None);
    assert_eq!(bundle_of(&p("/x/target/release/mdview")), None);
    assert_eq!(bundle_of(&p("/x/mdview/Contents/MacOS/mdview")), None);
}

/// The whole macOS installation with the system's `ditto`: a zip of a bundle replaces the
/// bundle, and a zip of anything else changes nothing.
#[cfg(target_os = "macos")]
#[test]
fn a_zipped_bundle_replaces_the_bundle() {
    use std::process::Command;
    let d = std::env::temp_dir().join(format!("mdshared-bundle-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&d);
    let exe = std::env::current_exe().unwrap();
    let exe_name = exe.file_name().unwrap();
    let make = |root: &std::path::Path, text: &str| {
        std::fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
        std::fs::write(root.join("Contents/Info.plist"), "<plist/>").unwrap();
        std::fs::write(root.join("Contents/MacOS").join(exe_name), text).unwrap();
    };
    let zip_of = |root: &std::path::Path| {
        let zip = root.with_extension("zip");
        let ok = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(root)
            .arg(&zip)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        std::fs::read(zip).unwrap()
    };
    let app = d.join("mdtest.app");
    make(&app, "version 1");
    make(&d.join("new/mdtest.app"), "version 2");
    make(&d.join("other/other.app"), "something else");
    let installed = |app: &std::path::Path| {
        std::fs::read_to_string(app.join("Contents/MacOS").join(exe_name)).unwrap()
    };
    let files = Files {
        app: app.clone(),
        engine: None,
        extras: Vec::new(),
    };
    let offer_of = |zip: &[u8]| Offer {
        version: Version::parse("0.3.0").unwrap(),
        notes: None,
        app: Download {
            url: "https://example.com/mdtest.zip".to_string(),
            sha256: sha256_hex(zip),
        },
        engine: None,
        extras: Vec::new(),
    };

    let other = zip_of(&d.join("other/other.app"));
    let fetch = |_: &str, _: usize, _: Duration| Ok(other.clone());
    assert!(install(&offer_of(&other), &files, &fetch).is_err());
    assert_eq!(installed(&app), "version 1");
    assert!(!selfupdate::staged(&app).exists());

    let new = zip_of(&d.join("new/mdtest.app"));
    let fetch = |_: &str, _: usize, _: Duration| Ok(new.clone());
    install(&offer_of(&new), &files, &fetch).unwrap();
    assert_eq!(installed(&app), "version 2");
    assert_eq!(installed(&selfupdate::retired(&app)), "version 1");
    assert_eq!(crate::relaunch::target(Some(&files)), Some(app.clone()));
    clean_up(&files);
    assert!(!selfupdate::retired(&app).exists());
    _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_macos_builds_read_a_manifest_of_their_own() {
    assert_eq!(
        macos_manifest("https://example.com/dl/mdview-update.txt"),
        "https://example.com/dl/mdview-update-macos.txt"
    );
    assert_eq!(
        macos_manifest("https://example.com/m"),
        "https://example.com/m-macos"
    );
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

const WITH_AI: App = App {
    manifest_url: "https://example.com/update-2.txt",
    extras: &["mdtest-ai"],
    ..APP
};

/// A format-2 Windows manifest: the app, the engine and, when given, the further file.
fn win_manifest_2(version: &str, ai: Option<&[u8]>) -> String {
    let mut m =
        win_manifest(version, b"app 2", Some(b"engine 2")).replace("manifest: 1", "manifest: 2");
    if let Some(ai) = ai {
        m.push_str(&format!(
            "windows-x86_64.mdtest-ai.url: https://example.com/ai\n\
             windows-x86_64.mdtest-ai.sha256: {}\n",
            sha256_hex(ai)
        ));
    }
    m
}

fn release_2(url: &str, _: usize, _: Duration) -> Result<Vec<u8>, String> {
    Ok(match url.rsplit('/').next().unwrap_or("") {
        "app" => b"app 2".to_vec(),
        "engine" => b"engine 2".to_vec(),
        "ai" => b"ai 2".to_vec(),
        _ => return Err(format!("no such file: {url}")),
    })
}

#[test]
fn further_files_are_needed_installed_and_restored() {
    let current = WITH_AI.current_version();
    // Windows: a release without the further file is not an offer; with it, it is.
    let bare = win_manifest_2("9.0.0", None);
    assert_eq!(win_offer(&WITH_AI, &bare, &current), Ok(None));
    let full = win_manifest_2("9.0.0", Some(b"ai 2"));
    let o = win_offer(&WITH_AI, &full, &current).unwrap().unwrap();
    assert_eq!(o.extras.len(), 1);
    assert_eq!(o.extras[0].url, "https://example.com/ai");
    // An application without further files ignores them.
    let o1 = win_offer(&APP, &full, &current).unwrap().unwrap();
    assert!(o1.extras.is_empty());
    // Elsewhere they are inside the AppImage or the bundle.
    let linux = full.replace("windows-x86_64", "linux-x86_64");
    let ol = offer_for(&WITH_AI, linux.as_bytes(), &current, Platform::LinuxX64)
        .unwrap()
        .unwrap();
    assert!(ol.extras.is_empty() && ol.engine.is_none());

    // Install replaces all three; a damaged further file replaces nothing.
    let mut files = files("extras", true);
    let ai = files.app.parent().unwrap().join("ai.exe");
    std::fs::write(&ai, "ai 1").unwrap();
    files.extras = vec![ai.clone()];
    let damaged = |url: &str, n: usize, t: Duration| {
        if url.ends_with("/ai") {
            Ok(b"ai 2, damaged".to_vec())
        } else {
            release_2(url, n, t)
        }
    };
    let err = install(&o, &files, &damaged).unwrap_err();
    assert!(err.contains("ai.exe"), "{err}");
    assert_eq!(read(&files.app), "app 1");
    assert_eq!(read(&ai), "ai 1");
    assert!(!selfupdate::staged(&ai).exists());
    // A copy that does not know the further file cannot take this offer.
    let mut short = files.clone();
    short.extras.clear();
    assert!(install(&o, &short, &release_2).is_err());
    assert_eq!(read(&files.app), "app 1");
    install(&o, &files, &release_2).unwrap();
    assert_eq!(read(&files.app), "app 2");
    assert_eq!(read(files.engine.as_ref().unwrap()), "engine 2");
    assert_eq!(read(&ai), "ai 2");
    clean_up(&files);

    // Restore: a copy updated by an older build has no further file; a manifest of its own
    // version puts it in place, and leaves the engine that is there alone.
    std::fs::remove_file(&ai).unwrap();
    let this = win_manifest_2("0.2.0", Some(b"ai 2"));
    let w = Platform::WindowsX64;
    let asked = RefCell::new(Vec::new());
    let fetch = |url: &str, n: usize, t: Duration| {
        asked.borrow_mut().push(url.to_string());
        release_2(url, n, t)
    };
    assert_eq!(
        restore_engine(&WITH_AI, &files, this.as_bytes(), w, &fetch),
        Ok(true)
    );
    assert_eq!(read(&ai), "ai 2");
    assert_eq!(*asked.borrow(), ["https://example.com/ai"]);
    // Nothing missing: nothing fetched.
    assert_eq!(
        restore_engine(&WITH_AI, &files, this.as_bytes(), w, &fetch),
        Ok(false)
    );
    assert_eq!(asked.borrow().len(), 1);
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

#[test]
fn a_verified_image_needs_a_writable_folder_to_become_an_update_target() {
    use std::ffi::OsStr;
    let files = files("verified-appimage", false);
    let dir = files.app.parent().unwrap().to_path_buf();
    let image = dir.join("mdtest.AppImage");
    std::fs::rename(&files.app, &image).unwrap();
    let appimage_files = |value| crate::appimage::path_from(value).and_then(super::appimage_files);
    let canonical = std::fs::canonicalize(&image).unwrap();
    assert_eq!(
        appimage_files(Some(image.as_os_str())),
        Some(Files {
            app: canonical,
            engine: None,
            extras: Vec::new(),
        })
    );
    assert_eq!(appimage_files(None), None);
    assert_eq!(appimage_files(Some(OsStr::new(""))), None);
    assert_eq!(appimage_files(Some(dir.as_os_str())), None, "a folder");
    let gone = dir.join("gone");
    assert_eq!(appimage_files(Some(gone.as_os_str())), None);
    // A read-only folder: skipped when the tests run as root, which ignores the mode.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        if !crate::appimage::folder_is_writable(&dir) {
            assert_eq!(appimage_files(Some(image.as_os_str())), None);
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    _ = std::fs::remove_dir_all(dir);
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
            engine: None,
            extras: Vec::new(),
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

#[test]
fn the_installation_is_checked_after_fetch_before_swap_and_before_cleanup() {
    use std::cell::Cell;
    use std::fs;
    let files = files("recheck", false);
    let manifest = manifest_for("linux-x86_64", "9.0.0", b"app 2", None);
    let offer = offer_for(
        &APP,
        manifest.as_bytes(),
        &APP.current_version(),
        Platform::LinuxX64,
    )
    .unwrap()
    .unwrap();
    let old = selfupdate::retired(&files.app);
    let staged = selfupdate::staged(&files.app);
    let saved = files.app.with_extension("moved");
    fs::write(&old, b"keep old").unwrap();
    let lost = Cell::new(false);
    let verify = |_: &Files| !lost.get();
    let fetch = |_: &str, _: usize, _: Duration| {
        fs::rename(&files.app, &saved).unwrap();
        fs::write(&files.app, b"replacement").unwrap();
        lost.set(true);
        Ok(b"app 2".to_vec())
    };
    assert!(install_checked(&offer, &files, &fetch, verify).is_err());
    assert!(!staged.exists(), "do not stage beside an unverified image");
    assert_eq!(read(&old), "keep old");
    assert_eq!(read(&files.app), "replacement");
    fs::remove_file(&files.app).unwrap();
    fs::rename(&saved, &files.app).unwrap();

    let verify_after_staging = |_: &Files| {
        if staged.exists() && !lost.get() {
            fs::rename(&files.app, &saved).unwrap();
            fs::write(&files.app, b"replacement").unwrap();
            lost.set(true);
        }
        !lost.get()
    };
    lost.set(false);
    let fetch = |_: &str, _: usize, _: Duration| Ok(b"app 2".to_vec());
    assert!(install_checked(&offer, &files, &fetch, verify_after_staging).is_err());
    assert_eq!(read(&saved), "app 1");
    assert_eq!(read(&files.app), "replacement");
    assert_eq!(read(&old), "keep old");
    #[cfg(target_os = "linux")]
    assert_eq!(
        read(&staged),
        "app 2",
        "no failure cleanup after identity was lost"
    );

    fs::write(&staged, b"keep staged").unwrap();
    clean_up_checked(&files, verify);
    assert_eq!(read(&staged), "keep staged");
    assert_eq!(read(&old), "keep old");
    fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn a_plain_linux_binary_neither_installs_nor_cleans_up_an_unverified_target() {
    let files = files("unverified", false);
    let manifest = manifest_for("linux-x86_64", "9.0.0", b"app 2", None);
    let offer = offer_for(
        &APP,
        manifest.as_bytes(),
        &APP.current_version(),
        Platform::LinuxX64,
    )
    .unwrap()
    .unwrap();
    let old = selfupdate::retired(&files.app);
    let staged = selfupdate::staged(&files.app);
    std::fs::write(&old, "keep old").unwrap();
    std::fs::write(&staged, "keep staged").unwrap();
    let never_fetch = |_: &str, _: usize, _: Duration| panic!("unverified install fetched");
    assert!(super::install(&offer, &files, &never_fetch).is_err());
    super::clean_up(&files);
    assert_eq!(read(&files.app), "app 1");
    assert_eq!(read(&old), "keep old");
    assert_eq!(read(&staged), "keep staged");
    std::fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn an_interrupted_appimage_install_can_retry_without_touching_other_entries() {
    use std::fs;
    use std::os::unix::fs::symlink;

    let files = files("stale-stage-retry", false);
    let manifest = manifest_for("linux-x86_64", "9.0.0", b"app 2", None);
    let offer = offer_for(
        &APP,
        manifest.as_bytes(),
        &APP.current_version(),
        Platform::LinuxX64,
    )
    .unwrap()
    .unwrap();
    let fetch = |_: &str, _: usize, _: Duration| Ok(b"app 2".to_vec());
    let staged = selfupdate::staged(&files.app);
    let unrelated = staged.with_extension("unrelated");
    fs::write(&unrelated, b"leave me alone").unwrap();
    symlink(&unrelated, &staged).unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    assert_eq!(fs::read_link(&staged).unwrap(), unrelated);
    fs::remove_file(&staged).unwrap();
    fs::hard_link(&unrelated, &staged).unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    assert_eq!(read(&staged), "leave me alone");
    fs::remove_file(&staged).unwrap();
    assert_eq!(read(&unrelated), "leave me alone");
    fs::create_dir(&staged).unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    assert!(staged.is_dir());
    fs::remove_dir(&staged).unwrap();

    let active = crate::appimage::lock_update(&files.app).unwrap();
    fs::write(&staged, b"partial download").unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    assert_eq!(read(&staged), "partial download");
    drop(active);

    assert_eq!(read(&files.app), "app 1");
    install(&offer, &files, &fetch).unwrap();
    assert!(!staged.exists());
    assert_eq!(read(&files.app), "app 2");
    assert_eq!(read(&selfupdate::retired(&files.app)), "app 1");
    fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn an_appimage_install_locks_a_private_writable_sibling_not_the_image() {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let files = files("writable-update-lock", false);
    let manifest = manifest_for("linux-x86_64", "9.0.0", b"app 2", None);
    let offer = offer_for(
        &APP,
        manifest.as_bytes(),
        &APP.current_version(),
        Platform::LinuxX64,
    )
    .unwrap()
    .unwrap();
    let fetch = |_: &str, _: usize, _: Duration| Ok(b"app 2".to_vec());
    let lock_path = files.app.with_file_name(format!(
        "{}.lck",
        files.app.file_name().unwrap().to_str().unwrap()
    ));
    symlink(&files.app, &lock_path).unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    assert_eq!(fs::read_link(&lock_path).unwrap(), files.app);
    fs::remove_file(&lock_path).unwrap();
    fs::hard_link(&files.app, &lock_path).unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    fs::remove_file(&lock_path).unwrap();
    fs::write(&lock_path, b"leave me alone").unwrap();
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    assert_eq!(read(&lock_path), "leave me alone");
    fs::remove_file(&lock_path).unwrap();

    fs::set_permissions(&files.app, fs::Permissions::from_mode(0o500)).unwrap();
    let image = fs::File::open(&files.app).unwrap();
    image.try_lock_shared().unwrap();
    // Another read-only opener must be able to take a shared lock at the same time.
    fs::File::open(&files.app)
        .unwrap()
        .try_lock_shared()
        .unwrap();
    let mut guard = crate::appimage::lock_update(&files.app).unwrap();
    assert_eq!(
        guard.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    guard.write_all(b"writable descriptor").unwrap();
    assert!(install(&offer, &files, &fetch).is_err());
    drop(guard);
    install(&offer, &files, &fetch).unwrap();
    assert_eq!(read(&files.app), "app 2");
    assert_eq!(read(&lock_path), "writable descriptor");
    fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn the_staged_appimage_is_private_and_exclusive_before_the_first_write() {
    // A child owns its umask; setting it here would also affect unrelated parallel tests.
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", "umask 000; exec \"$@\"", "sh"])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "updater::tests::probe_the_staged_appimage_open",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "helper run under a permissive umask by the staged AppImage open test"]
fn probe_the_staged_appimage_open() {
    use std::os::unix::fs::PermissionsExt;
    let files = files("private-stage-open", false);
    let staged = selfupdate::staged(&files.app);
    let f = create_staged_appimage(&staged).unwrap();
    assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(f.metadata().unwrap().len(), 0);
    assert_eq!(
        create_staged_appimage(&staged).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    drop(f);
    std::fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
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
fn a_restart_is_offered_only_once_installed_and_only_with_a_file_to_start() {
    let target = std::path::PathBuf::from("/apps/mdtest.AppImage");
    let mut u = Updates::default();
    u.preview("available");
    u.set_relaunch(Some(target.clone()));
    assert_eq!(u.relaunch_target(), None, "not before the install");
    assert!(u.begin().is_some());
    assert_eq!(u.relaunch_target(), None, "not while it downloads");
    assert!(u.finished(&APP, Err("no".into())).is_some());
    assert_eq!(u.relaunch_target(), None, "a failure keeps nothing");
    u.set_relaunch(Some(target.clone()));
    assert!(u.begin().is_some());
    assert_eq!(u.finished(&APP, Ok(())), None);
    assert_eq!(u.relaunch_target(), Some(target.as_path()));
    assert_eq!(u.about_label(), "Restart");
    // Beside the button the bar does not say that the version starts at the next opening.
    let stage = u.bar().unwrap();
    let said = Updates::bar_label_with(&APP, stage, true);
    assert!(said.ends_with("is installed. Restart to use it."), "{said}");
    assert_eq!(
        Updates::bar_label_with(&APP, stage, false),
        Updates::bar_label(&APP, stage)
    );
    assert!(Updates::bar_label(&APP, stage).contains("the next time you open"));
    // A copy with no file to start gets the plain bar and the old About button.
    let mut plain = Updates::default();
    plain.preview("installed-plain");
    assert_eq!(plain.relaunch_target(), None);
    assert_eq!(plain.about_label(), CHECK);
    plain.preview("installed");
    assert!(plain.relaunch_target().is_some());
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

const CLOSED_STDERR_CHILD: &str = "MDSHARED_TEST_CLOSED_STDERR_CHILD";

/// A failed look and a failed installation print a line, and `std`'s `eprintln!` panics when
/// stderr is a closed pipe; an application's release build aborts on that panic (found on
/// 2026-11-22, 0xC0000409 in mdedit). This test runs [`closed_stderr_child`] in a copy of the
/// test binary whose stderr is a pipe with no reader, and fails if the copy panics.
#[test]
fn a_closed_stderr_is_not_a_panic() {
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "updater::tests::closed_stderr_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CLOSED_STDERR_CHILD, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(writer)
        .status()
        .expect("the copy runs");
    assert!(
        status.success(),
        "the updater's prints ended a run whose stderr was closed: {status}"
    );
}

/// Prints each of the window's failure lines; it does something only when
/// [`a_closed_stderr_is_not_a_panic`] starts it.
#[test]
fn closed_stderr_child() {
    if std::env::var_os(CLOSED_STDERR_CHILD).is_none() {
        return;
    }
    let mut u = Updates::default();
    assert!(u.checked(&APP, true, Err("Timeout".into())).is_some());
    u.preview("available");
    assert!(u.begin().is_some());
    assert!(u.finished(&APP, Err("no match".into())).is_some());
}
