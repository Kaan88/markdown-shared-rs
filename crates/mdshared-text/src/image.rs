//! Pictures read from files: PNG and JPEG decoded into 8-bit RGBA under limits that a hostile
//! file cannot get past, the turn a camera wrote into a JPEG's EXIF data applied, and a
//! downscale that averages areas, for thumbnails and cover art.
//!
//! The caller owns the bytes (read from a file, a tag, a download) and decides where the
//! result goes; nothing here opens a file, starts a thread or keeps a cache. A thumbnail cache
//! that keeps scaled pictures in memory and on disk is built on top of this, in `mdshared-ui`.
//!
//! [`decode`] reads a picture whole: it looks at the first bytes to know the format
//! ([`sniff`]), refuses one larger than its [`Limits`] before allocating its pixels, decodes,
//! and turns a JPEG upright by its EXIF orientation. [`dimensions`] reads only the headers,
//! for a layout that wants the size before the pixels. [`Image::scaled_to_fit`] shrinks a
//! picture into a box with an area average in premultiplied alpha, so a shrunk picture is
//! neither aliased nor darkened at transparent edges.
//!
//! JPEG is decoded by `zune-jpeg` (pure Rust; the owner chose it on 2026-11-24). A panic inside
//! a decoder on a damaged file is caught and reported as [`Error::Damaged`], so one bad picture
//! cannot take a window down. That needs `panic = "unwind"` (the workspace's profiles have it);
//! a build with `panic = "abort"` loses this guard. None was seen in the tests' damaged input.
//!
//! Deliberately not here: GIF, WebP, BMP, HEIC and AVIF (they are recognised by [`sniff`] and
//! refused as [`Error::Unsupported`]; a decoder for one is a new dependency, which the owner
//! decides); animation (the first frame of an APNG is what PNG gives); colour profiles (ICC
//! data is ignored, so a wide-gamut picture shows a little dull); progressive display while a
//! picture loads; encoding of anything but what `Bitmap::write_png` already writes.
//!
//! The existing [`decode_png_rgba`](crate::decode_png_rgba) is unchanged and keeps its own
//! limit of 4096 px a side.

use std::fmt;

/// The kind of picture a run of bytes holds, from its first bytes alone.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Portable Network Graphics: decoded.
    Png,
    /// JPEG (JFIF or EXIF): decoded.
    Jpeg,
    /// GIF: recognised, not decoded.
    Gif,
    /// WebP: recognised, not decoded.
    WebP,
    /// Windows bitmap: recognised, not decoded.
    Bmp,
    /// None of the above, or too few bytes to tell.
    Unknown,
}

/// What a run of bytes is, by its signature. Never fails: anything it does not know, an empty
/// slice included, is [`Format::Unknown`].
pub fn sniff(bytes: &[u8]) -> Format {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Format::Png
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Format::Jpeg
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Format::Gif
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Format::WebP
    } else if bytes.starts_with(b"BM") && bytes.len() >= 14 {
        Format::Bmp
    } else {
        Format::Unknown
    }
}

/// The largest picture [`decode`] agrees to read: a longest side and a number of pixels. A
/// picture over either is refused from its header, before its pixels are allocated, so a file
/// that claims to be 60,000 px square costs nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    max_side: u32,
    max_pixels: u64,
}

impl Limits {
    /// 16,384 px a side and 40 million pixels (160 MB of RGBA): a large camera photo passes,
    /// a decompression bomb does not.
    pub const DEFAULT: Limits = Limits {
        max_side: 16_384,
        max_pixels: 40_000_000,
    };

    /// Limits of the caller's own. A zero in either refuses every picture.
    pub const fn new(max_side: u32, max_pixels: u64) -> Limits {
        Limits {
            max_side,
            max_pixels,
        }
    }

    /// The longest side allowed, in pixels.
    pub const fn max_side(self) -> u32 {
        self.max_side
    }

    /// The most pixels allowed.
    pub const fn max_pixels(self) -> u64 {
        self.max_pixels
    }

    /// Whether a picture of this size is within the limits. A zero side is never within them.
    pub fn allows(self, width: u32, height: u32) -> bool {
        width > 0
            && height > 0
            && width <= self.max_side
            && height <= self.max_side
            && u64::from(width) * u64::from(height) <= self.max_pixels
    }
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::DEFAULT
    }
}

/// Why a picture could not be read.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// A format that is recognised (or not) but not decoded here.
    Unsupported(Format),
    /// The header says a size over the [`Limits`] (or a zero side).
    TooLarge { width: u32, height: u32 },
    /// The bytes are not a whole, readable picture of the format they start as.
    Damaged,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(Format::Unknown) => write!(f, "not a picture this can read"),
            Error::Unsupported(format) => write!(f, "{format:?} pictures are not read here"),
            Error::TooLarge { width, height } => {
                write!(f, "the picture is too large ({width} x {height})")
            }
            Error::Damaged => write!(f, "the picture is damaged"),
        }
    }
}

impl std::error::Error for Error {}

/// A picture in 8-bit RGBA, top-down, `width * 4` bytes a row, straight (not premultiplied)
/// alpha: what `Painter::image` takes.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// A picture from its pixels, or `None` when the bytes are not exactly `width * height * 4`
    /// or a side is zero.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Image> {
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (width > 0 && height > 0 && rgba.len() == len).then_some(Image {
            width,
            height,
            rgba,
        })
    }

    /// The pixel at `(x, y)` as `[r, g, b, a]`, or `None` outside the picture.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ])
    }

    /// How many bytes the pixels take.
    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }

    /// The picture shrunk to fit inside `max_w` x `max_h`, keeping its proportions, with each
    /// new pixel the average of the area of old pixels it covers (in premultiplied alpha). It
    /// never grows: a picture that already fits comes back as a copy. A box with a side of zero
    /// (or one that is not a number) gives `None`. Each side of the result is at least 1 px.
    pub fn scaled_to_fit(&self, max_w: u32, max_h: u32) -> Option<Image> {
        if max_w == 0 || max_h == 0 {
            return None;
        }
        if self.width <= max_w && self.height <= max_h {
            return Some(self.clone());
        }
        let k = (f64::from(max_w) / f64::from(self.width))
            .min(f64::from(max_h) / f64::from(self.height));
        let w = ((f64::from(self.width) * k).round() as u32).clamp(1, max_w);
        let h = ((f64::from(self.height) * k).round() as u32).clamp(1, max_h);
        self.scaled(w, h)
    }

    /// The picture resampled to exactly `w` x `h` by area averaging. Meant for shrinking; a
    /// side that grows is stretched by the same rule (each new pixel covers part of one old
    /// one), which is blocky but correct. `None` for a zero side.
    pub fn scaled(&self, w: u32, h: u32) -> Option<Image> {
        if w == 0 || h == 0 {
            return None;
        }
        let (sw, sh) = (self.width as usize, self.height as usize);
        let (w, h) = (w as usize, h as usize);
        // Premultiplied, in floats: [r*a, g*a, b*a, a] with a in 0..1.
        let pre: Vec<[f32; 4]> = self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                let a = f32::from(c[3]) / 255.0;
                [
                    f32::from(c[0]) * a,
                    f32::from(c[1]) * a,
                    f32::from(c[2]) * a,
                    a,
                ]
            })
            .collect();
        let across = coverage(sw, w);
        let down = coverage(sh, h);
        // Horizontal pass: sh rows of w pixels.
        let mut mid = vec![[0.0f32; 4]; sh * w];
        for y in 0..sh {
            let row = &pre[y * sw..(y + 1) * sw];
            for (x, taps) in across.iter().enumerate() {
                let mut acc = [0.0f32; 4];
                for &(i, wt) in taps {
                    for (a, v) in acc.iter_mut().zip(row[i]) {
                        *a += v * wt;
                    }
                }
                mid[y * w + x] = acc;
            }
        }
        // Vertical pass, then back to straight alpha.
        let mut out = Vec::with_capacity(w * h * 4);
        for taps in &down {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for &(i, wt) in taps {
                    for (a, v) in acc.iter_mut().zip(mid[i * w + x]) {
                        *a += v * wt;
                    }
                }
                let a = acc[3];
                let un = |v: f32| {
                    if a > 0.0 {
                        (v / a).round().clamp(0.0, 255.0) as u8
                    } else {
                        0
                    }
                };
                out.extend_from_slice(&[
                    un(acc[0]),
                    un(acc[1]),
                    un(acc[2]),
                    (a * 255.0).round().clamp(0.0, 255.0) as u8,
                ]);
            }
        }
        Image::new(w as u32, h as u32, out)
    }
}

/// For each of `to` new pixels along one side, the old pixels it covers and how much of each,
/// the weights summing to 1: new pixel `j` covers old coordinates `[j * s, (j + 1) * s)` with
/// `s = from / to`.
fn coverage(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let s = from as f64 / to as f64;
    (0..to)
        .map(|j| {
            let (lo, hi) = (j as f64 * s, (j + 1) as f64 * s);
            let first = lo.floor() as usize;
            let last = (hi.ceil() as usize).min(from).max(first + 1);
            (first..last)
                .filter_map(|i| {
                    let part = (hi.min(i as f64 + 1.0) - lo.max(i as f64)) / s;
                    (part > 0.0).then_some((i, part as f32))
                })
                .collect()
        })
        .collect()
}

/// Reads a whole picture: PNG or JPEG into RGBA, a JPEG turned upright by its EXIF
/// orientation. A picture over `limits` is refused from its header ([`Error::TooLarge`],
/// with the size the header claims); a format not decoded here is [`Error::Unsupported`];
/// anything that fails to decode, panics in the decoder included, is [`Error::Damaged`].
/// Empty input is `Unsupported(Unknown)`.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Image, Error> {
    match sniff(bytes) {
        Format::Png => guarded(|| decode_png(bytes, limits)),
        Format::Jpeg => guarded(|| decode_jpeg(bytes, limits)),
        other => Err(Error::Unsupported(other)),
    }
}

/// The size a picture will have once [`decode`]d (for a JPEG, after its EXIF turn), read from
/// its headers alone. `None` for a format not decoded here or headers that cannot be read.
/// Limits are not applied: a layout may want to know that a picture is huge.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match sniff(bytes) {
        Format::Png => {
            let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
            let reader = decoder.read_info().ok()?;
            let info = reader.info();
            Some((info.width, info.height))
        }
        Format::Jpeg => guarded(|| {
            let mut d = jpeg_decoder(bytes, Limits::new(u32::from(u16::MAX), u64::MAX));
            d.decode_headers().map_err(|_| Error::Damaged)?;
            let info = d.info().ok_or(Error::Damaged)?;
            let (w, h) = (u32::from(info.width), u32::from(info.height));
            let turned = d.exif().map(|e| exif_orientation(e)).unwrap_or(1) >= 5;
            Ok(if turned { (h, w) } else { (w, h) })
        })
        .ok(),
        _ => None,
    }
}

/// Runs a decoder with any panic in it turned into [`Error::Damaged`].
fn guarded<T>(f: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(Err(Error::Damaged))
}

fn decode_png(bytes: &[u8], limits: Limits) -> Result<Image, Error> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| Error::Damaged)?;
    let (width, height) = (reader.info().width, reader.info().height);
    if !limits.allows(width, height) {
        return Err(Error::TooLarge { width, height });
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or(Error::Damaged)?];
    let frame = reader.next_frame(&mut buf).map_err(|_| Error::Damaged)?;
    buf.truncate(frame.buffer_size());
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(Error::Damaged);
    }
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Indexed => return Err(Error::Damaged),
    };
    Image::new(frame.width, frame.height, rgba).ok_or(Error::Damaged)
}

fn jpeg_decoder(
    bytes: &[u8],
    limits: Limits,
) -> zune_jpeg::JpegDecoder<zune_jpeg::zune_core::bytestream::ZCursor<&[u8]>> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let side = limits.max_side.min(u32::from(u16::MAX)) as usize;
    let options = DecoderOptions::default()
        .set_max_width(side)
        .set_max_height(side)
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options)
}

fn decode_jpeg(bytes: &[u8], limits: Limits) -> Result<Image, Error> {
    // The headers first, with no limit on the side, so a refusal can say the size.
    let mut probe = jpeg_decoder(bytes, Limits::new(u32::from(u16::MAX), u64::MAX));
    probe.decode_headers().map_err(|_| Error::Damaged)?;
    let info = probe.info().ok_or(Error::Damaged)?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if !limits.allows(width, height) {
        return Err(Error::TooLarge { width, height });
    }
    let orientation = probe.exif().map(|e| exif_orientation(e)).unwrap_or(1);
    let mut decoder = jpeg_decoder(bytes, limits);
    let rgba = decoder.decode().map_err(|_| Error::Damaged)?;
    let image = Image::new(width, height, rgba).ok_or(Error::Damaged)?;
    Ok(orient(image, orientation))
}

/// The EXIF orientation (1 to 8) in a block of EXIF data, as JPEG carries it in its APP1
/// segment: the TIFF header (`II*\0` or `MM\0*`), with or without the `Exif\0\0` before it.
/// 1 (upright) when there is none, when the block is damaged, or when the value is outside 1
/// to 8. Only the first directory is read; nothing outside the block is ever touched.
pub fn exif_orientation(exif: &[u8]) -> u8 {
    let tiff = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
    let little = match tiff.get(0..4) {
        Some(b"II*\0") => true,
        Some(b"MM\0*") => false,
        _ => return 1,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b = tiff.get(at..at.checked_add(2)?)?;
        Some(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b = tiff.get(at..at.checked_add(4)?)?;
        Some(if little {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    let read = || -> Option<u8> {
        let ifd = u32_at(4)? as usize;
        let count = u16_at(ifd)? as usize;
        for k in 0..count {
            let entry = ifd.checked_add(2)?.checked_add(k.checked_mul(12)?)?;
            if u16_at(entry)? == 0x0112 {
                // SHORT (type 3), one value, kept in the first two bytes of the value field.
                if u16_at(entry + 2)? != 3 {
                    return None;
                }
                let v = u16_at(entry + 8)?;
                return (1..=8).contains(&v).then_some(v as u8);
            }
        }
        None
    };
    read().unwrap_or(1)
}

/// A picture turned and mirrored as EXIF orientation `o` says it must be shown: 2 mirrored
/// left to right, 3 turned a half, 4 mirrored top to bottom, 5 mirrored across the main
/// diagonal, 6 turned a quarter clockwise, 7 mirrored across the other diagonal, 8 turned a
/// quarter anticlockwise. From 5 on, width and height swap. 1 and any other value give the
/// picture as it is.
pub fn orient(image: Image, o: u8) -> Image {
    if !(2..=8).contains(&o) {
        return image;
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let (ow, oh) = if o >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; image.rgba.len()];
    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let (si, di) = ((sy * w + sx) * 4, (y * ow + x) * 4);
            out[di..di + 4].copy_from_slice(&image.rgba[si..si + 4]);
        }
    }
    Image {
        width: ow as u32,
        height: oh as u32,
        rgba: out,
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
