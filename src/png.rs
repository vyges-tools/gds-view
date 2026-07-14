//! Rasterize a flattened GDS cell to a **PNG** — a bounded-size bitmap regardless
//! of polygon count, unlike the SVG path which emits one vector per shape (a real
//! block can vectorize to hundreds of MB; the raster stays a fixed thumbnail).
//!
//! std-only, matching the house style: a scanline polygon filler plus a minimal
//! PNG encoder (8-bit RGB, uncompressed DEFLATE — CRC32 + Adler32 by hand).

use crate::gds::{Cell, Element};

/// Same 14-colour palette as the SVG renderer, as RGB.
const PALETTE: &[(u8, u8, u8)] = &[
    (78, 121, 167), (242, 142, 43), (89, 161, 79), (225, 87, 89), (176, 122, 161),
    (118, 183, 178), (237, 201, 72), (255, 157, 167), (156, 117, 95), (186, 176, 172),
    (27, 158, 119), (217, 95, 2), (117, 112, 179), (231, 41, 138),
];

fn color(layer: i16) -> (u8, u8, u8) {
    PALETTE[(layer as usize).wrapping_rem(PALETTE.len())]
}

fn keep(layer: i16, layers: Option<&[i16]>) -> bool {
    layers.map(|ls| ls.contains(&layer)).unwrap_or(true)
}

/// Render `cell` to a PNG byte stream, fitted into a `max_dim`-pixel square
/// (aspect preserved, Y flipped so up is up), translucent so overlaps show.
pub fn render_png(cell: &Cell, layers: Option<&[i16]>, max_dim: u32) -> Vec<u8> {
    let mut pts: Vec<(i32, i32)> = Vec::new();
    for e in &cell.elements {
        match e {
            Element::Boundary { layer, pts: p, .. }
            | Element::Path { layer, pts: p, .. }
            | Element::Box { layer, pts: p, .. }
                if keep(*layer, layers) =>
            {
                pts.extend_from_slice(p)
            }
            _ => {}
        }
    }
    let (x0, y0, x1, y1) = match crate::geom::bbox(&pts) {
        Some(b) => (b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64),
        None => (0.0, 0.0, 1.0, 1.0),
    };
    let (dw, dh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let s = (max_dim as f64) / dw.max(dh);
    let w = ((dw * s).ceil() as u32).max(1);
    let h = ((dh * s).ceil() as u32).max(1);
    // db-unit -> pixel (Y flipped)
    let map = |x: f64, y: f64| ((x - x0) * s, (y1 - y) * s);

    let mut buf = vec![250u8; (w * h * 3) as usize]; // #fafafa background

    for e in &cell.elements {
        match e {
            Element::Boundary { layer, pts: p, .. } | Element::Box { layer, pts: p, .. }
                if keep(*layer, layers) =>
            {
                let poly: Vec<(f64, f64)> = p.iter().map(|&(x, y)| map(x as f64, y as f64)).collect();
                fill(&mut buf, w, h, &poly, color(*layer), 0.45);
            }
            Element::Path { layer, pts: p, width, .. } if keep(*layer, layers) => {
                let hw = ((*width as f64 * s) / 2.0).max(0.5);
                let px: Vec<(f64, f64)> = p.iter().map(|&(x, y)| map(x as f64, y as f64)).collect();
                for seg in px.windows(2) {
                    let (ax, ay) = seg[0];
                    let (bx, by) = seg[1];
                    let (dx, dy) = (bx - ax, by - ay);
                    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
                    let (nx, ny) = (-dy / len * hw, dx / len * hw);
                    let quad = [(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)];
                    fill(&mut buf, w, h, &quad, color(*layer), 0.55);
                }
            }
            _ => {}
        }
    }
    encode_rgb(&buf, w, h)
}

/// Even-odd scanline fill of a pixel-space polygon, alpha-blended into `buf`.
fn fill(buf: &mut [u8], w: u32, h: u32, poly: &[(f64, f64)], col: (u8, u8, u8), a: f64) {
    if poly.len() < 3 {
        return;
    }
    let (mut ymin, mut ymax) = (f64::MAX, f64::MIN);
    for &(_, y) in poly {
        ymin = ymin.min(y);
        ymax = ymax.max(y);
    }
    let ya = (ymin.floor().max(0.0)) as i32;
    let yb = (ymax.ceil().min(h as f64)) as i32;
    let n = poly.len();
    for py in ya..yb {
        let yc = py as f64 + 0.5;
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n {
            let (ax, ay) = poly[i];
            let (bx, by) = poly[(i + 1) % n];
            if (ay <= yc && by > yc) || (by <= yc && ay > yc) {
                xs.push(ax + (yc - ay) / (by - ay) * (bx - ax));
            }
        }
        xs.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            let xa = xs[i].round().max(0.0) as i32;
            let xb = xs[i + 1].round().min(w as f64) as i32;
            for px in xa..xb {
                let idx = ((py as u32 * w + px as u32) * 3) as usize;
                buf[idx] = blend(buf[idx], col.0, a);
                buf[idx + 1] = blend(buf[idx + 1], col.1, a);
                buf[idx + 2] = blend(buf[idx + 2], col.2, a);
            }
            i += 2;
        }
    }
}

fn blend(dst: u8, src: u8, a: f64) -> u8 {
    (dst as f64 * (1.0 - a) + src as f64 * a).round().clamp(0.0, 255.0) as u8
}

// --- minimal PNG encoder (8-bit RGB, stored DEFLATE) ----------------------- //

fn encode_rgb(buf: &[u8], w: u32, h: u32) -> Vec<u8> {
    // filtered scanlines: a 0 (None) filter byte per row, then the row's RGB.
    let row = (w * 3) as usize;
    let mut raw = Vec::with_capacity(h as usize * (1 + row));
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&buf[y * row..y * row + row]);
    }
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, colour type 2 (RGB)
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_store(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, typ: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(data);
    let mut c = 0xffff_ffffu32;
    for &b in typ.iter().chain(data.iter()) {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    out.extend_from_slice(&(c ^ 0xffff_ffff).to_be_bytes());
}

/// zlib stream over `raw` using uncompressed (stored) DEFLATE blocks.
fn zlib_store(raw: &[u8]) -> Vec<u8> {
    let mut o = vec![0x78, 0x01];
    let mut i = 0;
    loop {
        let n = (raw.len() - i).min(0xffff);
        let last = if i + n >= raw.len() { 1u8 } else { 0 };
        o.push(last); // BFINAL bit, BTYPE=00 (stored)
        o.extend_from_slice(&(n as u16).to_le_bytes());
        o.extend_from_slice(&(!(n as u16)).to_le_bytes());
        o.extend_from_slice(&raw[i..i + n]);
        i += n;
        if last == 1 {
            break;
        }
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    o.extend_from_slice(&((b << 16) | a).to_be_bytes());
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gds::{Cell, Element};
    use crate::geom::Rect;

    #[test]
    fn emits_a_valid_png_signature_and_iend() {
        let cell = Cell {
            elements: vec![Element::Boundary { layer: 66, datatype: 0, pts: Rect::new(0, 0, 100, 80).as_boundary() }],
            ..Default::default()
        };
        let png = render_png(&cell, None, 64);
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]); // PNG magic
        assert!(png.windows(4).any(|w| w == b"IHDR"));
        assert!(png.windows(4).any(|w| w == b"IDAT"));
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}
