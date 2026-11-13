# Target-app parity — how SliceCraft measures itself

SliceCraft targets **Simplify3D 5.1.2** behaviour. Parity is not a vibe; it is a
number produced by a method, with the method written down so anyone can redo it.

## Why this document exists

A slicer can look finished and still be useless: it emits G-code, the preview is
pretty, and the print fails on layer 3. The sibling CADCraft solves this by defining
readiness **per audience**, weighting features by use, and multiplying by discounts
for file exchange, stability and interaction. SliceCraft uses the same shape:

```
readiness = Σ(feature weight × presence) × file_exchange × stability × interaction
```

## The audiences

| Audience | What they need | Weight focus |
|---|---|---|
| **First print** | import a model, pick a material, slice, print it | import, profiles, basic shells/infill, safe start/end G-code |
| **Everyday user** | the above, reliably, on their printer, with sane supports and good surface finish | support, adhesion, cooling, retraction, seam quality, estimates |
| **Print farm / power user** | batch, multi-tool, per-job overrides, preview before the 4-hour print | CLI/MCP, multi-tool, toolpath preview, estimates, profiles inheritance |

Readiness is reported for each audience, not as one number, because "40% ready" means
different things to someone printing a phone stand and someone running six printers.

## The dimensions

| Dimension | How it is measured |
|---|---|
| Features | every menu item / documented capability of the target, present or absent, weighted by use; sourced from `docs/parity-checklist.md` |
| Geometry precision | analytic fixtures: cross-section area, offset distances, infill clipping, loop orientation — exact where an answer exists, tolerance-documented otherwise |
| Output | G-code validity (start/end, units, absolute/relative, temperature sequence, per-flavour command set) |
| Estimates | predicted time and material vs. measured, once a printer exists |
| Interaction | layer scrub, preview responsiveness at 100k layers, no modal stalls |
| Stability | fuzz/fixture results on hostile files; zero panics |
| Platforms | Windows, macOS, Linux, FreeBSD; wasm for preview |
| Ecosystem | CLI, control channel, MCP tools, scripting |
| Localisation | interface string coverage, `craft-i18n` catalogues at join time |

## Presence scale (per feature)

| Value | Meaning |
|---|---|
| 0% | absent |
| 25% | a command exists, wrong results or missing options |
| 50% | correct on simple geometry, fails on real models |
| 75% | correct and tested, options incomplete |
| 100% | correct, tested, documented, and parity-checked against observed behaviour |

## Current numbers

**Not measured yet.** The scaffold has no user-facing feature set. The first honest
numbers appear after P1 (import, plate, cross sections); until then every table in
this repository says "not measured" rather than inventing a percentage.

| Dimension | Parity | Remaining | Doc |
|---|---:|---|---|
| Features | not measured | — | [gaps.md](gaps.md) |
| Geometry precision | not measured | — | [gaps.md](gaps.md) |
| Output | partial (valid header/tail, no shells) | — | [gaps.md](gaps.md) |
| Estimates | none | — | [gaps.md](gaps.md) |
| Stability | 17 test binaries green, no fuzzing | — | [gaps.md](gaps.md) |
| Platforms | Windows dev machine only | — | [gaps.md](gaps.md) |
| Ecosystem | CLI only | — | [gaps.md](gaps.md) |

## Estimation calibration

Once P1 lands, effort is estimated from this repository's own throughput (lines and
features per agent-hour), not borrowed from a sibling. The sibling repos' calibration
(≈25 commands per agent-hour at presence quality, 2–5 h per moderate feature,
15–100 h per subsystem) is the starting hypothesis, kept in
[PLAN.md](PLAN.md) §4 and corrected when the first phase is measured.
