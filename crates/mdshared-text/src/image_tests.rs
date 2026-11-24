//! Tests of `image`. The JPEG pictures under `testdata/` are made by `testdata/gen.py`.

use super::*;

const HALVES: &[u8] = include_bytes!("../testdata/halves.jpg");
const HALVES_PROGRESSIVE: &[u8] = include_bytes!("../testdata/halves-progressive.jpg");
const HALVES_420: &[u8] = include_bytes!("../testdata/halves-420.jpg");
const HALVES_ORIENT6: &[u8] = include_bytes!("../testdata/halves-orient6.jpg");
const GRAY: &[u8] = include_bytes!("../testdata/gray.jpg");

const RED: [u8; 3] = [220, 30, 30];
const BLUE: [u8; 3] = [30, 30, 220];

fn near(px: [u8; 4], rgb: [u8; 3], tolerance: u8) -> bool {
    px[3] == 255 && (0..3).all(|i| px[i].abs_diff(rgb[i]) <= tolerance)
}

/// A PNG of the given colour type, written with the png crate.
fn png_bytes(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(data).unwrap();
    }
    out
}

/// A small fixed-seed generator for damaged input.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// A picture whose every pixel is numbered, so a turn can be followed.
fn numbered(w: u32, h: u32) -> Image {
    let rgba = (0..w * h)
        .flat_map(|i| [i as u8, (i >> 8) as u8, 7, 255])
        .collect();
    Image::new(w, h, rgba).unwrap()
}

#[test]
fn each_signature_is_known_by_its_first_bytes() {
    assert_eq!(sniff(HALVES), Format::Jpeg);
    assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Format::Png);
    assert_eq!(sniff(b"GIF89a...."), Format::Gif);
    assert_eq!(sniff(b"GIF87a...."), Format::Gif);
    assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Format::WebP);
    assert_eq!(sniff(b"BM\0\0\0\0\0\0\0\0\0\0\0\0"), Format::Bmp);
    assert_eq!(sniff(b""), Format::Unknown);
    assert_eq!(sniff(b"\xFF\xD8"), Format::Unknown, "too short for a JPEG");
    assert_eq!(sniff(b"RIFF\0\0\0\0WAVE"), Format::Unknown);
    assert_eq!(sniff(b"hello"), Format::Unknown);
}

#[test]
fn the_limits_refuse_a_zero_side_a_long_side_and_too_many_pixels() {
    let l = Limits::new(100, 5_000);
    assert!(l.allows(100, 50));
    assert!(!l.allows(101, 1));
    assert!(!l.allows(1, 101));
    assert!(!l.allows(100, 51), "5,100 pixels");
    assert!(!l.allows(0, 10) && !l.allows(10, 0));
    assert!(!Limits::new(0, 0).allows(1, 1));
    assert!(Limits::DEFAULT.allows(6000, 4000));
    assert!(!Limits::DEFAULT.allows(16_384, 16_384));
    assert_eq!(Limits::default(), Limits::DEFAULT);
    assert_eq!((l.max_side(), l.max_pixels()), (100, 5_000));
}

#[test]
fn a_jpeg_decodes_to_its_size_and_colours() {
    for (name, bytes) in [
        ("baseline", HALVES),
        ("progressive", HALVES_PROGRESSIVE),
        ("4:2:0", HALVES_420),
    ] {
        let image = decode(bytes, Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((image.width, image.height), (16, 8), "{name}");
        assert_eq!(image.byte_len(), 16 * 8 * 4);
        // Away from the edge between the halves, where subsampled colour bleeds.
        for y in 0..8 {
            for x in [0, 3, 5] {
                assert!(near(image.pixel(x, y).unwrap(), RED, 12), "{name} {x},{y}");
                assert!(near(image.pixel(15 - x, y).unwrap(), BLUE, 12), "{name}");
            }
        }
        assert_eq!(dimensions(bytes), Some((16, 8)), "{name}");
    }
}

#[test]
fn a_grey_jpeg_comes_out_as_grey_rgba() {
    let image = decode(GRAY, Limits::DEFAULT).unwrap();
    assert_eq!((image.width, image.height), (8, 8));
    for px in image.rgba.as_chunks::<4>().0.iter() {
        assert!(
            near([px[0], px[1], px[2], px[3]], [128, 128, 128], 3),
            "{px:?}"
        );
    }
}

#[test]
fn a_jpeg_is_turned_upright_by_its_exif_orientation() {
    let image = decode(HALVES_ORIENT6, Limits::DEFAULT).unwrap();
    assert_eq!(
        (image.width, image.height),
        (16, 8),
        "stored 8 x 16, shown 16 x 8"
    );
    assert!(near(image.pixel(1, 4).unwrap(), RED, 12));
    assert!(near(image.pixel(14, 4).unwrap(), BLUE, 12));
    assert_eq!(dimensions(HALVES_ORIENT6), Some((16, 8)));
}

#[test]
fn a_picture_over_the_limits_is_refused_with_the_size_its_header_claims() {
    let side = decode(HALVES, Limits::new(10, u64::MAX));
    assert_eq!(
        side,
        Err(Error::TooLarge {
            width: 16,
            height: 8
        })
    );
    let count = decode(HALVES, Limits::new(1000, 100));
    assert_eq!(
        count,
        Err(Error::TooLarge {
            width: 16,
            height: 8
        })
    );
    let png = png_bytes(4, 3, png::ColorType::Rgb, &[9; 36]);
    assert_eq!(
        decode(&png, Limits::new(3, 100)),
        Err(Error::TooLarge {
            width: 4,
            height: 3
        })
    );
    assert!(decode(&png, Limits::new(4, 12)).is_ok());
}

#[test]
fn damaged_and_unknown_bytes_give_an_error_and_never_a_panic() {
    assert_eq!(
        decode(b"", Limits::DEFAULT),
        Err(Error::Unsupported(Format::Unknown))
    );
    assert_eq!(
        decode(b"GIF89a....", Limits::DEFAULT),
        Err(Error::Unsupported(Format::Gif))
    );
    assert_eq!(decode(&HALVES[..60], Limits::DEFAULT), Err(Error::Damaged));
    let png = png_bytes(4, 3, png::ColorType::Rgb, &[9; 36]);
    assert_eq!(
        decode(&png[..png.len() - 20], Limits::DEFAULT),
        Err(Error::Damaged)
    );
    assert_eq!(dimensions(b"nothing"), None);
    assert_eq!(dimensions(&HALVES[..4]), None);
    // A JPEG or PNG start followed by noise, and real pictures with bytes overwritten.
    let mut lcg = Lcg(7);
    for round in 0..300 {
        let mut bytes = if round % 3 == 0 {
            let mut b = vec![0xFF, 0xD8, 0xFF];
            b.extend((0..lcg.next() % 400).map(|_| lcg.next() as u8));
            b
        } else if round % 3 == 1 {
            HALVES.to_vec()
        } else {
            png.clone()
        };
        if round % 3 != 0 {
            for _ in 0..1 + lcg.next() % 6 {
                let at = (lcg.next() as usize) % bytes.len();
                bytes[at] = lcg.next() as u8;
            }
        }
        // Either a picture within the limits or an error: never a panic.
        if let Ok(image) = decode(&bytes, Limits::new(64, 4096)) {
            assert!(image.width <= 64 && image.height <= 64);
            assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
        }
        let _ = dimensions(&bytes);
    }
}

#[test]
fn a_png_decodes_with_its_transparency_in_each_colour_type() {
    let rgba: Vec<u8> = (0..6u8)
        .flat_map(|i| [i * 40, 255 - i * 40, 9, i * 50])
        .collect();
    let image = decode(
        &png_bytes(3, 2, png::ColorType::Rgba, &rgba),
        Limits::DEFAULT,
    )
    .unwrap();
    assert_eq!((image.width, image.height, &image.rgba), (3, 2, &rgba));
    let rgb: Vec<u8> = (0..6u8).flat_map(|i| [i, i + 1, i + 2]).collect();
    let image = decode(&png_bytes(3, 2, png::ColorType::Rgb, &rgb), Limits::DEFAULT).unwrap();
    assert_eq!(image.pixel(2, 1), Some([5, 6, 7, 255]));
    let ga = [10, 200, 20, 100];
    let image = decode(
        &png_bytes(2, 1, png::ColorType::GrayscaleAlpha, &ga),
        Limits::DEFAULT,
    )
    .unwrap();
    assert_eq!(image.rgba, [10, 10, 10, 200, 20, 20, 20, 100]);
    let g = [77];
    let image = decode(
        &png_bytes(1, 1, png::ColorType::Grayscale, &g),
        Limits::DEFAULT,
    )
    .unwrap();
    assert_eq!(image.rgba, [77, 77, 77, 255]);
    assert_eq!(
        dimensions(&png_bytes(3, 2, png::ColorType::Rgba, &rgba)),
        Some((3, 2))
    );
}

/// TIFF data with one directory holding one entry.
fn tiff(little: bool, tag: u16, kind: u16, value: u16) -> Vec<u8> {
    let w16 = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let w32 = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut b = Vec::new();
    b.extend_from_slice(if little { b"II*\0" } else { b"MM\0*" });
    b.extend_from_slice(&w32(8));
    b.extend_from_slice(&w16(2));
    // An entry of another tag first, then the one asked for.
    for (t, k, v) in [(0x010F, 2, 0), (tag, kind, value)] {
        b.extend_from_slice(&w16(t));
        b.extend_from_slice(&w16(k));
        b.extend_from_slice(&w32(1));
        b.extend_from_slice(&w16(v));
        b.extend_from_slice(&[0, 0]);
    }
    b.extend_from_slice(&w32(0));
    b
}

#[test]
fn the_exif_orientation_is_read_in_either_byte_order_and_defaults_to_upright() {
    for little in [true, false] {
        for o in 1..=8 {
            assert_eq!(exif_orientation(&tiff(little, 0x0112, 3, o)), o as u8);
        }
        let mut with_prefix = b"Exif\0\0".to_vec();
        with_prefix.extend(tiff(little, 0x0112, 3, 6));
        assert_eq!(exif_orientation(&with_prefix), 6);
        assert_eq!(
            exif_orientation(&tiff(little, 0x0112, 3, 9)),
            1,
            "out of range"
        );
        assert_eq!(exif_orientation(&tiff(little, 0x0112, 3, 0)), 1);
        assert_eq!(
            exif_orientation(&tiff(little, 0x0112, 4, 6)),
            1,
            "not a SHORT"
        );
        assert_eq!(
            exif_orientation(&tiff(little, 0x0110, 3, 6)),
            1,
            "no orientation"
        );
        // Cut anywhere: never read past the end.
        let whole = tiff(little, 0x0112, 3, 6);
        for cut in 0..whole.len() {
            let o = exif_orientation(&whole[..cut]);
            assert!(o == 1 || o == 6);
        }
    }
    assert_eq!(exif_orientation(b""), 1);
    assert_eq!(exif_orientation(b"XX*\0\0\0\0\x08"), 1);
    // A directory that claims more entries than there are bytes, or lies past the end.
    let mut lying = tiff(true, 0x0112, 3, 6);
    lying[8] = 0xFF;
    lying[9] = 0xFF;
    assert_eq!(exif_orientation(&lying), 6, "found before running out");
    let mut far = tiff(true, 0x0112, 3, 6);
    far[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(exif_orientation(&far), 1);
}

/// The second way of turning: a quarter turn clockwise and two mirrors, composed.
fn turn_cw(im: &Image) -> Image {
    let (w, h) = (im.width, im.height);
    let mut out = vec![0; im.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            // (x, y) goes to (h - 1 - y, x) in a picture h wide.
            let d = ((x * h + (h - 1 - y)) * 4) as usize;
            out[d..d + 4].copy_from_slice(&im.pixel(x, y).unwrap());
        }
    }
    Image::new(h, w, out).unwrap()
}

fn mirror(im: &Image, across: bool) -> Image {
    let (w, h) = (im.width, im.height);
    let mut out = vec![0; im.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (tx, ty) = if across {
                (w - 1 - x, y)
            } else {
                (x, h - 1 - y)
            };
            let d = ((ty * w + tx) * 4) as usize;
            out[d..d + 4].copy_from_slice(&im.pixel(x, y).unwrap());
        }
    }
    Image::new(w, h, out).unwrap()
}

#[test]
fn each_orientation_is_the_turn_and_mirror_it_names() {
    let im = numbered(3, 2);
    let cw = |i: &Image| turn_cw(i);
    let half = |i: &Image| cw(&cw(i));
    let expected = [
        (1, im.clone()),
        (2, mirror(&im, true)),
        (3, half(&im)),
        (4, mirror(&im, false)),
        (5, cw(&mirror(&im, false))),
        (6, cw(&im)),
        (7, cw(&mirror(&im, true))),
        (8, cw(&half(&im))),
    ];
    for (o, want) in expected {
        assert_eq!(orient(im.clone(), o), want, "orientation {o}");
    }
    assert_eq!(orient(im.clone(), 0), im);
    assert_eq!(orient(im.clone(), 9), im);
}

#[test]
fn a_picture_is_made_only_from_pixels_that_fit_its_size() {
    assert!(Image::new(2, 2, vec![0; 16]).is_some());
    assert!(Image::new(2, 2, vec![0; 15]).is_none());
    assert!(Image::new(0, 2, vec![]).is_none());
    assert!(Image::new(u32::MAX, u32::MAX, vec![]).is_none());
    let im = numbered(3, 2);
    assert_eq!(im.pixel(2, 1), Some([5, 0, 7, 255]));
    assert_eq!(im.pixel(3, 0), None);
    assert_eq!(im.pixel(0, 2), None);
}

/// The second way of shrinking: every new pixel straight from the area of every old one
/// under it, in two dimensions at once.
fn area_average(im: &Image, w: u32, h: u32) -> Vec<f64> {
    let (sx, sy) = (
        f64::from(im.width) / f64::from(w),
        f64::from(im.height) / f64::from(h),
    );
    let mut out = Vec::new();
    for j in 0..h {
        for i in 0..w {
            let (x0, x1) = (f64::from(i) * sx, f64::from(i + 1) * sx);
            let (y0, y1) = (f64::from(j) * sy, f64::from(j + 1) * sy);
            let mut acc = [0.0f64; 4];
            for y in 0..im.height {
                for x in 0..im.width {
                    let ox = (x1.min(f64::from(x + 1)) - x0.max(f64::from(x))).max(0.0);
                    let oy = (y1.min(f64::from(y + 1)) - y0.max(f64::from(y))).max(0.0);
                    let area = ox * oy / (sx * sy);
                    let p = im.pixel(x, y).unwrap();
                    let a = f64::from(p[3]) / 255.0;
                    for (sum, &v) in acc.iter_mut().zip(&p[..3]) {
                        *sum += f64::from(v) * a * area;
                    }
                    acc[3] += a * area;
                }
            }
            let a = acc[3];
            out.extend(acc[..3].iter().map(|&v| if a > 0.0 { v / a } else { 0.0 }));
            out.push(a * 255.0);
        }
    }
    out
}

#[test]
fn a_shrunk_picture_is_the_area_average_of_the_old_one() {
    let mut lcg = Lcg(11);
    for (sw, sh, w, h) in [
        (8, 8, 2, 2),
        (5, 3, 2, 2),
        (7, 5, 3, 2),
        (16, 9, 5, 4),
        (9, 1, 4, 1),
    ] {
        let rgba: Vec<u8> = (0..sw * sh * 4)
            .map(|k| {
                if k % 4 == 3 {
                    128 + (lcg.next() % 128) as u8
                } else {
                    lcg.next() as u8
                }
            })
            .collect();
        let im = Image::new(sw, sh, rgba).unwrap();
        let got = im.scaled(w, h).unwrap();
        let want = area_average(&im, w, h);
        assert_eq!((got.width, got.height), (w, h));
        for (k, (&g, &e)) in got.rgba.iter().zip(&want).enumerate() {
            assert!(
                (f64::from(g) - e).abs() <= 1.0,
                "{sw}x{sh} -> {w}x{h} at {k}: {g} vs {e}"
            );
        }
    }
}

#[test]
fn a_transparent_pixel_does_not_darken_its_neighbour_when_shrunk() {
    let im = Image::new(2, 1, vec![220, 30, 30, 255, 0, 255, 0, 0]).unwrap();
    let one = im.scaled(1, 1).unwrap();
    assert_eq!(one.rgba, [220, 30, 30, 128]);
    let clear = Image::new(2, 1, vec![9; 8].into_iter().map(|_| 0).collect()).unwrap();
    assert_eq!(clear.scaled(1, 1).unwrap().rgba, [0, 0, 0, 0]);
}

#[test]
fn scaled_to_fit_keeps_the_proportions_and_never_grows() {
    let im = numbered(40, 20);
    let small = im.scaled_to_fit(10, 10).unwrap();
    assert_eq!((small.width, small.height), (10, 5));
    let tall = numbered(20, 40).scaled_to_fit(10, 100).unwrap();
    assert_eq!((tall.width, tall.height), (10, 20));
    assert_eq!(im.scaled_to_fit(100, 100).unwrap(), im, "fits: a copy");
    assert_eq!(im.scaled_to_fit(40, 20).unwrap(), im);
    let thin = numbered(1000, 2).scaled_to_fit(10, 10).unwrap();
    assert_eq!((thin.width, thin.height), (10, 1), "at least a pixel");
    assert!(im.scaled_to_fit(0, 10).is_none());
    assert!(im.scaled_to_fit(10, 0).is_none());
    assert!(im.scaled(0, 1).is_none());
    // Grown on one side by `scaled`: blocky, the right size.
    let wide = numbered(2, 2).scaled(4, 2).unwrap();
    assert_eq!(wide.pixel(0, 0), wide.pixel(1, 0));
}

#[test]
fn every_error_says_what_went_wrong() {
    for e in [
        Error::Unsupported(Format::Unknown),
        Error::Unsupported(Format::Gif),
        Error::TooLarge {
            width: 9,
            height: 9,
        },
        Error::Damaged,
    ] {
        assert!(!e.to_string().is_empty());
    }
    assert!(
        Error::Unsupported(Format::WebP)
            .to_string()
            .contains("WebP")
    );
}
