//! Thumbnails of picture files, made on worker threads and kept: in memory up to a byte cap,
//! and on disk under a folder the application names, so a folder of photos or an album's
//! covers costs its decoding once.
//!
//! [`Thumbnails`] is what a window holds. While drawing, it asks [`Thumbnails::request`] for
//! the thumbnail of a path at a size: a thumbnail kept in memory is there at once; otherwise
//! the work is queued on a [`Pool`] (rows on screen first, by the priority the caller gives)
//! and the answer is [`Lookup::Pending`]. When a thumbnail is made the pool calls the wake the
//! application gave (it sends the window a redraw), and the next frame calls
//! [`Thumbnails::collect`] to take what was made, then draws it.
//!
//! The thread that draws never touches the disk: the memory is keyed by the path and the size
//! alone. The worker reads the file's size and change time, looks in the [`DiskCache`] under a
//! key made of the path, those two and the size, and only on a miss reads, decodes
//! ([`mdshared_text::image::decode`]) and shrinks the picture, and stores the result. A file
//! that changes on disk gets a new disk key by itself (unless it is written again within one
//! tick of the file system's clock at the same length, which a key of this kind cannot see,
//! as no thumbnail cache keyed so can); for the memory the application calls
//! [`Thumbnails::invalidate`] when its watcher says so. Opening another folder is
//! [`Thumbnails::set_generation`]: work still queued for the old one is dropped.
//!
//! Sizes come in buckets ([`SIZES`]): a thumbnail asked for at 90 px is made at 128 and drawn
//! smaller, so a slider that resizes the icons does not decode everything again for every
//! pixel of its travel.
//!
//! Deliberately not here: the system's own thumbnails and file icons (Explorer's thumbnail
//! cache, freedesktop's `~/.cache/thumbnails`, QuickLook; roadmap F5, which can fill the same
//! caches); video frames and documents; watching the files (the application's watcher calls
//! `invalidate`); drawing (`Bitmap::image` draws an [`Image`]'s pixels); and a clock: the disk cache's
//! age is the files' own modification time.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use mdshared_app::pool::{Pool, Token};
use mdshared_text::image::{self, Image, Limits};

/// The sides thumbnails are made at, in pixels. A request is made at the smallest of these at
/// least as large as it asks, and at the largest when it asks for more.
pub const SIZES: [u32; 5] = [64, 128, 256, 512, 1024];

/// The largest picture file a worker reads, in bytes: a larger one fails with
/// [`Failure::TooLarge`] rather than filling memory. 128 MB passes any photo.
pub const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

/// The bucket a requested side falls in: the smallest of [`SIZES`] at least `side`, or the
/// largest. A side of 0 is the smallest bucket.
pub fn bucket(side: u32) -> u32 {
    SIZES
        .iter()
        .copied()
        .find(|&s| s >= side)
        .unwrap_or(SIZES[SIZES.len() - 1])
}

/// Why a thumbnail could not be made. The application shows its own icon for the file then.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    /// The file could not be read: missing, a folder, no permission.
    Unreadable(io::ErrorKind),
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// The file is not a picture this can decode, or a damaged one, or one over the limits.
    Picture(image::Error),
}

/// What [`Thumbnails::request`] answers.
#[derive(Clone, Debug)]
pub enum Lookup {
    /// The thumbnail, at most the bucket's size on its longer side.
    Ready(Arc<Image>),
    /// Being made, or queued; a wake will come.
    Pending,
    /// It could not be made, and is not tried again until [`Thumbnails::invalidate`] or
    /// [`Thumbnails::forget_failures`].
    Failed(Failure),
}

/// Thumbnails kept in memory: the least recently used go first when the bytes pass the cap.
/// Keys are anything the caller names a thumbnail by; [`Thumbnails`] uses a path and a size.
pub struct MemoryCache<K> {
    map: HashMap<K, (Arc<Image>, u64)>,
    bytes: usize,
    cap: usize,
    tick: u64,
}

impl<K: std::hash::Hash + Eq + Clone> MemoryCache<K> {
    /// An empty cache that keeps at most `cap_bytes` of pixels. A cap of 0 keeps nothing.
    pub fn new(cap_bytes: usize) -> MemoryCache<K> {
        MemoryCache {
            map: HashMap::new(),
            bytes: 0,
            cap: cap_bytes,
            tick: 0,
        }
    }

    /// The thumbnail under `key`, which becomes the most recently used.
    pub fn get(&mut self, key: &K) -> Option<Arc<Image>> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|(image, used)| {
            *used = tick;
            image.clone()
        })
    }

    /// Whether a thumbnail is kept under `key`; it does not count as a use.
    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Keeps `image` under `key` (replacing what was there), as the most recently used, then
    /// lets the least recently used go until the bytes are within the cap. A picture larger
    /// than the cap on its own is not kept.
    pub fn insert(&mut self, key: K, image: Arc<Image>) {
        self.remove(&key);
        let size = image.byte_len();
        if size > self.cap {
            return;
        }
        self.tick += 1;
        self.bytes += size;
        self.map.insert(key, (image, self.tick));
        self.trim();
    }

    /// Lets the thumbnail under `key` go. Returns whether there was one.
    pub fn remove(&mut self, key: &K) -> bool {
        match self.map.remove(key) {
            Some((image, _)) => {
                self.bytes -= image.byte_len();
                true
            }
            None => false,
        }
    }

    /// Lets go every thumbnail whose key `gone` says yes to.
    pub fn remove_where(&mut self, mut gone: impl FnMut(&K) -> bool) {
        let keys: Vec<K> = self.map.keys().filter(|k| gone(k)).cloned().collect();
        for k in keys {
            self.remove(&k);
        }
    }

    /// A new cap; what no longer fits goes at once, the least recently used first.
    pub fn set_cap(&mut self, cap_bytes: usize) {
        self.cap = cap_bytes;
        self.trim();
    }

    /// The bytes of pixels kept.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// How many thumbnails are kept.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Lets everything go.
    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }

    fn trim(&mut self) {
        while self.bytes > self.cap {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.remove(&oldest);
        }
    }
}

/// What names a thumbnail on disk: the file, what it was when the thumbnail was made, and the
/// size. A file that is saved again has another change time or size, so its old thumbnail is
/// simply never asked for again, and ages out.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct DiskKey {
    /// The picture file, as the application names it.
    pub path: PathBuf,
    /// Its length in bytes.
    pub len: u64,
    /// Its change time, in nanoseconds since 1970; 0 where the system gives none.
    pub modified: u128,
    /// The bucket ([`SIZES`]).
    pub side: u32,
}

impl DiskKey {
    /// The key for a file as it is on disk now, or the error reading its metadata gave. A
    /// folder is `Unreadable(InvalidInput)`.
    pub fn of(path: &Path, side: u32) -> Result<DiskKey, Failure> {
        let meta = std::fs::metadata(path).map_err(|e| Failure::Unreadable(e.kind()))?;
        if !meta.is_file() {
            return Err(Failure::Unreadable(io::ErrorKind::InvalidInput));
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        Ok(DiskKey {
            path: path.to_path_buf(),
            len: meta.len(),
            modified,
            side: bucket(side),
        })
    }

    /// The file name the thumbnail has in the cache folder: the SHA-256 of the key's parts
    /// (the path as the system's bytes or wide characters, the length, the change time and the
    /// side), in hex, with `.png`. The same key always gives the same name, on every run.
    pub fn file_name(&self) -> String {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"mdshared-thumb-1\0");
        bytes.extend_from_slice(self.path.as_os_str().as_encoded_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&self.len.to_le_bytes());
        bytes.extend_from_slice(&self.modified.to_le_bytes());
        bytes.extend_from_slice(&self.side.to_le_bytes());
        format!("{}.png", mdshared_app::update::sha256_hex(&bytes))
    }
}

/// Thumbnails kept as PNG files in one folder, with a cap on the folder's total size. Every
/// method may be called from any thread; two writers of one name write the same bytes.
#[derive(Clone, Debug)]
pub struct DiskCache {
    dir: PathBuf,
    cap: u64,
}

impl DiskCache {
    /// A cache in `dir` (made when the first thumbnail is stored) holding at most `cap_bytes`
    /// once [`trim`](Self::trim) runs.
    pub fn new(dir: impl Into<PathBuf>, cap_bytes: u64) -> DiskCache {
        DiskCache {
            dir: dir.into(),
            cap: cap_bytes,
        }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The thumbnail stored under `key`, or `None`. A stored file that does not decode, or
    /// decodes to a picture larger than the key's side, is damaged: it is removed and `None`
    /// is answered. A thumbnail read has its file's change time set to now, so it counts as
    /// recently used when the folder is trimmed.
    pub fn load(&self, key: &DiskKey) -> Option<Image> {
        let path = self.dir.join(key.file_name());
        let bytes = std::fs::read(&path).ok()?;
        let side = key.side.max(1);
        match image::decode(&bytes, Limits::new(side, u64::from(side) * u64::from(side))) {
            Ok(im) => {
                if let Ok(f) = std::fs::File::options().write(true).open(&path) {
                    _ = f.set_modified(SystemTime::now());
                }
                Some(im)
            }
            Err(_) => {
                _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    /// Stores `image` under `key`: written to a temporary name and renamed into place, so a
    /// reader never sees half a file.
    pub fn store(&self, key: &DiskKey, image: &Image) -> io::Result<()> {
        let bytes = image::encode_png(image);
        mdshared_app::settings::write_atomic_bytes(&self.dir.join(key.file_name()), &bytes)
    }

    /// Removes the least recently used thumbnails (by their files' change time) until the
    /// folder's `.png` files total at most the cap. Other files are left alone. Returns the
    /// bytes removed. A folder that does not exist holds nothing: `Ok(0)`.
    pub fn trim(&self) -> io::Result<u64> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file().then(|| {
                    (
                        m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        m.len(),
                        e.path(),
                    )
                })
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort();
        let mut removed = 0;
        for (_, len, path) in files {
            if total <= self.cap {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
                removed += len;
            }
        }
        Ok(removed)
    }
}

/// The source pixels covered by one output pixel along an axis, with their shares of its area.
/// These are the weights of `Image::scaled`, generated as needed rather than stored.
fn taps(pixel: u32, step: f64, limit: u32) -> impl Iterator<Item = (usize, f32)> + Clone {
    let lo = f64::from(pixel) * step;
    let hi = f64::from(pixel + 1) * step;
    (lo.floor() as u32..(hi.ceil() as u32).min(limit)).map(move |i| {
        let share = (hi.min(f64::from(i + 1)) - lo.max(f64::from(i))) / step;
        (i as usize, share as f32)
    })
}

/// Shrinks an already decoded picture to a nonzero thumbnail bucket, or returns it as is
/// when it fits. Keeps just two four-channel sums on the stack: finish each horizontal
/// average before weighting it vertically, in the same order and precision as `Image::scaled`.
fn shrink(source: Image, side: u32) -> Option<Image> {
    let longest = source.width.max(source.height);
    if longest <= side {
        return Some(source);
    }
    let k = f64::from(side) / f64::from(longest);
    let size = |n| ((f64::from(n) * k).round() as u32).clamp(1, side);
    let (w, h) = (size(source.width), size(source.height));
    let dx = f64::from(source.width) / f64::from(w);
    let dy = f64::from(source.height) / f64::from(h);
    let pixels = source.rgba.as_chunks::<4>().0;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let rows = taps(y, dy, source.height);
        for x in 0..w {
            let cols = taps(x, dx, source.width);
            let mut sum = [0.0; 4];
            for (sy, wy) in rows.clone() {
                let row = &pixels[sy * source.width as usize..];
                let mut across = [0.0; 4];
                for (sx, wx) in cols.clone() {
                    let [r, g, b, a] = row[sx];
                    let a = f32::from(a) / 255.0;
                    let pre = [f32::from(r) * a, f32::from(g) * a, f32::from(b) * a, a];
                    for (v, c) in across.iter_mut().zip(pre) {
                        *v += c * wx;
                    }
                }
                for (v, c) in sum.iter_mut().zip(across) {
                    *v += c * wy;
                }
            }
            let [r, g, b, a] = sum;
            let straight = |v: f32| if a > 0.0 { (v / a).round() as u8 } else { 0 };
            out.extend([
                straight(r),
                straight(g),
                straight(b),
                (a * 255.0).round() as u8,
            ]);
        }
    }
    Image::new(w, h, out)
}

/// Makes one thumbnail, as a worker does: the disk cache first, then the file. This is the
/// work [`Thumbnails`] queues, public so a caller with its own threads can use it.
/// The source is decoded under the usual [`Limits::DEFAULT`]. After decoding, shrinking
/// allocates only the final thumbnail's RGBA bytes (at most 4 MiB at the largest bucket).
pub fn make(
    path: &Path,
    side: u32,
    disk: Option<&DiskCache>,
    token: Option<&Token>,
) -> Result<Image, Failure> {
    let key = DiskKey::of(path, side)?;
    if key.len > MAX_FILE_BYTES {
        return Err(Failure::TooLarge);
    }
    if let Some(found) = disk.and_then(|d| d.load(&key)) {
        return Ok(found);
    }
    if token.is_some_and(Token::cancelled) {
        // No result is wanted; the error is never seen.
        return Err(Failure::Unreadable(io::ErrorKind::Interrupted));
    }
    let bytes = std::fs::read(path).map_err(|e| Failure::Unreadable(e.kind()))?;
    let full = image::decode(&bytes, Limits::DEFAULT).map_err(Failure::Picture)?;
    drop(bytes);
    let thumb = shrink(full, key.side).ok_or(Failure::Picture(image::Error::Damaged))?;
    if let Some(d) = disk {
        // A cache that cannot be written is a cache that does not help; the thumbnail stands.
        _ = d.store(&key, &thumb);
    }
    Ok(thumb)
}

/// The thumbnails of one window: the memory cache, the disk cache and the workers. See the
/// [module](self).
pub struct Thumbnails {
    memory: MemoryCache<(PathBuf, u32)>,
    failed: HashMap<(PathBuf, u32), Failure>,
    asked: HashMap<(PathBuf, u32), i32>,
    disk: Option<Arc<DiskCache>>,
    pool: Pool<(PathBuf, u32), Result<Image, Failure>>,
}

impl Thumbnails {
    /// Thumbnails made by at most `workers` threads (at least one; made only when needed),
    /// kept in memory up to `memory_cap` bytes and, when `disk` is given, on disk. `wake` is
    /// called from a worker when thumbnails wait to be [`collect`](Self::collect)ed; it must be
    /// cheap and must not block (send the window an event).
    pub fn new(
        workers: usize,
        memory_cap: usize,
        disk: Option<DiskCache>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Thumbnails {
        Thumbnails {
            memory: MemoryCache::new(memory_cap),
            failed: HashMap::new(),
            asked: HashMap::new(),
            disk: disk.map(Arc::new),
            pool: Pool::new(workers, wake),
        }
    }

    /// The thumbnail of `path` at `side` px (its bucket), or `Pending` after queueing it with
    /// `priority` (higher first: the rows on screen above the rows that will be) under the
    /// caller's current `generation`, or why it failed. Asking again while it is being made
    /// queues nothing more; asking with a higher priority moves queued work up. Touches no
    /// file on this thread.
    pub fn request(&mut self, path: &Path, side: u32, priority: i32, generation: u64) -> Lookup {
        let key = (path.to_path_buf(), bucket(side));
        if let Some(image) = self.memory.get(&key) {
            return Lookup::Ready(image);
        }
        if let Some(&why) = self.failed.get(&key) {
            return Lookup::Failed(why);
        }
        if let Some(&asked_at) = self.asked.get(&key) {
            // Queued at this priority already, or running: it will come.
            if asked_at >= priority || !self.pool.queued(&key) {
                return Lookup::Pending;
            }
        }
        let disk = self.disk.clone();
        let (p, s) = key.clone();
        let submitted = self
            .pool
            .submit(key.clone(), priority, generation, move |token| {
                make(&p, s, disk.as_deref(), Some(token))
            });
        match submitted {
            Ok(true) => {
                self.asked.insert(key, priority);
                Lookup::Pending
            }
            // A stale generation: nothing will come for it.
            Ok(false) => Lookup::Pending,
            Err(e) => Lookup::Failed(Failure::Unreadable(e.kind())),
        }
    }

    /// Takes the thumbnails made since the last call into the memory cache (and the failures
    /// into the failure list), and answers which `(path, bucket)` pairs changed, so the caller
    /// can redraw just those, or everything. Call it at the start of a frame, after a wake.
    pub fn collect(&mut self) -> Vec<(PathBuf, u32)> {
        let mut changed = Vec::new();
        for (key, result) in self.pool.drain() {
            self.asked.remove(&key);
            match result {
                Ok(image) => {
                    self.failed.remove(&key);
                    self.memory.insert(key.clone(), Arc::new(image));
                }
                Err(Failure::Unreadable(io::ErrorKind::Interrupted)) => continue,
                Err(why) => {
                    self.failed.insert(key.clone(), why);
                }
            }
            changed.push(key);
        }
        if self.pool.is_idle() {
            // Nothing runs, so nothing more comes for what was asked: work that panicked, or
            // was asked under a generation already old. Asked again, it is queued again.
            self.asked.clear();
        }
        changed
    }

    /// Forgets everything about `path` at every size (kept thumbnails, failures), so the next
    /// request makes it anew. For a file the watcher says changed or was removed.
    pub fn invalidate(&mut self, path: &Path) {
        self.memory.remove_where(|(p, _)| p == path);
        self.failed.retain(|(p, _), _| p != path);
    }

    /// Forgets every failure, so files that could not be read are tried again (after
    /// permissions changed, a drive came back).
    pub fn forget_failures(&mut self) {
        self.failed.clear();
    }

    /// Says that work asked for under a generation below `generation` is of no use (another
    /// folder is shown): it is dropped unrun, or its result thrown away. Kept thumbnails stay.
    pub fn set_generation(&mut self, generation: u64) {
        if generation > self.pool.generation() {
            self.pool.set_generation(generation);
            self.asked.clear();
        }
    }

    /// The memory cache, for its size and cap.
    pub fn memory(&mut self) -> &mut MemoryCache<(PathBuf, u32)> {
        &mut self.memory
    }

    /// The disk cache, if there is one; [`DiskCache::trim`] it now and then (at start, on a
    /// worker).
    pub fn disk(&self) -> Option<&DiskCache> {
        self.disk.as_deref()
    }

    /// Whether no work is queued or running.
    pub fn is_idle(&self) -> bool {
        self.pool.is_idle()
    }
}

#[cfg(test)]
#[path = "thumbs_tests.rs"]
mod tests;
