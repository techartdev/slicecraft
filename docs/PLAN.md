# SliceCraft — program plan

> **Status:** MVP (2026-11-13): window, slicing pipeline, toolpath preview — see `ROADMAP.md`. **Approval to join the ArtCraft suite: pending.**
> **Target:** Simplify3D 5.1.2 behaviour, clean-room, pure Rust.
> **Reference:** observed black-box (the running application and the G-code it writes) and public documentation.

This document is the program plan. It says what we are building, in what order, why
the order is that one, what each phase proves, and what is still unknown. It is the
one document that changes when the plan changes; the architecture document describes
code that exists.

---

## 1. Where SliceCraft sits

The suite is a set of clean-room Rust applications that replace a commercial product
each, sharing conventions rather than code:

| App | Replaces | State |
|---|---|---|
| ArtCraft | AI image/video IDE | shipping |
| CADCraft | AutoCAD | pre-alpha |
| PhotoCraft, LightCraft, VectorCraft, FilmCraft, EffectCraft, DeckCraft, WordCraft, GridCraft, PdfCraft, FigCraft, DesignCraft, SoundCraft, MaterialCraft, TextureCraft, AniCraft | Photoshop, Lightroom, Illustrator, Premiere, After Effects, PowerPoint, Word, Excel, Acrobat, Figma, InDesign, a DAW, a material designer, a texture painter, a 2D animator | early |
| **SolidCraft, MailCraft** | — | **approved community apps** |
| **SliceCraft** | **Simplify3D** | **candidate: this repository** |

Membership is not ours to declare. Two community apps have been approved; SliceCraft
is not one of them yet. The plan therefore does two things at once:

1. Build the application so it is worth approving.
2. Build it to the suite's conventions so approval is a formality, not a rewrite.

### 1.1 What "joins the suite" concretely requires

Derived from what the approved and existing repositories actually contain — the
house rules in `storytold/craft-contrib`, the layout of `cadcraft`, `craft-anything`
and `craft-libs`, and the launcher/wiki expectations:

| Requirement | Where it comes from | SliceCraft now |
|---|---|---|
| House-rules blocks in `AGENTS.md` (core + rust) | `craft-contrib/standards/*` | present |
| Never-crash: no `unwrap/expect/panic/todo/unimplemented`, no `unsafe`; crate-root `deny` lists | rust house rules | present |
| Licence allowlist respected; `MIT OR Apache-2.0` | house rules | MIT text present, Apache text to add at first release |
| `ATTRIBUTION.md` for every non-code asset | house rules | present (empty, enforced later by xtask) |
| Layered crates with a swappable UI crate | cadcraft, craft-anything | present, checked by `cargo xtask layers` |
| Everything is a command; agent surface names commands | cadcraft | present |
| `ROADMAP.md` with measured parity and honest estimates | cadcraft | present, numbers marked as not yet measured |
| `docs/` set: architecture, parity, gaps, research | cadcraft, craft-anything | present |
| CI calling `storytold/craft-common-actions` | house rules | **not yet** — needs repo approval before workflows reference shared actions |
| Packaging per platform (Windows MSI, macOS dmg, Linux nfpm/Flatpak, FreeBSD) | cadcraft | **not yet** (phase P6) |
| App id `ai.storyteller.slicecraft`, icons in `assets/app-icon/`, wiki entries | cadcraft, craft-wiki | **not yet** |
| Community repo `storytold/slicecraft-corpus` for test models | house rule 3 (shared corpora) | **not yet** |
| `contributors/people.toml` entry | house rules | **not yet** |
| Reuse of `craft-libs` where a library already exists (`craft-gcode`, `craft-atomic`, `craft-settings`, `craft-ui`, `craft-quit`, `craft-platform`, `craft-docpath`) | craft-libs catalogue | **deliberately deferred** — see §7 |

Rule of thumb until approval: **nothing in this repository may assume a GitHub
remote, a CI runner, a release process or a package name that only the organisation
can grant.** Everything must build and test with `cargo` on one machine.

---

## 2. The product

### 2.1 What a slicer must do (ours)

| Capability | Detail | Phase |
|---|---|---|
| Import | STL (binary + ASCII), OBJ, 3MF; orientation, placement, scale, duplicate, arrange | P1 |
| Repair | non-manifold detection, hole filling, normal correction, degenerate facet removal | P2 |
| Profiles | material / quality / printer profiles, inheritance, overrides, FFF settings | P2 |
| Layers | layer height (incl. adaptive), first-layer height, Z offset, layer ranges | P1 |
| Perimeters | shells, order (inner/outer), seam placement, gap fill, thin walls | P3 |
| Infill | rectilinear, grid, triangles, concentric, gyroid-class; density, angle layering, overlap | P3 |
| Solid layers | top/bottom count, threshold angles, "solid at same Z" | P3 |
| Support | normal/tree, overhang angle, dense/base layers, interfaces, blockers/enforcers | P4 |
| Adhesion | skirt, brim, raft, prime pillar, ooze shield | P4 |
| Bridges | detection, speed/fan/squish modifiers, extra expansion | P4 |
| Temperature & cooling | per-layer/per-feature fan curves, slow layers, cooldown, standby | P4 |
| Retraction | distance/speed, wipe, coasting, z-hop, restart extra | P4 |
| Multi-tool | per-region extruder, tool offsets, prime/purge towers | P5 |
| Output | G-code for Marlin, RepRap, Klipper, Smoothie, GRBL-class; per-flavour templates; thumbnails | P5 |
| Preview | layer scrubber, toolpath view coloured by feature/speed/flow, time & material estimate | P5 |
| Printer control | USB serial, network (mDNS/HTTP) discovery and job upload | P6 |
| Agent surface | MCP tools over engine commands, JSON control channel, CLI | P5 |
| Scripting | batch slicing from the CLI, per-job parameter overrides | P5 |

### 2.2 What we do *not* do

- No copying of Simplify3D's profiles, firmware definitions, supports, or strings.
- No proprietary file formats (`.factory`) written or read in phase 1; our own
  project format is a documented JSON/ZIP container, decided in P2.
- No cloud service, no account, no telemetry.

---

## 3. Architecture

Mirrors CADCraft's proven shape, sized for a slicer:

```
L0  geom      f64 vectors, bounds, triangles, meshes, plane crossings, 2D regions
L1  doc       scene objects, bed, print settings + validation, project format
L2  render    cross sections, loop stitching, display lists, layer/toolpath geometry
L3  io        STL/OBJ/3MF import, G-code flavours + writer, project load/save
L4  engine    Session, commands, slicing pipeline, estimates
L5  ui-egui   the swappable front end (egui + wgpu canvas)
L5  mcp       agent surface, one tool per command
app slicecraft (desktop) · slicecraft-cli (headless) · slicecraft-web (later)
```

Rules: nothing below L5 depends on egui/eframe/winit/wgpu/rfd; every action is a
command; the UI is thin; errors are values; input is hostile.

**Slicing pipeline (the part that matters).** Each stage is a separate module with
its own tests, and each produces a documented intermediate so stages can be compared
against observed reference output without sharing code:

```
mesh ─▶ repair ─▶ transform to bed ─▶ per-Z slice plane
   ─▶ segment stitching ─▶ loop classification (outer / hole / nested)
   ─▶ shells (offset in, N times) ─▶ gap fill (thin walls)
   ─▶ solid infill regions (top/bottom via layer lookahead)
   ─▶ sparse infill (pattern generator ∩ region)
   ─▶ support regions (overhang projection, from the outside up)
   ─▶ per-layer path ordering + seam placement + travel minimisation
   ─▶ extrusion model (mm of filament per mm of path) ─▶ speed/fan/temperature schedule
   ─▶ G-code flavours (templates) ─▶ estimates (time, material)
```

---

## 4. Phases, deliverables, proof

Estimates are agent wall-clock hours at "presence quality" (the standard used by the
sibling repos), and they assume one working stream. They will be re-measured after
P1, when we know the real cost of geometry work in this codebase.

### P0 — Scaffold (done, 2026-11-13)

Deliverable: workspace, seven crates, command registry, ASCII STL import, plane
cross sections, minimal G-code writer, layering check, docs set.

Proof: `cargo xtask ci` green; 17 test binaries; `slicecraft-cli slice` produces a
readable G-code file from a cube.

### P1 — Import, plate, cross sections (≈ 12–20 h)

- Binary STL, OBJ (positions + faces), 3MF (ZIP + model XML) import with caps.
- Transform: translate, rotate Z, uniform scale; snapping to bed; arrange (grid + bounding-box shelf).
- Loop stitching: segments → closed loops, orientation fix, nesting (outer vs hole) via winding + containment.
- Adaptive layer heights (profile-driven), first layer height.
- `slicecraft-cli`: `info`, `slice`, `--json` output.
- Corpus: 30 synthetic models under `slicecraft-corpus` (later), 5 tiny ones inline for tests.

Proof: a cube, a torus and a hollow box slice into closed, correctly oriented loops;
areas computed analytically match within 1e-6 relative; no panics on truncated files.

### P2 — Profiles, project format, repair (≈ 25–40 h)

- Profile model: printer / material / quality with inheritance and overrides; validated on load.
- Our own project container (documented; `.slicecraft` = ZIP with `project.json` + meshes).
- Repair: degenerate removal, duplicate vertex weld, hole detection, normal fix, non-manifold report.
- Settings UI data model (no widgets yet): every setting has an id, unit, range, default and help text.

Proof: 100% of the reference profile surface is *named* (not copied) — a table of
ids we implement vs. a table of behaviours we observed; loading a project twice is
byte-identical (round-trip test).

### P3 — Perimeters, infill, solid layers (≈ 60–90 h)

- Offset loops inward (`geom::offset`), N shells, order, seam placement (aligned, random, shortest).
- Thin-wall handling, gap fill, overlap percentage.
- Sparse infill patterns: rectilinear, grid, triangles, concentric, gyroid-class; density, alternating angle, region clipping.
- Solid top/bottom from layer lookahead with angle threshold; "dense" layers.
- Extrusion model: flow = f(width, height, filament diameter); multipliers.

Proof: for a set of primitives, per-layer path length and material use are within a
documented tolerance of an analytic expectation; a synthetic 20 mm cube's total
filament length is within ±3% of a hand-computed value.

### P4 — Support, adhesion, cooling, retraction (≈ 60–90 h)

- Overhang detection, support region projection, dense base layers, interface layers, blockers/enforcers.
- Skirt/brim/raft, prime pillar, ooze shield.
- Bridge detection and modifiers.
- Fan curves, slow layers, cooldown/standby, per-feature temperatures.
- Retraction, wipe, coasting, z-hop, extra restart.

Proof: support never crosses the model where it must not (geometry test); a
temperature/fan schedule matches the profile it was generated from for every layer.

### P5 — Output, preview, agent surface (≈ 50–80 h)

- G-code flavours with our own template table; thumbnails; per-flavour header/footer.
- Toolpath preview: per-feature colouring, layer scrubber, stats.
- Time and material estimates calibrated against real prints.
- MCP server + JSON control channel + `slicecraft-cli slice --json`.
- Multi-tool: per-region assignment, tool offsets, prime tower.

Proof: every tool in `slicecraft-mcp` names an engine command (tested); the same job
driven by CLI, control channel and MCP produces identical G-code (hash test).

### P6 — Desktop UI, packaging, platform (≈ 60–100 h)

- egui/eframe shell, wgpu canvas, panels (objects, settings, preview, console), theme tokens from `craft-ui`.
- Native menus, dialogs, reduced-motion handling, space-mouse if cheap.
- Packaging: Windows MSI, macOS dmg, Linux nfpm + Flatpak, FreeBSD; app id `ai.storyteller.slicecraft`.
- Serial + network printer control.
- Wasm build of the UI (preview only, no serial).

Proof: installs and slices on Windows and Linux from the packaged build; UI smoke
test drives a slice through the control channel and reads a screenshot.

**Total to a usable pre-alpha:** ≈ 150–230 h (P0–P3). **To beta:** ≈ 260–420 h.

---

## 5. Research program

Research is how we replace guessing with evidence. Every research note has the same
shape: question, method, evidence (observations and measurements), inference, what it
changes in our design, open questions.

| # | Question | Method | Status |
|---|---|---|---|
| R2 | What is the profile surface — which settings exist, grouped how? | Public documentation, the running application's settings windows, and `.fff` profile files | **done** → `docs/research/02-feature-inventory.md` |
| R3 | How does the reference place seams, order paths, and minimise travel? | G-code analysis of primitives (cylinder, cube, torus) for seam/order/travel patterns | open |
| R4 | What exactly do its infill patterns look like geometrically? | Slice known shapes at known angles, measure spacing/anchoring per pattern | open |
| R5 | How are supports projected, dilated and interfaced? | Slice overhang fixtures, measure offsets and layer counts | open |
| R6 | How does the extrusion model respond to width/height/speed? | Sweep parameters, solve the relation from emitted E values | open |
| R7 | What are the flavour differences (header, start/end, commands)? | Diff G-code for the same job across flavour settings | open |
| R8 | How is time/material estimated? | Compare its estimates to computed path integrals | open |

Method rules (clean room): we record **behaviour**, **structure** and **numbers we
measured**. We never decompile or disassemble its program, copy its user-visible text, or
reuse its profile files.

---

## 6. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Approval does not come | The suite's shared CI, packaging and wiki work is wasted effort | Build standalone-first; only reference shared actions once a remote exists (§1.1) |
| Geometry work is underestimated | P3 slips badly — offsets and clipping are the hard part | Timebox spikes early: an offset+clip spike is the first task of P3; if it slips, fall back to a simpler shell algorithm and record the debt in `gaps.md` |
| Clean-room boundary is crossed by accident | The most serious possible failure for the suite | Research notes carry no code; only behaviour tables. Review each research note for copied strings before commit |
| Reference behaviour differs by printer/firmware | "Parity" becomes undefined | Parity is defined against *our documented* profile defaults plus observed output on a fixed synthetic corpus |
| G-code flavours explode | Endless small differences | One template table, one test per flavour, no per-flavour code paths in the pipeline |
| Real-print validation is unavailable | Estimates and support quality unverifiable | Estimates are labelled "uncalibrated" until prints exist; support quality is geometric, not print-based, until then |
| Rust 1.93 vs suite's 1.95 | CI breakage at join time | `rust-version = "1.93"` now; bump at join time in one commit |

---

## 7. Deliberate deferrals

- **`craft-libs` reuse.** `craft-gcode` (Grbl-style writer), `craft-atomic`,
  `craft-settings`, `craft-ui`, `craft-quit`, `craft-platform`, `craft-docpath`
  exist and should replace our local equivalents *at join time*. Until then, adding
  a git dependency on a suite repository implies a relation that is not approved yet.
  Our writer is written so the swap is mechanical: `GcodeWriter` is the only place
  that formats a move.
- **Corpus repo.** Synthetic models stay in `tests/` (small, original). A shared
  corpus moves to `storytold/slicecraft-corpus` after approval.
- **CI.** `cargo xtask ci` is the CI until a remote can call shared workflows.
- **Fonts.** No fonts are needed for a slicer UI beyond the shared ones; if that
  changes, they go to `craft-fonts`, never here.
- **Web build.** `slicecraft-web` is created in P6, not now, so the wasm gate does
  not exist before there is something to gate.

---

## 8. Definition of done for the join

A single checklist that turns "we would like to join" into "this is ready to review":

- [ ] `cargo xtask ci` green on Windows and Linux from a clean clone.
- [ ] No `unwrap/expect/panic/todo/unimplemented`/`unsafe` in non-test code (checked by
      the crate-root deny lists plus a grep in `xtask ci`).
- [ ] House-rules blocks current against `craft-contrib` (sha recorded in the markers).
- [ ] `ATTRIBUTION.md` complete and enforced by `cargo xtask assets`.
- [ ] App id, icons, desktop/metainfo/plist/wxs files present for every platform.
- [ ] `docs/architecture.md`, `target-app-parity.md`, `gaps.md`, `ROADMAP.md` measured,
      not estimated, at least for the scaffold's own numbers.
- [ ] Three end-to-end workflows pass on the packaged build: import → slice → save,
      profile change → re-slice → compare, CLI slice of a corpus model.
- [ ] One research note per implemented subsystem, each with an evidence/inference split.
- [ ] Contributor entry in `contributors/people.toml`, added with the person's consent.
- [ ] No secrets, no personal paths, no machine-specific notes anywhere in the tree.

---

## 9. Open questions for the operator

1. **Approve a corpus of reference G-code?** Comparing our output to observed
   output needs a place to keep their G-code; it is *our* record of *observed
   behaviour*, not their asset. Storing it under `docs/research/` as data would
   make parity checkable and keep the clean room honest. Confirm the line.
2. **Which firmware flavours matter first?** Marlin alone is the safe default;
   Klipper and RepRap are cheap to add; Smoothie/GRBL differ enough to be phase 5.
3. **Is a real printer available for calibration?** Time and material estimates and
   support quality cannot be validated without one.
4. **Does the suite want `slicecraft-web`?** Preview in a browser is cheap once the
   UI crate is wasm-clean; serial control never will be.

---

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | first | Program plan after scaffold |
| 2026-11-13 | MVP | Window + real pipeline landed ahead of the phase order (P1/P3 mostly, slices of P4–P6); licence exceptions for the egui stack recorded in `ATTRIBUTION.md`; state in `ROADMAP.md` |
