//! Checks the dimensions of each WebP bitstream before handing it to the decoder. In
//! image-webp 0.2.4 the VP8 planes are allocated before the inner and outer sizes are compared.
//! Only RIFF framing and fixed-size headers are read here; the decoder still owns all pixels.

use std::io::Cursor;

pub(crate) fn decoder<B: AsRef<[u8]>>(
    bytes: B,
    max_pixels: u64,
) -> Option<image_webp::WebPDecoder<Cursor<B>>> {
    validate(bytes.as_ref(), max_pixels)?;
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
    decoder.set_memory_limit(max_pixels as usize * 8);
    Some(decoder)
}

fn validate(bytes: &[u8], max_pixels: u64) -> Option<()> {
    if bytes.len() as u64 > super::MAX_BYTES {
        return None;
    }
    let (riff, body, trailing) = chunk(bytes)?;
    if riff != b"RIFF" || !trailing.is_empty() {
        return None;
    }
    let mut chunks = body.strip_prefix(b"WEBP")?;
    let (tag, header, rest) = chunk(chunks)?;
    let fits = |(w, h): (u32, u32)| w > 0 && h > 0 && u64::from(w) * u64::from(h) <= max_pixels;
    if tag != b"VP8X" {
        return fits(dimensions(tag, header)?).then_some(());
    }
    // VP8X is the sole source of canvas dimensions in an extended file.
    if header.len() != 10 {
        return None;
    }
    let canvas = (le24(&header[4..])? + 1, le24(&header[7..])? + 1);
    if !fits(canvas) {
        return None;
    }
    let animated = header[0] & 2 != 0;
    chunks = rest;
    let mut images = 0;
    while !chunks.is_empty() {
        let (tag, data, rest) = chunk(chunks)?;
        chunks = rest;
        match tag {
            b"VP8 " | b"VP8L" => {
                if animated || dimensions(tag, data)? != canvas {
                    return None;
                }
                images += 1;
            }
            b"ANMF" => {
                // The decoder steps from one frame to the next without consuming padding.
                // A valid frame is even, since its header and all its subchunks are padded.
                if !animated || !data.len().is_multiple_of(2) {
                    return None;
                }
                let x = 2 * le24(data)?;
                let y = 2 * le24(data.get(3..)?)?;
                let w = le24(data.get(6..)?)? + 1;
                let h = le24(data.get(9..)?)? + 1;
                if x + w > canvas.0 || y + h > canvas.1 {
                    return None;
                }
                let (tag, frame, rest) = chunk(data.get(16..)?)?;
                let (tag, frame) = if tag == b"ALPH" {
                    let (tag, frame, _) = chunk(rest)?;
                    if tag != b"VP8 " {
                        return None;
                    }
                    (tag, frame)
                } else {
                    (tag, frame)
                };
                if dimensions(tag, frame)? != (w, h) {
                    return None;
                }
                images += 1;
            }
            _ => {}
        }
    }
    (images != 0).then_some(())
}

type Chunk<'a> = (&'a [u8; 4], &'a [u8], &'a [u8]);

fn chunk(bytes: &[u8]) -> Option<Chunk<'_>> {
    let (header, data) = bytes.split_first_chunk::<8>()?;
    let (tag, len) = header.as_chunks::<4>().0.split_at(1);
    let len = u32::from_le_bytes(len[0]) as usize;
    Some((
        &tag[0],
        data.get(..len)?,
        data.get(len.checked_add(len & 1)?..)?,
    ))
}

fn le24(bytes: &[u8]) -> Option<u32> {
    let [a, b, c] = *bytes.first_chunk::<3>()?;
    Some(u32::from_le_bytes([a, b, c, 0]))
}

fn dimensions(tag: &[u8; 4], data: &[u8]) -> Option<(u32, u32)> {
    match tag {
        b"VP8 " => {
            let &[flags, _, _, 0x9d, 0x01, 0x2a, a, b, c, d] = data.first_chunk::<10>()? else {
                return None;
            };
            (flags & 1 == 0).then(|| {
                (
                    u32::from(u16::from_le_bytes([a, b]) & 0x3fff),
                    u32::from(u16::from_le_bytes([c, d]) & 0x3fff),
                )
            })
        }
        b"VP8L" => {
            let &[0x2f, a, b, c, d] = data.first_chunk::<5>()? else {
                return None;
            };
            let packed = u32::from_le_bytes([a, b, c, d]);
            Some(((packed & 0x3fff) + 1, ((packed >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}

#[cfg(test)]
#[test]
fn mismatched_webp_dimensions_are_rejected_before_decoder_construction() {
    const LOSSY: &[u8] = include_bytes!("../../../../fixtures/md/assets/lossy.webp");
    fn pack(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }
    for animated in [false, true] {
        for alpha in [false, true] {
            if alpha && !animated {
                continue;
            }
            let mut body = b"WEBP".to_vec();
            let flags = (u8::from(animated) * 2) | (u8::from(alpha) * 16);
            body.extend(pack(b"VP8X", &[flags, 0, 0, 0, 31, 0, 0, 31, 0, 0]));
            let width_offset = if animated {
                body.extend(pack(b"ANIM", &[0; 6]));
                let mut frame = vec![0; 16];
                frame[6] = 31;
                frame[9] = 31;
                if alpha {
                    frame.extend(pack(b"ALPH", &[0; 1 + 32 * 32]));
                }
                frame.extend(&LOSSY[12..]);
                let frame = pack(b"ANMF", &frame);
                body.extend_from_slice(&frame);
                let width_offset = 8 + body.len() + 8 + 6;
                body.extend_from_slice(&frame);
                width_offset
            } else {
                body.extend(&LOSSY[12..]);
                24
            };
            let valid = pack(b"RIFF", &body);
            let mut checked = decoder(valid.as_slice(), 1024).expect("matching headers");
            let mut pixels = vec![0; checked.output_buffer_size().unwrap()];
            checked
                .read_image(&mut pixels)
                .expect("matching headers decode");
            // The plane is still just 32×32; this is a small inconsistency, not a large decode.
            // Change the canvas width for a still, or the second frame's width for an animation.
            let mut wrong = valid;
            wrong[width_offset] = 30;
            assert!(decoder(wrong.as_slice(), 1024).is_none());
        }
    }
}
