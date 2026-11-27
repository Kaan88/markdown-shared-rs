use super::*;
use std::os::unix::fs::symlink;

const EXE_SLOT: i32 = 1000;
const DIR_SLOT: i32 = 1001;
const RUNTIME: &str = "4321";
const TYPE2: &[u8] = b"\x7fELF\x02\x01\x01\0AI\x02 test runtime";

#[test]
fn parent_ignores_non_utf8_status_fields_and_accepts_only_an_ascii_pid() {
    let f = Fixture::new("status-bytes", false);
    let process = f.proc.join("self");
    let status = process.join("status");
    fs::write(&status, b"Name:\tapp\xff\nPPid:\t4321\nOther:\xfe\n").unwrap();
    assert_eq!(parent(&process), Some(4321));
    for field in [
        b"PPid:\t0".as_slice(),
        b"PPid:\t+1",
        b"PPid:\t9\xff",
        b"Other:\t123",
    ] {
        fs::write(&status, field).unwrap();
        assert_eq!(parent(&process), None);
    }
}

struct Fixture {
    root: PathBuf,
    proc: PathBuf,
    dir: PathBuf,
    image: PathBuf,
    // A proc-style link to one of these refers to what the runtime actually opened, even
    // after renaming the image. For FUSE mode, the real pipe models the keepalive pair.
    _image: File,
    _pipe: (std::io::PipeReader, std::io::PipeWriter),
}

impl Fixture {
    fn new(name: &str, mounted: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("mdshared-runtime-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let proc = root.join("proc");
        for entry in ["self/fd", "self/fdinfo", "4321/fd", "4321/fdinfo"] {
            fs::create_dir_all(proc.join(entry)).unwrap();
        }
        let dir = root.join(if mounted {
            ".mount_unnamed1234"
        } else {
            "appimage_extracted_bd84293f355d109893d581a6ad2a9bb9"
        });
        fs::create_dir(&dir).unwrap();
        let exe = dir.join("mdtest");
        fs::write(&exe, b"application").unwrap();
        // The file has no app-name prefix (nor does it need one). The fixture exposes the
        // actual runtime exe and its backing descriptor; mountinfo does not supply a path.
        let image = root.join("My copy.AppImage");
        fs::write(&image, TYPE2).unwrap();
        let backing = File::open(&image).unwrap();
        let pipe = std::io::pipe().unwrap();
        let backing_fd = format!("/proc/self/fd/{}", backing.as_raw_fd());
        symlink(&backing_fd, proc.join(format!("{RUNTIME}/exe"))).unwrap();
        symlink(&exe, proc.join("self/exe")).unwrap();
        fs::write(proc.join("self/status"), format!("PPid:\t{RUNTIME}\n")).unwrap();
        if mounted {
            symlink(backing_fd, proc.join(format!("{RUNTIME}/fd/5"))).unwrap();
            symlink(&dir, proc.join(format!("self/fd/{APPDIR_FD}"))).unwrap();
            Self::fdinfo(&proc, "self", APPDIR_FD, 42, "0100000");
            symlink(
                format!("/proc/self/fd/{}", pipe.0.as_raw_fd()),
                proc.join("self/fd/3"),
            )
            .unwrap();
            Self::fdinfo(&proc, "self", 3, 1, "00");
            symlink(
                format!("/proc/self/fd/{}", pipe.1.as_raw_fd()),
                proc.join(format!("{RUNTIME}/fd/4")),
            )
            .unwrap();
            Self::fdinfo(&proc, RUNTIME, 4, 1, "01");
            symlink("/dev/fuse", proc.join(format!("{RUNTIME}/fd/6"))).unwrap();
        }
        Self::fdinfo(&proc, "self", EXE_SLOT as u32, 42, "0100000");
        Self::fdinfo(&proc, "self", DIR_SLOT as u32, 42, "0100000");
        let mounts = if mounted {
            format!(
                "42 2 0:3 / {} ro,nodev - fuse.squashfuse unnamed.AppImage ro\n",
                dir.display()
            )
        } else {
            "42 2 0:3 / / rw - tmpfs tmpfs rw\n".into()
        };
        fs::write(proc.join("self/mountinfo"), mounts).unwrap();
        Self {
            root,
            proc,
            dir,
            image,
            _image: backing,
            _pipe: pipe,
        }
    }

    fn fdinfo(proc: &Path, pid: &str, fd: u32, mount: u64, flags: &str) {
        fs::write(
            proc.join(format!("{pid}/fdinfo/{fd}")),
            format!("pos:\t0\nflags:\t{flags}\nmnt_id:\t{mount}\n"),
        )
        .unwrap();
    }

    fn verify(&self, value: &OsStr) -> Option<PathBuf> {
        find(&self.proc, value, &self.dir, EXE_SLOT, DIR_SLOT)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_image_source_belongs_to_the_exact_fuse_mount() {
    let mounts = b"1 2 0:3 / /tmp/App\\040Dir ro shared:4 - fuse.squashfuse /apps/mdtest\\0401.AppImage ro\n\
                   2 1 0:4 / /tmp/App ro - tmpfs /some/file ro";
    let mount = mount_by_id(mounts, 1).unwrap();
    assert!(mount.fuse_at(Path::new("/tmp/App Dir")));
    assert_eq!(mount.source, Path::new("/apps/mdtest 1.AppImage"));
    assert!(!mount.fuse_at(Path::new("/tmp/App")));
    assert!(
        !mount_by_id(mounts, 2)
            .unwrap()
            .fuse_at(Path::new("/tmp/App"))
    );
}

#[test]
fn a_renamed_image_with_a_basename_mount_label_uses_the_daemons_open_inode() {
    let launch = Fixture::new("mount-label", true);
    assert_eq!(
        launch.verify(launch.image.as_os_str()),
        Some(launch.image.clone())
    );
    fs::remove_file(launch.proc.join(format!("{RUNTIME}/fd/5"))).unwrap();
    assert_eq!(launch.verify(launch.image.as_os_str()), None);
}

#[test]
fn extract_and_run_uses_the_parents_pinned_executable_after_it_closes_the_image_fd() {
    let launch = Fixture::new("extract-and-run", false);
    // The fake proc exe points through a real fd to simulate the kernel's executable pin.
    // The parent has no image descriptor in its fd directory after extraction.
    assert_eq!(
        fs::read_dir(launch.proc.join(format!("{RUNTIME}/fd")))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        launch.verify(launch.image.as_os_str()),
        Some(launch.image.clone())
    );
    fs::write(launch.proc.join("self/status"), "PPid:\t99\n").unwrap();
    assert_eq!(launch.verify(launch.image.as_os_str()), None);
    fs::write(
        launch.proc.join("self/status"),
        format!("PPid:\t{RUNTIME}\n"),
    )
    .unwrap();
    fs::rename(&launch.image, launch.root.join("original")).unwrap();
    fs::write(&launch.image, TYPE2).unwrap();
    assert_eq!(launch.verify(launch.image.as_os_str()), None);
}

#[test]
fn an_ancestors_pipe_for_another_mount_cannot_select_that_image() {
    let launch = Fixture::new("wrapper-a", true);
    let other = Fixture::new("wrapper-b", true);
    assert_eq!(
        other.verify(other.image.as_os_str()),
        Some(other.image.clone())
    );
    assert_ne!(
        identity(&fs::metadata(launch.proc.join("self/fd/3")).unwrap()),
        identity(&fs::metadata(other.proc.join("self/fd/3")).unwrap())
    );
    let mounts = launch.proc.join("self/mountinfo");
    fs::write(
        &mounts,
        format!(
            "42 2 0:3 / {} ro - fuse.squashfuse image-a ro\n\
             43 2 0:4 / {} ro - fuse.squashfuse image-b ro\n",
            launch.dir.display(),
            other.dir.display()
        ),
    )
    .unwrap();
    fs::rename(other.proc.join(RUNTIME), launch.proc.join("6543")).unwrap();
    assert_eq!(
        launch.verify(launch.image.as_os_str()),
        Some(launch.image.clone())
    );
    assert_eq!(launch.verify(other.image.as_os_str()), None);

    // The wrapper keeps AppDir A and both separate read ends. The application itself
    // still has AppDir A, but has closed the keepalive read end it inherited.
    let wrapper = launch.proc.join("5432");
    for name in ["fd", "fdinfo"] {
        fs::create_dir_all(wrapper.join(name)).unwrap();
    }
    symlink(&launch.dir, wrapper.join(format!("fd/{APPDIR_FD}"))).unwrap();
    Fixture::fdinfo(&launch.proc, "5432", APPDIR_FD, 42, "0100000");
    fs::rename(launch.proc.join("self/fd/3"), wrapper.join("fd/3")).unwrap();
    fs::rename(other.proc.join("self/fd/3"), wrapper.join("fd/4")).unwrap();
    Fixture::fdinfo(&launch.proc, "5432", 3, 1, "00");
    Fixture::fdinfo(&launch.proc, "5432", 4, 1, "00");
    fs::write(launch.proc.join("self/status"), "PPid:\t5432\n").unwrap();
    fs::write(wrapper.join("status"), "PPid:\t1\n").unwrap();
    assert_eq!(launch.verify(other.image.as_os_str()), None);
    assert_eq!(launch.verify(launch.image.as_os_str()), None);
}

#[test]
fn a_replacement_at_the_mount_label_and_an_executable_on_a_nested_mount_are_refused() {
    let launch = Fixture::new("replaced-or-nested", true);
    let mounts = launch.proc.join("self/mountinfo");
    fs::write(
        &mounts,
        format!(
            "42 2 0:3 / {0} ro - fuse.squashfuse {1}/mdtest.AppImage ro\n\
         43 42 0:4 / {0}/nested rw - tmpfs tmpfs rw\n",
            launch.dir.display(),
            launch.root.display(),
        ),
    )
    .unwrap();
    let named = launch.root.join("mdtest.AppImage");
    fs::rename(&launch.image, &named).unwrap();
    assert_eq!(launch.verify(named.as_os_str()), Some(named.clone()));

    let nested = launch.dir.join("nested/mdtest");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, b"some other copy").unwrap();
    fs::remove_file(launch.proc.join("self/exe")).unwrap();
    symlink(&nested, launch.proc.join("self/exe")).unwrap();
    Fixture::fdinfo(&launch.proc, "self", EXE_SLOT as u32, 43, "0100000");
    assert_eq!(launch.verify(named.as_os_str()), None);
    Fixture::fdinfo(&launch.proc, "self", EXE_SLOT as u32, 42, "0100000");

    fs::rename(&named, launch.root.join("while-still-mounted")).unwrap();
    // Even the same contents at the exact same source path are a different inode.
    fs::write(&named, TYPE2).unwrap();
    assert_eq!(launch.verify(named.as_os_str()), None);
}

#[test]
fn an_inherited_or_unrelated_appimage_is_never_an_update_target() {
    let launch = Fixture::new("unrelated", true);
    let other = launch.root.join("mdtest-other.AppImage");
    fs::write(&other, TYPE2).unwrap();
    assert_eq!(launch.verify(other.as_os_str()), None);
    // A plain child may inherit the real APPDIR/APPIMAGE and its descriptor and pipe.
    fs::remove_file(launch.proc.join("self/exe")).unwrap();
    symlink(&other, launch.proc.join("self/exe")).unwrap();
    assert_eq!(launch.verify(launch.image.as_os_str()), None);
    assert_eq!(fs::read(other).unwrap(), TYPE2);
}
