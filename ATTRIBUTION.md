# Attribution

Every asset in this repository that is not original code must have a row here, with
author, source and licence. `cargo xtask assets` (planned) fails when a file under
`assets/` has no row.

SliceCraft currently ships **no file assets**: icons are drawn in code, fonts are not
bundled (they come from `storytold/craft-fonts` at build time, never as a dependency),
and test models are generated in code (`slicecraft_render::fixtures`) or are original
geometry (`examples/cube.stl`).

| Asset | Author | Source | Licence |
|---|---|---|---|
| — | — | — | — |

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

Where a crate offers a choice that includes an allowlisted licence
(`MIT OR Zlib`, `Apache-2.0 OR GPL-2.0-only`, `Unlicense OR MIT`, `0BSD OR MIT`,
`MIT OR Apache-2.0 OR LGPL-2.1-or-later`, `… OR Apache-2.0 WITH LLVM-exception`),
SliceCraft takes the allowlisted option (MIT or Apache-2.0).

## Research material

`docs/research/*` records observed behaviour of a reference implementation and public
format specifications. It contains no third-party code and no third-party assets. See
each note's clean-room statement.
