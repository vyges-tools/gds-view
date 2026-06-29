//! Render a (flattened) GDS cell to a single self-contained SVG string.
//!
//! Design geometry is in GDS database units; it is fitted into a fixed pixel
//! canvas (preserving aspect, Y flipped so up is up) with a layer legend on the
//! right. Shapes are filled translucently with a solid same-colour stroke so
//! overlaps stay visible. Optional violation rectangles are drawn last, on top, in
//! red — the *where* to a `vyges-drc` (or any engine's) *what*.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::gds::{Cell, Element};
use crate::geom::Rect;

/// A violation/region to box on top of the layout, in GDS db-unit coordinates.
pub struct Mark {
    pub r: Rect,
    pub label: String,
}

/// Drawing-area width in pixels (the legend column is added to the right).
const DRAW_W: f64 = 760.0;
const LEGEND_W: f64 = 180.0;
const MARGIN: f64 = 16.0;

/// A fixed, high-contrast palette; layer number indexes into it.
const PALETTE: &[&str] = &[
    "#4e79a7", "#f28e2b", "#59a14f", "#e15759", "#b07aa1", "#76b7b2", "#edc948",
    "#ff9da7", "#9c755f", "#bab0ac", "#1b9e77", "#d95f02", "#7570b3", "#e7298a",
];

fn color(layer: i16) -> &'static str {
    PALETTE[(layer as usize).wrapping_rem(PALETTE.len())]
}

/// Every (x,y) vertex that appears as drawn geometry, for bbox.
fn points(cell: &Cell, layers: Option<&[i16]>) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for e in &cell.elements {
        match e {
            Element::Boundary { layer, pts, .. }
            | Element::Path { layer, pts, .. }
            | Element::Box { layer, pts, .. }
                if keep(*layer, layers) =>
            {
                v.extend_from_slice(pts)
            }
            _ => {}
        }
    }
    v
}

fn keep(layer: i16, layers: Option<&[i16]>) -> bool {
    layers.map(|ls| ls.contains(&layer)).unwrap_or(true)
}

/// `render(cell, None, &[])` draws all layers; pass `Some(&[..])` to restrict.
pub fn render(cell: &Cell, layers: Option<&[i16]>, marks: &[Mark]) -> String {
    let pts = points(cell, layers);
    let mark_pts: Vec<(i32, i32)> = marks.iter().flat_map(|m| m.r.as_boundary()).collect();
    let all: Vec<(i32, i32)> = pts.iter().chain(mark_pts.iter()).copied().collect();

    let (x0, y0, x1, y1) = match crate::geom::bbox(&all) {
        Some(b) => (b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64),
        None => (0.0, 0.0, 1.0, 1.0), // empty cell — still emit a valid stub
    };
    let (dw, dh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let s = DRAW_W / dw.max(dh); // uniform fit
    let draw_h = dh * s;
    let canvas_w = DRAW_W + LEGEND_W + MARGIN * 3.0;
    let canvas_h = draw_h.max(legend_height(cell, layers)) + MARGIN * 2.0;
    let sw = (DRAW_W / 400.0).max(0.6); // stroke width

    // db-unit (x,y) -> canvas pixel (y flipped).
    let map = |x: f64, y: f64| (MARGIN + (x - x0) * s, MARGIN + (y1 - y) * s);

    let mut o = String::new();
    let _ = write!(
        o,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{cw:.0}\" height=\"{ch:.0}\" \
         viewBox=\"0 0 {cw:.0} {ch:.0}\" font-family=\"monospace\" font-size=\"11\">\n\
         <rect width=\"{cw:.0}\" height=\"{ch:.0}\" fill=\"#fafafa\"/>\n",
        cw = canvas_w,
        ch = canvas_h,
    );

    let mut seen: BTreeSet<i16> = BTreeSet::new();
    for e in &cell.elements {
        match e {
            Element::Boundary { layer, pts, .. } | Element::Box { layer, pts, .. }
                if keep(*layer, layers) =>
            {
                seen.insert(*layer);
                let poly = poly(pts, &map);
                let c = color(*layer);
                let _ = writeln!(
                    o,
                    "<polygon points=\"{poly}\" fill=\"{c}\" fill-opacity=\"0.35\" stroke=\"{c}\" stroke-width=\"{sw:.2}\"/>",
                );
            }
            Element::Path { layer, pts, width, .. } if keep(*layer, layers) => {
                seen.insert(*layer);
                let pl = poly(pts, &map);
                let c = color(*layer);
                let w = (*width as f64 * s).max(sw);
                let _ = writeln!(
                    o,
                    "<polyline points=\"{pl}\" fill=\"none\" stroke=\"{c}\" stroke-width=\"{w:.2}\" stroke-opacity=\"0.6\" stroke-linecap=\"round\"/>",
                );
            }
            _ => {}
        }
    }

    // violation overlay, on top, in red.
    for m in marks {
        let (mx0, my0) = map(m.r.x0 as f64, m.r.y0 as f64);
        let (mx1, my1) = map(m.r.x1 as f64, m.r.y1 as f64);
        let (rx, ry) = (mx0.min(mx1), my0.min(my1));
        let _ = writeln!(
            o,
            "<rect x=\"{rx:.1}\" y=\"{ry:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"none\" stroke=\"#d00\" stroke-width=\"{lw:.2}\"/>",
            w = (mx1 - mx0).abs(),
            h = (my1 - my0).abs(),
            lw = sw * 2.0,
        );
        if !m.label.is_empty() {
            let _ = writeln!(o, "<text x=\"{rx:.1}\" y=\"{ty:.1}\" fill=\"#d00\">{}</text>", esc(&m.label), ty = ry - 2.0);
        }
    }

    // legend.
    let lx = DRAW_W + MARGIN * 2.0;
    let _ = writeln!(o, "<text x=\"{lx:.1}\" y=\"{MARGIN}\" font-weight=\"bold\">layers</text>");
    for (i, layer) in seen.iter().enumerate() {
        let y = MARGIN + 14.0 + i as f64 * 18.0;
        let c = color(*layer);
        let _ = write!(
            o,
            "<rect x=\"{lx:.1}\" y=\"{ry:.1}\" width=\"12\" height=\"12\" fill=\"{c}\" fill-opacity=\"0.6\" stroke=\"{c}\"/>\n\
             <text x=\"{tx:.1}\" y=\"{ty:.1}\">layer {layer}</text>\n",
            ry = y,
            tx = lx + 18.0,
            ty = y + 10.0,
        );
    }
    if !marks.is_empty() {
        let y = MARGIN + 14.0 + seen.len() as f64 * 18.0;
        let _ = write!(
            o,
            "<rect x=\"{lx:.1}\" y=\"{ry:.1}\" width=\"12\" height=\"12\" fill=\"none\" stroke=\"#d00\" stroke-width=\"2\"/>\n\
             <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"#d00\">{n} violation(s)</text>\n",
            ry = y,
            tx = lx + 18.0,
            ty = y + 10.0,
            n = marks.len(),
        );
    }

    o.push_str("</svg>\n");
    o
}

fn legend_height(cell: &Cell, layers: Option<&[i16]>) -> f64 {
    let n = cell
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Boundary { layer, .. } | Element::Box { layer, .. } | Element::Path { layer, .. }
                if keep(*layer, layers) =>
            {
                Some(*layer)
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .len();
    MARGIN + 14.0 + (n as f64 + 1.0) * 18.0
}

fn poly(pts: &[(i32, i32)], map: &dyn Fn(f64, f64) -> (f64, f64)) -> String {
    let mut s = String::new();
    for (x, y) in pts {
        let (px, py) = map(*x as f64, *y as f64);
        let _ = write!(s, "{px:.1},{py:.1} ");
    }
    s.trim_end().to_string()
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gds::Element;

    fn cell() -> Cell {
        Cell {
            name: "top".into(),
            elements: vec![
                Element::Boundary { layer: 66, datatype: 0, pts: Rect::new(0, 0, 100, 50).as_boundary() },
                Element::Boundary { layer: 68, datatype: 0, pts: Rect::new(20, 60, 80, 90).as_boundary() },
            ],
        }
    }

    #[test]
    fn renders_a_polygon_per_shape_and_a_legend() {
        let svg = render(&cell(), None, &[]);
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<polygon").count(), 2, "one polygon per boundary");
        assert!(svg.contains("layer 66") && svg.contains("layer 68"), "both layers in legend");
        // layer 66 and 68 get distinct palette colours
        assert_ne!(color(66), color(68));
    }

    #[test]
    fn layer_filter_restricts_output() {
        let svg = render(&cell(), Some(&[66]), &[]);
        assert_eq!(svg.matches("<polygon").count(), 1);
        assert!(svg.contains("layer 66") && !svg.contains("layer 68"));
    }

    #[test]
    fn marks_draw_a_red_box_on_top() {
        let m = Mark { r: Rect::new(10, 10, 30, 30), label: "space < 100".into() };
        let svg = render(&cell(), None, std::slice::from_ref(&m));
        assert!(svg.contains("stroke=\"#d00\""), "violation box is red");
        assert!(svg.contains("space &lt; 100"), "label is escaped + shown");
        assert!(svg.contains("1 violation(s)"));
    }

    #[test]
    fn empty_cell_still_emits_valid_svg() {
        let svg = render(&Cell { name: "e".into(), elements: vec![] }, None, &[]);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
    }
}
