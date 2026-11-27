//! Linux self-update needs proof that `$APPIMAGE` names the file the AppImage runtime is
//! using, not merely an environment variable or the human-readable FUSE mount source.
//! The Type 2 runtime gives its AppRun an open AppDir (fd 1023) and a pipe whose other end
//! keeps the FUSE daemon alive. We cannot use a wrapper's pipes to identify the child's mount.
//! For extract-and-run the runtime stays as the direct parent, even after closing its image fd.
//! Inaccessible proc files are not proof of anything.
//!
//! Three things are asked, and all must hold. This process is the image's application: the
//! file it executes is the one the AppDir's `AppRun` leads to, so a second program packed in
//! the same image (an engine, a sister application) never updates the image as if it were its
//! own. The image is in use: a live runtime process executes it (the FUSE daemon whose
//! keepalive pipe this process holds, or the waiting parent of an extract-and-run). And, for
//! a mounted image, the mount is that image: the kernel shows no link from a FUSE mount to
//! the process that serves it, so the pipe alone would let the daemon of image B vouch while
//! the AppDir is image A's. The link used instead is the filesystem itself: the size and the
//! inode count that `statvfs` reports for the AppDir must be those in the squashfs superblock
//! inside the file ([`Geometry`]). Two images agree in both only when they hold the same
//! filesystem, to the block and the inode.
//!
//! What this is for: an inherited or mixed-up environment must never make the updater
//! replace a file that is not the image this process runs from. It is no defence against
//! another program of the same account, which can write a matching file, or replace the
//! image, without asking; nothing in this process can be.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileExt, FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const APPDIR_FD: u32 = 1023;

/// The running AppImage's path. At each call, verify the mount and the kernel's image
/// identity again. The caller also needs to check that the installation directory can be written.
pub(crate) fn current() -> Option<PathBuf> {
    let value = std::env::var_os("APPIMAGE")?;
    let dir = PathBuf::from(std::env::var_os("APPDIR")?)
        .canonicalize()
        .ok()?;
    let proc = Path::new("/proc");
    // Do not reopen current_exe() by its pathname: an overlay can have hidden the executing
    // file or the mount that contains it. fdinfo of /proc/self/exe identifies the real mount.
    let exe = path_handle(&proc.join("self/exe"))?;
    let appdir = path_handle(&dir)?;
    if !appdir.metadata().ok()?.is_dir() {
        return None;
    }
    find(
        proc,
        &value,
        &dir,
        exe.as_raw_fd(),
        appdir.as_raw_fd(),
        Geometry::mounted(&appdir),
    )
}

/// How large a squashfs is, as both its superblock and `statvfs` on its mount say it: the
/// number of blocks its bytes take, and the number of inodes. This is what ties a mounted
/// AppDir to the image file it comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Geometry {
    blocks: u64,
    inodes: u64,
}

impl Geometry {
    /// What the filesystem `dir` is on reports. `None` when it cannot be asked.
    // The two counts are 64 bits wide on some targets and narrower on others.
    #[allow(clippy::unnecessary_cast)]
    fn mounted(dir: &File) -> Option<Self> {
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: the descriptor is open and `stat` is writable storage for one statvfs.
        if unsafe { libc::fstatvfs(dir.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: fstatvfs returned 0, so it filled the structure.
        let stat = unsafe { stat.assume_init() };
        Some(Self {
            blocks: stat.f_blocks as u64,
            inodes: stat.f_files as u64,
        })
    }

    /// What the squashfs inside the Type 2 image `image` says. The filesystem begins where the
    /// runtime's ELF ends: after its section header table or after its last section, whichever
    /// is later, as the runtime itself finds it; the end of the table is tried as well.
    /// `None` for anything but a 64-bit little-endian ELF followed by a squashfs 4 superblock.
    fn of_image(image: &File) -> Option<Self> {
        let mut elf = [0; 64];
        image.read_exact_at(&mut elf, 0).ok()?;
        // EI_CLASS 2 is 64-bit and EI_DATA 1 is little-endian.
        if !elf.starts_with(b"\x7fELF") || elf[4] != 2 || elf[5] != 1 {
            return None;
        }
        let table = le(&elf, 0x28, 8)?;
        let (entry, count) = (le(&elf, 0x3a, 2)?, le(&elf, 0x3c, 2)?);
        let table_end = table.checked_add(entry * count)?;
        let last_section_end = (|| {
            let mut header = [0; 64];
            let at = table.checked_add(entry * count.checked_sub(1)?)?;
            image.read_exact_at(&mut header, at).ok()?;
            le(&header, 0x18, 8)?.checked_add(le(&header, 0x20, 8)?)
        })();
        let ends = [
            last_section_end.map(|end| end.max(table_end)),
            Some(table_end),
        ];
        ends.into_iter().flatten().find_map(|at| {
            let mut block = [0; 48];
            image.read_exact_at(&mut block, at).ok()?;
            Self::of_superblock(&block)
        })
    }

    /// The first 48 bytes of a squashfs 4 superblock: the magic, the inode count at 4, the block
    /// size at 12 with its logarithm at 22, the major version at 28, the bytes used at 40.
    fn of_superblock(block: &[u8; 48]) -> Option<Self> {
        let log = le(block, 22, 2)?;
        let used = le(block, 40, 8)?;
        if !block.starts_with(b"hsqs")
            || le(block, 28, 2)? != 4
            || !(12..=20).contains(&log)
            || le(block, 12, 4)? != 1 << log
            || used == 0
        {
            return None;
        }
        Some(Self {
            blocks: ((used - 1) >> log) + 1,
            inodes: le(block, 4, 4)?,
        })
    }

    /// Whether a mount reporting `self` is the filesystem inside an image that says `image`.
    /// A mount may count a last, partly used block or not.
    fn is_mount_of(self, image: Self) -> bool {
        self.inodes == image.inodes
            && (self.blocks == image.blocks || self.blocks.checked_add(1) == Some(image.blocks))
    }
}

/// The little-endian number of `len` bytes at `at`.
fn le(bytes: &[u8], at: usize, len: usize) -> Option<u64> {
    let field = bytes.get(at..)?.get(..len)?;
    let mut number = [0; 8];
    number.get_mut(..len)?.copy_from_slice(field);
    Some(u64::from_le_bytes(number))
}

fn path_handle(path: &Path) -> Option<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH)
        .open(path)
        .ok()
}

fn mount_id(info: &Path) -> Option<u64> {
    let fields = fs::read_to_string(info).ok()?;
    fields
        .lines()
        .find_map(|l| l.strip_prefix("mnt_id:"))?
        .trim()
        .parse()
        .ok()
}

fn find(
    proc: &Path,
    value: &OsStr,
    dir: &Path,
    exe_fd: i32,
    dir_fd: i32,
    mounted: Option<Geometry>,
) -> Option<PathBuf> {
    let exe = fs::read_link(proc.join("self/exe")).ok()?;
    let exe_id = mount_id(&proc.join(format!("self/fdinfo/{exe_fd}")))?;
    let dir_id = mount_id(&proc.join(format!("self/fdinfo/{dir_fd}")))?;
    if exe.strip_prefix(dir).ok()?.as_os_str().is_empty() || exe_id != dir_id {
        return None;
    }
    // Only the application the image starts updates it: `AppRun`, followed to its end, is the
    // file this process executes. An image whose `AppRun` is a script or a launcher of its own
    // is not updated in place.
    let running = fs::metadata(proc.join("self/exe")).ok()?;
    if !fs::metadata(dir.join("AppRun")).is_ok_and(|entry| same_file(&entry, &running)) {
        return None;
    }
    let mounts = fs::read(proc.join("self/mountinfo")).ok()?;
    let mount = mount_by_id(&mounts, exe_id)?;
    let target = super::path_from(Some(value))?;
    let opened = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&target)
        .ok()?;
    let image = opened.metadata().ok()?;
    let mut magic = [0; 11];
    if !image.is_file()
        || opened.read_exact_at(&mut magic, 0).is_err()
        || !magic.starts_with(b"\x7fELF")
        || magic[8..] != *b"AI\x02"
    {
        return None;
    }

    let verified = if mount.fuse_at(dir) {
        // The daemon shows that the image is in use; the geometry, that this mount is it.
        mounted
            .zip(Geometry::of_image(&opened))
            .is_some_and(|(mount, image)| mount.is_mount_of(image))
            && mounted_runtime(proc, dir, dir_id, &image)
    } else if dir.starts_with(&mount.point) && extraction_dir(dir) {
        extract_runtime(proc, &image)
    } else {
        false
    };
    // The canonical name must still denote the opened inode, without following a new link.
    (verified && same_file(&fs::symlink_metadata(&target).ok()?, &image)).then_some(target)
}

fn extraction_dir(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|name| name.as_bytes().strip_prefix(b"appimage_extracted_"))
        .is_some_and(|hash| hash.len() == 32 && hash.iter().all(u8::is_ascii_hexdigit))
}

fn same_file(meta: &Metadata, image: &Metadata) -> bool {
    meta.is_file() && identity(meta) == identity(image)
}

fn identity(meta: &Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}

fn parent(process: &Path) -> Option<u32> {
    let status = fs::read(process.join("status")).ok()?;
    let field = status
        .split(|&byte| byte == b'\n')
        .find_map(|line| line.strip_prefix(b"PPid:"))?
        .trim_ascii();
    if !field.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(field)
        .ok()?
        .parse::<u32>()
        .ok()
        .filter(|&pid| pid != 0)
}

fn pinned_process(process: &Path) -> Option<(File, PathBuf)> {
    // Walk through the open proc directory, rather than resolving the numeric PID twice.
    let handle = path_handle(process)?;
    let pinned = PathBuf::from(format!("/proc/self/fd/{}", handle.as_raw_fd()));
    Some((handle, pinned))
}

fn extract_runtime(proc: &Path, image: &Metadata) -> bool {
    let check = || {
        let this = proc.join("self");
        let pid = parent(&this)?;
        let (_process, pinned) = pinned_process(&proc.join(pid.to_string()))?;
        let exe = path_handle(&pinned.join("exe"))?;
        // The kernel pins the executable of the waiting parent. It need not have a copy
        // open in its descriptor table after extraction, and the pathname can be replaced.
        (same_file(&exe.metadata().ok()?, image) && parent(&this) == Some(pid)).then_some(())
    };
    check().is_some()
}

fn mounted_runtime(proc: &Path, dir: &Path, id: u64, image: &Metadata) -> bool {
    let self_dir = proc.join("self");
    let fd_dir = self_dir.join(format!("fd/{APPDIR_FD}"));
    if mount_id(&self_dir.join(format!("fdinfo/{APPDIR_FD}"))) != Some(id)
        || !fs::metadata(fd_dir)
            .ok()
            .zip(fs::metadata(dir).ok())
            .is_some_and(|(handle, path)| handle.is_dir() && identity(&handle) == identity(&path))
    {
        return false;
    }
    // An ancestor could hold its own AppDir and read ends for unrelated AppImages.
    // Missing application descriptors are not a reason to guess from that ancestor.
    let pipes: Vec<_> = descriptors(&self_dir)
        .filter_map(|(fd, name)| pipe(&self_dir, fd, &name, libc::O_RDONLY as u32))
        .collect();
    !pipes.is_empty()
        && fs::read_dir(proc).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry.file_name().as_bytes().iter().all(u8::is_ascii_digit)
                    && process_matches(&entry.path(), image, &pipes)
            })
        })
}

fn descriptors(dir: &Path) -> impl Iterator<Item = (u32, PathBuf)> {
    fs::read_dir(dir.join("fd"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_str()?.parse().ok()?, e.path())))
}

fn pipe(process: &Path, fd: u32, name: &Path, access: u32) -> Option<(u64, u64)> {
    if fd < 3 {
        return None;
    }
    let meta = fs::metadata(name).ok()?;
    if !meta.file_type().is_fifo() || !is_anonymous_pipe(name, meta.ino()) {
        return None;
    }
    let info = fs::read_to_string(process.join(format!("fdinfo/{fd}"))).ok()?;
    let flags = info.lines().find_map(|l| l.strip_prefix("flags:"))?;
    (u32::from_str_radix(flags.trim(), 8).ok()? & libc::O_ACCMODE as u32 == access)
        .then(|| identity(&meta))
}

fn is_anonymous_pipe(name: &Path, inode: u64) -> bool {
    let Ok(link) = fs::read_link(name) else {
        return false;
    };
    // The proc fixture points a fake fd slot at a real anonymous pipe in this process.
    #[cfg(test)]
    let link = if link.starts_with("/proc/self/fd") {
        fs::read_link(&link).unwrap_or(link)
    } else {
        link
    };
    link.as_os_str() == OsStr::new(&format!("pipe:[{inode}]"))
}

fn process_matches(process: &Path, image: &Metadata, read_ends: &[(u64, u64)]) -> bool {
    // First filter by image. Then hold the resolved proc directory open so PID reuse cannot
    // contribute the exe from one process and the backing fd or pipe from another.
    if !fs::metadata(process.join("exe")).is_ok_and(|meta| same_file(&meta, image)) {
        return false;
    }
    let Some((_handle, pinned)) = pinned_process(process) else {
        return false;
    };
    if !fs::metadata(pinned.join("exe")).is_ok_and(|meta| same_file(&meta, image)) {
        return false;
    }
    let (mut backing, mut keepalive, mut fuse) = (false, false, false);
    for (fd, name) in descriptors(&pinned) {
        if !backing && fs::metadata(&name).is_ok_and(|meta| same_file(&meta, image)) {
            backing = true;
        }
        if !keepalive {
            keepalive = pipe(&pinned, fd, &name, libc::O_WRONLY as u32)
                .is_some_and(|p| read_ends.contains(&p));
        }
        if !fuse {
            fuse = fs::read_link(name).is_ok_and(|p| p == Path::new("/dev/fuse"));
        }
        if backing && keepalive && fuse {
            return true;
        }
    }
    false
}

struct Mount<'a> {
    root: PathBuf,
    point: PathBuf,
    read_only: bool,
    kind: &'a [u8],
    #[cfg(test)]
    source: PathBuf,
}

impl Mount<'_> {
    fn fuse_at(&self, dir: &Path) -> bool {
        self.root == Path::new("/")
            && self.point == dir
            && self.read_only
            && (self.kind == b"fuse" || self.kind.starts_with(b"fuse."))
    }
}

/// Match by the kernel's mount ID, not by a path prefix or by the mount source label.
fn mount_by_id(mounts: &[u8], id: u64) -> Option<Mount<'_>> {
    mounts.split(|&c| c == b'\n').find_map(|line| {
        let mut fields = line.split(|&c| c == b' ');
        if std::str::from_utf8(fields.next()?)
            .ok()?
            .parse::<u64>()
            .ok()?
            != id
        {
            return None;
        }
        let root = mount_path(fields.nth(2)?)?;
        let point = mount_path(fields.next()?)?;
        let read_only = fields.next()?.split(|&b| b == b',').any(|f| f == b"ro");
        fields.find(|&f| f == b"-")?;
        Some(Mount {
            root,
            point,
            read_only,
            kind: fields.next()?,
            #[cfg(test)]
            source: mount_path(fields.next()?)?,
        })
    })
}

/// Mountinfo escapes whitespace and backslashes as octal bytes.
fn mount_path(mut rest: &[u8]) -> Option<PathBuf> {
    let mut out = Vec::new();
    while let Some((&byte, tail)) = rest.split_first() {
        rest = tail;
        out.push(if byte == b'\\' {
            let (digits, tail) = rest.split_at_checked(3)?;
            rest = tail;
            u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok()?
        } else {
            byte
        });
    }
    Some(OsString::from_vec(out).into())
}

#[cfg(test)]
mod tests;
