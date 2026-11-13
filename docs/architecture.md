# SliceCraft architecture

> **Last reviewed:** 2026-11-13 · **Change:** MVP — real slicing pipeline and the
> desktop window · **Target:** Simplify3D 5.1.2 behaviour (clean room)

How SliceCraft is built today. Plans live in [PLAN.md](PLAN.md); this document
describes code that exists. The rules that keep the shape are in
[AGENTS.md](../AGENTS.md).

## Workspace

13 packages: 7 libraries, 2 apps, `xtask`. About 13,700 lines of Rust
(`cargo xtask stats`, 2026-11-13), 154 tests in 17 test binaries.

| Layer | Crate | What it does |
|---|---|---|
| L0 | `geom` | `Vec3`, `Point2`, bounds, `Triangle`, `Mesh` (finite and within `±MAX_COORD`), `region`: polygons with holes, offset, union/difference/intersection on a 1 µm grid via `i_overlay` |
| L1 | `doc` | `Document` (bed, profile, objects), `SceneObject` + `Transform` (centre → scale → rotate Z → place), `PrintSettings` (25 validated fields incl. `Flavor`), arrange, off-bed check |
| L2 | `render` | `section`: triangle → directed segment → stitched loops; `paths`: toolpath types, perimeter shells, line infill, path ordering; `fixtures` for tests |
| L3 | `io` | STL (binary + ASCII, detected by content), OBJ; `GcodeWriter` and `write_gcode` with the extrusion model, retraction, fan and estimates; `project` (`.scproj`); `fff` (Simplify3D profile import) |
| L4 | `engine` | `Session`, `Command`/`Outcome`, `COMMANDS`, `SliceJob`; `pipeline`: the parallel slicing pipeline; `support`: support areas and paths |
| L5 | `ui-egui` | eframe window: `app` (panels, commands, slicing thread), `camera` (orbit camera math), `scene` (bed, models, toolpaths as egui meshes) |
| L5 | `mcp` | `ToolSpec` table bound to engine command ids, `dispatch` with arguments |
| app | `slicecraft` | Opens the window, with model files from the command line pre-loaded |
| app | `slicecraft-cli` | `commands`, `info <model>`, `slice <model>... -o out.gcode [options] [--json]` |
| tool | `xtask` | `ci`, `layers`, `stats` |

`cargo xtask layers` reads each manifest's package name and dependencies: nothing
below L5 may depend on egui, eframe, winit, wgpu or rfd, and only apps may depend on
`slicecraft-ui-egui`.

## Data model

- `Document` owns a `Bed`, a `PrintSettings` and up to `MAX_OBJECTS` `SceneObject`s.
- A `SceneObject` keeps its mesh in file coordinates plus a `Transform`. Its
  reference point is the centre of its bounding box in X/Y and its bottom in Z;
  `translate` puts that point on the bed. `world_mesh()` applies scale, rotation
  about Z and translation. Models always sit on the bed.
- `PrintSettings::validate` bounds every field (lengths, speeds, temperatures, counts),
  which also bounds the work a profile can cause. `Document::sliceable` adds "has a
  visible model", "valid placement" and "fits on the bed".

## Slicing

`engine::pipeline::slice_document` runs these stages; the per-layer ones run on scoped
worker threads that are joined explicitly, so a worker failure is an error, not a crash,
and the calling thread reports progress and relays cancellation.

1. **Bed meshes** — each visible object's world mesh, dropped to z = 0.
2. **Layer schedule** — layer 0 is `first_layer_height` thick, the rest
   `layer_height`; heights come from the index, never accumulated. Each layer is cut
   at the middle of its slab. Capped at `MAX_LAYERS`.
3. **Sections** — `render::section`: a vertex is *above* when `z ≥ plane`, so a plane
   through a vertex cannot lose a segment; crossings are computed from the above end
   of each edge, so shared edges give bit-identical points; segments run with the
   solid on their left. Stitching joins exact endpoints, then joins ends within
   0.01 mm, closes chains with gaps under 1 mm, and counts what it drops.
4. **Regions** — all loops of a layer become one `Region` under the non-zero rule
   (overlapping bodies merge; an inverted mesh still fills).
5. **Shells** — `perimeters`: shell *k* is the region offset by −(w/2 + k·w); the
   area inside the last shell's inner edge is kept for infill.
6. **Solid vs sparse** — an area is solid unless every layer within
   `solid_layers_bottom` below and `solid_layers_top` above covers it (intersection of
   their regions); near the bottom and top of the stack everything is solid. Slivers
   under w² are left sparse.
7. **Infill** — `line_infill`: parallel lines on a global grid (so layers line up),
   45°/135° alternating by default, clipped by an edge-bucket scanline, solid at
   spacing w, sparse at w·100/density. Infill regions are grown by the overlap.
8. **Skirt** — first layer only, around the first layer's region.
9. **Ordering** — per layer: skirt, inner shells, outer shells, solid, sparse; each
   group nearest-neighbour from where the nozzle is, closed paths starting at their
   nearest vertex, open paths reversed when that is shorter.
10. **G-code** — `io::write_gcode`.

## G-code

`GcodeWriter` is the only place that formats a move. It tracks the E axis and
retraction state, emits `F` only when the feed changes, and keeps estimates.

- Extrusion: filament per mm = bead area / filament area × multiplier, with the bead a
  rectangle with semicircular sides, `(w − h)·h + π·h²/4`.
- Every layer: retract, `G0 Z`, `G92 E0` (E stays small for single-precision
  firmware), fan on at `fan_from_layer`.
- Travel: `G0`, never with `E`; retract when the travel is at least
  `retract_min_travel` and something has been extruded; unretract before the next
  extrusion.
- Start: heat bed and nozzle, wait, `G21 G90 M82 G28 G92 E0 M107`. End: retract, fan,
  heaters off, lift 10 mm (within the bed height), `G28 X Y`, `M84`.
- Header comments carry the profile summary, filament length/weight and the time
  estimate (constant feed, no acceleration).

## Commands

`COMMANDS` in `crates/engine/src/lib.rs` is the single list: `import`, `place`,
`remove`, `arrange`, `profile`, `preview`, `slice`, `export-gcode`, `gcode-preview`
(Ready) and `supports` (Planned). `Command` is the request enum; `Command::id` names
its entry, and a test checks every variant maps to a Ready command. `Session::execute`
is the one entry point for the window, the CLI and MCP. Every mutation bumps the
session revision; a slice is served only for the revision it was taken at, and a
`SliceJob` result from an older revision is refused.

## The window

`ui-egui` is an eframe app (glow backend), split into modules:

| Module | What it holds |
|---|---|
| `app` | window state, actions shared by panels, the slicing thread, shortcuts, drag-and-drop |
| `menus` | menu bar (File, Edit, View, Model, Repair, Supports, Slice, Machine, Help) and the icon toolbar; built from `COMMANDS`, planned items disabled or explained |
| `panels` | prepare mode: model list (visibility, multi-select, context menu), profile presets and quick settings, slice button; placement panel (position, size and scale per axis, rotation, mirror, mesh check); preview mode: build summary, show toggles, colouring, playback and layer range |
| `viewport` | orbit, pan, zoom, picking, the select/move/rotate/scale tools, section view, overlays (legend with feature toggles, layer slider, nozzle position) |
| `scene` | CPU projection into egui meshes: bed, grid, volume, axes, shaded depth-sorted models, toolpaths with travel, retractions and nozzle, colouring by feature, speed or height |
| `dialogs` | profile editor rendered from the settings schema (tabs, sections, search, changed-only, planned tags, per-setting reset), preferences, mesh analysis, machine control (placeholder), shortcuts, feature status, about |
| `icons` | toolbar glyphs drawn in code |
| `camera` | orbit camera, view presets, fit to a box, ray to the bed plane |
| `config` | what is remembered between sessions: `preferences.txt` (preferences, recent projects) and `last.scprofile` in the per-user folder (`%APPDATA%\SliceCraft`, `~/Library/Application Support/SliceCraft`, `$XDG_CONFIG_HOME/slicecraft`; `SLICECRAFT_CONFIG_DIR` overrides) |

Every change to the document goes through `Session::execute`. Dragging a model and
editing the profile coalesce into one undo step per gesture.

## Settings schema

`crates/doc/src/settings.rs` holds `SETTINGS`: one row per setting with key, label,
tab, section, kind (float, int, bool, choice or text, with range and unit), default
and help. A row with a binding reads and writes a `PrintSettings` field the slicer
uses; a row without one is a placeholder stored in `PrintSettings::extra`.
Validation, the profile editor, profile files (`doc::profile`) and the "unused
settings" slice note all read this table. `Document::set_settings` keeps the bed in
step with the profile's machine settings.

## Projects

`io::project` reads and writes `.scproj`: one UTF-8 text file with a header, a
`[profile]` section in `.scprofile` form and one `[object]` block per model (name,
visibility, translate/rotate/scale, triangle count, mesh as base64 little-endian
`f64`). `f64` text uses Rust's shortest round-trip form, so open-then-save is
byte-identical. Reading checks the header, the triangle count against
`MAX_TRIANGLES` and the base64 length before decoding, every coordinate through
`Mesh::new` and every placement through `Transform::is_valid`; unknown keys and
sections are skipped with a warning.

The engine commands are `new-project`, `open-project` and `save-project`. The session
keeps the project path and the revision at the last save, so `is_modified` drives the
window title and the "save changes?" prompt before New, Open and Quit. Opening or
starting a project clears undo. The CLI's `slice` accepts a project in place of (or
with) models, and MCP has `open_project` and `save_project`.

## Support

`engine::support` runs after the layer regions and before the shells:

1. **Overhang:** per layer (in parallel), the area beyond the layer below plus
   `h · tan(angle)`.
2. **Joining:** each neighbouring pair of overhang strips is closed, so a slope's
   thin strips become one area.
3. **Columns:** stacked from the top down, cut by the model, and for "from the bed
   only" filtered from the bottom up. This is the one sequential pass, unions only.
4. **Pillars** (default style): per layer, the column area is snapped to grid cells
   fixed to the bed, a line width apart.
5. **Clearance and density:** per layer (in parallel), the horizontal and bottom
   gaps, an opening by half a line, and dense zones under the model, on the model
   and at the base. Each pillar or area is filled with one zigzag
   (`render::paths::zigzag`).

Paths are `PathKind::Support` and `PathKind::SupportInterface`. They are printed
first on each layer, at `support_speed_percent`. The first layer's support is inside
the skirt and brim. The design and its comparison with the reference are in
[research/05-support.md](research/05-support.md).

## Importing Simplify3D profiles

`io::fff` reads `.fff` profiles of both generations (plain values in 4.x, option lists
in 5.x) with `roxmltree`, with no DTD and a node limit. It converts mm/min to mm/s,
the automatic width to 1.2 × the nozzle, and the 4.x speed fractions to SliceCraft's
wall/outer-wall model. Every value goes through `PrintSettings::set`, so ranges hold.
It returns the profile and an `ImportReport`: settings the slicer uses, settings kept
as planned, notes, and elements with no counterpart. `Command::LoadProfile` recognises
the file by content, so the window, `--profile` and MCP all accept it. The
measurements behind each conversion are in [research/09-fff-profiles.md](research/09-fff-profiles.md).

## Agent control

`mcp::TOOLS` binds each tool to an engine command and `dispatch` runs it with string
arguments (`path`, `index`); a programmatic call never opens a dialog. Transport
(JSON-RPC over stdio) and the JSON control channel are phase P5. Until then,
`slicecraft-cli` (with `--json`) is the automation surface.

## Safety

Every production crate root carries
`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`;
the workspace forbids `unsafe_code`. Input-sized work has named caps
(`MAX_TRIANGLES`, `MAX_FACETS_DECLARED`, `MAX_OBJ_VERTICES`, `MAX_FILE_BYTES`,
`MAX_COORD`, `MAX_OBJECTS`, `MAX_SCALE`, `MAX_LAYERS`, `MAX_SCANLINES`,
`MAX_OPEN_CHAINS_TO_JOIN`, `MAX_PATHS_TO_ORDER`). Region operations use only
`i_overlay`'s fallible fixed-scale entry points, because its infallible float entry
points panic on out-of-range input.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | first | Scaffold measured: 7 crates, 2 apps, xtask; ASCII STL + cross sections + templated G-code writer |
| 2026-11-13 | MVP | Loops, regions, shells, solid/sparse infill, skirt, ordering, extrusion model, binary STL + OBJ, command API, egui window with 3D bed and toolpath preview |
| 2026-11-13 | UI | Settings schema (169 settings), profiles, undo/redo, XYZ transforms, mirror/duplicate, brim, more patterns and seams, scripts, offsets; full window: menus, toolbars, placement, profile editor, preview controls, analysis |
| 2026-11-13 | projects | `.scproj` project files, new/open/save/recent, unsaved-changes prompt, preferences and last profile remembered |
| 2026-11-13 | fff | Simplify3D `.fff` import with a report; heat-up before custom start scripts |
| 2026-11-13 | support | Region-based automatic support with interface, floor and base layers; `supports` command |
