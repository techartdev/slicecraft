# Attribution

Every asset in this repository that is not original code must have a row here, with
author, source and licence. `cargo xtask assets` (planned) fails when a file under
`assets/` has no row.

The program ships **no file assets**: icons, the application icon included, are drawn
in code (`ui-egui::appicon`; `slicecraft --write-icon` writes it as an `.ico` for the
installer), and fonts are not
bundled (the window reads the operating system's own interface font at run time when
one is installed, such as Segoe UI, Noto Sans or DejaVu Sans, and otherwise uses
egui's built-in fonts; no font file is copied into the program or the repository).
Test models are generated in code (`slicecraft_render::fixtures`) or are original
geometry (`examples/cube.stl`).

The repository holds screenshots for the documentation, in `docs/images/`:

| Asset | Author | Source | Licence |
|---|---|---|---|
| `docs/images/*.jpg` (screenshots of SliceCraft) | the SliceCraft contributors | taken from the running program | CC-BY-4.0 |
| The vase shown in `preview.jpg`, `preview-layers.jpg`, `vase-mode.jpg`, `plate-preview.jpg`, `rotate-tool.jpg`, `section-view.jpg`, `wireframe.jpg`: *Spiral vase* | JJ76 | https://www.thingiverse.com/thing:2795194 | CC-BY-4.0 |
| `docs/images/cube-infill.jpg`, `settings-*.jpg`: models shown | the SliceCraft contributors | `examples/cube.stl` and models named only in the list | original |

The model file itself is not in the repository; the screenshots only show it.

## Packaging tools

| Tool | Used for | Licence |
|---|---|---|
| Inno Setup 6 (Jordan Russell, Martijn Laan) | the Windows installer, `packaging/windows/slicecraft.iss`; its setup and uninstall program is inside the installer | Inno Setup License (permissive; keeps its copyright notice). Not an SPDX identifier on the allowlist: the installer is not published until the operator approves it. The zip contains only SliceCraft's own build. |

## Third-party code

No third-party source code is vendored. Dependencies are declared in `Cargo.toml`
files and must be inside the licence allowlist in `AGENTS.md`, with the exceptions
below.

### Direct dependencies

| Crate | Used by | Licence |
|---|---|---|
| `i_overlay` 9 (+ `i_float`, `i_shape`, `i_tree`, `i_key_sort`, `libm`) | `geom`: polygon offset and booleans | MIT OR Apache-2.0 (deps MIT) |
| `eframe` / `egui` 0.35 | `ui-egui`: the window | MIT OR Apache-2.0 |
| `rfd` 0.17 | `ui-egui`: native file dialogs | MIT |
| `roxmltree` 0.21 (+ `memchr`) | `io`: reading Simplify3D `.fff` profiles | MIT OR Apache-2.0 (`memchr`: Unlicense OR MIT, taken as MIT) |
| `miniz_oxide` 0.8 (+ `adler2`) | `io`: inflating 3MF packages (already in the window's tree) | MIT OR Zlib OR Apache-2.0, taken as MIT (`adler2`: 0BSD OR MIT OR Apache-2.0, taken as MIT) |
| `serial2` 0.2 (+ `libc` on Unix, `windows-sys` on Windows) | `io`: printer connection over a serial port | BSD-2-Clause OR Apache-2.0 (deps MIT OR Apache-2.0) |
| `zune-jpeg` 0.5 (+ `zune-core`) | `ui-egui`: decoding print-camera snapshots (already in the window's tree) | MIT OR Apache-2.0 OR Zlib, taken as MIT |
| `eframe` `wgpu` feature (+ `wgpu` 29, `naga`, `egui-wgpu` and their backends) | `ui-egui`: the 3D view on the graphics card | MIT OR Apache-2.0 (deps MIT/Apache-2.0, ISC, CC0-1.0, BSD-2-Clause; `foldhash` 0.1 Zlib, see exceptions) |
| `skrifa` 0.42 | `ui-egui`: checking that a system font parses before the window uses it (already in the window's tree, through egui) | MIT OR Apache-2.0 |

Everything below the window (`geom` … `engine`, `mcp`, the CLI) depends on
`i_overlay`, `roxmltree`, `miniz_oxide` and `serial2` only. The exceptions are confined to the window (`ui-egui` and the
`slicecraft` app).

### Dependency licence exceptions

Recorded 2026-11-13, approved by the operator for the MVP window, **to be proposed
upstream in `storytold/craft-contrib`** (a repository may not widen the allowlist in
its own copy). Measured with `cargo metadata` over every target platform.

| Licence | Crates (version) | Pulled in by | Avoidable? |
|---|---|---|---|
| `(MIT OR Apache-2.0) AND Unicode-3.0` | `unicode-ident` 1.0 | `proc-macro2`, i.e. every proc-macro crate | no — any Rust GUI stack has it |
| `Unicode-3.0` | `icu_*` 2.4, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, `zerovec` (+ derives) | `url` → `webbrowser` → `egui-winit`'s `links` feature, forced on by `eframe` | only by dropping `eframe` |
| `BSL-1.0` | `clipboard-win` 5.4, `error-code` 3.4 (Windows only) | `arboard` → `egui-winit`'s `clipboard` feature, forced on by `eframe` | only by dropping `eframe` |
| `Zlib` | `foldhash` 0.2, `slotmap` 1.1 | `epaint` (via `hashbrown`), `glow` | no, for egui 0.35 |
| `Zlib` | `foldhash` 0.1 | `wgpu` 29 (via `hashbrown` 0.15), for the GPU 3D view; approved by the operator 2026-11-14 | only by drawing the 3D view on the processor |

Where a crate offers a choice that includes an allowlisted licence
(`MIT OR Zlib`, `Apache-2.0 OR GPL-2.0-only`, `Unlicense OR MIT`, `0BSD OR MIT`,
`MIT OR Apache-2.0 OR LGPL-2.1-or-later`, `… OR Apache-2.0 WITH LLVM-exception`),
SliceCraft takes the allowlisted option (MIT or Apache-2.0).

## Research material

`docs/research/*` records observed behaviour of a reference implementation and public
format specifications. It contains no third-party code and no third-party assets. See
each note's clean-room statement.
