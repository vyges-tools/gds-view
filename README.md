# vyges-gds-view

Headless **GDS layout viewer**: a laid-out **GDS** in, a single self-contained
**SVG** out — every shape coloured by its layer, with an optional **violation
overlay** that boxes exactly where another engine flagged a problem.

> **Vyges open EDA tools.** Commercial-grade silicon sign-off capability, built on
> open standards and plain file formats — and meant to be accessible to everyone,
> not only teams who can license a six-figure tool. `vyges-gds-view` makes the
> geometry *visible*.

> **Stability: experimental (v0.1.0).** Real flatten-and-render with a violation
> overlay; this is a *look at it* aid, not an interactive layout editor — see
> **Current state** for what is and isn't there.

## Why this exists

`vyges-drc` and `vyges-lvs` tell you *what* is wrong with a layout. They don't show
you *where*. `vyges-gds-view` closes that loop: render the GDS, drop the violation
coordinates on top, and open the result in any browser — or commit it to a report,
or diff it in CI. No X server, no GUI toolkit, no install beyond the binary.

It's the visual companion to the geometry engines, riding the **same
`vyges-layout`** GDS/geometry kernel they do — one toolset, one language.

## How this is solved today

Looking at a layout today means **KLayout** or **Magic** — capable GUIs, but a
running display, a heavy install, and Tcl/Ruby/C++ to script. There is no small,
embeddable "GDS to a picture, headless" step you can wire into a flow. `vyges-gds-view`
is that step: pure **Rust**, std-only beyond the shared kernel, output is plain SVG
text.

## Use it

```sh
cargo build --release

vyges-gds-view demo -o demo.svg                                  # built-in sample layout
vyges-gds-view render block.gds -o block.svg                     # flatten top cell -> SVG
vyges-gds-view render block.gds --top mycell --layers 66,68      # pick cell + restrict layers
vyges-gds-view render block.gds --marks viols.txt -o block.svg   # overlay violation boxes
# flags: --top CELL · --layers LIST · --marks FILE · -o FILE · -h · -V
```

The **marks file** is one violation per line — the trivial format any engine can
emit (`#` starts a comment):

```text
# x0 y0 x1 y1  label   (GDS db units)
120 0 200 80   space < min
40 200 360 240 width < min
```

So a DRC-then-look pass is two commands and a one-line glue: run `vyges-drc`, turn
its violations into `x0 y0 x1 y1 label` lines, and `render --marks` them onto the
layout.

## How it works

- **Flatten** the chosen top cell through `vyges-layout` (SREF/AREF expanded).
- **Fit** the design bounding box into a fixed pixel canvas, aspect preserved, the
  Y axis flipped so up is up.
- **Draw** each `Boundary`/`Box` as a translucent filled polygon and each `Path` as
  a stroked polyline, coloured from a fixed palette indexed by **GDS layer number**;
  a legend lists the layers present.
- **Overlay** each mark as a red outline box (with its label) on top — the *where*.

## Current state (v0.1.0)

**Working & tested:** flatten + render of `Boundary` / `Box` / `Path` geometry, the
per-layer palette and legend, a `--layers` filter, the `--marks` violation overlay
(reversed corners normalized, labels escaped), and an empty cell still produces
valid SVG. A `demo` subcommand renders a built-in layout with no input.

**Depth reserved (honest):**

- always renders a **flattened** top cell — per-instance / hierarchical views are a
  follow-up;
- **datatype** is not yet part of the colour key (layer number only); a named
  layer-map (layer/datatype to name + colour, à la a KLayout layer properties file)
  is the next step;
- consumes violations as the simple **marks** text format — reading a `vyges-drc`
  (or `-lvs`, `-extract`) JSON report directly is the follow-up once those reports
  carry coordinates;
- raster (PNG) output and any interactivity are out of scope — SVG only, by design.

**Validation roadmap:** render representative open-PDK blocks (sky130, gf180) and
eyeball against KLayout — the same layout the rest of Loom already reads.
