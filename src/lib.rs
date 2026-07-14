//! vyges-gds-view — headless **GDS layout viewer**.
//!
//! A laid-out **GDS** in, a single self-contained **SVG** out: every drawn shape
//! coloured by its layer, a layer legend, and an optional **violation overlay**
//! that boxes the spots another Loom engine flagged (e.g. `vyges-drc`). No display
//! server, no GUI toolkit — it renders to a text file you open in any browser, drop
//! in a report, or diff in CI.
//!
//! This is the *look at it* companion to the geometry engines: where `vyges-drc`
//! and `vyges-lvs` say *what* is wrong, `vyges-gds-view` shows you *where*. It rides
//! the same `vyges-layout` GDS/geometry kernel and is pure std beyond it.
//!
//! v0 renders `Boundary` / `Box` / `Path` geometry from a flattened top cell and
//! overlays violation rectangles from a trivial whitespace marks file. Per-cell
//! (un-flattened) views, fills vs. outlines per layer, and reading engine JSON
//! directly are the depth passes.

pub use vyges_layout::{flatten, gds, geom};

pub mod png;
pub mod svg;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "© 2026 Vyges. All Rights Reserved.  https://vyges.com";
