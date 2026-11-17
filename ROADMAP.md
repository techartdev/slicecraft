# SliceCraft roadmap

**Stage: feature-complete beta candidate, not yet test-printed** · next: real prints and calibration · membership
of the ArtCraft suite: **pending approval**

> **Last reviewed:** 2026-11-13 · **Change:** every planned command and all but one
> setting implemented: model roles, groups, variable layers, network printing, camera,
> setup assistant, MCP over stdio, multi-tool, the remaining settings ·
> **Target:** Simplify3D 5.1.2 behaviour, clean room

SliceCraft is a slicer: import a model, put it on a bed, choose a profile, slice it
into layers, generate perimeters, infill and support, and write G-code for a real
printer. It targets Simplify3D's feature set and adds what Simplify3D does not have:
agent control over MCP, a headless CLI, a Linux/BSD build and a free licence.

**Why beta candidate:** every command the menus list works (49 of 49) and all 172
profile settings drive the slicer. The window speaks English and Bulgarian. Import (STL, OBJ, 3MF), placement, repair and mesh tools, profiles (with
Simplify3D `.fff` import), setting zones, support with editing, all adhesion types,
ten infill patterns, multi-tool output, preview, USB and network printing, and an
MCP server for agents are in. Nothing has been printed yet: until real prints confirm
the extrusion model, the estimates and support removal, this is not a release — see
[gaps.md](docs/gaps.md).

## Headline numbers

| Number | Value | Kind |
|---|---|---|
| Phases complete | P0–P5 in code; P6 with Windows, macOS and Linux packaging | measured against the phase table |
| Crates | 7 libraries, 2 apps, 1 tool | measured |
| Lines of Rust | ≈ 28,000 | measured (`cargo xtask stats`) |
| Tests | 281, all green | measured (`cargo test --workspace`) |
| Commands | 50, all ready | measured (`engine::COMMANDS`) |
| Profile settings | 172, all used by the slicer | measured (`doc::settings::SETTINGS`) |
| Crown with support (350 layers, release) | sliced in ≈ 1.5 s; support filament 676 mm vs the reference's 682 mm; 2606 retractions vs 4384 | measured on one Windows machine |
| Direct external dependencies | 9 (`i_overlay`, `roxmltree`, `miniz_oxide`, `serial2`; `eframe` with `wgpu`, `rfd`, `zune-jpeg`, `skrifa` in the window only) | measured |
| Simplify3D profiles imported | 16 of 16 operator files (4.0 and 5.1.2) | measured (`io::fff::tests::corpus_profiles_import`, corpus outside the repo) |
| 20 mm cube, defaults | 99 layers, 1.17 m filament, ≈ 14 min (no acceleration) | measured, uncalibrated |
| Filament vs an independent area estimate | within ±10% on the 20 mm cube | measured (`engine::tests::cube_filament_matches_hand_estimate`) |
| 20k-triangle torus, release build | sliced in ≈ 0.4 s | measured on one Windows machine |
| Side by side with Simplify3D, same settings | 10 real jobs: layers equal, filament −3.6 % to +1.9 %, 4–13 % less time by one estimator, retractions −8 % to +53 % (median +4 %) ([R10](docs/research/10-s3d-comparison.md)) | measured, not printed |
| Feature parity with the target | every capability and its status is in [research/02-feature-inventory.md](docs/research/02-feature-inventory.md) | — |
| Remaining to beta | real-print calibration and fixes: ≈ 20–60 h | estimated |
| Remaining to 1.0 | packaging, wasm, fuzzing, UI polish: ≈ 80–160 h | estimated |

Estimates use the sibling repositories' calibration (2–5 h per moderate feature,
15–100 h per subsystem). Support quality, real-print calibration and robustness on
damaged meshes dominate the uncertainty.

## Phases

| Phase | What lands | State | Proof it landed |
|---|---|---|---|
| **P0** Scaffold | workspace, crates, command registry, ASCII STL, cross sections, G-code writer, layering check, docs | **done** | `cargo xtask ci` green |
| **P1** Import & sections | STL ✓, OBJ ✓, 3MF ✓ (ZIP64); transforms ✓; arrange ✓ (rows); stitching ✓ with repair modes; adaptive and zone-driven layer heights ✓; CLI `--json` ✓ | **done** | every prefix of a binary STL is read or refused without a panic; 24 of 24 operator 3MF files import |
| **P2** Profiles & project | settings table ✓; profile files ✓; Simplify3D import ✓; projects ✓; setup assistant ✓; mesh repair, split, merge, cut, hollow, reduce ✓; printer/material libraries ✗ | **done** (libraries later) | `io::project` round trips, 16 of 16 operator `.fff` files import and slice |
| **P3** Shells & infill | shells ✓, seams ✓, thin walls and gap fill ✓, ten infill patterns ✓, solid top/bottom, bridges, ironing, reinforcement ✓, extrusion model ✓ (matches the reference's outer-wall filament within 0.1%) | **done** | crown outer perimeter 1641 vs 1642 mm |
| **P4** Support & control | support pillars and areas ✓, preview ✓, add/block boxes ✓; skirt, brim, raft ✓; retraction rules, coast, wipe, combing ✓; schedules ✓ | **done** | crown support filament 676 vs 682 mm |
| **P5** Output & agents | three flavours, arcs, thumbnails ✓; preview ✓; turn-weighted estimates ✓; MCP over stdio ✓ (32 tools); multi-tool with prime tower and ooze shield ✓ (synthetic tests) | **done** | `mcp::server` tests and a scripted MCP session slicing a 190k-triangle model |
| **P6** UI & packaging | egui window ✓, 3D bed view ✓, panels ✓, dialogs ✓, visual overhaul (theme, cards, floating view and tool bars, settings editor with a tab list, machine panel with a jog pad, light theme) ✓, GPU 3D view (wgpu: lit beads, depth, 4× anti-aliasing, CPU fallback) ✓; USB and network printing ✓, camera ✓; packaging: Windows zip and installer ✓ (`cargo xtask dist`), other platforms ✗; wasm ✗ | started | window driven and screenshotted on Windows 11, dark, light, English and Bulgarian |

## What is next (concrete)

1. **Real-print check** of the 20 mm cube, the crown with support and a two-tool part;
   record filament, time and support removal as measured.
2. **Calibrate the time estimate** against the printer's own report.
3. **Packaging** for Linux and macOS (builds, packages) and the **wasm build**.
4. **Fuzz** the STL, 3MF, project, profile and G-code readers.
5. **Upstream the licence exceptions** recorded in [ATTRIBUTION.md](ATTRIBUTION.md).

## By dimension

| Dimension | State | Doc |
|---|---|---|
| Features | every listed command; see the inventory for limits | [gaps.md](docs/gaps.md) |
| Geometry | stitched loops, regions with holes, offsets and booleans on a 1 µm grid | [architecture.md](docs/architecture.md) |
| Output | Marlin, RepRapFirmware and Klipper G-code; rectangle-bead extrusion; multi-tool | [gaps.md](docs/gaps.md) |
| Estimates | turn-weighted acceleration time, filament, weight, cost; uncalibrated | [gaps.md](docs/gaps.md) |
| Stability | 236 tests, no fuzzing; worker failures become errors; hostile files refused | [gaps.md](docs/gaps.md) |
| Platforms | Windows 11 only (built and run) | [gaps.md](docs/gaps.md) |
| Ecosystem | CLI with `--json`; MCP server over stdio (`slicecraft-cli mcp`) | [gaps.md](docs/gaps.md) |
| Suite integration | house rules and layout present; licence exceptions to propose; CI, packaging, corpus, `craft-libs` deferred | [PLAN.md](docs/PLAN.md) §1.1, §7 |

## Non-goals until after 1.0

- Cloud slicing, accounts, telemetry.
- Proprietary project formats.
- Any code, profile, string or asset copied from another slicer.
