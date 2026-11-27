use super::*;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// A folder of its own under the system's temporary folder, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!(
            "mdshared-thumbs-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A `w`×`h` picture of one colour.
fn plain(w: u32, h: u32, rgba: [u8; 4]) -> Image {
    Image::new(w, h, rgba.repeat((w * h) as usize)).unwrap()
}

/// Writes a `w`×`h` PNG of one colour at `path`.
fn write_png(path: &Path, w: u32, h: u32, rgba: [u8; 4]) {
    std::fs::write(path, image::encode_png(&plain(w, h, rgba))).unwrap();
}

/// Writes the file again as a save would, and moves its change time on by ten seconds: two
/// writes in one tick of the file system's clock, of the same length, are the same file to a
/// key of length and change time, and a test must not depend on the clock's tick.
fn rewrite_png(path: &Path, w: u32, h: u32, rgba: [u8; 4]) {
    let before = std::fs::metadata(path).unwrap().modified().unwrap();
    write_png(path, w, h, rgba);
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(before + Duration::from_secs(10)).unwrap();
}

/// Thumbnails whose wake sends on a channel.
fn thumbnails(memory_cap: usize, disk: Option<DiskCache>) -> (Thumbnails, Receiver<()>) {
    let (tx, rx) = channel();
    let tx = std::sync::Mutex::new(tx);
    let t = Thumbnails::new(2, memory_cap, disk, move || {
        _ = tx.lock().unwrap().send(());
    });
    (t, rx)
}

/// Collects until `key` has changed, or fails after five seconds.
fn wait_for(t: &mut Thumbnails, rx: &Receiver<()>, path: &Path, side: u32) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if t.collect()
            .iter()
            .any(|(p, s)| p == path && *s == bucket(side))
        {
            return;
        }
        let left = end.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no thumbnail for {path:?} came");
        _ = rx.recv_timeout(left.min(Duration::from_millis(50)));
    }
}

fn ready(l: Lookup) -> Arc<Image> {
    match l {
        Lookup::Ready(im) => im,
        other => panic!("not ready: {other:?}"),
    }
}

#[test]
fn a_thumbnail_refuses_an_oversized_source_from_its_header() {
    let dir = Dir::new("source-limit");
    let path = dir.join("large.png");
    // An 8001 x 5000 RGBA PNG header and an empty data chunk. The header is over the
    // decoder's 40 MP limit; trying to read the pixels would give Damaged instead.
    let header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x1f\x41\0\0\x13\x88\x08\x06\0\0\0\xd3\x28\x30\x67\0\0\0\0IDAT\x35\xaf\x06\x1e";
    std::fs::write(&path, header).unwrap();
    assert_eq!(
        make(&path, 64, None, None),
        Err(Failure::Picture(image::Error::TooLarge {
            width: 8001,
            height: 5000
        }))
    );
}

#[test]
fn thumbnail_pixels_match_the_original_area_average() {
    let dir = Dir::new("scale-pixels");
    let path = dir.join("picture.png");
    for (w, h) in [(937, 613), (157, 281), (1, 901), (731, 1), (19, 7)] {
        // Uneven dimensions cut through source pixels. Vary all four channels,
        // including fully transparent regions with colour that must not bleed.
        let mut bytes = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if x % 19 < 8 {
                    0
                } else {
                    (x * 13 + y * 7) as u8
                };
                bytes.extend([x as u8, y as u8, (x ^ y) as u8, a]);
            }
        }
        let source = Image::new(w, h, bytes).unwrap();
        std::fs::write(&path, image::encode_png(&source)).unwrap();
        let expected = source.scaled_to_fit(128, 128).unwrap();
        let thumb = make(&path, 128, None, None).unwrap();
        assert_eq!(
            (thumb.width, thumb.height),
            (expected.width, expected.height)
        );
        assert!(
            thumb.rgba == expected.rgba,
            "different pixels for {w} x {h}"
        );
    }
}

#[test]
fn a_24_megapixel_source_makes_a_thumbnail_with_only_the_output_buffer() {
    let dir = Dir::new("24mp");
    let path = dir.join("photo.png");
    write_png(&path, 6000, 4000, [23, 58, 173, 255]);
    let thumb = make(&path, 1024, None, None).unwrap();
    assert_eq!((thumb.width, thumb.height), (1024, 683));
    assert_eq!(thumb.rgba.capacity(), 1024 * 683 * 4);
    assert!(
        thumb
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|c| *c == [23, 58, 173, 255])
    );
}

#[test]
fn a_side_falls_in_the_smallest_bucket_that_holds_it() {
    assert_eq!(bucket(0), 64);
    assert_eq!(bucket(64), 64);
    assert_eq!(bucket(65), 128);
    assert_eq!(bucket(90), 128);
    assert_eq!(bucket(1024), 1024);
    assert_eq!(bucket(5000), 1024);
}

#[test]
fn the_memory_cache_lets_the_least_recently_used_go_first() {
    let one = Arc::new(plain(4, 4, [1, 2, 3, 255])); // 64 bytes
    let mut m = MemoryCache::new(64 * 3);
    m.insert("a", one.clone());
    m.insert("b", one.clone());
    m.insert("c", one.clone());
    assert_eq!((m.len(), m.bytes()), (3, 192));
    // "a" is used, so "b" is now the oldest.
    assert!(m.get(&"a").is_some());
    m.insert("d", one.clone());
    assert!(m.contains(&"a") && !m.contains(&"b"));
    assert!(m.contains(&"c") && m.contains(&"d"));
    assert_eq!(m.bytes(), 192);
    // Replacing a key does not count it twice.
    m.insert("d", one.clone());
    assert_eq!((m.len(), m.bytes()), (3, 192));
    m.set_cap(64);
    assert_eq!(m.len(), 1);
    assert!(m.contains(&"d"));
    m.clear();
    assert!(m.is_empty());
    assert_eq!(m.bytes(), 0);
}

#[test]
fn a_picture_larger_than_the_memory_cap_is_not_kept() {
    let mut m = MemoryCache::new(100);
    m.insert(1, Arc::new(plain(4, 4, [0; 4])));
    m.insert(2, Arc::new(plain(8, 8, [0; 4])));
    assert!(m.contains(&1));
    assert!(!m.contains(&2));
    assert_eq!(m.bytes(), 64);
}

#[test]
fn remove_where_lets_go_of_the_matching_keys() {
    let mut m = MemoryCache::new(1 << 20);
    for k in 0..6 {
        m.insert(k, Arc::new(plain(2, 2, [0; 4])));
    }
    m.remove_where(|k| k % 2 == 0);
    assert_eq!(m.len(), 3);
    assert_eq!(m.bytes(), 3 * 16);
    assert!(!m.remove(&0));
    assert!(m.remove(&1));
}

#[test]
fn a_disk_key_changes_with_the_file_and_the_size() {
    let dir = Dir::new("key");
    let path = dir.join("a.png");
    write_png(&path, 10, 10, [255, 0, 0, 255]);
    let small = DiskKey::of(&path, 50).unwrap();
    assert_eq!(small.side, 64);
    assert_eq!(
        small.file_name(),
        DiskKey::of(&path, 64).unwrap().file_name()
    );
    assert_ne!(
        small.file_name(),
        DiskKey::of(&path, 200).unwrap().file_name()
    );
    assert!(small.file_name().ends_with(".png"));
    assert_eq!(small.file_name().len(), 64 + 4);
    rewrite_png(&path, 12, 10, [255, 0, 0, 255]);
    assert_ne!(
        small.file_name(),
        DiskKey::of(&path, 50).unwrap().file_name()
    );
    assert_eq!(
        DiskKey::of(&dir.join("missing.png"), 64),
        Err(Failure::Unreadable(io::ErrorKind::NotFound))
    );
    assert_eq!(
        DiskKey::of(&dir.0, 64),
        Err(Failure::Unreadable(io::ErrorKind::InvalidInput))
    );
}

#[test]
fn the_disk_cache_gives_back_what_it_stored_and_drops_a_damaged_file() {
    let dir = Dir::new("disk");
    let path = dir.join("a.png");
    write_png(&path, 10, 10, [0, 0, 255, 255]);
    let cache = DiskCache::new(dir.join("cache"), 1 << 20);
    let key = DiskKey::of(&path, 64).unwrap();
    assert!(cache.load(&key).is_none());
    let thumb = plain(8, 6, [9, 8, 7, 200]);
    cache.store(&key, &thumb).unwrap();
    assert_eq!(cache.load(&key), Some(thumb));
    let stored = cache.dir().join(key.file_name());
    std::fs::write(&stored, b"\x89PNG not really").unwrap();
    assert!(cache.load(&key).is_none());
    assert!(!stored.exists());
}

#[test]
fn a_stored_picture_larger_than_its_key_counts_as_damaged() {
    let dir = Dir::new("toolarge");
    let path = dir.join("a.png");
    write_png(&path, 10, 10, [0, 0, 255, 255]);
    let cache = DiskCache::new(dir.join("cache"), 1 << 20);
    let key = DiskKey::of(&path, 64).unwrap();
    cache.store(&key, &plain(65, 1, [0; 4])).unwrap();
    assert!(cache.load(&key).is_none());
    assert!(!cache.dir().join(key.file_name()).exists());
}

#[test]
fn trimming_removes_the_oldest_thumbnails_until_the_folder_fits() {
    let dir = Dir::new("trim");
    let cache_dir = dir.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let now = SystemTime::now();
    for (i, name) in ["old.png", "mid.png", "new.png"].iter().enumerate() {
        let p = cache_dir.join(name);
        std::fs::write(&p, [0u8; 100]).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(now - Duration::from_secs(1000 - i as u64 * 100))
            .unwrap();
    }
    std::fs::write(cache_dir.join("notes.txt"), [0u8; 500]).unwrap();
    let cache = DiskCache::new(&cache_dir, 200);
    assert_eq!(cache.trim().unwrap(), 100);
    assert!(!cache_dir.join("old.png").exists());
    assert!(cache_dir.join("mid.png").exists());
    assert!(cache_dir.join("new.png").exists());
    assert!(cache_dir.join("notes.txt").exists());
    assert_eq!(DiskCache::new(dir.join("none"), 0).trim().unwrap(), 0);
}

#[test]
fn make_shrinks_the_picture_into_its_bucket_and_stores_it() {
    let dir = Dir::new("make");
    let path = dir.join("wide.png");
    write_png(&path, 300, 150, [10, 200, 30, 255]);
    let cache = DiskCache::new(dir.join("cache"), 1 << 20);
    let thumb = make(&path, 100, Some(&cache), None).unwrap();
    assert_eq!((thumb.width, thumb.height), (128, 64));
    assert_eq!(thumb.pixel(5, 5), Some([10, 200, 30, 255]));
    let key = DiskKey::of(&path, 100).unwrap();
    assert_eq!(cache.load(&key), Some(thumb));
    // A picture already smaller than the bucket is not enlarged.
    let tiny = dir.join("tiny.png");
    write_png(&tiny, 20, 10, [0; 4]);
    let t = make(&tiny, 256, None, None).unwrap();
    assert_eq!((t.width, t.height), (20, 10));
}

#[test]
fn make_says_why_a_file_has_no_thumbnail() {
    let dir = Dir::new("fail");
    let text = dir.join("notes.png");
    std::fs::write(&text, b"just words").unwrap();
    assert!(matches!(
        make(&text, 64, None, None),
        Err(Failure::Picture(_))
    ));
    assert_eq!(
        make(&dir.join("gone.png"), 64, None, None),
        Err(Failure::Unreadable(io::ErrorKind::NotFound))
    );
}

#[test]
fn a_request_is_pending_until_collected_then_ready_from_memory() {
    let dir = Dir::new("request");
    let path = dir.join("a.png");
    write_png(&path, 200, 100, [1, 2, 3, 255]);
    let (mut t, rx) = thumbnails(1 << 20, None);
    assert!(matches!(t.request(&path, 64, 0, 0), Lookup::Pending));
    // Asking again does not queue it twice.
    assert!(matches!(t.request(&path, 64, 0, 0), Lookup::Pending));
    wait_for(&mut t, &rx, &path, 64);
    let im = ready(t.request(&path, 64, 0, 0));
    assert_eq!((im.width, im.height), (64, 32));
    // Another size in the same bucket is the same thumbnail.
    assert!(Arc::ptr_eq(&im, &ready(t.request(&path, 40, 0, 0))));
    assert!(matches!(t.request(&path, 128, 0, 0), Lookup::Pending));
}

#[test]
fn a_failure_is_kept_until_the_path_is_invalidated() {
    let dir = Dir::new("failed");
    let path = dir.join("a.png");
    std::fs::write(&path, b"not yet a picture").unwrap();
    let (mut t, rx) = thumbnails(1 << 20, None);
    assert!(matches!(t.request(&path, 64, 0, 0), Lookup::Pending));
    wait_for(&mut t, &rx, &path, 64);
    assert!(matches!(
        t.request(&path, 64, 0, 0),
        Lookup::Failed(Failure::Picture(_))
    ));
    write_png(&path, 10, 10, [5, 5, 5, 255]);
    // Still failed: nothing told it the file changed.
    assert!(matches!(t.request(&path, 64, 0, 0), Lookup::Failed(_)));
    t.invalidate(&path);
    assert!(matches!(t.request(&path, 64, 0, 0), Lookup::Pending));
    wait_for(&mut t, &rx, &path, 64);
    assert_eq!(ready(t.request(&path, 64, 0, 0)).width, 10);
}

#[test]
fn invalidating_makes_a_changed_file_anew() {
    let dir = Dir::new("invalidate");
    let path = dir.join("a.png");
    write_png(&path, 10, 10, [255, 0, 0, 255]);
    let cache = DiskCache::new(dir.join("cache"), 1 << 20);
    let (mut t, rx) = thumbnails(1 << 20, Some(cache));
    t.request(&path, 64, 0, 0);
    wait_for(&mut t, &rx, &path, 64);
    assert_eq!(
        ready(t.request(&path, 64, 0, 0)).pixel(0, 0),
        Some([255, 0, 0, 255])
    );
    rewrite_png(&path, 11, 10, [0, 255, 0, 255]);
    t.invalidate(&path);
    assert_eq!(t.memory().len(), 0);
    t.request(&path, 64, 0, 0);
    wait_for(&mut t, &rx, &path, 64);
    let im = ready(t.request(&path, 64, 0, 0));
    assert_eq!((im.width, im.pixel(0, 0)), (11, Some([0, 255, 0, 255])));
}

#[test]
fn a_second_window_finds_the_thumbnail_on_disk() {
    let dir = Dir::new("second");
    let path = dir.join("a.png");
    write_png(&path, 300, 300, [7, 7, 7, 255]);
    let cache = DiskCache::new(dir.join("cache"), 1 << 20);
    let (mut t, rx) = thumbnails(1 << 20, Some(cache.clone()));
    t.request(&path, 256, 0, 0);
    wait_for(&mut t, &rx, &path, 256);
    drop(t);
    // Spoil the picture but keep its length and change time: only the disk cache can answer.
    let meta = std::fs::metadata(&path).unwrap();
    std::fs::write(&path, vec![0u8; meta.len() as usize]).unwrap();
    let f = std::fs::File::options().write(true).open(&path).unwrap();
    f.set_modified(meta.modified().unwrap()).unwrap();
    drop(f);
    assert!(make(&path, 256, None, None).is_err());
    let (mut t, rx) = thumbnails(1 << 20, Some(cache));
    t.request(&path, 256, 0, 0);
    wait_for(&mut t, &rx, &path, 256);
    let im = ready(t.request(&path, 256, 0, 0));
    assert_eq!((im.width, im.pixel(9, 9)), (256, Some([7, 7, 7, 255])));
}

#[test]
fn a_new_generation_drops_the_old_ones_work() {
    let dir = Dir::new("generation");
    let path = dir.join("a.png");
    write_png(&path, 10, 10, [1, 1, 1, 255]);
    let (mut t, rx) = thumbnails(1 << 20, None);
    t.set_generation(5);
    // Work asked for under an older generation is never made.
    assert!(matches!(t.request(&path, 64, 0, 4), Lookup::Pending));
    assert!(t.is_idle());
    assert!(t.collect().is_empty());
    assert!(matches!(t.request(&path, 64, 0, 5), Lookup::Pending));
    wait_for(&mut t, &rx, &path, 64);
    assert!(matches!(t.request(&path, 64, 0, 5), Lookup::Ready(_)));
}
