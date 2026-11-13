# Gaps

Everything SliceCraft does not do yet, grouped, with the phase that closes it. This
list is the honest counterpart to [PLAN.md](PLAN.md); when a gap is closed it moves to
the architecture document, not to a quieter line here.

> **Last reviewed:** 2026-11-13, after the full UI pass. The complete, sourced list of
> what the reference can do, and our status on each item, is
> [research/02-feature-inventory.md](research/02-feature-inventory.md); this page is
> the short version.

## Import and model handling

- **3MF** — not read (ZIP + model XML + units + transforms). Refused with "not
  implemented yet".
- **STEP / IGES** — not planned; 3MF and mesh repair cover the same need.
- **Multi-object files** — one object per file; an OBJ with several groups becomes one
  object.
- **Units** — the mesh is treated as millimetres; inch and centimetre models are fixed
  by hand (Model ▸ Correct units). No automatic detection.
- **Repair** — analysis reports open, non-manifold and degenerate faces; nothing
  repairs them yet. Slicing tolerates small gaps (joins ends within 0.01 mm, closes
  chains with gaps under 1 mm) and reports what it drops.
- **Placement** — no lay-flat, no grouping, no split or merge. Arrange is a row
  packer, not a nesting algorithm.
- **No model export.**

## Geometry

- **Self-intersecting or overlapping shells** are merged by the non-zero rule, which
  is right for overlapping bodies and wrong for meshes with inverted inner shells.

## Slicing pipeline

- **Support** — automatic, as pillars (default) or exact areas: overhang angle,
  everywhere or bed-only, lines or grid, gaps, interface/floor/base layers, expansion,
  walls; each pillar is one zigzag. A preview in the prepare view, and boxes that add
  or block support (full height; height ranges exist in the data and file format
  but have no editor yet). Edit boxes are in bed coordinates and stay put when a
  model moves.
- **Bridges** — solid areas over air print straight across their short span with their own
  flow, speed and fan; no bridge detection for perimeters.
- **Adhesion** — skirt (loops, layers), brim and raft (base and top layers, margin, air
  gap); no prime tower or ooze shield (they need several tools).
- **Infill patterns** — rectilinear, one-direction lines, grid, triangles, concentric, gyroid,
  honeycomb, wiggle, cubic and octahedral all work, with dense layers under tops, small
  islands made solid, solid expansion, top surfaces with their own flow/speed/fan,
  monotonic tops and ironing.
- **Thin walls / gap fill** — single lines along walls thinner than a line and in gaps the
  shells leave (on by default); inner loops are skipped where they would overlap
  themselves by more than 30 %. Gap lines use the normal line width (no variable
  width).
- **Seams** — nearest, aligned to a point, or scattered; no preferred-region hiding.
- **Travel** — nearest-neighbour ordering per feature group; combing walks around holes
  inside the part.
- **Multi-tool** — absent.
- **Cooling** — fan from a chosen layer, minimum layer time with a slowest speed, full fan
  on quick layers, fan kick-start; no bridge or top-surface fan yet.
- **Retraction** — distance, speed, minimum travel, minimum extrusion, Z lift,
  layer-change retraction, extra restart, "only when crossing outlines", coasting,
  wipe and firmware retraction (G10/G11). Travel inside the part walks around holes.
- **Adaptive layer heights, size compensation, layer ranges, per-object settings** —
  absent.
- **First-layer width** — set by percentage; no separate first-layer line width for the part on a raft.

## Output

- **G-code flavours** — Marlin, RepRap and Klipper share one command set today; only
  the header names the flavour. No binary or printer-specific formats.
- **Relative extrusion** (M83) works; no arc moves, no pressure advance, no thumbnails.
- **Scripts** — start, end and layer-change scripts with placeholders; no retraction,
  tool-change or profile-change scripts, no conditionals. Coordinate offsets work.
- **Estimates** ignore acceleration and are optimistic; weight and cost use the
  profile's density and price. Neither is calibrated against a real print.

## Profiles and project

- **Profiles** — 169 typed settings with ranges and help; 59 drive the slicer, 110 are
  placeholders that are saved and shown as planned. Profiles save and load as text and
  the last one is restored at start-up; no inheritance or comparison.
- **Simplify3D import** — `.fff` profiles (4.x and 5.x) load through Load Profile with
  a report. Only the first extruder, the first two temperature steps and the first
  fan speed are carried over; material/quality presets inside a 4.x file and
  `.factory` project files are not read. Some value meanings are inferred, not
  measured ([R9](research/09-fff-profiles.md) §4).
- **Several profiles per job, height zones** — not supported (menu items are planned).
- **No printer or material database**; the bed size comes from the profile.
- **Projects** — `.scproj` holds models, placement, visibility and the profile. It is
  uncompressed (≈ 96 bytes per triangle), keeps no undo history and no slice result,
  and does not remember the camera. Opening a project clears undo.

## UI

- **Rendering** is CPU projection into egui meshes (no depth buffer); fine to about
  400k triangles, thinned above that. The preview draws at most 200k moves.
- **No gizmos** — tools are drag modes (move on the bed, rotate about Z, scale);
  exact values come from the placement panel.
- **Placeholders in the window**: lay flat, repair,
  several profiles, setting zones, machine control, printing, orthographic camera,
  wireframe, colour by tool/flow/profile, language,
  shortcut editor. Each is shown disabled or opens a "planned" note.
- **Keyboard shortcuts** are untested with a physical keyboard: synthetic key events
  from the test driver were unreliable (Ctrl+R once worked, Ctrl+P, Ctrl+S and Ctrl+D
  did not register). Every shortcut also has a menu item, and those were driven.
- **File dialogs block the window** while open (native dialogs on the UI thread); the
  view does not repaint behind them.
- **No app icon, no `craft-ui` theme tokens, no i18n catalogue.**
- **Accessibility** — AccessKit is on through egui; the 3D view has no accessible
  description.

## Automation and platform

- **No JSON control channel.**
- **No MCP transport** (the tool table and dispatch exist; stdio JSON-RPC does not).
- **No printer I/O** — no serial, no discovery, no job upload.
- **No packaging** — no MSI, dmg, nfpm, Flatpak, FreeBSD package.
- **No CI** beyond `cargo xtask ci`; shared workflows are deferred until approval.
- **No wasm build.**
- **macOS, Linux and FreeBSD have never built this tree**; the window has only been run
  on Windows 11.

## Stability and quality

- **No fuzzing** on the STL/OBJ readers or the region code (every prefix of a binary
  STL is tested; nothing random).
- **No property tests** on geometry.
- **Performance measured only on small models** (a 20k-triangle torus slices in about
  0.4 s in release); no large-model benchmark.
- **No real-print validation** of anything.

## Repository and suite integration

- **Not a git repository yet**; no remote.
- **Licence exceptions** for the window's dependencies are recorded in
  `ATTRIBUTION.md` and must be proposed upstream before joining the suite.
- **Apache-2.0 text missing** (MIT present); dual licensing to be completed at first release.
- **No `cargo xtask assets`**, no `contributors/people.toml`, no app icon set or
  desktop/metainfo/plist/wxs files, no `storytold/slicecraft-corpus`.
- **No `craft-libs` dependencies** — deliberately deferred until approval (PLAN §7).
