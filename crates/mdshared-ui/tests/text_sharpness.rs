//! A headless specimen of small interface text on both toolkit palettes.
//! The test writes to `target/specimen` unless the caller supplies `TEXT_SHARPNESS_SHOTS`.
//! It also records the measured widths as float bits.

use mdshared_text::Bitmap;
use mdshared_ui::gfx::{Face, Gfx, Painter, Rect};
use mdshared_ui::theme::{DARK, LIGHT};
use std::path::PathBuf;

#[test]
#[ignore]
fn small_ui_text_sheet() {
    let out = std::env::var_os("TEXT_SHARPNESS_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen"));
    std::fs::create_dir_all(&out).unwrap();
    for (scale, suffix) in [(1.0, ""), (1.25, "-125"), (1.5, "-150")] {
        let mut bitmap = Bitmap::new((640.0 * scale) as u32, (204.0 * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bitmap, &mut gfx, scale);
        let mut widths = Vec::new();
        for (column, pal) in [LIGHT, DARK].iter().enumerate() {
            let x = column as f32 * 320.0;
            p.fill(Rect::new(x, 0.0, 320.0, 204.0), pal.page);
            for (row, size) in [12.0, 13.0, 14.0].into_iter().enumerate() {
                let y = 16.0 + row as f32 * 66.0;
                for (i, (face, label)) in [
                    (
                        Face::Ui,
                        format!("{size} px   Settings · Wi-Fi & Bluetooth"),
                    ),
                    (Face::Semibold, "Save changes   Aa HExg 0123456789".into()),
                    (Face::Ui, "Refresh   café · naïve · file explorer".into()),
                ]
                .into_iter()
                .enumerate()
                {
                    let width = p.measure(face, size, &label);
                    let drawn = p.text(
                        face,
                        size,
                        x + 12.0,
                        y + i as f32 * 18.0,
                        &label,
                        if i == 2 { pal.muted } else { pal.text },
                        f32::INFINITY,
                    );
                    assert_eq!(width.to_bits(), drawn.to_bits());
                    widths.extend(width.to_le_bytes());
                }
            }
        }
        bitmap
            .write_png(&out.join(format!("ui{suffix}.png")))
            .unwrap();
        std::fs::write(out.join(format!("ui{suffix}.widths")), widths).unwrap();
    }
}
