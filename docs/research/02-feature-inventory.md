# R2 — Feature inventory of the reference slicer

> **Research note 2.** Question: *what does Simplify3D 5.1.x let a person do,
> and where does SliceCraft stand on each item?* Method: public documentation and
> independent reviews, and the running application observed by the operator
> (screenshots of its windows). **This note copies no third-party text, labels,
> setting names, defaults, profiles or icons.** Every capability is described in
> our own words.

---

## 1. Method

| Source | What it gave | Grade |
|---|---|---|
| Public: vendor release notes 5.0–5.1.2, "what's new" pages for 4.0/5.0/5.1, feature and specification pages, knowledge-base articles (import, mesh repair, variable settings, multi-extrusion, rafts/skirts/brims, supports, thin walls, multi-part, first layer, G-code), the print-quality troubleshooting guide, a quick-start guide and a keyboard-shortcut sheet | the user-facing feature set, by release | direct (public) |
| Public, third party: user settings sheets for V5, a hands-on review, forum excerpts | V5 dialog layout and settings groups where the vendor does not document them | third-party |
| Operator screenshots of 5.1.2 (models panel, transforms panel, preview mode, process dialog) | layout of the main window, preview summary and process dialog | direct (observed) |

## 2. What the evidence says about the shape of the application

**[direct]** The main window is a three-column workspace: a model list with
import/remove/arrange, a list of *processes* (named settings sets, several per job,
each bound to chosen models), a 3D bed view with a view toolbar and a tool toolbar,
and a docked transform panel. Slicing switches to a preview mode whose left column is
a build summary (time, length, volume, weight, cost, build size, layer/model counts)
with show-toggles and a colouring choice, and whose bottom bar has playback and a
two-ended layer (or line) range.

**[direct]** The settings dialog has 12 tabs and a search box; settings are typed
(bool, int, float, choice, text, and per-layer setpoint lists), carry ranges and
units, and are *replicated* per extruder, per temperature controller and per fan.
Many checkboxes enable a group of dependent settings.

**[design]** SliceCraft keeps its settings as data: one schema drives the profile
editor, profile files and validation (`crates/doc/src/settings.rs`).

## 3. Inventory and SliceCraft status

Status: **done** = works and is tested · **partial** = works with stated limits ·
**placeholder** = present in the UI or profile, marked planned, does nothing yet ·
**—** = not started. Phase = `docs/PLAN.md` phase that closes it.

### 3.1 Models and the build plate

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Import STL / OBJ | public | done | P1 |
| Import 3MF | public | done (ZIP64, units, components, one model per build item) | P1 |
| Drag-and-drop import | public | done | P1 |
| Preview an existing G-code file | public | placeholder | P5 |
| Export models (STL/OBJ/3MF) | public | done | P2 |
| Project file holding models, placement, settings, supports | public | done (models, placement, visibility, profile; supports when they exist) | P2 |
| Recent projects | public | done | P2 |
| Ask before dropping unsaved changes | public | done (new, open, quit) | P2 |
| Reload a model when its file changes | public | — | P6 |
| Detect and correct inch/cm models | public | partial (manual unit correction) | P1 |
| Automatic actions on every import (rotate/scale/offset) | public | placeholder (preferences) | P6 |
| Select, move, rotate, scale tools; multi-select | public | done | P1 |
| Exact position/rotation/scale per axis, uniform toggle | public, screenshots | done | P1 |
| Rotate in 90° steps | public | done | P1 |
| Arrange all models on the bed | public | done (row packing) | P1 |
| Drop to bed | public | done (always on the bed) | P1 |
| Lay a face flat on the bed | public | placeholder | P2 |
| Mirror X/Y/Z | public | done | P1 |
| Copy/duplicate | public | done | P1 |
| Group / ungroup / combine / separate parts | public | placeholder | P2 |
| Scale to the largest size that fits | observed | done | P1 |
| Model role: print / support / cutter | observed | placeholder | P4 |
| Out-of-bounds warning | public | done | P1 |
| Undo / redo | public | done | P1 |
| Visibility per model | public | done (also excludes from slicing) | P1 |

### 3.2 Mesh analysis and repair

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Mesh statistics (volume, area, errors) | public | done (`Mesh::report`) | P2 |
| Find non-manifold edges / open edges | public | done (counts) | P2 |
| Find self-intersections | public | placeholder | P2 |
| Repair flipped faces / flip a whole surface | public | placeholder | P2 |
| Remove duplicate and orphaned faces | public | placeholder | P2 |
| Reduce triangle count | public | placeholder | P2 |
| Repair during slicing: close open outlines, choose how overlaps fill | public | partial (closes gaps ≤ 1 mm, non-zero fill); the choices are placeholders | P2 |
| Cross-section view | public | partial (horizontal cut of the model view) | P2 |
| Display modes: wireframe, normals, points | public | placeholder | P6 |

### 3.3 Processes, profiles and assistants

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Named profile; presets | public | done (4 built-in presets) | P2 |
| Save / load profile files | public | done (own text format) | P2 |
| Import Simplify3D `.fff` profiles (4.x and 5.x) | operator request | done, with an import report ([R9](09-fff-profiles.md)) | P2 |
| Search settings by name; show changed settings | public | done | P2 |
| Revert a setting to its default | public | done | P2 |
| Compare two profiles | public | — | P2 |
| Several profiles per job, each for chosen models | public | placeholder | P5 |
| Profiles switching at heights (variable settings, start/stop Z) | public | placeholder | P5 |
| Sequential printing (one object at a time) with clearance | observed | — | P5 |
| Quick-apply groups (material, quality) | public | partial (quick panel + presets) | P2 |
| Printer setup assistant and printer database | public | placeholder | P6 |
| Pre-slice checks with warnings | public | partial (validation + slice notes) | P2 |

### 3.4 Settings, by tab (counts are SliceCraft's schema)

| Tab (ours) | Reference covers | SliceCraft: settings / used by the slicer |
|---|---|---|
| Extruder | nozzle, width (auto/manual), flow, retraction, restart extra, Z lift, coast, wipe, tool change | 18 / 10 |
| Layers | layer height, first-layer height/width/speed, adaptive heights, size compensation, walls, solid layers, seam strategy, wall and island order, vase mode | 18 / 9 |
| Adhesion | skirt/brim, raft, prime tower, ooze shield (with shapes and auto-stop) | 19 / 6 |
| Infill | ~11 sparse patterns, rotation, width, combine layers, overlap, minimum length, dense layers, solid pattern and rotation, small-area solid, expansion, ordered (monotonic) lines, top-surface flow/speed/fan, ironing, reinforcement layers | 24 / 7 |
| Support | placement, overhang angle, pattern and density, walls, base and dense layers, expansion, clearances, every-N layers, manual editing | 16 / 0 |
| Temperature | several controllers (nozzle, bed, chamber), per-layer schedules, idle cool-down | 9 / 5 |
| Cooling | several fans, per-layer schedules, kick-start, quick-layer boost, bridge and top fan | 8 / 2 |
| Speeds | default and per-feature percentages, travel XY/Z, acceleration and jerk for estimates, slow-downs for quick layers, short walls and flow limits | 12 / 7 |
| G-code | output formats, firmware options, relative E, thumbnails, coordinate offsets, toolhead offsets | 11 / 4 |
| Scripts | start, end, layer change, retraction, tool change, profile change, post-processing, variables and conditionals | 6 / 3 |
| Machine | build volume (box or cylinder), origin, homing, tools | 9 / 3 |
| Material | filament diameter, density, price | 3 / 3 |
| Advanced | thin walls, single-line walls, retraction rules, wipe scope, travel detours, slicing repair modes | 16 / 0 |
| **Total** | — | **169 / 59** |

The reference's settings are grouped differently in places (bridging and filament
properties sit on an "other" tab; machine geometry on the output tab); SliceCraft's
grouping is its own.

### 3.5 Slicing features

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Perimeters, inside-out or outside-in | public | done | P3 |
| Seam: nearest / aligned to a point / scattered; preferred regions | public | partial (no preferred regions) | P3 |
| Solid top/bottom layers | public | done | P3 |
| Sparse patterns: rectilinear, one-direction lines, grid, triangles, concentric | public | done | P3 |
| Sparse patterns: honeycomb, gyroid, wiggle, cubic, octahedral | public | done (`render::patterns`, clipped polylines; one honeycomb kind) | P3 |
| Solid patterns: rectilinear, one-direction, concentric | public | done | P3 |
| Infill before walls | public | done | P3 |
| Skirt (several loops, several layers), brim | public | done | P4 |
| Raft | public | done (base and top layers, margin, air gap) | P4 |
| Prime tower, ooze shield | public | placeholder (needs several tools) | P5 |
| Thin walls, gap fill, single-line walls | public | done (constant line width) | P3 |
| Adaptive layer heights | public | placeholder | P3 |
| Dimensional (hole/outer) compensation | public | placeholder | P3 |
| Bridging detection and modifiers | public | placeholder | P4 |
| Ironing, ordered top lines, top-surface modifiers | public | placeholder | P4 |
| Automatic support | public | done: pillars (default) or areas, own design ([R5](05-support.md)) | P4 |
| Retract only when crossing outlines | public | done (on by default) | P4 |
| Avoid crossing outlines (combing) with a detour limit | public | done (on by default) | P4 |
| Island order: nearest, consistent, all at once | public | done (nearest by default) | P3 |
| Support preview before slicing | public | done (background, pillars drawn as prisms) | P4 |
| Manual support add/remove, blockers | public | done as boxes on the bed (add: every downward surface inside; block: none); not per pillar | P4 |
| Retraction with Z lift, first-layer temperatures, wait-for-heat, scripts | public | done | P4 |
| Coast, wipe, extra restart, retraction rules | public | placeholder | P4 |
| Fan from a layer | public | done | P4 |
| Per-layer temperature and fan schedules; slow-downs for quick layers | public | placeholder | P4 |
| Multi-extrusion (per-feature tools, offsets, tool-change sequences, wizard) | public | placeholder | P5 |
| Coordinate offsets in output | public | done | P5 |

### 3.6 Preview and analysis

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Preview by layer, layer range, only-last-N | public | done | P5 |
| Preview by move (line), playback with speed | public | done | P5 |
| Colour by feature type | public | done | P5 |
| Colour by speed, by layer height/Z | public | done | P5 |
| Colour by tool, by profile, by flow rate | public | placeholder | P5 |
| Show travel, retractions, nozzle with position | public | done | P5 |
| Feature toggles in the legend | public | done | P5 |
| Build summary: time, length, volume, weight, cost, size, counts | public, screenshots | done | P5 |
| Per-feature breakdown | public | partial (extruded length by feature) | P5 |
| Acceleration-aware time estimate | public | placeholder | P5 |
| Live preview following a running print | public | — | P6 |

### 3.7 Printing and machine control

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Export G-code to disk | public | done | P5 |
| Printer-specific binary formats, thumbnails | public | placeholder | P5 |
| USB printing, network printing (several firmware families), discovery | public | placeholder | P6 |
| Control panel: connect, jog, home, temperatures, fan, terminal, pause/stop | public | placeholder window | P6 |
| Several printers at once | public | — | P6 |
| Bed levelling assistant | public | — | P6 |

### 3.8 Application

| Capability | Evidence | SliceCraft | Phase |
|---|---|---|---|
| Menus: file, edit, view, model, repair, supports, slice, machine, help | observed | done (our own grouping) | P6 |
| View presets (7 directions), fit all / fit selection | public | done | P6 |
| Perspective / orthographic | observed | placeholder | P6 |
| Toggle grid, build volume, axes | observed | done | P6 |
| Light/dark theme | public | done | P6 |
| Camera options: invert zoom, orbit speed | observed | done | P6 |
| Camera schemes of other CAD tools, 3D mouse | public | — | P6 |
| Keyboard shortcuts list; shortcut editor | public | partial (list only) | P6 |
| Preferences that persist | public | done (preferences, recent projects, last profile) | P6 |
| Language selection | public | placeholder | P6 |
| Workspace tabs | observed | — | P6 |
| Agent control (CLI, MCP) | absent in the reference | partial (CLI done, MCP dispatch done, no transport) | P5 |

## 4. What this changes in SliceCraft

1. **The settings schema is the single source** for the editor, profile files and
   validation, with an explicit "used by the slicer" flag per setting. Placeholders
   are saved and shown but never pretend to work; a slice lists changed placeholders
   in its notes.
2. **Planned commands are first-class.** `COMMANDS` lists every capability above
   with a Ready/Planned state; menus are built from it, and a planned item opens an
   explanation instead of doing nothing.
3. **The window follows the workflow the evidence shows** (prepare → slice →
   preview → export) with our own layout, wording, icons (drawn in code) and colours.

## 5. Open questions

1. Which sparse patterns matter enough to build first after rectilinear/grid/
   triangles? (Gyroid is the most requested in reviews.)
2. Support: pillar-grid (as the reference) or a region-based design? Answered: region-based, pillars possible later ([R5](05-support.md)).
3. Several profiles per job: per-model, per-height, or both from the start?
4. Should SliceCraft read the reference's project or profile files for migration?
   Profiles: **yes**, decided by the operator on 2026-11-13 and done ([R9](09-fff-profiles.md)).
   Project files (`.factory`): still open.
5. Preferences persistence: answered for now with two text files in the platform's
   per-user folder (`ui-egui::config`). Move to `craft-platform` / `craft-docpath`
   when SliceCraft joins the suite.

## 6. Clean-room statement

This note records what features exist and where SliceCraft stands. It contains no
third-party code, no setting identifiers, labels, defaults or ranges, no profile
data and no icons. SliceCraft's keys, labels, defaults, ranges and help text are its
own and live only in `crates/doc/src/settings.rs`.
