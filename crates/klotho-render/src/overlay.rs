//! CPU HUD overlay. PR 15 owns attention policy; this only stamps widgets onto pixels.

use klotho_manifest::{UiManifest, WidgetKind};

/// Draw HUD widgets into an RGBA8 buffer (`width * height * 4`).
pub fn overlay_hud(rgba: &mut [u8], width: u32, height: u32, ui: &UiManifest) {
    if width == 0 || height == 0 {
        return;
    }
    let bar_h = 36u32.min(height);
    let y0 = height.saturating_sub(bar_h);
    fill_rect(rgba, width, height, 0, y0, width, bar_h, [12, 14, 18, 210]);
    let mut x = 8u32;
    let y = y0 + 12;
    for w in &ui.widgets {
        match w.kind {
            WidgetKind::Bar { value, cap } => {
                let bw = 88u32;
                fill_rect(rgba, width, height, x, y, bw, 12, [40, 40, 48, 255]);
                let fill = if cap <= 0 {
                    0
                } else {
                    ((value.clamp(0, cap) as u32) * bw) / cap as u32
                };
                fill_rect(rgba, width, height, x, y, fill, 12, [180, 70, 50, 255]);
                blit_text(rgba, width, height, x, y.saturating_sub(10), &w.body);
                x = x.saturating_add(bw + 12);
            }
            _ => {
                blit_text(rgba, width, height, x, y, &w.body);
                x = x.saturating_add(6 * (w.body.len() as u32 + 1));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(rgba: &mut [u8], width: u32, height: u32, x: u32, y: u32, w: u32, h: u32, c: [u8; 4]) {
    let x1 = x.saturating_add(w).min(width);
    let y1 = y.saturating_add(h).min(height);
    for yy in y..y1 {
        for xx in x..x1 {
            let i = ((yy * width + xx) * 4) as usize;
            if i + 3 >= rgba.len() {
                return;
            }
            let a = u16::from(c[3]);
            if a == 255 {
                rgba[i] = c[0];
                rgba[i + 1] = c[1];
                rgba[i + 2] = c[2];
                rgba[i + 3] = 255;
            } else {
                let ia = 255 - a;
                rgba[i] = ((u16::from(rgba[i]) * ia + u16::from(c[0]) * a) / 255) as u8;
                rgba[i + 1] = ((u16::from(rgba[i + 1]) * ia + u16::from(c[1]) * a) / 255) as u8;
                rgba[i + 2] = ((u16::from(rgba[i + 2]) * ia + u16::from(c[2]) * a) / 255) as u8;
                rgba[i + 3] = 255;
            }
        }
    }
}

fn blit_text(rgba: &mut [u8], width: u32, height: u32, mut x: u32, y: u32, s: &str) {
    for ch in s.chars() {
        blit_glyph(rgba, width, height, x, y, ch);
        x = x.saturating_add(6);
        if x + 5 >= width {
            break;
        }
    }
}

fn blit_glyph(rgba: &mut [u8], width: u32, height: u32, x: u32, y: u32, ch: char) {
    let rows = glyph(ch);
    for (row, bits) in rows.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) == 0 {
                continue;
            }
            let xx = x + col;
            let yy = y + row as u32;
            if xx >= width || yy >= height {
                continue;
            }
            let i = ((yy * width + xx) * 4) as usize;
            if i + 3 < rgba.len() {
                rgba[i] = 230;
                rgba[i + 1] = 230;
                rgba[i + 2] = 220;
                rgba[i + 3] = 255;
            }
        }
    }
}

fn glyph(ch: char) -> [u8; 7] {
    match ch {
        ' ' => [0; 7],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'a' | 'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'b' | 'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'c' | 'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'd' | 'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'e' | 'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'f' | 'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'g' | 'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'h' | 'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'i' | 'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'l' | 'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'm' | 'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'n' | 'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'o' | 'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'p' | 'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'r' | 'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        's' | 'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        't' | 'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'u' | 'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'w' | 'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'y' | 'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        ':' => [0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}

/// 24-bit BMP (BGR, bottom-up). Preview-openable; no extra crate.
pub fn write_bmp(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> std::io::Result<()> {
    let row = (width * 3).div_ceil(4) * 4;
    let pixels = row * height;
    let file_size = 54 + pixels;
    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&pixels.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut row_bytes = vec![0u8; row as usize];
    for y in 0..height {
        let src_y = height - 1 - y;
        for x in 0..width {
            let i = ((src_y * width + x) * 4) as usize;
            let o = (x * 3) as usize;
            if i + 2 < rgba.len() {
                row_bytes[o] = rgba[i + 2];
                row_bytes[o + 1] = rgba[i + 1];
                row_bytes[o + 2] = rgba[i];
            }
        }
        out.extend_from_slice(&row_bytes);
    }
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use klotho_core::Epoch;
    use klotho_manifest::{UiManifest, Widget, WidgetKind};

    use super::*;

    #[test]
    fn overlay_writes_non_clear_pixels() {
        let mut px = vec![0u8; 64 * 64 * 4];
        let ui = UiManifest::from_widgets(
            Epoch::ZERO,
            [Widget {
                kind: WidgetKind::Text,
                body: "owed 50 copper".into(),
            }],
        );
        overlay_hud(&mut px, 64, 64, &ui);
        assert!(px.iter().any(|&b| b > 20));
    }
}
