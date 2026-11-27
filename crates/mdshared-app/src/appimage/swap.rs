//! The Linux updater's swap beside an AppImage, done on files it holds and not only on names.
//!
//! [`crate::selfupdate::swap_in`] works by name: it removes `<name>.old`, renames the target to
//! it and `<name>.new` to the target. Beside an AppImage those names can be given to other files
//! between two steps of an installation, which takes minutes while it downloads. So the file
//! each target's name denoted is noted first ([`Pinned::of`]), every staged file stays open from
//! its exclusive creation ([`Pinned::hold`]), and [`Pinned::swap`] checks after each rename that
//! the file it moved is the one noted: the file retired to `.old` is the one that was verified,
//! and the file put in its place is the one this process wrote. Anything else is moved back and
//! the swap fails. [`remove_leftovers`] is the clean-up at a later launch, under the same lock
//! as an installation ([`super::lock_update`]), so the two never work on the same files at once.
//!
//! Deliberately not here: any defence against another program of the same account, which can
//! replace the image without asking. A rename and a removal still take a name, so a file given
//! that name between the check and the call is moved or removed in the other's place; the
//! check after each rename finds that out, and nothing here can prevent it. A file is told
//! from every other by its device and inode numbers, which no other file has while it exists.

use crate::selfupdate::{retired, staged};
use std::fs::{File, Metadata, remove_file, rename, symlink_metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

type Identity = (u64, u64);

fn identity(info: &Metadata) -> Identity {
    (info.dev(), info.ino())
}

/// Whether `path` itself (a link is not followed) is the regular file `held` is open on.
fn denotes(path: &Path, held: &File) -> bool {
    match (symlink_metadata(path), held.metadata()) {
        (Ok(named), Ok(held)) => named.is_file() && identity(&named) == identity(&held),
        _ => false,
    }
}

fn replaced(what: &str) -> io::Error {
    io::Error::other(format!("{what} was replaced during the update"))
}

/// The files of one installation: what each target was before the installation was verified,
/// and the open handle of each staged copy.
pub(crate) struct Pinned {
    displaced: Vec<(PathBuf, Option<Identity>)>,
    staged: Vec<(PathBuf, File)>,
}

impl Pinned {
    /// Notes what each of `targets` names now (`None` for a name with nothing behind it). Call
    /// this before verifying the installation: a target that changes between this note and the
    /// verification then fails the swap instead of being retired unverified.
    pub(crate) fn of(targets: &[PathBuf]) -> Self {
        let displaced = targets
            .iter()
            .map(|target| {
                let now = symlink_metadata(target).ok();
                (target.clone(), now.map(|info| identity(&info)))
            })
            .collect();
        Self {
            displaced,
            staged: Vec::new(),
        }
    }

    /// Keeps `stage`, the handle the staged copy of `target` was created through, and lends it
    /// back for writing.
    pub(crate) fn hold(&mut self, target: &Path, stage: File) -> &mut File {
        let at = self.staged.len();
        self.staged.push((target.to_path_buf(), stage));
        &mut self.staged[at].1
    }

    /// Whether a staged file is held.
    pub(crate) fn any(&self) -> bool {
        !self.staged.is_empty()
    }

    fn displaced(&self, target: &Path) -> Option<Identity> {
        let noted = self.displaced.iter().find(|(path, _)| path == target);
        noted.and_then(|(_, identity)| *identity)
    }

    /// Puts every held staged file in its target's place, all or none, as
    /// [`crate::selfupdate::swap_in`] does, checking each file moved against what was noted.
    /// On an error the targets are as they were, as far as renames can put them back, and the
    /// staged files are still at `.new`.
    pub(crate) fn swap(&self) -> io::Result<()> {
        let mut done: Vec<&(PathBuf, File)> = Vec::new();
        for entry in &self.staged {
            let (target, stage) = entry;
            if let Err(e) = swap_one(target, stage, self.displaced(target)) {
                for (target, stage) in done.into_iter().rev() {
                    undo(target, stage, self.displaced(target).is_some());
                }
                return Err(e);
            }
            done.push(entry);
        }
        Ok(())
    }

    /// Removes each staged file that is still the one this process created. A `.new` that now
    /// names another file is left alone.
    pub(crate) fn discard(&self) {
        for (target, stage) in &self.staged {
            let path = staged(target);
            if denotes(&path, stage) {
                _ = remove_file(&path);
            }
        }
    }
}

fn swap_one(target: &Path, stage: &File, displaced: Option<Identity>) -> io::Result<()> {
    let (new, old) = (staged(target), retired(target));
    if !denotes(&new, stage) {
        return Err(replaced("the staged file"));
    }
    // A copy retired by an earlier update goes, if it is this account's plain file.
    super::remove_stale_stage(&old)?;
    match displaced {
        Some(noted) => {
            rename(target, &old)?;
            let moved = symlink_metadata(&old).ok();
            if moved.map(|info| identity(&info)) != Some(noted) {
                // Not the file that was verified: it gets its name back.
                _ = rename(&old, target);
                return Err(replaced("the installed file"));
            }
        }
        None if symlink_metadata(target).is_ok() => {
            return Err(replaced("the installed file"));
        }
        None => {}
    }
    let placed = rename(&new, target).and_then(|()| {
        if denotes(target, stage) {
            Ok(())
        } else {
            Err(replaced("the staged file"))
        }
    });
    if placed.is_err() {
        // A file that is not the staged one may have taken the target's name: it goes back
        // to `.new`, where it came from, and then the retired copy returns.
        if !denotes(target, stage) && symlink_metadata(target).is_ok() {
            _ = rename(target, &new);
        }
        undo(target, stage, displaced.is_some());
    }
    placed
}

/// Takes one swap back: the staged file returns to `.new` if it is the one at the target, and
/// the retired copy returns to a target name that is then free.
fn undo(target: &Path, stage: &File, had_target: bool) {
    if denotes(target, stage) {
        _ = rename(target, staged(target));
    }
    if had_target && symlink_metadata(target).is_err() {
        _ = rename(retired(target), target);
    }
}

/// Removes the retired (`.old`) and staged (`.new`) copies an earlier update left beside
/// `targets`, each only when it is a regular file with one name owned by this account (a link is
/// not followed), while holding the update lock of `image`. When an installation holds that
/// lock the leftovers are its working files: nothing is removed, and a later launch tries
/// again. With nothing left behind, no lock file is made. The caller has verified `image`.
pub(crate) fn remove_leftovers(image: &Path, targets: &[PathBuf]) {
    let leftovers: Vec<PathBuf> = targets
        .iter()
        .flat_map(|target| [retired(target), staged(target)])
        .filter(|path| symlink_metadata(path).is_ok())
        .collect();
    if leftovers.is_empty() {
        return;
    }
    let Ok(_lock) = super::lock_update(image) else {
        return;
    };
    for path in leftovers {
        _ = super::remove_stale_stage(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mdshared-pinned-{name}-{}", std::process::id()));
        _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    /// A target reading "app 1" with a staged copy reading "app 2", pinned.
    fn staged_update(dir: &Path) -> (PathBuf, Pinned) {
        let target = dir.join("app.AppImage");
        fs::write(&target, "app 1").unwrap();
        let mut pinned = Pinned::of(std::slice::from_ref(&target));
        fs::write(staged(&target), "app 2").unwrap();
        let stage = File::open(staged(&target)).unwrap();
        pinned.hold(&target, stage);
        (target, pinned)
    }

    #[test]
    fn the_held_files_are_swapped_and_the_old_one_kept_aside() {
        let d = dir("ok");
        let (target, pinned) = staged_update(&d);
        // Left by an earlier update.
        fs::write(retired(&target), "app 0").unwrap();
        assert!(pinned.any());
        pinned.swap().unwrap();
        assert_eq!(read(&target), "app 2");
        assert_eq!(read(&retired(&target)), "app 1");
        assert!(!staged(&target).exists());
        // Nothing of this installation is left to discard, and the new file is not touched.
        pinned.discard();
        assert_eq!(read(&target), "app 2");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_stage_given_to_another_file_is_neither_installed_nor_removed() {
        let d = dir("stage");
        let (target, pinned) = staged_update(&d);
        fs::remove_file(staged(&target)).unwrap();
        fs::write(staged(&target), "someone else's").unwrap();
        assert!(pinned.swap().is_err());
        assert_eq!(read(&target), "app 1");
        assert!(!retired(&target).exists());
        pinned.discard();
        assert_eq!(read(&staged(&target)), "someone else's");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_target_given_to_another_file_after_it_was_noted_is_put_back_and_not_retired() {
        let d = dir("target");
        let (target, pinned) = staged_update(&d);
        let aside = d.join("the verified image");
        fs::rename(&target, &aside).unwrap();
        fs::write(&target, "another application").unwrap();
        assert!(pinned.swap().is_err());
        assert_eq!(read(&target), "another application");
        assert_eq!(read(&aside), "app 1");
        assert!(!retired(&target).exists());
        assert_eq!(read(&staged(&target)), "app 2");
        // The stage is still this installation's, so giving up removes it.
        pinned.discard();
        assert!(!staged(&target).exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_target_that_appeared_after_none_was_noted_is_not_overwritten() {
        let d = dir("appeared");
        let target = d.join("engine");
        let mut pinned = Pinned::of(std::slice::from_ref(&target));
        fs::write(staged(&target), "engine 2").unwrap();
        pinned.hold(&target, File::open(staged(&target)).unwrap());
        fs::write(&target, "appeared").unwrap();
        assert!(pinned.swap().is_err());
        assert_eq!(read(&target), "appeared");
        fs::remove_file(&target).unwrap();
        // With the name free again the staged file takes it, and nothing is retired.
        pinned.swap().unwrap();
        assert_eq!(read(&target), "engine 2");
        assert!(!retired(&target).exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_failure_on_the_second_target_puts_the_first_back() {
        let d = dir("rollback");
        let (app, engine) = (d.join("app"), d.join("engine"));
        fs::write(&app, "app 1").unwrap();
        fs::write(&engine, "engine 1").unwrap();
        let mut pinned = Pinned::of(&[app.clone(), engine.clone()]);
        for (target, text) in [(&app, "app 2"), (&engine, "engine 2")] {
            fs::write(staged(target), text).unwrap();
            pinned.hold(target, File::open(staged(target)).unwrap());
        }
        // A folder where the engine's retired copy would go is not this account's plain file.
        fs::create_dir(retired(&engine)).unwrap();
        assert!(pinned.swap().is_err());
        assert_eq!(read(&app), "app 1");
        assert_eq!(read(&engine), "engine 1");
        assert_eq!(read(&staged(&app)), "app 2");
        assert_eq!(read(&staged(&engine)), "engine 2");
        assert!(!retired(&app).exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn leftovers_wait_for_a_running_installation_and_only_plain_files_go() {
        let d = dir("leftovers");
        let image = d.join("app.AppImage");
        fs::write(&image, "app 2").unwrap();
        let targets = std::slice::from_ref(&image);
        // Nothing left behind: no lock file appears beside the image.
        remove_leftovers(&image, targets);
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);

        fs::write(retired(&image), "app 1").unwrap();
        fs::write(staged(&image), "a download under way").unwrap();
        let installing = super::super::lock_update(&image).unwrap();
        remove_leftovers(&image, targets);
        assert_eq!(read(&retired(&image)), "app 1");
        assert_eq!(read(&staged(&image)), "a download under way");
        drop(installing);
        remove_leftovers(&image, targets);
        assert!(!retired(&image).exists() && !staged(&image).exists());

        // A folder, a link and a file with a second name are not leftovers of an update.
        let kept = d.join("kept");
        fs::write(&kept, "keep").unwrap();
        fs::create_dir(retired(&image)).unwrap();
        std::os::unix::fs::symlink(&kept, staged(&image)).unwrap();
        remove_leftovers(&image, targets);
        assert!(retired(&image).is_dir());
        assert_eq!(fs::read_link(staged(&image)).unwrap(), kept);
        fs::remove_dir(retired(&image)).unwrap();
        fs::hard_link(&kept, retired(&image)).unwrap();
        remove_leftovers(&image, targets);
        assert_eq!(read(&retired(&image)), "keep");
        assert_eq!(read(&kept), "keep");
        assert_eq!(read(&image), "app 2");
        fs::remove_dir_all(d).unwrap();
    }
}
