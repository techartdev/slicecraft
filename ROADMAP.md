# SliceCraft roadmap

**Stage: MVP + full UI + projects + support** · next: placeholders in order of use · membership
of the ArtCraft suite: **pending approval**

> **Last reviewed:** 2026-11-13 · **Change:** support pillars, retraction only when
> crossing outlines, support preview and add/block boxes ·
> **Target:** Simplify3D 5.1.2 behaviour, clean room

SliceCraft is a slicer: import a model, put it on a bed, choose a profile, slice it
into layers, generate perimeters, infill and support, and write G-code for a real
printer. It targets Simplify3D's feature set and adds what Simplify3D does not have:
agent control over MCP, a headless CLI, a Linux/BSD build and a free licence.

**Why MVP:** the window opens, imports STL/OBJ (drop or dialog), places and arranges
models on a 3D bed, slices them on a worker thread into perimeters, solid and sparse
infill and a skirt, previews the toolpaths layer by layer and exports G-code with a
real extrusion model and retraction. Support, adhesion beyond a skirt, saved profiles
and printer control are not there yet, and nothing has been printed — see
[gaps.md](docs/gaps.md).

## Headline numbers

| Number | Value | Kind |
|---|---|---|
| Phases complete | **P0**; P1, P3 most of the way; slices of P4–P6 | measured against the phase table |
| Crates | 7 libraries, 2 apps, 1 tool | measured |
| Lines of Rust | ≈ 17,300 | measured (`cargo xtask stats`) |
| Tests | 158 in 17 test binaries, all green | measured (`cargo test --workspace`) |
| Commands | 48 (25 ready, 23 planned) | measured (`engine::COMMANDS`) |
| Profile settings | 170 (77 used by the slicer, 93 placeholders) | measured (`doc::settings::SETTINGS`) |
| Crown with support (350 layers, release) | sliced in ≈ 1.5 s; support filament 676 mm vs the reference's 682 mm; 4927 retractions vs 4384 | measured on one Windows machine |
| Direct external dependencies | 4 (`i_overlay`, `roxmltree`; `eframe`, `rfd` in the window only) | measured |
| Simplify3D profiles imported | 16 of 16 operator files (4.0 and 5.1.2) | measured (`io::fff::tests::corpus_profiles_import`, corpus outside the repo) |
| 20 mm cube, defaults | 99 layers, 1.17 m filament, ≈ 14 min (no acceleration) | measured, uncalibrated |
| Filament vs an independent area estimate | within ±10% on the 20 mm cube | measured (`engine::tests::cube_filament_matches_hand_estimate`) |
| 20k-triangle torus, release build | sliced in ≈ 0.4 s | measured on one Windows machine |
| Feature parity with the target | **not measured as a number** — every capability and its status is in [research/02-feature-inventory.md](docs/research/02-feature-inventory.md) | — |
| Remaining to beta (P2–P5) | ≈ 180–270 h | estimated |
| Remaining to 1.0 (P2–P6) | ≈ 240–370 h | estimated |

Estimates use the sibling repositories' calibration (2–5 h per moderate feature,
15–100 h per subsystem). Support quality, real-print calibration and robustness on
damaged meshes dominate the uncertainty.

## Phases

| Phase | What lands | State | Proof it landed |
|---|---|---|---|
| **P0** Scaffold | workspace, crates, command registry, ASCII STL, cross sections, G-code writer, layering check, docs | **done** | `cargo xtask ci` green |
| **P1** Import & sections | binary STL ✓, OBJ ✓, 3MF ✗; transforms ✓; arrange ✓ (rows); loop stitching ✓; nesting ✓; adaptive heights ✗; CLI `--json` ✓ | **mostly done** | cube, hollow box and octahedron-through-vertices slice into closed oriented loops with exact areas; every prefix of a binary STL is read or refused without a panic |
| **P2** Profiles & project | settings table ✓ (no inheritance); profile files ✓; Simplify3D profile import ✓; project container ✓; printer/material libraries ✗; mesh repair ✗ | **started** | `io::project::tests::project_round_trip_is_byte_identical`, `engine::tests::projects_save_open_and_track_unsaved_changes` |
| **P3** Shells & infill | shells ✓, inside-out order ✓, nearest seams (partial), thin walls ✗, gap fill ✗, rectilinear infill ✓ (other patterns ✗), solid top/bottom ✓, extrusion model ✓ | **mostly done** | shell lengths exact on a square; cube filament within ±10% of an area estimate |
| **P4** Support & control | automatic support ✓ pillars and areas, preview ✓, add/block boxes ✓; skirt ✓ brim ✓ (raft/pillar/shield ✗); bridges ✗; fan from a layer ✓; retraction ✓ (wipe/coast/z-hop ✗) | started | — |
| **P5** Output & agents | flavours named (one command set); toolpath preview ✓; estimates (uncalibrated) ✓; MCP dispatch ✓ (no transport); multi-tool ✗ | started | — |
| **P6** UI & packaging | egui window ✓, 3D bed view ✓, panels ✓, file dialogs ✓; packaging ✗; printer I/O ✗; wasm ✗ | started | window driven and screenshotted on Windows 11 |

## What is next (concrete)

1. **Turn placeholders into features in order of use** (island-by-island ordering next) (from R2 §3): adaptive layers,
   gyroid infill, raft, thin walls/gap fill, combing, coast/wipe, schedules.
2. **Several profiles per job and height zones.**
3. **3MF import and model export.**
4. **Real-print check** of the 20 mm cube and a torus; record results as measured.
5. **Upstream the licence exceptions** recorded in [ATTRIBUTION.md](ATTRIBUTION.md).

## By dimension

| Dimension | State | Doc |
|---|---|---|
| Features | MVP: import, place, slice (shells, infill, skirt), preview, export | [gaps.md](docs/gaps.md) |
| Geometry | stitched loops, regions with holes, offsets and booleans on a 1 µm grid | [architecture.md](docs/architecture.md) |
| Output | Marlin-style G-code with area-based extrusion, retraction, fan, estimates | [gaps.md](docs/gaps.md) |
| Estimates | constant-feed time, filament length and PLA weight; uncalibrated | [gaps.md](docs/gaps.md) |
| Stability | 158 tests, no fuzzing; worker failures become errors; hostile project files refused | [gaps.md](docs/gaps.md) |
| Platforms | Windows 11 only (built and run) | [gaps.md](docs/gaps.md) |
| Ecosystem | CLI with `--json`; MCP dispatch without transport | [gaps.md](docs/gaps.md) |
| Suite integration | house rules and layout present; licence exceptions to propose; CI, packaging, corpus, `craft-libs` deferred | [PLAN.md](docs/PLAN.md) §1.1, §7 |

## Non-goals until after 1.0

- Cloud slicing, accounts, telemetry.
- Proprietary project formats.
- Any code, profile, string or asset copied from another slicer.
