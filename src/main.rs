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
use vyges_gds_view::{flatten, VERSION};

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
  -o FILE        write SVG to FILE (default: stdout)
  --describe            print a machine-readable JSON description of the command
  -h, --help · -V, --version
";

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
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
        out.push(Mark { r: Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)), label });
    }
    Ok(out)
}

/// A small built-in layout with two layers — for a no-input smoke render.
fn demo_lib() -> Library {
    let mut lib = Library::default();
    lib.cells.push(Cell {
        name: "demo".into(),
        elements: vec![
            Element::Boundary { layer: 66, datatype: 0, pts: Rect::new(0, 0, 400, 80).as_boundary() },
            Element::Boundary { layer: 66, datatype: 0, pts: Rect::new(0, 160, 400, 240).as_boundary() },
            Element::Boundary { layer: 68, datatype: 0, pts: Rect::new(120, 0, 200, 240).as_boundary() },
            Element::Path { layer: 70, datatype: 0, width: 20, pts: vec![(40, 40), (360, 40), (360, 200)] },
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

fn layer_filter(args: &[String]) -> Option<Vec<i16>> {
    opt(args, "--layers").map(|s| {
        s.split(',').filter_map(|t| t.trim().parse::<i16>().ok()).collect()
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--describe") {
        // Machine-readable description of `render` for tooling that drives it.
        const DESCRIBE: &str = r#"{
  "name": "gds-view",
  "summary": "headless layout viewer — GDS/OASIS → layered SVG",
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
  "artifacts": [ { "role": "svg", "from_arg": "out" } ]
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
            let svg = svg::render(&cell, layer_filter(&args).as_deref(), &[]);
            write_out(&args, &svg);
        }
        "render" => {
            let Some(path) = args.get(1).filter(|a| !a.starts_with('-')) else {
                eprintln!("error: `render` needs a LAYOUT.gds path\n{USAGE}");
                exit(2);
            };
            let lib = Library::load_any(path).unwrap_or_else(|e| die(&format!("{path}: {e}")));
            let top = opt(&args, "--top").or_else(|| lib.cells.last().map(|c| c.name.clone()));
            let Some(top) = top else { die("the GDS has no cells") };
            let cell = flatten::flatten(&lib, &top).unwrap_or_else(|e| die(&e));

            let marks = match opt(&args, "--marks") {
                Some(mp) => {
                    let t = std::fs::read_to_string(&mp).unwrap_or_else(|e| die(&format!("{mp}: {e}")));
                    parse_marks(&t).unwrap_or_else(|e| die(&e))
                }
                None => Vec::new(),
            };
            let svg = svg::render(&cell, layer_filter(&args).as_deref(), &marks);
            write_out(&args, &svg);
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
        assert_eq!((m[0].r.x0, m[0].r.y0, m[0].r.x1, m[0].r.y1), (10, 10, 30, 30));
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
}
