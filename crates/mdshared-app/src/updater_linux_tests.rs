//! The updater on Linux: what an installation does to the files beside an AppImage when their
//! names are given to other files while it runs. Built and run on Linux only.

use super::*;

fn linux_offer() -> Offer {
    let manifest = manifest_for("linux-x86_64", "9.0.0", b"app 2", None);
    offer_for(
        &APP,
        manifest.as_bytes(),
        &APP.current_version(),
        Platform::LinuxX64,
    )
    .unwrap()
    .unwrap()
}

fn app_2(_: &str, _: usize, _: Duration) -> Result<Vec<u8>, String> {
    Ok(b"app 2".to_vec())
}

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
    assert!(super::super::install(&offer, &files, &never_fetch).is_err());
    super::super::clean_up(&files);
    assert_eq!(read(&files.app), "app 1");
    assert_eq!(read(&old), "keep old");
    assert_eq!(read(&staged), "keep staged");
    std::fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[test]
fn a_stage_given_to_another_file_before_the_swap_is_not_installed() {
    use std::cell::Cell;
    use std::fs;
    let files = files("pinned-stage", false);
    let staged = selfupdate::staged(&files.app);
    let checks = Cell::new(0);
    // The third check is the one before the swap: the staged file is written by then.
    let verify = |_: &Files| {
        checks.set(checks.get() + 1);
        if checks.get() == 3 {
            assert_eq!(read(&staged), "app 2");
            fs::remove_file(&staged).unwrap();
            fs::write(&staged, b"not the download").unwrap();
        }
        true
    };
    assert!(install_checked(&linux_offer(), &files, &app_2, verify).is_err());
    assert_eq!(read(&files.app), "app 1");
    assert!(!selfupdate::retired(&files.app).exists());
    assert_eq!(
        read(&staged),
        "not the download",
        "a file this installation did not write is not its to remove"
    );
    fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[test]
fn a_target_given_to_another_file_before_the_swap_is_not_retired() {
    use std::cell::Cell;
    use std::fs;
    let files = files("pinned-target", false);
    let aside = files.app.with_extension("aside");
    let checks = Cell::new(0);
    let verify = |_: &Files| {
        checks.set(checks.get() + 1);
        if checks.get() == 3 {
            fs::rename(&files.app, &aside).unwrap();
            fs::write(&files.app, b"another application").unwrap();
        }
        true
    };
    assert!(install_checked(&linux_offer(), &files, &app_2, verify).is_err());
    assert_eq!(read(&files.app), "another application");
    assert_eq!(read(&aside), "app 1");
    assert!(!selfupdate::retired(&files.app).exists());
    assert!(
        !selfupdate::staged(&files.app).exists(),
        "its own download is removed"
    );
    fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}

#[test]
fn a_clean_up_leaves_a_running_installation_its_files() {
    use std::fs;
    let files = files("clean-up-lock", false);
    let old = selfupdate::retired(&files.app);
    let staged = selfupdate::staged(&files.app);
    fs::write(&old, b"app 0").unwrap();
    fs::write(&staged, b"a download under way").unwrap();
    let installing = crate::appimage::lock_update(&files.app).unwrap();
    clean_up(&files);
    assert_eq!(read(&old), "app 0");
    assert_eq!(read(&staged), "a download under way");
    drop(installing);
    clean_up(&files);
    assert!(!old.exists() && !staged.exists());
    assert_eq!(read(&files.app), "app 1");
    fs::remove_dir_all(files.app.parent().unwrap()).unwrap();
}
