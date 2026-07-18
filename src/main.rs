//! vyges-gds-view CLI.
//!
//!   vyges-gds-view render LAYOUT.gds [--top CELL] [--layers 66,68] [--marks M.txt] [-o OUT.svg]
//!   vyges-gds-view demo [-o OUT.svg]
//!
//! Renders a flattened GDS top cell to a single self-contained SVG. A `--marks`
//! file boxes violation regions on top (e.g. from `vyges-drc`). Exit codes:
//! 0 ok · 1 runtime error · 2 usage.

use std::process::exit;

use vyges_gds_view::gds::{Cell, Element, Library};
use vyges_gds_view::geom::Rect;
use vyges_gds_view::svg::{self, Mark};
use vyges_gds_view::{flatten, png, VERSION};

use std::collections::BTreeSet;

const USAGE: &str = "\
vyges-gds-view — headless layout viewer (GDS or OASIS in, layered SVG out)

usage:
  vyges-gds-view render LAYOUT [--top CELL] [--layers L1,L2] [--marks FILE] [-o OUT.svg]
  # LAYOUT is GDSII (.gds) or OASIS (.oas/.oasis) — picked by extension
  vyges-gds-view demo [-o OUT.svg]

flags:
  --top CELL     top cell to flatten + render (default: last cell in the GDS)
  --layers LIST  comma-separated GDS layer numbers to show (default: all)
  --marks FILE   overlay violation boxes; each line: `x0 y0 x1 y1 [label...]`
  -o FILE        write to FILE (default: stdout). SVG, or a PNG if FILE ends in .png
  --png          force PNG (bounded raster thumbnail — for dense real blocks)
  --width N      PNG fit size in pixels (default: 700)
  --window BOX   PNG only: frame this db-unit region instead of the whole cell,
                 as `x0,y0,x1,y1` — the occurrence-level view for one violation
  --describe            print a machine-readable JSON description of the command
  -h, --help · -V, --version
";

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Parse a `--window x0,y0,x1,y1` region in db units.
///
/// Rejected rather than silently repaired when the corners are inverted: `x1,y0,x0,y1` is far
/// more likely a caller that swapped its arguments than one asking for a mirrored view, and
/// quietly normalising it would render a plausible image of the wrong place.
fn parse_window(spec: &str) -> Result<vyges_gds_view::geom::Rect, String> {
    let n: Vec<&str> = spec.split(',').map(str::trim).collect();
    if n.len() != 4 {
        return Err(format!(
            "--window needs 4 comma-separated db-unit values `x0,y0,x1,y1`, got {:?}",
            spec
        ));
    }
    let v: Result<Vec<i32>, _> = n.iter().map(|t| t.parse::<i32>()).collect();
    let v = v.map_err(|_| format!("--window values must be integers (db units), got {spec:?}"))?;
    if v[2] <= v[0] || v[3] <= v[1] {
        return Err(format!(
            "--window {spec:?} is empty or inverted: needs x1 > x0 and y1 > y0"
        ));
    }
    Ok(vyges_gds_view::geom::Rect {
        x0: v[0],
        y0: v[1],
        x1: v[2],
        y1: v[3],
    })
}

/// Parse a marks file: one violation per line, `x0 y0 x1 y1 [label...]`, `#` comments.
fn parse_marks(text: &str) -> Result<Vec<Mark>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let mut num = || -> Result<i32, String> {
            it.next()
                .ok_or_else(|| format!("marks line {}: expected `x0 y0 x1 y1`", n + 1))?
                .parse::<i32>()
                .map_err(|e| format!("marks line {}: {e}", n + 1))
        };
        let (x0, y0, x1, y1) = (num()?, num()?, num()?, num()?);
        let label = it.collect::<Vec<_>>().join(" ");
        out.push(Mark {
            r: Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)),
            label,
        });
    }
    Ok(out)
}

/// A small built-in layout with two layers — for a no-input smoke render.
fn demo_lib() -> Library {
    let mut lib = Library::default();
    lib.cells.push(Cell {
        name: "demo".into(),
        elements: vec![
            Element::Boundary {
                layer: 66,
                datatype: 0,
                pts: Rect::new(0, 0, 400, 80).as_boundary(),
            },
            Element::Boundary {
                layer: 66,
                datatype: 0,
                pts: Rect::new(0, 160, 400, 240).as_boundary(),
            },
            Element::Boundary {
                layer: 68,
                datatype: 0,
                pts: Rect::new(120, 0, 200, 240).as_boundary(),
            },
            Element::Path {
                layer: 70,
                datatype: 0,
                width: 20,
                pts: vec![(40, 40), (360, 40), (360, 200)],
            },
        ],
    });
    lib
}

fn write_out(args: &[String], svg: &str) {
    match opt(args, "-o") {
        Some(p) => {
            if let Err(e) = std::fs::write(&p, svg) {
                die(&format!("{p}: {e}"));
            }
        }
        None => print!("{svg}"),
    }
}

/// PNG output when `--png` is passed or the `-o` path ends in `.png`.
fn wants_png(args: &[String]) -> bool {
    args.iter().any(|a| a == "--png")
        || opt(args, "-o")
            .map(|p| p.to_ascii_lowercase().ends_with(".png"))
            .unwrap_or(false)
}

fn write_bytes(args: &[String], bytes: &[u8]) {
    match opt(args, "-o") {
        Some(p) => {
            if let Err(e) = std::fs::write(&p, bytes) {
                die(&format!("{p}: {e}"));
            }
        }
        None => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(bytes);
        }
    }
}

fn layer_filter(args: &[String]) -> Option<Vec<i16>> {
    opt(args, "--layers").map(|s| {
        s.split(',')
            .filter_map(|t| t.trim().parse::<i16>().ok())
            .collect()
    })
}

/// Emit the vyges-events completion summary for a render — to stderr only (the SVG
/// goes to stdout / -o and is never touched). This tool has no findings, so the trail
/// is a single INFO summary keyed by code=GDSVIEW-DONE, co-referencing the top cell.
fn emit_gds_view_events(top: &str, cell: &Cell, layers: Option<&[i16]>, out: Option<&str>) {
    use vyges_events::{Event, Severity};
    let keep = |layer: i16| layers.map(|ls| ls.contains(&layer)).unwrap_or(true);
    let mut shapes = 0usize;
    let mut seen: BTreeSet<i16> = BTreeSet::new();
    for e in &cell.elements {
        match e {
            Element::Boundary { layer, .. }
            | Element::Box { layer, .. }
            | Element::Path { layer, .. }
                if keep(*layer) =>
            {
                shapes += 1;
                seen.insert(*layer);
            }
            _ => {}
        }
    }
    let dest = out.unwrap_or("stdout");
    let msg = format!(
        "rendered top '{top}': {shapes} shape(s) across {} layer(s) -> SVG ({dest})",
        seen.len()
    );
    vyges_events::emit(
        &Event::new("vyges-gds-view", Severity::Info, msg)
            .with_code("GDSVIEW-DONE")
            .with_objects(vec![format!("cell:{top}")]),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--describe") {
        // Machine-readable description of `render` for tooling that drives it.
        const DESCRIBE: &str = r#"{
  "name": "gds-view",
  "summary": "headless layout viewer — GDS/OASIS → layered SVG",
  "maturity": "structured",
  "provenance_limitations": [
      "input_hash covers the argument vector, not the content of the layout or layer map it names."
  ],
  "invocation": {
    "args_template": ["render", "{gds}"],
    "optional": [
      { "arg": "top",    "flag": "--top" },
      { "arg": "layers", "flag": "--layers" },
      { "arg": "marks",  "flag": "--marks" },
      { "arg": "out",    "flag": "-o" }
    ],
    "emits_json": false
  },
  "inputs": {
    "type": "object",
    "required": ["gds"],
    "properties": {
      "gds":    { "type": "string", "description": "layout file (.gds or .oas)" },
      "top":    { "type": "string", "description": "top cell (default: the sole cell)" },
      "layers": { "type": "string", "description": "comma-separated layers to draw, e.g. 66,68" },
      "marks":  { "type": "string", "description": "a marks file to overlay" },
      "out":    { "type": "string", "description": "write the SVG to this path (default: stdout)" }
    }
  },
  "artifacts": [ { "role": "svg", "from_arg": "out" } ],
  "assertion": {
    "id": "layout-render",
    "not_applicable": true
  }
}
"#;
        print!("{DESCRIBE}");
        return;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") || args.is_empty() {
        print!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("vyges-gds-view {VERSION}");
        return;
    }

    match args[0].as_str() {
        "demo" => {
            let lib = demo_lib();
            let cell = flatten::flatten(&lib, "demo").unwrap_or_else(|e| die(&e));
            let layers = layer_filter(&args);
            let svg = svg::render(&cell, layers.as_deref(), &[]);
            emit_gds_view_events(
                "demo",
                &cell,
                layers.as_deref(),
                opt(&args, "-o").as_deref(),
            );
            write_out(&args, &svg);
        }
        "render" => {
            let Some(path) = args.get(1).filter(|a| !a.starts_with('-')) else {
                eprintln!("error: `render` needs a LAYOUT.gds path\n{USAGE}");
                exit(2);
            };
            let lib = Library::load_any(path).unwrap_or_else(|e| die(&format!("{path}: {e}")));
            let top = opt(&args, "--top").or_else(|| lib.cells.last().map(|c| c.name.clone()));
            let Some(top) = top else {
                die("the GDS has no cells")
            };
            let cell = flatten::flatten(&lib, &top).unwrap_or_else(|e| die(&e));

            let marks = match opt(&args, "--marks") {
                Some(mp) => {
                    let t =
                        std::fs::read_to_string(&mp).unwrap_or_else(|e| die(&format!("{mp}: {e}")));
                    parse_marks(&t).unwrap_or_else(|e| die(&e))
                }
                None => Vec::new(),
            };
            let layers = layer_filter(&args);
            emit_gds_view_events(&top, &cell, layers.as_deref(), opt(&args, "-o").as_deref());
            if wants_png(&args) {
                // Raster output: bounded PNG thumbnail (real blocks are too dense for SVG).
                let dim = opt(&args, "--width")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(700);
                // --window frames a chosen region: a violation is invisible in a
                // whole-block thumbnail, so the caller says which part to look at.
                let bytes = match opt(&args, "--window") {
                    Some(spec) => {
                        let w = parse_window(&spec).unwrap_or_else(|e| die(&e));
                        png::render_png_window(&cell, layers.as_deref(), dim, w)
                    }
                    None => png::render_png(&cell, layers.as_deref(), dim),
                };
                write_bytes(&args, &bytes);
            } else {
                write_out(&args, &svg::render(&cell, layers.as_deref(), &marks));
            }
        }
        other => {
            eprintln!("error: unknown command {other:?}\n{USAGE}");
            exit(2);
        }
    }
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_marks_with_and_without_labels() {
        let m = parse_marks("# a comment\n0 0 50 400 width 50 < 100\n10 10 30 30\n").unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].label, "width 50 < 100");
        assert_eq!((m[0].r.x0, m[0].r.y1), (0, 400));
        assert_eq!(m[1].label, "");
    }

    #[test]
    fn marks_normalize_reversed_corners() {
        let m = parse_marks("30 30 10 10\n").unwrap();
        assert_eq!(
            (m[0].r.x0, m[0].r.y0, m[0].r.x1, m[0].r.y1),
            (10, 10, 30, 30)
        );
    }

    #[test]
    fn bad_marks_line_errors() {
        assert!(parse_marks("0 0 50\n").is_err());
        assert!(parse_marks("0 0 x 9\n").is_err());
    }

    #[test]
    fn demo_renders() {
        let lib = demo_lib();
        let cell = flatten::flatten(&lib, "demo").unwrap();
        let svg = svg::render(&cell, None, &[]);
        assert!(svg.contains("layer 66") && svg.contains("layer 70"));
    }

    // ---- --window parsing ----

    #[test]
    fn a_window_parses_to_its_db_unit_rect() {
        let w = parse_window("10,20,30,40").expect("a well-formed window");
        assert_eq!((w.x0, w.y0, w.x1, w.y1), (10, 20, 30, 40));
        // whitespace around the values is the natural thing to type
        let w = parse_window(" 10 , 20 , 30 , 40 ").expect("spaces are tolerated");
        assert_eq!((w.x0, w.y0, w.x1, w.y1), (10, 20, 30, 40));
        // db units are signed: a window may sit below/left of the origin
        let w = parse_window("-40,-40,0,0").expect("negative db coordinates are valid");
        assert_eq!((w.x0, w.y0, w.x1, w.y1), (-40, -40, 0, 0));
    }

    /// Inverted corners are refused rather than normalised. `x1,y0,x0,y1` is far more likely
    /// a caller that swapped its arguments than one asking for a mirrored view, and quietly
    /// repairing it would render a plausible image of the wrong place — the worst outcome for
    /// something whose whole job is to be evidence.
    #[test]
    fn an_inverted_or_empty_window_is_refused() {
        for spec in [
            "100,100,50,50",
            "100,0,50,80",
            "0,100,80,50",
            "10,10,10,20",
            "10,10,20,10",
        ] {
            let e = parse_window(spec).expect_err("{spec} should be refused");
            assert!(
                e.contains("empty or inverted"),
                "{spec} should say why: {e}"
            );
        }
    }

    #[test]
    fn a_malformed_window_says_what_it_wanted() {
        for spec in ["1,2,3", "1,2,3,4,5", "", "a,b,c,d", "1,2,3,x"] {
            let e = parse_window(spec).expect_err("{spec} should be refused");
            assert!(
                e.contains("--window"),
                "the error should name the flag: {e}"
            );
        }
    }
}
