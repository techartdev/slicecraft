# Gaps

Everything SliceCraft does not do yet, grouped, with the phase that closes it. This
list is the honest counterpart to [PLAN.md](PLAN.md); when a gap is closed it moves to
the architecture document, not to a quieter line here.

> **Last reviewed:** 2026-11-13, after the full UI pass. The complete, sourced list of
> what the reference can do, and our status on each item, is
> [research/02-feature-inventory.md](research/02-feature-inventory.md); this page is
> the short version.

## Import and model handling

- **3MF** — read (ZIP and ZIP64 packages, units, build items, components with
  transforms; one model per build item). Colours, materials and slice data are ignored.
- **STEP / IGES** — not planned; 3MF and mesh repair cover the same need.
- **Multi-object files** — one object per file; an OBJ with several groups becomes one
  object.
- **Units** — the mesh is treated as millimetres; inch and centimetre models are fixed
  by hand (Model ▸ Correct units). No automatic detection.
- **Repair** — weld, degenerate-face removal, consistent outward normals per part, and
  hole filling (fan from the hole's centre: fine for small, roughly flat holes);
  self-intersections are counted, not removed; triangle reduction merges vertices on a
  grid (simple, not shape-preserving).
- **Placement** — lay flat (largest flat face), split into parts, merge models, cut at a
  height (capped), hollow (inner shell along averaged normals: thin features can
  self-intersect). Groups select and move together. A model can be printed as
  support (it prints with support settings where it stands) or used as a cutter (its
  section is removed from the parts on every layer). Arrange is a row packer, not a
  nesting algorithm and not group-aware.
- **Model export** — STL (binary), OBJ and 3MF of the visible models as placed.

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
  gap); prime tower and ooze shield on multi-tool jobs (see Multi-tool).
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
- **Multi-tool** — a model zone picks a model's tool (`tool_number`); support, skirt,
  brim and raft use the profile's tool. Each layer prints one run per tool, starting
  with the tool that ended the layer below. Tool changes retract by the tool-change
  length, drop the idle tool to its standby temperature and wait for the new one; a
  tool-change script can wrap them. The prime tower (each tool fills its share) and the
  ooze shield (one wall, leaning in no more than its slope) stand up to the last layer
  that changes tools. Not yet: per-tool temperatures and offsets, a separate support
  tool, the tower on a raft, and cooling an idle tool only after a delay. Tested on
  synthetic two-tool jobs only.
- **Cooling** — fan from a chosen layer, minimum layer time with a slowest speed, full fan
  on quick layers, fan kick-start; no bridge or top-surface fan yet.
- **Retraction** — distance, speed, minimum travel, minimum extrusion, Z lift,
  layer-change retraction, extra restart, "only when crossing outlines", coasting,
  wipe and firmware retraction (G10/G11). Travel inside the part walks around holes.
- **Adaptive layer heights, size compensation, combined infill, vase mode** — done.
  **Layer ranges and per-model settings** — done as setting zones. A height zone
  that changes the layer height drives the layer schedule (variable layer heights);
  adaptive heights take precedence when both are on.
- **First-layer width** — set by percentage; no separate first-layer line width for the part on a raft.

## Output

- **G-code flavours** — Marlin, RepRap (fan as a fraction) and Klipper
  (`SET_PRINT_STATS_INFO` layers) differ where it matters most; no binary or
  printer-specific formats.
- **Relative extrusion**, **arc moves** (G2/G3, fitted within 0.01 mm), **thumbnails**
  (PNG in comments), centre origin and round beds, a post-processing program (window
  only). No pressure advance.
- **Scripts** — start, end, layer-change, retraction, tool-change and profile-change
  scripts with placeholders; no conditionals. Coordinate offsets work.
- **Estimates** weigh acceleration by how sharply each move turns; weight and cost use
  the profile's density and price. Neither is calibrated against a real print.

## Profiles and project

- **Profiles** — 172 typed settings with ranges and help, all used by the slicer.
  Binary G-code is written uncompressed from the published specification and has not
  been tried on a printer. Profiles save and load as text and
  the last one is restored at start-up; no inheritance or comparison.
- **Simplify3D import** — `.fff` profiles (4.x and 5.x) load through Load Profile with
  a report. Only the first extruder, the first two temperature steps and the first
  fan speed are carried over; material/quality presets inside a 4.x file and
  `.factory` project files are not read. Some value meanings are inferred, not
  measured ([R9](research/09-fff-profiles.md) §4).
- **Setting zones** — any settings can change over a height range and/or for chosen
  models. A model zone gets its own walls, infill and solid layers; speeds,
  temperatures and fan follow the layer (the height zones), not the model.
- **No printer or material database**; the bed size comes from the profile.
- **Projects** — `.scproj` holds models, placement, visibility and the profile. It is
  uncompressed (≈ 96 bytes per triangle), keeps no undo history and no slice result,
  and does not remember the camera. Opening a project clears undo.

## UI

- **Rendering** is CPU projection into egui meshes (no depth buffer); fine to about
  400k triangles, thinned above that. The preview draws at most 200k moves.
- **No gizmos** — tools are drag modes (move on the bed, rotate about Z, scale);
  exact values come from the placement panel.
- **Languages** — English and Bulgarian (`ui-egui::i18n`). Setting help, status-bar
  messages and errors from the engine are English in both; a missing translation shows
  the English text. Each is shown disabled with a "planned" note. Wireframe shows
  triangle edges only (no normals or points mode); shortcuts are rebindable for the
  18 main actions, while view keys, Esc and the preview keys stay fixed.
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
- **MCP** — `slicecraft-cli mcp` serves 32 tools over stdio (JSON-RPC 2.0, one message per
  line): tools only, no resources, prompts or progress notifications; long slices block
  the session until they finish.
- **Printer I/O** — USB serial: connect, temperatures, jogging, terminal, print with line
  numbers, checksums and resends, pause/resume/cancel. Tested against a simulated
  printer only. Network printing uploads to OctoPrint or Moonraker over plain HTTP
  (upload, or upload and start), tested against a local fake host; no HTTPS, no host
  discovery, no live status from network hosts. The print camera shows a JPEG
  snapshot address refreshed about once a second (no MJPEG stream). No SD-card upload.
- **Packaging: Windows and macOS** — `cargo xtask dist` makes a zip and an Inno Setup
  installer on Windows (unsigned, SmartScreen warns on first run) and a universal
  `SliceCraft.app` in a `.dmg` on macOS (ad-hoc signed, not notarized, so Gatekeeper
  asks on first open). On Linux it packs a tar.gz with a desktop file and icon, but no
  Linux release has been built yet. No nfpm, Flatpak, AppImage or FreeBSD package. The
  macOS app does not yet open files handed to it by Finder (only from the command line).
- **No CI** beyond `cargo xtask ci`; shared workflows are deferred until approval.
- **No wasm build.**
- **Linux and FreeBSD have never built this tree.** macOS 10.15 (Intel) builds it and
  passes every test; the window has been seen running only on Windows 11 (on macOS it
  opened, but behind a locked screen).

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
- **Test prints:** none yet; real-print calibration is the next step.
- **No `cargo xtask assets`**, no `contributors/people.toml`, no app icon set or
  desktop/metainfo/plist/wxs files, no `storytold/slicecraft-corpus`.
- **No `craft-libs` dependencies** — deliberately deferred until approval (PLAN §7).
