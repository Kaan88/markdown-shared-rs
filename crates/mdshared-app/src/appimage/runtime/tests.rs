use super::*;
use std::os::unix::fs::symlink;

const EXE_SLOT: i32 = 1000;
const DIR_SLOT: i32 = 1001;
const RUNTIME: &str = "4321";
/// A Type 2 image in miniature: a 64-bit little-endian ELF header with the AppImage mark, one
/// empty section header after it, and then the start of a squashfs 4 superblock that says the
/// filesystem has `inodes` inodes in `blocks` blocks of 128 KiB.
const fn type2(inodes: u8, blocks: u8) -> [u8; 176] {
    let mut image = [0; 176];
    let head = *b"\x7fELF\x02\x01\x01\0AI\x02";
    let mut i = 0;
    while i < head.len() {
        image[i] = head[i];
        i += 1;
    }
    // The section header table follows the ELF header: one entry of 64 bytes.
    image[0x28] = 64;
    image[0x3a] = 64;
    image[0x3c] = 1;
    let magic = *b"hsqs";
    i = 0;
    while i < magic.len() {
        image[128 + i] = magic[i];
        i += 1;
    }
    image[128 + 4] = inodes;
    // Block size 0x2_0000 and its logarithm, the major version, and the bytes used.
    image[128 + 14] = 2;
    image[128 + 22] = 17;
    image[128 + 28] = 4;
    image[128 + 42] = 2 * blocks;
    image
}

const TYPE2: &[u8] = &type2(7, 3);
/// What a mount of [`TYPE2`] reports.
const MOUNTED: Geometry = Geometry {
    blocks: 3,
    inodes: 7,
};

fn mount_path(path: &Path) -> Vec<u8> {
    let mut escaped = Vec::new();
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b' ' => escaped.extend(b"\\040"),
            b'\t' => escaped.extend(b"\\011"),
            b'\n' => escaped.extend(b"\\012"),
            b'\\' => escaped.extend(b"\\134"),
            _ => escaped.push(byte),
        }
    }
    escaped
}

#[test]
fn a_mount_fixture_can_use_all_four_escaped_path_characters() {
    let launch = Fixture::new("space tab\tnewline\nbackslash\\", true);
    assert_eq!(
        launch.verify(launch.image.as_os_str()),
        Some(launch.image.clone())
    );
}

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
        // The image starts this application.
        symlink("mdtest", dir.join("AppRun")).unwrap();
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
            [
                b"42 2 0:3 / ".as_slice(),
                &mount_path(&dir),
                b" ro,nodev - fuse.squashfuse unnamed.AppImage ro\n",
            ]
            .concat()
        } else {
            b"42 2 0:3 / / rw - tmpfs tmpfs rw\n".to_vec()
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
        self.verify_mounted(value, Some(MOUNTED))
    }

    /// [`Fixture::verify`] when the AppDir's filesystem reports `mounted`.
    fn verify_mounted(&self, value: &OsStr, mounted: Option<Geometry>) -> Option<PathBuf> {
        find(&self.proc, value, &self.dir, EXE_SLOT, DIR_SLOT, mounted)
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
        [
            b"42 2 0:3 / ".as_slice(),
            &mount_path(&launch.dir),
            b" ro - fuse.squashfuse image-a ro\n43 2 0:4 / ",
            &mount_path(&other.dir),
            b" ro - fuse.squashfuse image-b ro\n",
        ]
        .concat(),
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
    let launch = Fixture::new("replaced or nested", true);
    let mounts = launch.proc.join("self/mountinfo");
    let named = launch.root.join("mdtest.AppImage");
    fs::write(
        &mounts,
        [
            b"42 2 0:3 / ".as_slice(),
            &mount_path(&launch.dir),
            b" ro - fuse.squashfuse ",
            &mount_path(&named),
            b" ro\n43 42 0:4 / ",
            &mount_path(&launch.dir.join("nested")),
            b" rw - tmpfs tmpfs rw\n",
        ]
        .concat(),
    )
    .unwrap();
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

#[test]
fn another_images_live_daemon_and_pipe_cannot_stand_in_for_the_mounted_image() {
    let launch = Fixture::new("mount-a", true);
    let other = Fixture::new("mount-b", true);
    // Image B holds another filesystem. Its daemon runs, and this application has inherited
    // the read end of B's keepalive pipe beside its own: every question about B's runtime is
    // answered as it would be for a launch of B.
    let image_b = type2(9, 5);
    fs::write(&other.image, image_b).unwrap();
    fs::rename(other.proc.join(RUNTIME), launch.proc.join("6543")).unwrap();
    symlink(
        format!("/proc/self/fd/{}", other._pipe.0.as_raw_fd()),
        launch.proc.join("self/fd/7"),
    )
    .unwrap();
    Fixture::fdinfo(&launch.proc, "self", 7, 1, "00");
    // The AppDir is image A's, so `$APPIMAGE` naming B is refused, and A is still found.
    assert_eq!(launch.verify(other.image.as_os_str()), None);
    assert_eq!(
        launch.verify(launch.image.as_os_str()),
        Some(launch.image.clone())
    );
    // What refuses B is the mount: an AppDir reporting B's filesystem would be B's.
    let mounted_b = Geometry {
        blocks: 5,
        inodes: 9,
    };
    assert_eq!(
        launch.verify_mounted(other.image.as_os_str(), Some(mounted_b)),
        Some(other.image.clone())
    );
    assert_eq!(
        launch.verify_mounted(launch.image.as_os_str(), Some(mounted_b)),
        None
    );
    // A mount that cannot be asked proves nothing.
    assert_eq!(launch.verify_mounted(launch.image.as_os_str(), None), None);
}

#[test]
fn only_the_application_apprun_starts_updates_the_image() {
    for mounted in [true, false] {
        let launch = Fixture::new(if mounted { "apprun-m" } else { "apprun-x" }, mounted);
        let image = launch.image.as_os_str();
        assert_eq!(launch.verify(image), Some(launch.image.clone()));
        // A second program packed in the same image, on the same mount.
        let engine = launch.dir.join("mdtest-engine");
        fs::write(&engine, b"engine").unwrap();
        let exe = launch.proc.join("self/exe");
        fs::remove_file(&exe).unwrap();
        symlink(&engine, &exe).unwrap();
        assert_eq!(launch.verify(image), None);
        // An AppRun that is a file of its own (a script) starts nothing we can name.
        fs::remove_file(&exe).unwrap();
        symlink(launch.dir.join("mdtest"), &exe).unwrap();
        assert_eq!(launch.verify(image), Some(launch.image.clone()));
        let apprun = launch.dir.join("AppRun");
        fs::remove_file(&apprun).unwrap();
        fs::write(&apprun, b"#!/bin/sh\nexec mdtest\n").unwrap();
        assert_eq!(launch.verify(image), None);
        fs::remove_file(&apprun).unwrap();
        assert_eq!(launch.verify(image), None, "no AppRun at all");
    }
}

#[test]
fn the_geometry_of_an_image_is_read_from_its_superblock_or_not_at_all() {
    let root = std::env::temp_dir().join(format!("mdshared-geometry-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let file = root.join("image");
    let geometry = |bytes: &[u8]| {
        fs::write(&file, bytes).unwrap();
        Geometry::of_image(&File::open(&file).unwrap())
    };
    let good = type2(7, 3);
    assert_eq!(geometry(&good), Some(MOUNTED));
    // A last section that ends after the table moves the filesystem there, as in the runtime.
    let mut padded = good[..128].to_vec();
    padded[64 + 0x18] = 128;
    padded[64 + 0x20] = 16;
    padded.extend([0; 16]);
    padded.extend(&good[128..]);
    assert_eq!(geometry(&padded), Some(MOUNTED));

    let changed = |at: usize, to: u8| {
        let mut bytes = good;
        bytes[at] = to;
        bytes
    };
    for bad in [
        changed(4, 1),         // a 32-bit ELF
        changed(5, 2),         // big-endian
        changed(128, b'x'),    // no squashfs where the ELF ends
        changed(128 + 28, 3),  // squashfs 3
        changed(128 + 22, 16), // a block size that is not two to its logarithm
        changed(128 + 22, 99),
        changed(128 + 42, 0), // nothing used
        changed(0x3c, 9),     // a section table that runs past the file
    ] {
        assert_eq!(geometry(&bad), None);
    }
    for short in [0, 10, 64, 128, 175] {
        assert_eq!(geometry(&good[..short]), None);
    }
    // A mount may count the last, partly used block or not; nothing else is the same mount.
    let image = MOUNTED;
    let mount = |blocks, inodes| Geometry { blocks, inodes };
    assert!(mount(3, 7).is_mount_of(image) && mount(2, 7).is_mount_of(image));
    assert!(!mount(4, 7).is_mount_of(image) && !mount(1, 7).is_mount_of(image));
    assert!(!mount(3, 8).is_mount_of(image) && !mount(0, 0).is_mount_of(image));
    assert!(!mount(u64::MAX, 7).is_mount_of(image));
    fs::remove_dir_all(root).unwrap();
}
