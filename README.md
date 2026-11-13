<p align="center">
  <picture>
    <img alt="SliceCraft" src="" width="0" height="0">
  </picture>
</p>

<h1 align="center">SliceCraft</h1>

<p align="center">
  <b>A 3D print slicer; an open-source, clean-room slicer written from scratch in pure Rust.</b>
</p>

<p align="center">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-b7410e?style=flat-square&logo=rust">
  <img alt="Status: pre-alpha" src="https://img.shields.io/badge/status-pre--alpha-d69e2e?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-3a3a3a?style=flat-square">
</p>

**SliceCraft is a candidate member of the ArtCraft crafting-app suite.** Community
membership is pending; nothing here claims approval. SolidCraft and MailCraft are
the approved community apps so far. Until approval lands, this repository is built
as a standalone project that follows the suite's conventions closely enough to be
adopted without a rewrite: the same layering, the same never-crash rules, the same
command-first architecture, the same licences.

SliceCraft does what a slicer does: import a model, place it on a bed, choose a
profile, slice it into layers, generate perimeters, infill and support, and emit
G-code for a real printer. The target behaviour is Simplify3D's feature set; the
implementation is our own.

## Status

**Pre-alpha.** The desktop window places models on a virtual bed, edits a full print
profile, slices into perimeters, solid and sparse infill, skirt and brim, previews the
toolpaths (by layer or move, coloured by feature, speed or height) and exports G-code.
Jobs save as `.scproj` projects, and Simplify3D `.fff` profiles import through
Load Profile. Automatic support is in. Features that are not built yet (manual support
editing, raft, printer control, several profiles per
job and more) are in the menus and the profile as clearly marked placeholders.
[docs/research/02-feature-inventory.md](docs/research/02-feature-inventory.md) lists
every capability and its status. Nothing has been test-printed yet.

```sh
cargo run --release -p slicecraft -- examples/cube.stl     # the window
cargo run -p slicecraft-cli -- info examples/cube.stl
cargo run -p slicecraft-cli -- slice examples/cube.stl -o cube.gcode --infill 15 --json
cargo test --workspace          # 154 tests
cargo xtask ci                  # fmt, clippy -D warnings, tests, layering
```

In the window: drop STL/OBJ files on it or use **Import**, move models with the
fields on the left or shift-drag, change the profile on the right, press **Slice**
(Ctrl+R), scrub layers with the slider or Up/Down, then **Export G-code** (Ctrl+E).

## The shape

```
geom ─┐        f64 vectors, bounds, meshes, planar regions (offset, booleans)
doc   ┤        scene objects, placement, bed, print profile, validation
render┤        cross sections, loops, shells, infill, path ordering
io    ┤        STL/OBJ import, G-code with extrusion model
engine┤        session, commands, parallel slicing pipeline
ui-egui · mcp  the egui window · the agent surface
apps: slicecraft · slicecraft-cli · (slicecraft-web later)
```

Nothing below `ui-egui` knows a UI exists. `cargo xtask layers` enforces it.

## Documentation

| Document | What it holds |
|---|---|
| [docs/PLAN.md](docs/PLAN.md) | The program plan: joining the suite, phases, estimates, risks |
| [docs/architecture.md](docs/architecture.md) | How the code is built today |
| [docs/research/](docs/research/README.md) | Research notes: feature inventory, support, extrusion model, profile import, side-by-side comparison |
| [docs/target-app-parity.md](docs/target-app-parity.md) | How parity is measured, audience by audience |
| [docs/gaps.md](docs/gaps.md) | Everything still missing, as a list |
| [ROADMAP.md](ROADMAP.md) | Stage, headline numbers, what is next |
| [AGENTS.md](AGENTS.md) | The rules that keep the shape (house rules, clean room, never crash) |

## Clean room

Learn from observed behaviour and public specifications. Do not copy another
slicer's code, assets, strings, profiles, icons or artwork. The research documents
record behaviour and structure; they carry no third-party code.

## Licence

MIT OR Apache-2.0, matching the suite.
